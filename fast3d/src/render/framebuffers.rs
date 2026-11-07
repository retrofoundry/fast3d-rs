pub(crate) mod coherence;
mod native_fill;
mod native_triangle;
pub(super) mod presentation;
pub(crate) mod targets;
pub(super) mod writeback;

use super::{workload::TargetId, SceneRenderer, CLEAR_COLOR, DEPTH_FORMAT};
use crate::{ClearPolicy, DiagKind, Diagnostic};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ImageLayout {
    pub width: u32,
    pub fmt: u8,
    pub siz: u8,
}

pub(super) struct ImageRequest {
    pub id: TargetId,
    pub layout: ImageLayout,
    pub height: u32,
    pub pc: u64,
}

pub(super) fn color_descriptor(label: &str, allocation: (u32, u32)) -> wgpu::TextureDescriptor<'_> {
    wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: allocation.0,
            height: allocation.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    }
}

pub(super) struct Framebuffer {
    pub color: wgpu::Texture,
    pub presentation: Option<presentation::Color>,
    pub attach: wgpu::TextureView,
    pub sampled: wgpu::TextureView,
    pub present_bg: wgpu::BindGroup,
    pub present_extent: wgpu::BindGroup,
    pub present_rows: std::ops::Range<u32>,
    pub height: u32,
    pub sampling: wgpu::Buffer,
    pub presentation_sampling: Option<wgpu::Buffer>,
    pub layout: ImageLayout,
}

#[derive(Clone)]
pub(crate) struct DepthImage {
    pub(crate) texture: wgpu::Texture,
    pub(super) presentation: Option<Box<DepthImage>>,
    pub(super) attach: wgpu::TextureView,
    pub(super) layout: ImageLayout,
    pub(super) delta_view: wgpu::TextureView,
    pub(super) delta_depth: Option<wgpu::TextureView>,
}

impl DepthImage {
    pub(super) fn new(device: &wgpu::Device, layout: ImageLayout, allocation: (u32, u32)) -> Self {
        Self::create(device, layout, allocation, true)
    }

    pub(super) fn snapshot(
        device: &wgpu::Device,
        layout: ImageLayout,
        allocation: (u32, u32),
    ) -> Self {
        Self::create(device, layout, allocation, false)
    }

