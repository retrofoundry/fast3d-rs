use crate::scene::{ColorImage, DrawOrigin, DrawRun, Scene, SceneOp, Scissor};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TargetId {
    Legacy,
    Guest(u64),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Operation {
    pub draw: SceneOp,
    pub scissor: Scissor,
    pub pc: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TargetWorkload {
    pub id: TargetId,
    pub color_image: ColorImage,
    pub depth_image: Option<u64>,
    pub logical_extent: (u32, u32),
    pub depth_clear: bool,
    pub operations: Vec<Operation>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Workload {
    pub targets: Vec<TargetWorkload>,
}

impl Workload {
    pub fn new(scene: &Scene) -> Self {
        let triangles: Vec<_> = scene
            .draw_origins
            .iter()
            .filter(|origin| origin.rectangle.is_none())
            .collect();
        let rectangles: std::collections::HashMap<_, _> = scene
            .draw_origins
            .iter()
            .filter_map(|origin| origin.rectangle.map(|key| (key, origin)))
            .collect();
        let mut targets = Vec::new();
        if !scene.draw_runs.is_empty() {
            let mut operations = Vec::new();
            for run in &scene.draw_runs {
                push_triangles(&mut operations, &triangles, run, None);
            }
            targets.push(TargetWorkload {
                id: TargetId::Legacy,
                color_image: ColorImage::default(),
                depth_image: None,
                logical_extent: super::PAIRLESS_LOGICAL_EXTENT,
                depth_clear: false,
                operations,
            });
        }
        for (pair_index, pair) in scene.framebuffer_pairs.iter().enumerate() {
            let mut scissor = pair.active_scissor;
            let mut operations = Vec::new();
            for (op_index, draw) in pair.ops.iter().enumerate() {
                match draw {
                    SceneOp::SetScissor(value) => scissor = *value,
                    SceneOp::Tris(run) => {
                        push_triangles(&mut operations, &triangles, run, Some(scissor))
                    }
                    _ => {
                        let origin = rectangles.get(&(pair_index, op_index));
                        operations.push(Operation {
                            draw: draw.clone(),
                            scissor,
                            pc: origin.map(|origin| origin.pc),
                        });
                    }
                }
            }
            targets.push(TargetWorkload {
                id: TargetId::Guest(pair.color_image.addr),
                color_image: pair.color_image,
                depth_image: pair.depth_image,
                logical_extent: super::pair_render_extent(pair),
                depth_clear: pair.is_depth_clear,
                operations,
            });
        }
        Self { targets }
    }
}

fn legacy_scissor() -> Scissor {
    Scissor {
        lrx: 320,
        lry: 240,
        ..Scissor::default()
    }
}

fn push_triangles(
    operations: &mut Vec<Operation>,
    origins: &[&DrawOrigin],
    run: &DrawRun,
    scissor: Option<Scissor>,
) {
    let end = run.index_start + run.index_count;
    let mut start = run.index_start;
    let first = origins.partition_point(|origin| origin.indices.end <= start);
    for origin in origins[first..]
        .iter()
        .take_while(|origin| origin.indices.start < end)
    {
        if start < origin.indices.start {
            operations.push(Operation {
                draw: SceneOp::Tris(DrawRun {
                    index_start: start,
                    index_count: origin.indices.start - start,
                    ..*run
                }),
                scissor: scissor.unwrap_or_else(legacy_scissor),
                pc: None,
            });
            start = origin.indices.start;
        }
        let next = origin.indices.end.min(end);
        operations.push(Operation {
            draw: SceneOp::Tris(DrawRun {
                index_start: start,
                index_count: next - start,
                ..*run
            }),
            scissor: scissor.unwrap_or(origin.scissor),
            pc: Some(origin.pc),
        });
        start = next;
    }
    if start < end {
        operations.push(Operation {
            draw: SceneOp::Tris(DrawRun {
                index_start: start,
                index_count: end - start,
                ..*run
            }),
            scissor: scissor.unwrap_or_else(legacy_scissor),
            pc: None,
        });
    }
}

use super::{
    build_tex_entry, clamp_scissor, material_sampling, rect_quad, texrect_quad,
    triangle_inv_tex_size, CombinerUniform, OutVertex, SceneRenderer, CLEAR_COLOR,
};
use crate::ClearPolicy;
use wgpu::util::DeviceExt;

impl TargetWorkload {
    fn output_extent(&self, renderer: &SceneRenderer) -> (u32, u32) {
        match self.id {
            TargetId::Legacy => (renderer.fb_w, renderer.fb_h),
            TargetId::Guest(_) => self.logical_extent,
        }
    }

    fn uses_depth(&self, scene: &Scene) -> bool {
        self.depth_image.is_some()
            || self.id == TargetId::Legacy
                && self.operations.iter().any(|op| match &op.draw {
                    SceneOp::Tris(run) => {
                        let mode = &scene.render_modes[run.render_mode_index as usize];
                        mode.z_test || mode.z_write || mode.z_mode == crate::hle::ZMode::Decal
                    }
                    _ => false,
                })
    }
}

struct DrawUpload {
    uniforms: wgpu::BindGroup,
    rectangles: Option<wgpu::Buffer>,
    rect_indices: Vec<u32>,
}

impl SceneRenderer {
    fn upload_materials(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene) {
        self.tex_caches.truncate(scene.materials.len());
        for (i, mat) in scene.materials.iter().enumerate() {
            let rebuild = self.tex_caches.get(i).is_none_or(|cache| {
                cache.sampling != material_sampling(mat)
                    || cache.w != mat.tex_w
                    || cache.h != mat.tex_h
                    || cache.bytes != mat.texture
                    || cache.wrap_s != mat.wrap_s
                    || cache.wrap_t != mat.wrap_t
                    || cache.tex1 != mat.tex1
                    || cache.mip_levels != mat.mip_levels
                    || cache.detail_tex != mat.detail_tex
            });
            if rebuild {
                let entry = build_tex_entry(
                    device,
                    queue,
                    self.textured.bind_group_layout(),
                    &self.samplers,
                    &self.dummy_view,
                    mat,
                );
                if i < self.tex_caches.len() {
                    self.tex_caches[i] = entry;
                } else {
                    self.tex_caches.push(entry);
                }
            }
        }
    }

    fn upload_draws(
        &self,
        device: &wgpu::Device,
        scene: &Scene,
        target: &TargetWorkload,
    ) -> DrawUpload {
        let (w, h) = target.output_extent(self);
        let mut pool = vec![0; target.operations.len() * 256];
        let mut rectangles: Vec<OutVertex> = Vec::new();
        let mut rect_indices = Vec::new();
        for (slot, operation) in target.operations.iter().enumerate() {
            rect_indices.push((rectangles.len() / 6) as u32);
            let mut uniform = match &operation.draw {
                SceneOp::Tris(run) => {
                    let mat = &scene.materials[run.material_index as usize];
                    let mode = &scene.render_modes[run.render_mode_index as usize];
                    let mut uniform = CombinerUniform::from_run(mat, mode, run.fog_color);
                    uniform.inv_tex_size = triangle_inv_tex_size(mat);
                    uniform
                }
                SceneOp::TexRect {
                    rect,
                    uls,
                    ult,
                    dsdx,
                    dtdy,
                    flip,
                    copy_mode,
                    material_index,
                    render_mode_index,
                    fog_color,
                    ..
                } => {
                    let mat = &scene.materials[*material_index as usize];
                    let mut uniform = if *copy_mode {
                        CombinerUniform::tex_copy(
                            scene.render_modes.get(*render_mode_index as usize),
                            mat.fmt,
                        )
                    } else {
                        CombinerUniform::from_rect(
                            mat,
                            &scene.render_modes[*render_mode_index as usize],
                            *fog_color,
                        )
                    };
                    uniform.inv_tex_size = triangle_inv_tex_size(mat);
                    uniform.inv_tex_size[2] = 1.0;
                    rectangles.extend_from_slice(&texrect_quad(
                        rect,
                        (*uls, *ult),
                        (*dsdx, *dtdy),
                        *flip,
                        *copy_mode,
                        target.logical_extent,
                    ));
                    uniform
                }
                SceneOp::FillRect {
                    rect, color_raw, ..
                } => {
                    rectangles.extend_from_slice(&rect_quad(rect, w, h, [1.0; 4], [[0.0; 2]; 4]));
                    CombinerUniform::fill_rect(*color_raw, target.color_image.siz)
                }
                SceneOp::SetScissor(_) => unreachable!("scissor is normalized onto draws"),
            };
            // Fills do not dither.
            if !matches!(operation.draw, SceneOp::FillRect { .. }) {
                uniform.frame = [self.frame_serial as u32, self.dither_seed, w, h];
            }
            let bytes = bytemuck::bytes_of(&uniform);
            pool[slot * 256..slot * 256 + bytes.len()].copy_from_slice(bytes);
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("workload-uniforms"),
            contents: &pool,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let uniforms = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("workload-uniforms"),
            layout: self.textured_fb.uniform_bind_group_layout(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<CombinerUniform>() as u64),
                }),
            }],
        });
        let rectangles = (!rectangles.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("workload-rectangles"),
                contents: bytemuck::cast_slice(&rectangles),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
        DrawUpload {
            uniforms,
            rectangles,
            rect_indices,
        }
    }

    pub fn render_into_store(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
        clear_policy: ClearPolicy,
    ) -> Option<TargetId> {
        if (scene.draw_runs.is_empty() || scene.raw_pos.is_empty() || scene.indices.is_empty())
            && scene.framebuffer_pairs.is_empty()
        {
            return None;
        }
        let workload = Workload::new(scene);
        self.upload_materials(device, queue, scene);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("workload"),
        });
        let buffers = self
            .rsp
            .process_scene(device, &mut encoder, scene, &workload);
        let mut last_target = None;
        for target in &workload.targets {
            let (w, h) = target.output_extent(self);
            let any_depth = target.uses_depth(scene);
            let depth =
                (any_depth || target.depth_clear).then(|| Self::make_depth_view(device, w, h));
            if target.depth_clear {
                clear_depth(&mut encoder, &depth.as_ref().unwrap().0);
                continue;
            }
            let created = self.ensure_fb(device, target.id, w, h);
            let mut color_load = self.fb_clear_op(target.id, created, clear_policy);
            last_target = Some(target.id);
            if target.operations.is_empty() {
                clear_color(
                    &mut encoder,
                    &self.framebuffers[&target.id].attach,
                    color_load,
                );
                continue;
            }
            let upload = self.upload_draws(device, scene, target);
            let material_bgs: Vec<_> = self
                .tex_caches
                .iter()
                .map(|cache| &cache.bind_group)
                .collect();
            let depth_bg = depth.as_ref().map(|(_, sampled)| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("workload-depth"),
                    layout: self.textured_fb.depth_bind_group_layout(),
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(sampled),
                    }],
                })
            });
            let is_decal = |operation: &Operation| {
                any_depth
                    && matches!(&operation.draw, SceneOp::Tris(run) if scene.render_modes[run.render_mode_index as usize].z_mode == crate::hle::ZMode::Decal)
            };
            let mut depth_initialized = false;
            let mut start = 0;
            while start < target.operations.len() {
                let read_depth = is_decal(&target.operations[start]);
                let end = start
                    + target.operations[start..]
                        .iter()
                        .take_while(|op| is_decal(op) == read_depth)
                        .count();
                if read_depth && !depth_initialized {
                    clear_depth(&mut encoder, &depth.as_ref().unwrap().0);
                    depth_initialized = true;
                }
                let attachment = depth.as_ref().filter(|_| !read_depth).map(|(view, _)| {
                    wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations {
                            load: if depth_initialized {
                                wgpu::LoadOp::Load
                            } else {
                                wgpu::LoadOp::Clear(1.0)
                            },
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("workload-segment"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.framebuffers[&target.id].attach,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: color_load,
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: attachment,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                color_load = wgpu::LoadOp::Load;
                depth_initialized |= any_depth && !read_depth;
                if let Some(index) = &buffers.indices {
                    pass.set_index_buffer(index.slice(..), wgpu::IndexFormat::Uint32);
                }
                if read_depth {
                    pass.set_bind_group(2, depth_bg.as_ref().unwrap(), &[]);
                }
                for (slot, operation) in target.operations.iter().enumerate().take(end).skip(start)
                {
                    let scissor = output_scissor(operation.scissor, target.logical_extent, (w, h));
                    let (x, y, width, height) = clamp_scissor(&scissor, w, h);
                    if width == 0 || height == 0 {
                        continue;
                    }
                    pass.set_scissor_rect(x, y, width, height);
                    match &operation.draw {
                        SceneOp::Tris(run) => {
                            let mode = &scene.render_modes[run.render_mode_index as usize];
                            let pipeline = if read_depth {
                                self.textured_fb.select_decal(
                                    run.cull,
                                    mode.fallback_class,
                                    mode.blend_class,
                                )
                            } else {
                                let test = any_depth && mode.z_test;
                                let write = any_depth && mode.z_write;
                                match &self.textured_fb.dual {
                                    Some(dual)
                                        if mode.blend_class == crate::hle::BlendClass::DualSrc =>
                                    {
                                        dual.select(run.cull, test, write, any_depth)
                                    }
                                    _ => self.textured_fb.select(
                                        run.cull,
                                        test,
                                        write,
                                        any_depth,
                                        mode.fallback_class,
                                    ),
                                }
                            };
                            pass.set_pipeline(pipeline);
                            if let Some(vertices) = buffers.vertices.get(&target.logical_extent) {
                                pass.set_vertex_buffer(0, vertices.slice(..));
                            }
                            pass.set_bind_group(0, material_bgs[run.material_index as usize], &[]);
                            pass.set_bind_group(1, &upload.uniforms, &[(slot * 256) as u32]);
                            pass.draw_indexed(
                                run.index_start..run.index_start + run.index_count,
                                0,
                                0..1,
                            );
                        }
                        SceneOp::FillRect { .. } | SceneOp::TexRect { .. } => {
                            self.draw_rect_op(
                                &mut pass,
                                device,
                                target,
                                &operation.draw,
                                scene,
                                &material_bgs,
                                upload.rectangles.as_ref(),
                                Some(&upload.uniforms),
                                slot as u32,
                                upload.rect_indices[slot],
                                any_depth,
                            );
                        }
                        SceneOp::SetScissor(_) => unreachable!("scissor is normalized onto draws"),
                    }
                }
                start = end;
            }
        }
        queue.submit(Some(encoder.finish()));
        last_target
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
        target: &wgpu::TextureView,
    ) {
        // Internal renders are independent frames; retain the dither frame chosen by the caller.
        self.first_touch.clear();
        let source = self.render_into_store(device, queue, scene, ClearPolicy::PerFrame);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("workload-present"),
        });
        if let Some(source) = source {
            self.scanout(&mut encoder, target, source);
        } else {
            clear_color(&mut encoder, target, wgpu::LoadOp::Clear(CLEAR_COLOR));
        }
        queue.submit(Some(encoder.finish()));
    }
}

fn clear_depth(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("workload-depth-init"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
}

fn output_scissor(scissor: Scissor, logical: (u32, u32), output: (u32, u32)) -> Scissor {
    let scale = |value: i32, from: u32, to: u32| {
        (i64::from(value) * i64::from(to) / i64::from(from)) as i32
    };
    Scissor {
        ulx: scale(scissor.ulx, logical.0, output.0),
        uly: scale(scissor.uly, logical.1, output.1),
        lrx: scale(scissor.lrx, logical.0, output.0),
        lry: scale(scissor.lry, logical.1, output.1),
        mode: scissor.mode,
    }
}

impl From<u64> for TargetId {
    fn from(address: u64) -> Self {
        Self::Guest(address)
    }
}

fn clear_color(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("workload-clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
}
