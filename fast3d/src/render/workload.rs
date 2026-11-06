use crate::scene::{ColorImage, DrawOrigin, DrawRun, Scene, SceneOp, Scissor};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TargetId {
    Legacy,
    Guest(u64),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Operation {
    pub draw: SceneOp,
    pub depth_image: Option<u64>,
    pub scissor: Scissor,
    pub presentation_scissor: Scissor,
    /// First command in this operation; individual command spans remain in `Scene::draw_origins`.
    pub pc: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TargetWorkload {
    pub color_image_epoch: u64,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UnsupportedGuestTask;

impl Workload {
    pub(crate) fn check_strict_guest(
        &self,
        scene: &Scene,
        coherence: &super::framebuffers::coherence::Coherence,
    ) -> Result<(), UnsupportedGuestTask> {
        let [target] = self.targets.as_slice() else {
            return Err(UnsupportedGuestTask);
        };
        let TargetId::Guest(address) = target.id else {
            return Err(UnsupportedGuestTask);
        };
        if target.color_image.fmt != 0
            || target.color_image.siz != 2
            || address & 1 != 0
            || target.depth_clear
        {
            return Err(UnsupportedGuestTask);
        }
        let (width, height) = target.logical_extent;
        let has_triangle = target
            .operations
            .iter()
            .any(|op| matches!(op.draw, SceneOp::Tris(_)));
        if has_triangle && (!width.is_power_of_two() || !height.is_power_of_two()) {
            return Err(UnsupportedGuestTask);
        }
        let mut known: Vec<std::ops::Range<u64>> = Vec::new();
        for operation in &target.operations {
            if operation.scissor.mode != 0 {
                return Err(UnsupportedGuestTask);
            }
            let (rect, needs_memory) = match &operation.draw {
                SceneOp::FillRect { rect, .. } => (
                    [
                        rect.ulx,
                        rect.uly,
                        rect.lrx.saturating_add(1),
                        rect.lry.saturating_add(1),
                    ],
                    false,
                ),
                SceneOp::TexRect {
                    rect,
                    copy_mode: false,
                    material_index,
                    render_mode_index,
                    fog_color,
                    fb_source: None,
                    ..
                } => {
                    if has_triangle {
                        return Err(UnsupportedGuestTask);
                    }
                    let mode = scene.render_modes[*render_mode_index as usize];
                    let material = &scene.materials[*material_index as usize];
                    let uniform = super::CombinerUniform::from_run(material, &mode, *fog_color);
                    let combiner = super::framebuffers::writeback::Combiner::new(&uniform, mode)
                        .ok_or(UnsupportedGuestTask)?;
                    let color = combiner.color([1.0; 4]).ok_or(UnsupportedGuestTask)?;
                    if mode.rgb_dither != 0
                        || mode.alpha_dither != 3
                        || mode.key_enabled
                        || mode.force_blend
                        || mode.blender_mux != 0
                        || color[3] != 1.0
                        || color[..3].iter().any(|&c| c != 0.0 && c != 1.0)
                        || [rect.ulx, rect.uly, rect.lrx, rect.lry]
                            .iter()
                            .any(|x| x & 3 != 0)
                    {
                        return Err(UnsupportedGuestTask);
                    }
                    (
                        [rect.ulx / 4, rect.uly / 4, rect.lrx / 4, rect.lry / 4],
                        mode.image_read && matches!(mode.cvg_dst, 1 | 3),
                    )
                }
                SceneOp::Tris(run) => {
                    let mode = scene.render_modes[run.render_mode_index as usize];
                    let material = &scene.materials[run.material_index as usize];
                    let uniform = super::CombinerUniform::from_run(material, &mode, run.fog_color);
                    let combiner = super::framebuffers::writeback::Combiner::new(&uniform, mode)
                        .ok_or(UnsupportedGuestTask)?;
                    if combiner.strict_color().is_none()
                        || mode.rgb_dither != 0
                        || mode.alpha_dither != 3
                        || mode.key_enabled
                        || mode.force_blend
                        || mode.blender_mux != 0
                    {
                        return Err(UnsupportedGuestTask);
                    }
                    if mode.image_read && mode.cvg_dst != 2 {
                        let s = operation.scissor;
                        for row in s.uly.max(0) as u32..(s.lry.max(0) as u32).min(height) {
                            let left = (s.ulx.max(0) as u32).min(width);
                            let right = (s.lrx.max(0) as u32).min(width);
                            let start =
                                address + (u64::from(row) * u64::from(width) + u64::from(left)) * 2;
                            let end = address
                                + (u64::from(row) * u64::from(width) + u64::from(right)) * 2;
                            if (start..end)
                                .step_by(2)
                                .any(|p| !known.iter().any(|range| range.contains(&p)))
                            {
                                return Err(UnsupportedGuestTask);
                            }
                        }
                    }
                    continue;
                }
                _ => return Err(UnsupportedGuestTask),
            };
            let left = rect[0].max(operation.scissor.ulx).max(0) as u32;
            let top = rect[1].max(operation.scissor.uly).max(0) as u32;
            let right = rect[2].min(operation.scissor.lrx).max(0) as u32;
            let bottom = rect[3].min(operation.scissor.lry).max(0) as u32;
            for y in top..bottom.min(height) {
                let start = address
                    .checked_add((u64::from(y) * u64::from(width) + u64::from(left)) * 2)
                    .ok_or(UnsupportedGuestTask)?;
                let end = address
                    .checked_add(
                        (u64::from(y) * u64::from(width) + u64::from(right.min(width))) * 2,
                    )
                    .ok_or(UnsupportedGuestTask)?;
                if start >= end {
                    continue;
                }
                if needs_memory
                    && !known
                        .iter()
                        .any(|range| range.start <= start && range.end >= end)
                    && (start..end)
                        .step_by(2)
                        .any(|address| coherence.stored_coverage_code(address).is_none())
                {
                    return Err(UnsupportedGuestTask);
                }
                known.push(start..end);
            }
        }
        Ok(())
    }

    pub(crate) fn check_writeback(
        &self,
        scene: &Scene,
        memory: &impl crate::Rdram,
    ) -> Result<(), crate::MemoryError> {
        for target in &self.targets {
            if target
                .operations
                .iter()
                .all(|op| matches!(op.draw, SceneOp::FramebufferCopy(_)))
            {
                continue;
            }
            let (width, height) = target.logical_extent;
            if !target.depth_clear {
                if let TargetId::Guest(address) = target.id {
                    let layout = ImageLayout {
                        width,
                        fmt: target.color_image.fmt,
                        siz: target.color_image.siz,
                    };
                    memory.check_write(address, layout.row_bytes().unwrap() * u64::from(height))?;
                }
            }
            if let Some(address) = target.depth_image.filter(|_| target.uses_depth(scene)) {
                memory.check_write(address, u64::from(width) * u64::from(height) * 2)?;
            }
        }
        Ok(())
    }

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
        let mut targets: Vec<TargetWorkload> = Vec::new();
        if !scene.draw_runs.is_empty() {
            let mut operations = Vec::new();
            for run in &scene.draw_runs {
                push_triangles(&mut operations, 0, &triangles, run, None);
            }
            for operation in operations {
                match targets.last_mut() {
                    Some(target) if target.depth_image == operation.depth_image => {
                        target.operations.push(operation);
                    }
                    _ => targets.push(TargetWorkload {
                        color_image_epoch: 0,
                        id: TargetId::Legacy,
                        color_image: ColorImage::default(),
                        depth_image: operation.depth_image,
                        logical_extent: super::PAIRLESS_LOGICAL_EXTENT,
                        depth_clear: false,
                        operations: vec![operation],
                    }),
                }
            }
        }
        for (pair_index, pair) in scene.framebuffer_pairs.iter().enumerate() {
            let mut scissor = pair.active_scissor;
            let mut operations = Vec::new();
            let mut batch_start = 0;
            for (op_index, draw) in pair.ops.iter().enumerate() {
                match draw {
                    SceneOp::SetScissor(value) => {
                        scissor = *value;
                        batch_start = operations.len();
                    }
                    SceneOp::Tris(run) => {
                        push_triangles(&mut operations, batch_start, &triangles, run, Some(scissor))
                    }
                    _ => {
                        let origin = rectangles.get(&(pair_index, op_index));
                        operations.push(Operation {
                            draw: draw.clone(),
                            depth_image: pair.depth_image,
                            scissor,
                            presentation_scissor: origin
                                .map_or(scissor, |origin| origin.presentation_scissor),
                            pc: origin.map(|origin| origin.pc),
                        });
                    }
                }
            }
            let mut target = TargetWorkload {
                color_image_epoch: pair.color_image_epoch,
                id: TargetId::Guest(pair.color_image.addr),
                color_image: pair.color_image,
                depth_image: pair.depth_image,
                logical_extent: super::pair_render_extent(pair),
                depth_clear: pair.is_depth_clear,
                operations: Vec::new(),
            };
            for operation in operations {
                let height = operation.scissor.lry.max(0) as u32;
                if height > target.logical_extent.1 {
                    if !target.operations.is_empty() {
                        targets.push(target.clone());
                        target.operations.clear();
                    }
                    target.logical_extent.1 = height;
                }
                target.operations.push(operation);
            }
            targets.push(target);
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
    batch_start: usize,
    origins: &[&DrawOrigin],
    run: &DrawRun,
    scissor: Option<Scissor>,
) {
    let mut push =
        |start, end, scissor, presentation_scissor, pc: Option<u64>, depth_image: Option<u64>| {
            let draw = DrawRun {
                index_start: start,
                index_count: end - start,
                ..*run
            };
            match operations[batch_start..].last_mut() {
                Some(Operation {
                    draw: SceneOp::Tris(previous),
                    scissor: previous_scissor,
                    presentation_scissor: previous_presentation_scissor,
                    pc: previous_pc,
                    depth_image: previous_depth,
                }) if previous.index_start + previous.index_count == start
                    && *previous_scissor == scissor
                    && *previous_presentation_scissor == presentation_scissor
                    && previous_pc.is_some() == pc.is_some()
                    && *previous_depth == depth_image
                    && *previous
                        == (DrawRun {
                            index_start: previous.index_start,
                            index_count: previous.index_count,
                            ..draw
                        }) =>
                {
                    previous.index_count += draw.index_count;
                }
                _ => operations.push(Operation {
                    draw: SceneOp::Tris(draw),
                    depth_image,
                    scissor,
                    presentation_scissor,
                    pc,
                }),
            }
        };
    let end = run.index_start + run.index_count;
    let mut start = run.index_start;
    let first = origins.partition_point(|origin| origin.indices.end <= start);
    for origin in origins[first..]
        .iter()
        .take_while(|origin| origin.indices.start < end)
    {
        if start < origin.indices.start {
            push(
                start,
                origin.indices.start,
                scissor.unwrap_or_else(legacy_scissor),
                scissor.unwrap_or_else(legacy_scissor),
                None,
                None,
            );
            start = origin.indices.start;
        }
        let next = origin.indices.end.min(end);
        push(
            start,
            next,
            scissor.unwrap_or(origin.scissor),
            origin.presentation_scissor,
            Some(origin.pc),
            origin.depth_image,
        );
        start = next;
    }
    if start < end {
        push(
            start,
            end,
            scissor.unwrap_or_else(legacy_scissor),
            scissor.unwrap_or_else(legacy_scissor),
            None,
            None,
        );
    }
}

use super::framebuffers::{ImageLayout, ImageRequest};
use super::inputs::{DrawInputs, OperationInputs, RenderInputs, TargetInputs, TextureInputs};
use super::{CombinerUniform, SceneRenderer, CLEAR_COLOR};
use crate::ClearPolicy;

impl TargetWorkload {
    pub(super) fn uses_depth(&self, scene: &Scene) -> bool {
        self.depth_clear
            || self.operations.iter().any(|op| {
                let index = match &op.draw {
                    SceneOp::Tris(run) => run.render_mode_index,
                    SceneOp::TexRect {
                        render_mode_index,
                        copy_mode: false,
                        ..
                    } => *render_mode_index,
                    _ => return false,
                };
                let mode = &scene.render_modes[index as usize];
                mode.z_test || mode.z_write
            })
    }
}

struct DrawUpload {
    uniforms: wgpu::BindGroup,
    rectangles: Option<wgpu::Buffer>,
}

impl SceneRenderer {
    pub(super) fn upload_materials(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        textures: &[TextureInputs],
    ) {
        self.texture_resources.collect(&self.profiling);
        let previous: Vec<_> = std::mem::take(&mut self.material_bindings)
            .into_iter()
            .enumerate()
            .map(|(index, binding)| {
                textures
                    .get(index)
                    .filter(|input| {
                        binding.may_reuse(input, &self.texture_resources, &self.profiling)
                    })
                    .map(|_| binding)
            })
            .collect();
        let mut previous = previous.into_iter();
        let mut builder = super::texture_bindings::BindingBuilder {
            device,
            queue,
            layout: self.textured.bind_group_layout(),
            samplers: &self.samplers,
            dummy: &self.dummy_view,
            resources: &mut self.texture_resources,
            copies: &mut self.framebuffer_copies,
            profiling: &self.profiling,
            presentation: false,
        };
        self.material_bindings = textures
            .iter()
            .map(|texture| builder.build(texture, previous.next().flatten()))
            .collect();
        self.presentation_bindings.clear();
        if self.render_scale > 1 {
            builder.presentation = true;
            self.presentation_bindings = textures
                .iter()
                .zip(&self.material_bindings)
                .map(|(texture, native)| {
                    if std::iter::once(&texture.texture)
                        .chain(texture.tex1.iter())
                        .chain(texture.mip_levels.iter())
                        .chain(texture.detail_tex.iter())
                        .any(|binding| {
                            matches!(
                                binding.source,
                                crate::hle::texture_request::TextureSource::Framebuffer(_)
                            )
                        })
                    {
                        builder.build(texture, None)
                    } else {
                        native.clone()
                    }
                })
                .collect();
        }
    }

    fn upload_draws(&self, device: &wgpu::Device, target: &TargetInputs) -> DrawUpload {
        let _span = self.profiling.span("resources");
        let mut uniform_bytes = std::borrow::Cow::Borrowed(target.uniforms.as_slice());
        if self.render_scale > 1 {
            let mut presentation = target.uniforms.clone();
            for (index, slot) in presentation.as_chunks_mut::<256>().0.iter_mut().enumerate() {
                let len = std::mem::size_of::<CombinerUniform>();
                let mut uniform: CombinerUniform = bytemuck::pod_read_unaligned(&slot[..len]);
                uniform.frame[2..].copy_from_slice(&[
                    target.extents.presentation.0,
                    target.extents.presentation.1,
                ]);
                uniform.raster[0] = self.render_scale;
                uniform.raster[1] = target.extents.native.0;
                uniform.raster[3] = match target.operations[index].draw {
                    DrawInputs::Tris { material_index, .. }
                    | DrawInputs::Rectangle {
                        material_index: Some(material_index),
                        fb_source: None,
                        ..
                    } => self.presentation_bindings[material_index as usize].high_resolution_mask,
                    DrawInputs::Rectangle {
                        fb_source: Some(_), ..
                    } => 1,
                    _ => 0,
                };
                uniform.lod_params[3] = self.render_scale as f32;
                slot[..len].copy_from_slice(bytemuck::bytes_of(&uniform));
            }
            uniform_bytes.to_mut().extend_from_slice(&presentation);
        }
        let buffer = super::buffer_init(
            device,
            &self.profiling,
            "uniforms",
            &wgpu::util::BufferInitDescriptor {
                label: Some("workload-uniforms"),
                contents: &uniform_bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
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
        let rectangles = (!target.rectangles.is_empty()).then(|| {
            super::buffer_init(
                device,
                &self.profiling,
                "rectangles",
                &wgpu::util::BufferInitDescriptor {
                    label: Some("workload-rectangles"),
                    contents: bytemuck::cast_slice(&target.rectangles),
                    usage: wgpu::BufferUsages::VERTEX,
                },
            )
        });
        DrawUpload {
            uniforms,
            rectangles,
        }
    }

    pub fn render_into_store(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
        clear_policy: ClearPolicy,
    ) -> Option<TargetId> {
        self.diagnostics.clear();
        self.dropped_runs = 0;
        let inputs = match self.prepare_inputs(device, scene) {
            Ok(Some(inputs)) => inputs,
            Ok(None) => return None,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return None;
            }
        };
        self.render_prepared(device, queue, &inputs, clear_policy)
    }

    pub(crate) fn prepare_inputs<'a>(
        &mut self,
        device: &wgpu::Device,
        scene: &'a Scene,
    ) -> Result<Option<RenderInputs<'a>>, crate::Diagnostic> {
        let _preparation = self.profiling.span("render_inputs");
        let Some(mut inputs) = RenderInputs::with_targets(
            scene,
            (self.fb_w, self.fb_h),
            [self.frame_serial as u32, self.dither_seed],
            &self.target_descriptors,
        ) else {
            return Ok(None);
        };
        self.preflight_inputs(&mut inputs, &device.limits())?;
        Ok(Some(inputs))
    }

    pub(crate) fn render_prepared(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        inputs: &RenderInputs<'_>,
        clear_policy: ClearPolicy,
    ) -> Option<TargetId> {
        let result = self.render_inputs(device, queue, inputs, clear_policy);
        self.diagnostics.extend_from_slice(&inputs.diagnostics);
        self.dropped_runs += inputs.dropped_runs;
        result
    }

    fn render_inputs(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        inputs: &RenderInputs<'_>,
        clear_policy: ClearPolicy,
    ) -> Option<TargetId> {
        self.diagnostics.clear();
        let encoding = self.profiling.span("encoding");
        self.framebuffer_copies.begin(
            device,
            inputs
                .targets
                .iter()
                .flat_map(|target| &target.operations)
                .filter_map(|op| match &op.draw {
                    DrawInputs::FramebufferCopy(copy) => Some(copy.clone()),
                    _ => None,
                }),
            self.render_scale,
        );
        self.upload_materials(device, queue, &inputs.textures);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("workload"),
        });
        self.texture_resources.encode(&mut encoder, &self.profiling);
        self.framebuffer_copies
            .encode(&mut encoder, 0, &self.profiling);
        let buffers = self.rsp.process_scene(
            device,
            &mut encoder,
            inputs.rsp.as_ref(),
            &inputs.targets,
            &self.profiling,
        );
        let mut last_target = None;
        let mut imported = std::collections::HashSet::new();
        for target in &inputs.targets {
            if !target.operations.is_empty()
                && target
                    .operations
                    .iter()
                    .all(|op| matches!(op.draw, DrawInputs::FramebufferCopy(_)))
            {
                for op in &target.operations {
                    if let DrawInputs::FramebufferCopy(copy) = &op.draw {
                        self.framebuffer_copies.copy(
                            &mut encoder,
                            copy,
                            &self.framebuffers,
                            &self.color_versions,
                            &self.presentation_versions,
                            self.coherence.as_ref().map(|c| &c.owners),
                            &self.profiling,
                        );
                    }
                }
                continue;
            }

            self.profiling
                .target(target.extents.logical, target.extents.presentation);
            #[cfg(feature = "capture")]
            if self.depth_reset_policy == crate::DepthResetPolicy::ColorImageSwitch
                && self.depth_color_epoch != Some(target.color_image_epoch)
            {
                self.discard_depth();
                self.depth_color_epoch = Some(target.color_image_epoch);
            }
            match target.record_descriptors(&mut self.target_descriptors) {
                Ok(true) => {}
                Ok(false) => {
                    self.dropped_runs += target.operations.len() as u32;
                    continue;
                }
                Err(diagnostic) => {
                    self.diagnostics.push(diagnostic);
                    continue;
                }
            }
            let (native_w, native_h) = target.extents.native;
            let any_depth = target.any_depth;
            let depth_id = target
                .depth_image
                .filter(|_| any_depth)
                .map(TargetId::Guest)
                .or_else(|| any_depth.then_some(TargetId::Legacy));
            let color_layout = ImageLayout {
                width: native_w,
                fmt: target.color_image.fmt,
                siz: target.color_image.siz,
            };
            let depth_layout = ImageLayout {
                width: native_w,
                fmt: 0,
                siz: 2,
            };
            let mut native_allocation_height = native_h;
            if !target.depth_clear {
                if let Some(old) = self.framebuffers.get(&target.id).filter(|old| {
                    old.layout == color_layout
                        && (target.id != TargetId::Legacy || old.height == native_h)
                }) {
                    native_allocation_height = native_allocation_height.max(old.color.height());
                }
            }
            if let Some(old) = depth_id
                .and_then(|id| self.depthbuffers.get(&id))
                .filter(|old| {
                    old.layout == depth_layout
                        && (depth_id != Some(TargetId::Legacy) || old.texture.height() == native_h)
                })
            {
                native_allocation_height = native_allocation_height.max(old.texture.height());
            }
            let native_allocation = (native_w, native_allocation_height);
            let color_restore_from = self
                .framebuffers
                .get(&target.id)
                .filter(|old| old.layout == color_layout)
                .map_or(Some(0), |old| {
                    (old.color.height() < native_allocation_height).then_some(old.color.height())
                });
            let depth_restore_from = depth_id
                .and_then(|id| self.depthbuffers.get(&id))
                .filter(|old| old.layout == depth_layout)
                .map_or(Some(0), |old| {
                    (old.texture.height() < native_allocation_height)
                        .then_some(old.texture.height())
                });
            let color_initialize =
                clear_policy == ClearPolicy::PerFrame && !self.first_touch.contains(&target.id);
            let color_needs_import = color_restore_from.is_some() || color_initialize;
            let depth_initialize = clear_policy == ClearPolicy::PerFrame
                && depth_id.is_some_and(|id| !self.depth_first_touch.contains(&id));
            let depth_needs_import = depth_restore_from.is_some() || depth_initialize;
            #[cfg(not(target_arch = "wasm32"))]
            if self.coherence.as_ref().is_some_and(|coherence| {
                use super::framebuffers::coherence::Writer;
                let color = match target.id {
                    TargetId::Guest(address) if color_restore_from.is_some() => Some(Writer {
                        address,
                        layout: color_layout,
                        depth: false,
                    }),
                    _ => None,
                };
                let depth = match depth_id {
                    Some(TargetId::Guest(address)) if depth_restore_from.is_some() => {
                        Some(Writer {
                            address,
                            layout: depth_layout,
                            depth: true,
                        })
                    }
                    _ => None,
                };
                color
                    .into_iter()
                    .chain(depth)
                    .any(|writer| coherence.pending_overlaps(writer, native_allocation_height))
            }) {
                queue.submit([encoder.finish()]);
                self.profiling.count("submissions", 1);
                self.coherence.as_mut().unwrap().resolve(device);
                encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("workload"),
                });
            }
            if let Some(id) = depth_id {
                self.ensure_depth(
                    device,
                    &mut encoder,
                    ImageRequest {
                        id,
                        layout: depth_layout,
                        height: native_allocation_height,
                        pc: target.pc,
                    },
                    native_allocation,
                    clear_policy,
                );
            }
            if self.coherence.is_some() {
                if let Some(TargetId::Guest(address)) = depth_id {
                    let writer = super::framebuffers::coherence::Writer {
                        address,
                        layout: depth_layout,
                        depth: true,
                    };
                    if (imported.insert(writer) || depth_restore_from.is_some())
                        && depth_needs_import
                    {
                        if depth_initialize && depth_restore_from.is_some() {
                            self.import_target(device, &mut encoder, writer, None);
                        }
                        let uploaded =
                            self.import_target(device, &mut encoder, writer, depth_restore_from);
                        if uploaded && depth_restore_from.is_some() {
                            if let Some(coherence) = &mut self.coherence {
                                coherence.imported = true;
                            }
                        }
                    }
                }
            }
            if target.depth_clear {
                let depth = &self.depthbuffers[&depth_id.unwrap()];
                for (slot, operation) in target.operations.iter().enumerate() {
                    if let DrawInputs::DepthFill { word } = operation.draw {
                        if let (Some(coherence), Some(TargetId::Guest(address))) =
                            (&mut self.coherence, depth_id)
                        {
                            coherence.fill(
                                super::framebuffers::coherence::Writer {
                                    address,
                                    layout: depth_layout,
                                    depth: true,
                                },
                                operation.scissor,
                                word,
                            );
                        }
                        self.depth_pipeline.fill(
                            device,
                            &mut encoder,
                            depth,
                            word,
                            1,
                            operation.scissor,
                            &self.profiling,
                        );
                        if let Some(presentation) = &depth.presentation {
                            self.depth_pipeline.fill(
                                device,
                                &mut encoder,
                                presentation,
                                word,
                                self.render_scale,
                                target.presentation_scissor(slot),
                                &self.profiling,
                            );
                        }
                    } else if let DrawInputs::FramebufferCopy(copy) = &operation.draw {
                        #[cfg(not(target_arch = "wasm32"))]
                        if let Some(coherence) = &mut self.coherence {
                            queue.submit([encoder.finish()]);
                            self.profiling.count("submissions", 1);
                            coherence.resolve(device);
                            encoder =
                                device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                    label: Some("framebuffer-load"),
                                });
                        }
                        self.framebuffer_copies.copy(
                            &mut encoder,
                            copy,
                            &self.framebuffers,
                            &self.color_versions,
                            &self.presentation_versions,
                            self.coherence.as_ref().map(|c| &c.owners),
                            &self.profiling,
                        );
                    } else {
                        self.diagnostics.push(crate::Diagnostic {
                            at: operation.pc,
                            kind: crate::DiagKind::UnsupportedDepthAlias {
                                address: target.color_image.addr,
                            },
                        });
                    }
                }
                if let Some(TargetId::Guest(address)) = depth_id {
                    self.target_descriptors.written(address, true);
                }
                continue;
            }
            self.ensure_color(
                device,
                &mut encoder,
                ImageRequest {
                    id: target.id,
                    layout: color_layout,
                    height: native_h,
                    pc: target.pc,
                },
                native_allocation,
                clear_policy,
            );
            if self.coherence.is_some() {
                if let TargetId::Guest(address) = target.id {
                    let writer = super::framebuffers::coherence::Writer {
                        address,
                        layout: color_layout,
                        depth: false,
                    };
                    let first = imported.insert(writer);
                    if first || color_restore_from.is_some() {
                        if color_needs_import {
                            if color_initialize && color_restore_from.is_some() {
                                self.import_target(device, &mut encoder, writer, None);
                            }
                            let uploaded = self.import_target(
                                device,
                                &mut encoder,
                                writer,
                                color_restore_from,
                            );
                            if uploaded && color_restore_from.is_some() {
                                if let Some(coherence) = &mut self.coherence {
                                    coherence.imported = true;
                                }
                            }
                        }
                        if first && !writer.supported() {
                            self.diagnostics.push(crate::Diagnostic {
                                at: target.pc,
                                kind: crate::DiagKind::UnsupportedGuestWriteback {
                                    address,
                                    fmt: color_layout.fmt,
                                    siz: color_layout.siz,
                                },
                            });
                        }
                    }
                }
            }
            let mut source_valid: Vec<_> = target
                .operations
                .iter()
                .map(|operation| {
                    let DrawInputs::Rectangle {
                        fb_source: Some(source),
                        ..
                    } = operation.draw
                    else {
                        return true;
                    };
                    let valid = self.target_descriptors.get(source.address, false) == Some(source)
                        && self
                            .framebuffers
                            .get(&TargetId::Guest(source.address))
                            .is_some_and(|stored| {
                                stored.layout == source.layout && stored.height == source.height
                            })
                        && target.id != TargetId::Guest(source.address);
                    if !valid {
                        self.diagnostics.push(crate::Diagnostic {
                            at: operation.pc,
                            kind: crate::DiagKind::UnsupportedFramebufferAccess {
                                address: source.address,
                                reason: crate::FramebufferAccess::MissingSource,
                            },
                        });
                        self.dropped_runs += 1;
                    }
                    valid
                })
                .collect();
            let coverage_texture = self.coherence.as_ref().map(|_| {
                super::framebuffers::coherence::coverage_texture(
                    device,
                    native_w,
                    native_allocation_height,
                )
            });
            let coverage_view = coverage_texture
                .as_ref()
                .map(|texture| texture.create_view(&Default::default()));
            if let Some(view) = &coverage_view {
                clear_color(
                    &mut encoder,
                    view,
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                );
            }
            let depth = depth_id.map(|id| &self.depthbuffers[&id]);
            let mut color_load = wgpu::LoadOp::Load;
            last_target = Some(target.id);
            if target.operations.is_empty() {
                clear_color(
                    &mut encoder,
                    &self.framebuffers[&target.id].attach,
                    color_load,
                );
                continue;
            }
            let upload = self.upload_draws(device, target);
            let material_bgs: Vec<_> = self
                .material_bindings
                .iter()
                .map(|cache| &cache.bind_group)
                .collect();
            let presentation_bgs: Vec<_> = self
                .presentation_bindings
                .iter()
                .map(|cache| &cache.bind_group)
                .collect();
            let depth_bg = depth.map(|depth| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("workload-depth"),
                    layout: self.textured_fb.depth_bind_group_layout(),
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&depth.attach),
                    }],
                })
            });
            let presentation_depth_bg =
                depth
                    .and_then(|depth| depth.presentation.as_deref())
                    .map(|depth| {
                        device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("presentation-depth"),
                            layout: self.textured_fb.depth_bind_group_layout(),
                            entries: &[wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&depth.attach),
                            }],
                        })
                    });
            let mut start = 0;
            #[cfg(not(target_arch = "wasm32"))]
            let mut ownership_start = 0;
            #[cfg(target_arch = "wasm32")]
            let ownership_start = 0;
            while start < target.operations.len() {
                if let DrawInputs::FramebufferCopy(copy) = &target.operations[start].draw {
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(coherence) = &mut self.coherence {
                        use super::framebuffers::coherence::Writer;
                        let color = match target.id {
                            TargetId::Guest(address) => Some(Writer {
                                address,
                                layout: color_layout,
                                depth: false,
                            }),
                            TargetId::Legacy => None,
                        };
                        let touches = |writer: Writer| {
                            copy.spans.iter().any(|span| {
                                let address = copy.source.address
                                    + u64::from(
                                        span.origin[1] * copy.source.layout.width + span.origin[0],
                                    ) * 2;
                                let end = address + u64::from(span.width) * 2;
                                target.operations[ownership_start..start]
                                    .iter()
                                    .enumerate()
                                    .any(|(index, op)| {
                                        if matches!(
                                            op.draw,
                                            DrawInputs::FramebufferCopy(_) | DrawInputs::Rejected
                                        ) {
                                            return false;
                                        }
                                        let (x, y, width, height) =
                                            target.write_bounds(ownership_start + index);
                                        let first = writer.address
                                            + u64::from(y * writer.layout.width + x)
                                                * writer.pixel_bytes() as u64;
                                        let last = writer.address
                                            + (u64::from(
                                                (y + height.saturating_sub(1))
                                                    * writer.layout.width
                                                    + x
                                                    + width,
                                            )) * writer.pixel_bytes() as u64;
                                        first < end && address < last
                                    })
                            })
                        };
                        let depth_writer = depth_id.and_then(|id| match id {
                            TargetId::Guest(address) => Some(Writer {
                                address,
                                layout: depth_layout,
                                depth: true,
                            }),
                            _ => None,
                        });
                        if color.is_some_and(touches) || depth_writer.is_some_and(touches) {
                            coherence.snapshot(
                                device,
                                &mut encoder,
                                coverage_texture.as_ref().unwrap(),
                                color.map(|writer| (writer, &self.framebuffers[&target.id].color)),
                                depth_writer.map(|writer| {
                                    (writer, &self.depthbuffers[&depth_id.unwrap()].texture)
                                }),
                                target.coverage_draws(
                                    &source_valid,
                                    start,
                                    inputs.rsp.as_ref().map_or(&[], |rsp| rsp.indices),
                                ),
                                buffers.vertices.get(&target.extents.logical),
                                target.extents.native.1,
                                ownership_start,
                            );
                            ownership_start = start + 1;
                        }
                        let writer = Writer {
                            address: copy.source.address,
                            layout: copy.source.layout,
                            depth: false,
                        };
                        if coherence.pending_overlaps(writer, copy.source.height) {
                            queue.submit([encoder.finish()]);
                            self.profiling.count("submissions", 1);
                            coherence.resolve(device);
                            encoder =
                                device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                    label: Some("framebuffer-load"),
                                });
                        }
                    }
                    self.framebuffer_copies.copy(
                        &mut encoder,
                        copy,
                        &self.framebuffers,
                        &self.color_versions,
                        &self.presentation_versions,
                        self.coherence.as_ref().map(|c| &c.owners),
                        &self.profiling,
                    );
                    start += 1;
                    continue;
                }
                let read_depth = target.operations[start].reads_depth();
                let decal_write = |op: &OperationInputs| op.reads_depth() && op.writes_depth();
                let end = if self.coherence.is_some() || decal_write(&target.operations[start]) {
                    start + 1
                } else {
                    start
                        + target.operations[start..]
                            .iter()
                            .take_while(|op| {
                                !matches!(op.draw, DrawInputs::FramebufferCopy(_))
                                    && op.reads_depth() == read_depth
                                    && !decal_write(op)
                            })
                            .count()
                };
                for (slot, operation) in target.operations.iter().enumerate().take(end).skip(start)
                {
                    let material = match operation.draw {
                        DrawInputs::Tris { material_index, .. } => Some(material_index),
                        DrawInputs::Rectangle { material_index, .. } => material_index,
                        _ => None,
                    };
                    if let Some(kind) = material.and_then(|index| {
                        self.framebuffer_copies
                            .failure(&inputs.textures[index as usize])
                    }) {
                        self.diagnostics.push(crate::Diagnostic {
                            at: operation.pc,
                            kind,
                        });
                        self.dropped_runs += 1;
                        source_valid[slot] = false;
                    }
                }
                for presentation in
                    [false, true]
                        .into_iter()
                        .take(if self.render_scale > 1 { 2 } else { 1 })
                {
                    let (pass_w, pass_h) = if presentation {
                        target.extents.presentation
                    } else {
                        target.extents.native
                    };
                    let depth = depth.map(|depth| {
                        if presentation {
                            depth.presentation.as_deref().unwrap()
                        } else {
                            depth
                        }
                    });
                    let pass_allocation = (
                        pass_w,
                        native_allocation_height * if presentation { self.render_scale } else { 1 },
                    );
                    let depth_bg = if presentation {
                        presentation_depth_bg.as_ref()
                    } else {
                        depth_bg.as_ref()
                    };
                    let material_bgs = if presentation {
                        &presentation_bgs
                    } else {
                        &material_bgs
                    };
                    let uniform_base = if presentation {
                        target.operations.len()
                    } else {
                        0
                    };
                    let writes_depth = target.operations[start..end]
                        .iter()
                        .any(OperationInputs::writes_depth);
                    let decal_snapshot = (read_depth && writes_depth).then(|| {
                        let snapshot = super::framebuffers::DepthImage::new(
                            device,
                            depth_layout,
                            pass_allocation,
                        );
                        self.depth_pipeline
                            .copy(device, &mut encoder, depth.unwrap(), &snapshot);
                        snapshot
                    });
                    let snapshot_bg = decal_snapshot.as_ref().map(|snapshot| {
                        device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("decal-depth-snapshot"),
                            layout: self.textured_fb.depth_bind_group_layout(),
                            entries: &[wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&snapshot.attach),
                            }],
                        })
                    });
                    let depth_bg = snapshot_bg.as_ref().or(depth_bg);
                    let attachment = depth.filter(|_| !read_depth || writes_depth).map(|depth| {
                        wgpu::RenderPassDepthStencilAttachment {
                            view: &depth.attach,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }
                    });
                    if let Some(view) = coverage_view.as_ref().filter(|_| !presentation) {
                        let operation = &target.operations[start];
                        let (x, y, width, height) = operation.scissor;
                        if source_valid[start]
                            && width != 0
                            && height != 0
                            && !matches!(operation.draw, DrawInputs::Rejected)
                        {
                            let coverage = self.coverage.get_or_insert_with(|| {
                                super::framebuffers::coherence::Coverage::new(device)
                            });
                            let pipeline = coverage
                                .pipeline(device, &self.textured_fb, &operation.draw, any_depth)
                                .clone();
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("guest-coverage"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view,
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Load,
                                        store: wgpu::StoreOp::Store,
                                    },
                                    depth_slice: None,
                                })],
                                depth_stencil_attachment: attachment.clone(),
                                timestamp_writes: None,
                                occlusion_query_set: None,
                                multiview_mask: None,
                            });
                            pass.set_viewport(0.0, 0.0, native_w as f32, native_h as f32, 0.0, 1.0);
                            pass.set_scissor_rect(x, y, width, height);
                            if read_depth {
                                pass.set_bind_group(2, depth_bg.unwrap(), &[]);
                            }
                            match &operation.draw {
                                DrawInputs::Tris {
                                    index_start,
                                    index_count,
                                    material_index,
                                    ..
                                } => {
                                    pass.set_pipeline(&pipeline);
                                    if let Some(index) = &buffers.indices {
                                        pass.set_index_buffer(
                                            index.slice(..),
                                            wgpu::IndexFormat::Uint32,
                                        );
                                    }
                                    if let Some(vertices) =
                                        buffers.vertices.get(&target.extents.logical)
                                    {
                                        pass.set_vertex_buffer(0, vertices.slice(..));
                                    }
                                    pass.set_bind_group(
                                        0,
                                        material_bgs[*material_index as usize],
                                        &[],
                                    );
                                    pass.set_bind_group(
                                        1,
                                        &upload.uniforms,
                                        &[(start * 256) as u32],
                                    );
                                    pass.draw_indexed(
                                        *index_start..*index_start + *index_count,
                                        0,
                                        0..1,
                                    );
                                }
                                DrawInputs::Rectangle { .. } => self.draw_rect_op(
                                    &mut pass,
                                    device,
                                    &operation.draw,
                                    material_bgs,
                                    upload.rectangles.as_ref(),
                                    Some(&upload.uniforms),
                                    start as u32,
                                    target.rect_indices[start],
                                    any_depth,
                                    Some(&pipeline),
                                    false,
                                ),
                                _ => {}
                            }
                        }
                    }
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("workload-segment"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: if presentation {
                                &self.framebuffers[&target.id]
                                    .presentation
                                    .as_ref()
                                    .unwrap()
                                    .attach
                            } else {
                                &self.framebuffers[&target.id].attach
                            },
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
                    self.profiling.count("workload.segments", 1);
                    if presentation {
                        self.profiling.count("workload.presentation_segments", 1);
                    }
                    pass.set_viewport(0.0, 0.0, pass_w as f32, pass_h as f32, 0.0, 1.0);
                    if let Some(index) = &buffers.indices {
                        pass.set_index_buffer(index.slice(..), wgpu::IndexFormat::Uint32);
                    }
                    if read_depth {
                        pass.set_bind_group(2, depth_bg.unwrap(), &[]);
                    }
                    for (slot, operation) in
                        target.operations.iter().enumerate().take(end).skip(start)
                    {
                        if !source_valid[slot] {
                            continue;
                        }
                        let (x, y, width, height) = if presentation {
                            target.presentation_scissor(slot)
                        } else {
                            operation.scissor
                        };
                        if width == 0 || height == 0 {
                            continue;
                        }
                        pass.set_scissor_rect(x, y, width, height);
                        match &operation.draw {
                            DrawInputs::Rejected | DrawInputs::FramebufferCopy(_) => {}
                            DrawInputs::DepthFill { .. } => {
                                unreachable!("depth fills have no color attachment")
                            }
                            DrawInputs::Tris {
                                cull,
                                unclipped_depth,
                                index_start,
                                index_count,
                                material_index,
                                mode,
                            } => {
                                let pipeline = if read_depth {
                                    self.textured_fb.select_decal(
                                        *cull,
                                        mode.fallback_class,
                                        mode.blend_class,
                                        mode.z_write,
                                    )
                                } else {
                                    let test = any_depth && mode.z_test;
                                    let write = any_depth && mode.z_write;
                                    match &self.textured_fb.dual {
                                        Some(dual)
                                            if mode.blend_class
                                                == crate::hle::BlendClass::DualSrc =>
                                        {
                                            dual.select(*cull, test, write, any_depth)
                                        }
                                        _ => self.textured_fb.select(
                                            *cull,
                                            test,
                                            write,
                                            any_depth,
                                            mode.fallback_class,
                                        ),
                                    }
                                };
                                pass.set_pipeline(&pipeline[usize::from(*unclipped_depth)]);
                                if let Some(vertices) =
                                    buffers.vertices.get(&target.extents.logical)
                                {
                                    pass.set_vertex_buffer(0, vertices.slice(..));
                                }
                                pass.set_bind_group(0, material_bgs[*material_index as usize], &[]);
                                pass.set_bind_group(
                                    1,
                                    &upload.uniforms,
                                    &[((uniform_base + slot) * 256) as u32],
                                );
                                pass.draw_indexed(
                                    *index_start..*index_start + *index_count,
                                    0,
                                    0..1,
                                );
                            }
                            DrawInputs::Rectangle { .. } => {
                                self.draw_rect_op(
                                    &mut pass,
                                    device,
                                    &operation.draw,
                                    material_bgs,
                                    upload.rectangles.as_ref(),
                                    Some(&upload.uniforms),
                                    (uniform_base + slot) as u32,
                                    target.rect_indices[slot],
                                    any_depth,
                                    None,
                                    presentation,
                                );
                            }
                        }
                    }
                }
                start = end;
            }
            if let TargetId::Guest(address) = target.id {
                if target.operations.iter().any(|op| {
                    !matches!(
                        op.draw,
                        DrawInputs::FramebufferCopy(_) | DrawInputs::Rejected
                    )
                }) {
                    self.target_descriptors.written(address, false);
                }
            }
            let writes_depth = target.operations.iter().any(OperationInputs::writes_depth);
            if let Some(TargetId::Guest(address)) = depth_id.filter(|_| writes_depth) {
                self.target_descriptors.written(address, true);
            }
            if let (Some(coherence), Some(coverage)) = (&mut self.coherence, &coverage_texture) {
                if !target.operations[ownership_start..].iter().any(|op| {
                    !matches!(
                        op.draw,
                        DrawInputs::FramebufferCopy(_) | DrawInputs::Rejected
                    )
                }) {
                    continue;
                }
                use super::framebuffers::coherence::Writer;
                let color = match target.id {
                    TargetId::Guest(address) => Some((
                        Writer {
                            address,
                            layout: color_layout,
                            depth: false,
                        },
                        &self.framebuffers[&target.id].color,
                    )),
                    TargetId::Legacy => None,
                };
                let depth = match depth_id.filter(|_| writes_depth) {
                    Some(TargetId::Guest(address)) => Some((
                        Writer {
                            address,
                            layout: depth_layout,
                            depth: true,
                        },
                        &self.depthbuffers[&depth_id.unwrap()].texture,
                    )),
                    _ => None,
                };
                self.profiling.count(
                    "readback.coverage_bytes",
                    u64::from(native_w) * u64::from(native_allocation_height) * 8,
                );
                if color.is_some() {
                    self.profiling.count(
                        "readback.color_bytes",
                        u64::from(native_w) * u64::from(native_allocation_height) * 4,
                    );
                }
                if depth.is_some() {
                    self.profiling.count(
                        "readback.depth_bytes",
                        u64::from(native_w) * u64::from(native_allocation_height) * 4,
                    );
                }
                coherence.snapshot(
                    device,
                    &mut encoder,
                    coverage,
                    color,
                    depth,
                    target.coverage_draws(
                        &source_valid,
                        target.operations.len(),
                        inputs.rsp.as_ref().map_or(&[], |rsp| rsp.indices),
                    ),
                    buffers.vertices.get(&target.extents.logical),
                    target.extents.native.1,
                    ownership_start,
                );
            }
            if self.coherence.is_some() {
                if let TargetId::Guest(address) = target.id {
                    let end = address + color_layout.row_bytes().unwrap() * u64::from(native_h);
                    self.depthbuffers.retain(|id, depth| {
                        let TargetId::Guest(start) = *id else {
                            return true;
                        };
                        let depth_end = start
                            + depth.layout.row_bytes().unwrap() * u64::from(depth.texture.height());
                        start >= end || address >= depth_end
                    });
                }
            }
        }
        self.profile_resident_resources();
        let command = encoder.finish();
        drop(encoding);
        let _submission = self.profiling.span("submission");
        queue.submit(Some(command));
        self.texture_resources.submitted(queue, &self.profiling);
        self.profiling.count("submissions", 1);
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
        self.depth_first_touch.clear();
        let source = self.render_into_store(device, queue, scene, ClearPolicy::PerFrame);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("workload-present"),
        });
        if let Some(source) = source {
            self.scanout(device, &mut encoder, target, source, None);
        } else {
            clear_color(&mut encoder, target, wgpu::LoadOp::Clear(CLEAR_COLOR));
        }
        queue.submit(Some(encoder.finish()));
    }
}

pub(super) fn output_scissor(scissor: Scissor, logical: (u32, u32), output: (u32, u32)) -> Scissor {
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

pub(super) fn clear_color(
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

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "workload_tests.rs"]
mod unit_tests;