    fn create(
        device: &wgpu::Device,
        layout: ImageLayout,
        allocation: (u32, u32),
        writable: bool,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth-store"),
            size: wgpu::Extent3d {
                width: allocation.0,
                height: allocation.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let attach = texture.create_view(&Default::default());
        let mut descriptor = wgpu::TextureDescriptor {
            label: Some("depth-delta"),
            size: texture.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Uint,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let delta = device.create_texture(&descriptor);
        let delta_view = delta.create_view(&Default::default());
        descriptor.label = Some("depth-delta-test");
        descriptor.format = DEPTH_FORMAT;
        let delta_depth = writable.then(|| {
            device
                .create_texture(&descriptor)
                .create_view(&Default::default())
        });
        Self {
            texture,
            presentation: None,
            attach,
            layout,
            delta_view,
            delta_depth,
        }
    }
}

pub(super) struct DepthPipeline {
    fill: wgpu::RenderPipeline,
    copy_attributes: wgpu::RenderPipeline,
    copy: wgpu::RenderPipeline,
    fill_layout: wgpu::BindGroupLayout,
    copy_layout: wgpu::BindGroupLayout,
    import: wgpu::RenderPipeline,
    import_layout: wgpu::BindGroupLayout,
    expand: wgpu::RenderPipeline,
    expand_layout: wgpu::BindGroupLayout,
}

impl DepthPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("depth-storage"),
            source: wgpu::ShaderSource::Wgsl(include_str!("depth.wgsl").into()),
        });
        let fill_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("depth-fill"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let copy_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("depth-copy"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let import_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("depth-import"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let expand_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("depth-expand"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let make = |entry, attributes, group: &wgpu::BindGroupLayout| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(entry),
                bind_group_layouts: &[Some(group)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: if attributes {
                        &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::R8Uint,
                            blend: None,
                            write_mask: wgpu::ColorWrites::RED,
                        })]
                    } else {
                        &[]
                    },
                }),
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            fill: make("fill_depth", true, &fill_layout),
            copy_attributes: make("copy_attributes", true, &copy_layout),
            copy: make("copy_depth", false, &copy_layout),
            import: make("import_depth", true, &import_layout),
            expand: make("expand_depth", true, &expand_layout),
            expand_layout,
            import_layout,
            fill_layout,
            copy_layout,
        }
    }

    fn import(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        target: &DepthImage,
        spans: &[(u32, u32, u32)],
    ) {
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("depth-import"),
            layout: &self.import_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(source),
            }],
        });
        let mut pass = attribute_pass(encoder, target, wgpu::LoadOp::Load, 0);
        pass.set_pipeline(&self.import);
        pass.set_bind_group(0, &group, &[]);
        for &(x, y, width) in spans {
            pass.set_scissor_rect(x, y, width, 1);
            pass.draw(0..3, 0..1);
        }
        drop(pass);
        if let Some(&(x, y, width)) = spans.first() {
            let (left, top, right, bottom) = spans.iter().fold(
                (x, y, x + width, y + 1),
                |(left, top, right, bottom), &(x, y, width)| {
                    (
                        left.min(x),
                        top.min(y),
                        right.max(x + width),
                        bottom.max(y + 1),
                    )
                },
            );
            self.sync_delta_depth(
                device,
                encoder,
                target,
                (left, top, right - left, bottom - top),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn expand(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &DepthImage,
        target: &DepthImage,
        scale: u32,
        spans: &[(u32, u32, u32)],
        profiling: &crate::profiling::Recorder,
    ) {
        let buffer = super::buffer_init(
            device,
            profiling,
            "presentation",
            &wgpu::util::BufferInitDescriptor {
                label: Some("depth-expand-scale"),
                contents: bytemuck::cast_slice(&[scale, 0, 0, 0]),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("depth-expand"),
            layout: &self.expand_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&source.attach),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&source.delta_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });
        let mut pass = attribute_pass(encoder, target, wgpu::LoadOp::Load, 0);
        pass.set_pipeline(&self.expand);
        pass.set_bind_group(0, &group, &[]);
        for &(x, y, width) in spans {
            pass.set_scissor_rect(x * scale, y * scale, width * scale, scale);
            pass.draw(0..3, 0..1);
        }
        drop(pass);
        if let Some(&(x, y, width)) = spans.first() {
            let (left, top, right, bottom) = spans.iter().fold(
                (x, y, x + width, y + 1),
                |(left, top, right, bottom), &(x, y, width)| {
                    (
                        left.min(x),
                        top.min(y),
                        right.max(x + width),
                        bottom.max(y + 1),
                    )
                },
            );
            self.sync_delta_depth(
                device,
                encoder,
                target,
                (
                    left * scale,
                    top * scale,
                    (right - left) * scale,
                    (bottom - top) * scale,
                ),
            );
        }
    }

    pub(super) fn copy(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &DepthImage,
        target: &DepthImage,
    ) {
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("depth-copy"),
            layout: &self.copy_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&source.attach),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&source.delta_view),
                },
            ],
        });
        let mut pass = attribute_pass(encoder, target, wgpu::LoadOp::Load, 0);
        pass.set_pipeline(&self.copy_attributes);
        pass.set_bind_group(0, &group, &[]);
        pass.set_scissor_rect(0, 0, source.texture.width(), source.texture.height());
        pass.draw(0..3, 0..1);
        drop(pass);
    }

    pub(super) fn sync_delta_depth(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &DepthImage,
        bounds: (u32, u32, u32, u32),
    ) {
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("depth-delta-sync"),
            layout: &self.copy_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&target.attach),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&target.delta_view),
                },
            ],
        });
        let mut pass = depth_pass(
            encoder,
            target.delta_depth.as_ref().unwrap(),
            wgpu::LoadOp::Load,
        );
        pass.set_pipeline(&self.copy);
        pass.set_bind_group(0, &group, &[]);
        let (x, y, width, height) = bounds;
        pass.set_scissor_rect(x, y, width, height);
        pass.draw(0..3, 0..1);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &DepthImage,
        word: u32,
        scale: u32,
        bounds: (u32, u32, u32, u32),
        profiling: &crate::profiling::Recorder,
    ) {
        let (x, y, width, height) = bounds;
        if width == 0 || height == 0 {
            return;
        }
        if bounds == (0, 0, target.texture.width(), target.texture.height())
            && word & 0xfffc_fffc == 0xfffc_fffc
            && (word >> 16) & 3 == word & 3
        {
            let packed = word & 0xffff;
            let code = ((packed & 3) << 2) | (3 * (packed & 1));
            let pass = attribute_pass(
                encoder,
                target,
                wgpu::LoadOp::Clear(262143.0 / 262144.0),
                code,
            );
            drop(pass);

            drop(depth_pass(
                encoder,
                target.delta_depth.as_ref().unwrap(),
                wgpu::LoadOp::Clear(262143.0 / 262144.0),
            ));
            return;
        }
        let buffer = super::buffer_init(
            device,
            profiling,
            "uniforms",
            &wgpu::util::BufferInitDescriptor {
                label: Some("depth-fill"),
                contents: bytemuck::cast_slice(&[word, target.layout.width / scale, scale, 0]),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("depth-fill"),
            layout: &self.fill_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let mut pass = attribute_pass(encoder, target, wgpu::LoadOp::Load, 0);
        pass.set_pipeline(&self.fill);
        pass.set_bind_group(0, &group, &[]);
        pass.set_scissor_rect(x, y, width, height);
        pass.draw(0..3, 0..1);
        drop(pass);
        self.sync_delta_depth(device, encoder, target, bounds);
    }
}

fn attribute_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    target: &DepthImage,
    load: wgpu::LoadOp<f32>,
    delta: u32,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("depth-attributes"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &target.delta_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: if matches!(load, wgpu::LoadOp::Clear(_)) {
                    wgpu::LoadOp::Clear(wgpu::Color {
                        r: f64::from(delta),
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    })
                } else {
                    wgpu::LoadOp::Load
                },
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &target.attach,
            depth_ops: Some(wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

fn clear_image(encoder: &mut wgpu::CommandEncoder, target: &DepthImage) {
    drop(attribute_pass(encoder, target, wgpu::LoadOp::Clear(1.0), 0));
    clear_depth(encoder, target.delta_depth.as_ref().unwrap());
}

fn depth_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<f32>,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("depth-storage"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

pub(super) fn clear_depth(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    let _pass = depth_pass(encoder, view, wgpu::LoadOp::Clear(1.0));
}

pub(super) fn color_sampling(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    scale: u32,
) -> wgpu::Buffer {
    let tile = crate::hle::tile_sampling::TileSampling {
        image: [1, 1, 2, scale],
        modes: [2, 2, width, height],
        ..Default::default()
    };
    super::sampling_buffer(device, &[tile; super::TILE_SAMPLING_COUNT])
}

impl SceneRenderer {
    #[cfg(feature = "capture")]
    pub(crate) fn prime_legacy_attachments(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        color: wgpu::Color,
        depth: f32,
    ) {
        let mut encoder = device.create_command_encoder(&Default::default());
        let layout = ImageLayout {
            width: self.fb_w,
            fmt: 0,
            siz: 2,
        };
        self.ensure_color(
            device,
            &mut encoder,
            ImageRequest {
                id: TargetId::Legacy,
                layout,
                height: self.fb_h,
                pc: 0,
            },
            (self.fb_w, self.fb_h),
            ClearPolicy::Persist,
        );
        self.ensure_depth(
            device,
            &mut encoder,
            ImageRequest {
                id: TargetId::Legacy,
                layout,
                height: self.fb_h,
                pc: 0,
            },
            (self.fb_w, self.fb_h),
            ClearPolicy::Persist,
        );
        let framebuffer = &self.framebuffers[&TargetId::Legacy];
        for view in std::iter::once(&framebuffer.attach)
            .chain(framebuffer.presentation.as_ref().map(|p| &p.attach))
        {
            super::workload::clear_color(&mut encoder, view, wgpu::LoadOp::Clear(color));
        }
        let target = &self.depthbuffers[&TargetId::Legacy];
        for image in std::iter::once(target).chain(target.presentation.as_deref()) {
            drop(attribute_pass(
                &mut encoder,
                image,
                wgpu::LoadOp::Clear(depth),
                0,
            ));
            drop(depth_pass(
                &mut encoder,
                image.delta_depth.as_ref().unwrap(),
                wgpu::LoadOp::Clear(depth),
            ));
        }
        queue.submit([encoder.finish()]);
    }

    pub(super) fn ensure_color(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        request: ImageRequest,
        allocation: (u32, u32),
        policy: ClearPolicy,
    ) {
        let ImageRequest {
            id,
            layout,
            height,
            pc,
        } = request;
        let first = self.first_touch.insert(id);
        let old = self.framebuffers.get(&id);
        let compatible = old.is_some_and(|old| {
            old.layout == layout && (id != TargetId::Legacy || old.height == height)
        });
        let initialize = first && policy == ClearPolicy::PerFrame;
        let height = if compatible {
            height.max(old.unwrap().height)
        } else {
            height
        };
        if compatible && old.unwrap().color.height() >= allocation.1 {
            let old = self.framebuffers.get_mut(&id).unwrap();
            if old.height != height {
                old.height = height;
                old.sampling = color_sampling(device, layout.width, height, 1);
                old.presentation_sampling = (self.render_scale > 1)
                    .then(|| color_sampling(device, layout.width, height, self.render_scale));
                self.profiling.buffer(
                    "uniforms",
                    old.sampling.size(),
                    std::mem::size_of::<super::TileSamplingArray>() as u64,
                );
            }
            let old = &self.framebuffers[&id];
            if initialize {
                super::workload::clear_color(
                    encoder,
                    &old.attach,
                    wgpu::LoadOp::Clear(CLEAR_COLOR),
                );
                if let Some(presentation) = &old.presentation {
                    super::workload::clear_color(
                        encoder,
                        &presentation.attach,
                        wgpu::LoadOp::Clear(CLEAR_COLOR),
                    );
                }
            }
            return;
        }
        if let Some(old) = old.filter(|_| !compatible) {
            if self.coherence.is_some() {
                if let TargetId::Guest(address) = id {
                    self.color_versions.insert(
                        coherence::Writer {
                            address,
                            layout: old.layout,
                            depth: false,
                        },
                        old.color.clone(),
                    );
                    if let Some(image) = &old.presentation {
                        self.presentation_versions.insert(
                            coherence::Writer {
                                address,
                                layout: old.layout,
                                depth: false,
                            },
                            image.texture.clone(),
                        );
                    }
                }
            }
            self.reinterpretation(id, false, pc);
        }
        let new = self.make_fb(device, layout, allocation, height);
        super::workload::clear_color(encoder, &new.attach, wgpu::LoadOp::Clear(CLEAR_COLOR));
        if let Some(presentation) = &new.presentation {
            super::workload::clear_color(
                encoder,
                &presentation.attach,
                wgpu::LoadOp::Clear(CLEAR_COLOR),
            );
        }
        if compatible && !initialize {
            let old = &self.framebuffers[&id];
            self.profiling.count(
                "copy.framebuffer_bytes",
                u64::from(old.color.width()) * u64::from(old.color.height()) * 4,
            );
            encoder.copy_texture_to_texture(
                old.color.as_image_copy(),
                new.color.as_image_copy(),
                old.color.size(),
            );
            if let (Some(old), Some(new)) = (&old.presentation, &new.presentation) {
                encoder.copy_texture_to_texture(
                    old.texture.as_image_copy(),
                    new.texture.as_image_copy(),
                    old.texture.size(),
                );
            }
        }
        self.framebuffers.insert(id, new);
    }

    pub(super) fn ensure_depth(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        request: ImageRequest,
        allocation: (u32, u32),
        policy: ClearPolicy,
    ) {
        let ImageRequest {
            id,
            layout,
            height,
            pc,
        } = request;
        let first = self.depth_first_touch.insert(id);
        let old = self.depthbuffers.get(&id);
        let compatible = old.is_some_and(|old| {
            old.layout == layout && (id != TargetId::Legacy || old.texture.height() == height)
        });
        let initialize = first && policy == ClearPolicy::PerFrame;
        if compatible && old.unwrap().texture.height() >= allocation.1 {
            if initialize {
                clear_image(encoder, old.unwrap());
                if let Some(presentation) = &old.unwrap().presentation {
                    clear_image(encoder, presentation);
                }
            }
            return;
        }
        if old.is_some() && !compatible {
            self.reinterpretation(id, true, pc);
        }
        let resources = self.profiling.span("resources");
        let mut new = DepthImage::new(device, layout, allocation);
        if self.render_scale > 1 {
            let scaled = (
                allocation.0 * self.render_scale,
                allocation.1 * self.render_scale,
            );
            new.presentation = Some(Box::new(DepthImage::new(
                device,
                ImageLayout {
                    width: scaled.0,
                    ..layout
                },
                scaled,
            )));
            self.profiling.texture(
                "presentation_depth",
                u64::from(scaled.0) * u64::from(scaled.1) * 9,
                0,
            );
        }
        self.profiling.texture(
            "depth",
            u64::from(allocation.0) * u64::from(allocation.1) * 9,
            0,
        );
        drop(resources);
        clear_image(encoder, &new);
        if let Some(presentation) = &new.presentation {
            clear_image(encoder, presentation);
        }
        if compatible && !initialize {
            let old = &self.depthbuffers[&id];
            self.profiling.count(
                "copy.depth_raster_bytes",
                u64::from(old.texture.width()) * u64::from(old.texture.height()) * 4,
            );
            self.depth_pipeline.copy(device, encoder, old, &new);
            self.depth_pipeline.sync_delta_depth(
                device,
                encoder,
                &new,
                (0, 0, old.texture.width(), old.texture.height()),
            );
            if let (Some(old), Some(new)) = (&old.presentation, &new.presentation) {
                self.depth_pipeline.copy(device, encoder, old, new);
                self.depth_pipeline.sync_delta_depth(
                    device,
                    encoder,
                    new,
                    (0, 0, old.texture.width(), old.texture.height()),
                );
            }
        }
        self.depthbuffers.insert(id, new);
    }

    fn reinterpretation(&mut self, id: TargetId, depth: bool, pc: u64) {
        if self.coherence.is_some() {
            return;
        }
        if let TargetId::Guest(address) = id {
            self.diagnostics.push(Diagnostic {
                at: pc,
                kind: DiagKind::UnsupportedImageReinterpretation { address, depth },
            });
        }
    }
}

fn pack_depth(depth: f32) -> u16 {
    let z = ((depth * 262144.0) as u32).min(0x3ffff);
    let exponent = (0..8u32)
        .rev()
        .find(|e| z >= 0x40000 - (0x40000 >> e))
        .unwrap();
    let base = 0x40000 - (0x40000 >> exponent);
    let mantissa = (z - base) >> (6 - exponent.min(6));
    // Guest write-back does not serialize the presentation dz plane; it uses dz = 0.
    ((exponent << 13) | (mantissa << 2)) as u16
}

#[cfg(test)]
mod writeback_tests {
    use super::pack_depth;

    #[test]
    fn packed_depth_exponent_boundaries_and_neighbors() {
        for (base, previous, packed, next) in [
            (0, 0, 0, 0),
            (0x20000, 0x1ffc, 0x2000, 0x2000),
            (0x30000, 0x3ffc, 0x4000, 0x4000),
            (0x38000, 0x5ffc, 0x6000, 0x6000),
            (0x3c000, 0x7ffc, 0x8000, 0x8000),
            (0x3e000, 0x9ffc, 0xa000, 0xa000),
            (0x3f000, 0xbffc, 0xc000, 0xc004),
            (0x3f800, 0xdffc, 0xe000, 0xe004),
            (0x3ffff, 0xfff8, 0xfffc, 0xfffc),
        ] {
            for (z, expected) in [(base - 1, previous), (base, packed), (base + 1, next)] {
                assert_eq!(pack_depth(z as f32 / 262144.0), expected, "z18 {z:#x}");
            }
        }
    }

    #[test]
    fn packed_depth_zero_dz_words_roundtrip() {
        let bases = [
            0, 0x20000, 0x30000, 0x38000, 0x3c000, 0x3e000, 0x3f000, 0x3f800,
        ];
        let shifts = [6, 5, 4, 3, 2, 1, 0, 0];
        for word in (0u16..=0xfffc).step_by(4) {
            let exponent = (word >> 13) as usize;
            let z = bases[exponent] + (u32::from((word & 0x1ffc) >> 2) << shifts[exponent]);
            assert_eq!(pack_depth(z as f32 / 262144.0), word);
        }
        assert_eq!(pack_depth(1.0), 0xfffc);
    }
}

pub(crate) mod copies;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod delta_tests {
    use super::*;

    fn read(device: &wgpu::Device, queue: &wgpu::Queue, image: &DepthImage) -> Vec<u8> {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                r#"
@group(0) @binding(0) var codes: texture_2d<u32>;
@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var companion: texture_depth_2d;
@vertex fn vs(@builtin(vertex_index) i:u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(f32((i<<1u)&2u)*2.0-1.0, 1.0-f32(i&2u)*2.0, 0.0,1.0);
}
@fragment fn fs(@builtin(position) p:vec4<f32>) -> @location(0) vec4<f32> {
    let c=vec2<i32>(p.xy);
    return vec4<f32>(f32(textureLoad(codes,c,0).r)/255.0, 0.0,
        select(0.0,1.0,textureLoad(depth,c,0)==textureLoad(companion,c,0)),1.0);
}
"#
                .into(),
            ),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&image.delta_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&image.attach),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        image.delta_depth.as_ref().unwrap_or(&image.attach),
                    ),
                },
            ],
        });
        crate::tests::common::pixels_from_render(
            device,
            queue,
            image.texture.width(),
            image.texture.height(),
            wgpu::TextureFormat::Rgba8Unorm,
            |view| {
                let mut encoder = device.create_command_encoder(&Default::default());
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bg, &[]);
                pass.draw(0..3, 0..1);
                drop(pass);
                queue.submit([encoder.finish()]);
            },
        )
    }

    #[test]
    fn scaled_delta_growth_rebuild_and_strict_rollback() {
        let (device, queue, _) = super::super::headless_device();
        let mut renderer =
            SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 2, false);
        renderer.set_test_scale(2, 256 * 1024 * 1024);
        renderer.coherence = Some(Default::default());
        let layout = ImageLayout {
            width: 64,
            fmt: 0,
            siz: 2,
        };
        let id = TargetId::Guest(0x10000);
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.ensure_depth(
            &device,
            &mut encoder,
            ImageRequest {
                id,
                layout,
                height: 2,
                pc: 0,
            },
            (64, 2),
            ClearPolicy::Persist,
        );
        let depth = &renderer.depthbuffers[&id];
        for (image, scale) in [(depth, 1), (depth.presentation.as_deref().unwrap(), 2)] {
            renderer.depth_pipeline.fill(
                &device,
                &mut encoder,
                image,
                0x60016002,
                scale,
                (0, 0, 64 * scale, 2 * scale),
                &renderer.profiling,
            );
        }
        renderer.ensure_depth(
            &device,
            &mut encoder,
            ImageRequest {
                id,
                layout,
                height: 4,
                pc: 0,
            },
            (64, 4),
            ClearPolicy::Persist,
        );
        queue.submit([encoder.finish()]);
        for scale in [2, 4, 1, 2] {
            renderer.change_scale(&device, &queue, scale).unwrap();
            let depth = &renderer.depthbuffers[&id];
            for (image, multiplier) in
                std::iter::once((depth, 1)).chain(depth.presentation.as_deref().map(|p| (p, scale)))
            {
                let pixels = read(&device, &queue, image);
                for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
                    let x = index as u32 % image.texture.width() / multiplier;
                    let y = index as u32 / image.texture.width() / multiplier;
                    let code = if y >= 2 {
                        0
                    } else if x.is_multiple_of(2) {
                        7
                    } else {
                        8
                    };
                    assert_eq!(*pixel, [code, 0, 255, 255], "{scale}x, {x},{y}");
                }
                let snapshot = DepthImage::snapshot(
                    &device,
                    image.layout,
                    (image.texture.width(), image.texture.height()),
                );
                let mut encoder = device.create_command_encoder(&Default::default());
                renderer
                    .depth_pipeline
                    .copy(&device, &mut encoder, image, &snapshot);
                queue.submit([encoder.finish()]);
                assert!(snapshot.delta_depth.is_none());
                assert_eq!(read(&device, &queue, &snapshot), pixels);
            }
        }
        let depth = &renderer.depthbuffers[&id];
        let before = (
            read(&device, &queue, depth),
            read(&device, &queue, depth.presentation.as_deref().unwrap()),
        );
        let staged =
            renderer.stage_strict_task(&device, &queue, TargetId::Guest(0x20000), Some(id));
        let mut encoder = device.create_command_encoder(&Default::default());
        let depth = &renderer.depthbuffers[&id];
        for (image, scale) in [(depth, 1), (depth.presentation.as_deref().unwrap(), 2)] {
            renderer.depth_pipeline.fill(
                &device,
                &mut encoder,
                image,
                0xffffffff,
                scale,
                (0, 0, 64 * scale, 4 * scale),
                &renderer.profiling,
            );
        }
        queue.submit([encoder.finish()]);
        assert_ne!(read(&device, &queue, &renderer.depthbuffers[&id]), before.0);
        renderer.discard_strict_task(staged);
        let depth = &renderer.depthbuffers[&id];
        assert_eq!(
            (
                read(&device, &queue, depth),
                read(&device, &queue, depth.presentation.as_deref().unwrap())
            ),
            before
        );
    }

    #[test]
    fn authored_fill_growth_snapshot_clear_and_visible_only_import() {
        let (device, queue, _) = super::super::headless_device();
        let mut renderer =
            SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 2, false);
        let layout = ImageLayout {
            width: 64,
            fmt: 0,
            siz: 2,
        };
        let id = TargetId::Guest(0x10000);
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.ensure_depth(
            &device,
            &mut encoder,
            ImageRequest {
                id,
                layout,
                height: 2,
                pc: 0,
            },
            (64, 2),
            ClearPolicy::Persist,
        );
        renderer.depth_pipeline.fill(
            &device,
            &mut encoder,
            &renderer.depthbuffers[&id],
            0x60016002,
            1,
            (0, 0, 64, 2),
            &renderer.profiling,
        );
        renderer.ensure_depth(
            &device,
            &mut encoder,
            ImageRequest {
                id,
                layout,
                height: 4,
                pc: 0,
            },
            (64, 4),
            ClearPolicy::Persist,
        );
        let image = &renderer.depthbuffers[&id];
        let snapshot = DepthImage::new(&device, layout, (64, 4));
        renderer
            .depth_pipeline
            .copy(&device, &mut encoder, image, &snapshot);
        renderer
            .depth_pipeline
            .sync_delta_depth(&device, &mut encoder, &snapshot, (0, 0, 64, 4));
        queue.submit([encoder.finish()]);
        for image in [image, &snapshot] {
            let pixels = read(&device, &queue, image);
            for y in 0..4 {
                for x in 0usize..64 {
                    let code = if y >= 2 {
                        0
                    } else if x.is_multiple_of(2) {
                        7
                    } else {
                        8
                    };
                    assert_eq!(&pixels[(y * 64 + x) * 4..][..4], &[code, 0, 255, 255]);
                }
            }
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.depth_pipeline.fill(
            &device,
            &mut encoder,
            &snapshot,
            0xffffffff,
            1,
            (0, 0, 64, 4),
            &renderer.profiling,
        );
        queue.submit([encoder.finish()]);
        assert!(read(&device, &queue, &snapshot)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [15, 0, 255, 255]));
        let source = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Uint,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        queue.write_texture(
            source.as_image_copy(),
            bytemuck::cast_slice(&[0x6001u32; 256]),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: None,
            },
            source.size(),
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.depth_pipeline.import(
            &device,
            &mut encoder,
            &source.create_view(&Default::default()),
            &snapshot,
            &[(4, 1, 8)],
        );
        queue.submit([encoder.finish()]);
        let pixels = read(&device, &queue, &snapshot);
        assert_eq!(&pixels[(64 + 4) * 4..][..4], &[4, 0, 255, 255]);
        assert_eq!(&pixels[(64 + 3) * 4..][..4], &[15, 0, 255, 255]);
        let readonly = DepthImage::snapshot(&device, layout, (64, 4));
        assert!(readonly.delta_depth.is_none());
        let mut encoder = device.create_command_encoder(&Default::default());
        clear_image(&mut encoder, &snapshot);
        queue.submit([encoder.finish()]);
        assert!(read(&device, &queue, &snapshot)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [0, 0, 255, 255]));
        #[cfg(feature = "capture")]
        {
            renderer.prime_legacy_attachments(&device, &queue, wgpu::Color::BLACK, 0.0);
            let legacy = &renderer.depthbuffers[&TargetId::Legacy];
            assert!(read(&device, &queue, legacy)
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [0, 0, 255, 255]));
        }
    }
}
