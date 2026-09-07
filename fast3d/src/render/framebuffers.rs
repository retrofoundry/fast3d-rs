use super::{workload::TargetId, SceneRenderer, CLEAR_COLOR, DEPTH_FORMAT};
use crate::{ClearPolicy, DiagKind, Diagnostic};
use wgpu::util::DeviceExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ImageLayout {
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

pub(super) struct Framebuffer {
    pub color: wgpu::Texture,
    pub attach: wgpu::TextureView,
    pub sampled: wgpu::TextureView,
    pub present_bg: wgpu::BindGroup,
    pub present_extent: wgpu::BindGroup,
    pub height: u32,
    pub sampling: wgpu::Buffer,
    pub layout: ImageLayout,
}

pub(crate) struct DepthImage {
    pub(crate) texture: wgpu::Texture,
    pub(super) attach: wgpu::TextureView,
    pub(super) layout: ImageLayout,
}

impl DepthImage {
    pub(super) fn new(device: &wgpu::Device, layout: ImageLayout, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth-store"),
            size: wgpu::Extent3d {
                width: layout.width,
                height,
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
        Self {
            texture,
            attach,
            layout,
        }
    }
}

pub(super) struct DepthPipeline {
    fill: wgpu::RenderPipeline,
    copy: wgpu::RenderPipeline,
    fill_layout: wgpu::BindGroupLayout,
    copy_layout: wgpu::BindGroupLayout,
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
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let make = |entry, group: &wgpu::BindGroupLayout| {
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
                    targets: &[],
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
            fill: make("fill_depth", &fill_layout),
            copy: make("copy_depth", &copy_layout),
            fill_layout,
            copy_layout,
        }
    }

    fn copy(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &DepthImage,
        target: &DepthImage,
    ) {
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("depth-copy"),
            layout: &self.copy_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&source.attach),
            }],
        });
        let mut pass = depth_pass(encoder, &target.attach, wgpu::LoadOp::Load);
        pass.set_pipeline(&self.copy);
        pass.set_bind_group(0, &group, &[]);
        pass.set_scissor_rect(0, 0, source.texture.width(), source.texture.height());
        pass.draw(0..3, 0..1);
    }

    pub fn fill(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &DepthImage,
        word: u32,
        bounds: (u32, u32, u32, u32),
    ) {
        let (x, y, width, height) = bounds;
        if width == 0 || height == 0 {
            return;
        }
        if bounds == (0, 0, target.texture.width(), target.texture.height())
            && word & 0xfffc_fffc == 0xfffc_fffc
        {
            clear_depth(encoder, &target.attach);
            return;
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("depth-fill"),
            contents: bytemuck::cast_slice(&[word, target.layout.width, 0, 0]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("depth-fill"),
            layout: &self.fill_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let mut pass = depth_pass(encoder, &target.attach, wgpu::LoadOp::Load);
        pass.set_pipeline(&self.fill);
        pass.set_bind_group(0, &group, &[]);
        pass.set_scissor_rect(x, y, width, height);
        pass.draw(0..3, 0..1);
    }
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

pub(super) fn color_sampling(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Buffer {
    let tile = crate::hle::tile_sampling::TileSampling {
        image: [1, 1, 2, 1],
        modes: [2, 2, width, height],
        ..Default::default()
    };
    super::sampling_buffer(device, &[tile; super::TILE_SAMPLING_COUNT])
}

impl SceneRenderer {
    pub(super) fn ensure_color(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        request: ImageRequest,
        attachment_height: u32,
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
        if compatible && old.unwrap().color.height() >= attachment_height {
            if old.unwrap().height != height {
                let group = self.make_present_extent(
                    device,
                    layout.width,
                    height,
                    old.unwrap().color.height(),
                );
                let old = self.framebuffers.get_mut(&id).unwrap();
                old.height = height;
                old.present_extent = group;
                old.sampling = color_sampling(device, layout.width, height);
            }
            let old = &self.framebuffers[&id];
            if initialize {
                super::workload::clear_color(
                    encoder,
                    &old.attach,
                    wgpu::LoadOp::Clear(CLEAR_COLOR),
                );
            }
            return;
        }
        if old.is_some() && !compatible {
            self.reinterpretation(id, false, pc);
        }
        let new = self.make_fb(device, layout, attachment_height, height);
        super::workload::clear_color(encoder, &new.attach, wgpu::LoadOp::Clear(CLEAR_COLOR));
        if compatible && !initialize {
            let old = &self.framebuffers[&id];
            encoder.copy_texture_to_texture(
                old.color.as_image_copy(),
                new.color.as_image_copy(),
                old.color.size(),
            );
        }
        self.framebuffers.insert(id, new);
    }

    pub(super) fn ensure_depth(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        request: ImageRequest,
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
        if compatible && old.unwrap().texture.height() >= height {
            if initialize {
                clear_depth(encoder, &old.unwrap().attach);
            }
            return;
        }
        if old.is_some() && !compatible {
            self.reinterpretation(id, true, pc);
        }
        let new = DepthImage::new(device, layout, height);
        clear_depth(encoder, &new.attach);
        if compatible && !initialize {
            self.depth_pipeline
                .copy(device, encoder, &self.depthbuffers[&id], &new);
        }
        self.depthbuffers.insert(id, new);
    }

    fn reinterpretation(&mut self, id: TargetId, depth: bool, pc: u64) {
        if let TargetId::Guest(address) = id {
            self.diagnostics.push(Diagnostic {
                at: pc,
                kind: DiagKind::UnsupportedImageReinterpretation { address, depth },
            });
        }
    }
}
