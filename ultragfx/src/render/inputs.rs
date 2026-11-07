use super::{rsp_buffers as rb, workload::TargetId, CombinerUniform, OutVertex};
use crate::hle::texture_request::TextureBindingInput;
use crate::hle::{BlendClass, Material, ZMode};
use crate::scene::{CullKind, Scene, SceneOp};

// GPU submission only receives these inputs. Capture compares the same preparation result.
#[derive(Debug, PartialEq)]
pub(crate) struct RenderInputs<'a> {
    pub(crate) diagnostics: Vec<crate::Diagnostic>,
    pub(crate) dropped_runs: u32,
    pub(super) textures: Vec<TextureInputs>,
    pub(super) rsp: Option<RspInputs<'a>>,
    pub(super) targets: Vec<TargetInputs>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct TextureInputs {
    pub framebuffer_only: bool,
    pub sampling: super::TileSamplingArray,
    pub allocation_extent: [u32; 2],
    pub tex_w: u32,
    pub tex_h: u32,
    pub texture: TextureBindingInput,
    pub wrap_s: u8,
    pub wrap_t: u8,
    pub num_levels: u8,
    pub tex1: Option<TextureBindingInput>,
    pub mip_levels: Vec<TextureBindingInput>,
    pub detail_tex: Option<TextureBindingInput>,
}

impl From<&Material> for TextureInputs {
    fn from(mat: &Material) -> Self {
        Self {
            framebuffer_only: false,
            sampling: super::material_sampling(mat),
            allocation_extent: mat.sampling.allocation_extent(),
            tex_w: mat.tex_w,
            tex_h: mat.tex_h,
            texture: TextureBindingInput {
                source: mat.texture.clone(),
                sampling: mat.sampling,
            },
            wrap_s: mat.wrap_s,
            wrap_t: mat.wrap_t,
            num_levels: mat.num_levels,
            tex1: mat.tex1.as_ref().map(|t| t.binding()),
            mip_levels: mat.mip_levels.iter().map(|t| t.binding()).collect(),
            detail_tex: mat.detail_tex.as_ref().map(|t| t.binding()),
        }
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct RspInputs<'a> {
    pub source: BufferData<rb::SrcVertex>,
    pub mvp_table: BufferData<f32>,
    pub viewport_table: BufferData<rb::GpuViewport>,
    pub texcoord_table: BufferData<rb::GpuTexcoord>,
    pub lights_table: BufferData<rb::GpuLight>,
    pub lookat_table: BufferData<rb::GpuLookAt>,
    pub fog_table: BufferData<[f32; 2]>,
    pub indices: &'a [u32],
}

impl<'a> RspInputs<'a> {
    fn new(scene: &'a Scene) -> Option<Self> {
        if scene.raw_pos.is_empty() || scene.indices.is_empty() {
            return None;
        }
        Some(Self {
            source: rb::src_vertices(scene).into(),
            mvp_table: rb::mvp_table(scene).into(),
            viewport_table: rb::viewport_table(scene).into(),
            texcoord_table: rb::texcoord_table(scene).into(),
            lights_table: rb::lights_table(scene).into(),
            lookat_table: rb::lookat_table(scene).into(),
            fog_table: rb::fog_table(scene).into(),
            indices: &scene.indices,
        })
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct TargetInputs {
    pub color_image_epoch: u64,
    pub valid: bool,
    pub id: TargetId,
    pub color_image: crate::scene::ColorImage,
    pub depth_image: Option<u64>,
    pub pc: u64,
    pub extents: super::extents::TargetExtents,
    pub any_depth: bool,
    pub depth_clear: bool,
    pub operations: Vec<OperationInputs>,
    pub logical_scissors: Vec<crate::scene::Scissor>,
    pub uniforms: Vec<u8>,
    pub rectangles: BufferData<OutVertex>,
    pub rect_indices: Vec<u32>,
}

impl TargetInputs {
    pub(super) fn presentation_scissor(&self, slot: usize) -> (u32, u32, u32, u32) {
        let scissor = super::workload::output_scissor(
            self.logical_scissors[slot],
            self.extents.logical,
            self.extents.presentation,
        );
        super::clamp_scissor(
            &scissor,
            self.extents.presentation.0,
            self.extents.presentation.1,
        )
    }

    pub(super) fn coverage_draws(
        &self,
        source_valid: &[bool],
        end: usize,
        indices: &[u32],
    ) -> Vec<super::framebuffers::coherence::CoverageDraw> {
        self.operations[..end]
            .iter()
            .zip(source_valid)
            .enumerate()
            .map(|(index, (op, &valid))| {
                if valid && !matches!(op.draw, DrawInputs::Rejected) {
                    use super::framebuffers::coherence::{CoverageDraw, CoverageGeometry};
                    CoverageDraw {
                        cull: matches!(
                            op.draw,
                            DrawInputs::Tris {
                                cull: CullKind::Cull,
                                ..
                            }
                        ),
                        combiner: op.coverage_mode.and_then(|mode| {
                            let bytes = self.uniforms.get(
                                index * 256..index * 256 + std::mem::size_of::<CombinerUniform>(),
                            )?;
                            super::framebuffers::writeback::Combiner::new(
                                &bytemuck::pod_read_unaligned(bytes),
                                mode,
                            )
                        }),
                        literal: op.literal_fill,
                        mode: op.coverage_mode,
                        scissor: op.scissor,
                        geometry: match op.draw {
                            DrawInputs::Tris {
                                index_start,
                                index_count,
                                ..
                            } => CoverageGeometry::Triangles(
                                indices[index_start as usize..(index_start + index_count) as usize]
                                    .as_chunks::<3>()
                                    .0
                                    .to_vec(),
                            ),
                            _ => op
                                .coverage_rect
                                .map(CoverageGeometry::Rectangle)
                                .unwrap_or_default(),
                        },
                    }
                } else {
                    Default::default()
                }
            })
            .collect()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn write_bounds(&self, slot: usize) -> (u32, u32, u32, u32) {
        let op = &self.operations[slot];
        let (x, y, width, height) = op.scissor;
        if !matches!(op.draw, DrawInputs::Rectangle { .. }) {
            return op.scissor;
        }
        let start = self.rect_indices[slot] as usize * 6;
        let vertices = &self.rectangles[start..start + 6];
        let xs = vertices
            .iter()
            .map(|v| (v.position[0] + 1.0) * 0.5 * self.extents.native.0 as f32);
        let ys = vertices
            .iter()
            .map(|v| (1.0 - v.position[1]) * 0.5 * self.extents.native.1 as f32);
        let left = (xs.clone().fold(f32::INFINITY, f32::min).floor().max(0.0) as u32).max(x);
        let top = (ys.clone().fold(f32::INFINITY, f32::min).floor().max(0.0) as u32).max(y);
        let right = (xs.fold(0.0, f32::max).ceil() as u32).min(x + width);
        let bottom = (ys.fold(0.0, f32::max).ceil() as u32).min(y + height);
        (
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        )
    }

    pub(super) fn record_descriptors(
        &self,
        descriptors: &mut super::framebuffers::targets::TargetDescriptors,
    ) -> Result<bool, crate::Diagnostic> {
        if !self.valid {
            return Ok(false);
        }
        if self.id == TargetId::Legacy && self.extents.native != self.extents.logical {
            if let Some(address) = self.depth_image {
                return Err(crate::Diagnostic {
                    at: self.pc,
                    kind: crate::DiagKind::UnsupportedLegacyDepthExtent {
                        address,
                        canvas: self.extents.native,
                        depth: self.extents.logical,
                    },
                });
            }
        }
        let (width, height) = self.extents.native;
        if let TargetId::Guest(address) = self.id {
            if !self.depth_clear {
                let _ = descriptors.record(
                    address,
                    super::framebuffers::ImageLayout {
                        width,
                        fmt: self.color_image.fmt,
                        siz: self.color_image.siz,
                    },
                    height,
                    false,
                );
            }
        }
        if let Some(address) = self.depth_image {
            let _ = descriptors.record(
                address,
                super::framebuffers::ImageLayout {
                    width,
                    fmt: 0,
                    siz: 2,
                },
                height,
                true,
            );
        }
        Ok(true)
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct OperationInputs {
    pub literal_fill: Option<u32>,
    pub coverage_mode: Option<crate::hle::RenderMode>,
    pub coverage_rect: Option<[f32; 4]>,
    pub pc: u64,
    pub scissor: (u32, u32, u32, u32),
    pub draw: DrawInputs,
}

#[derive(Debug, PartialEq)]
pub(super) enum DrawInputs {
    FramebufferCopy(std::sync::Arc<crate::hle::framebuffer_load::FramebufferCopy>),
    Rejected,
    DepthFill {
        word: u32,
    },
    Tris {
        cull: CullKind,
        unclipped_depth: bool,
        index_start: u32,
        index_count: u32,
        material_index: u32,
        mode: DrawMode,
    },
    Rectangle {
        material_index: Option<u32>,
        fb_source: Option<super::framebuffers::targets::ImageDescriptor>,
        mode: DrawMode,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct DrawMode {
    pub z_test: bool,
    pub z_write: bool,
    pub read_depth: bool,
    pub fallback_class: BlendClass,
    pub blend_class: BlendClass,
}

impl DrawMode {
    fn new(mode: &crate::hle::RenderMode, any_depth: bool) -> Self {
        Self {
            z_test: any_depth && mode.z_test,
            z_write: any_depth && mode.z_write,
            read_depth: any_depth && mode.z_test && mode.z_mode == ZMode::Decal,
            fallback_class: mode.fallback_class,
            blend_class: mode.blend_class,
        }
    }
}

impl OperationInputs {
    pub fn reads_depth(&self) -> bool {
        matches!(&self.draw, DrawInputs::Tris { mode, .. } | DrawInputs::Rectangle { mode, .. } if mode.read_depth)
    }

    pub fn writes_depth(&self) -> bool {
        matches!(&self.draw, DrawInputs::Tris { mode, .. } | DrawInputs::Rectangle { mode, .. } if mode.z_write)
    }
}

impl<'a> RenderInputs<'a> {
    #[cfg(any(test, feature = "profiling"))]
    pub(crate) fn profile_targets(&self, recorder: &crate::profiling::Recorder) {
        for target in &self.targets {
            recorder.target(target.extents.logical, target.extents.presentation);
        }
    }

    #[cfg(any(test, feature = "capture", feature = "profiling"))]
    pub fn update_target_descriptors(
        &self,
        descriptors: &mut super::framebuffers::targets::TargetDescriptors,
    ) {
        for target in &self.targets {
            let _ = target.record_descriptors(descriptors);
        }
    }

    #[cfg(any(test, feature = "profiling"))]
    pub(crate) fn census_update_targets(
        &self,
        descriptors: &mut super::framebuffers::targets::TargetDescriptors,
    ) {
        for target in &self.targets {
            if !target.operations.is_empty()
                && target
                    .operations
                    .iter()
                    .all(|op| matches!(op.draw, DrawInputs::FramebufferCopy(_)))
            {
                continue;
            }
            if !matches!(target.record_descriptors(descriptors), Ok(true)) {
                continue;
            }
            let depth = target.depth_image.filter(|_| target.any_depth);
            if target.depth_clear {
                if let Some(address) = depth {
                    descriptors.written(address, true);
                }
                continue;
            }
            if let TargetId::Guest(address) = target.id {
                if target.operations.iter().any(|op| {
                    !matches!(
                        op.draw,
                        DrawInputs::FramebufferCopy(_) | DrawInputs::Rejected
                    )
                }) {
                    descriptors.written(address, false);
                }
            }
            let writes_depth = target.operations.iter().any(|op| {
                matches!(&op.draw, DrawInputs::Tris { mode, .. } | DrawInputs::Rectangle { mode, .. } if mode.z_write)
            });
            if let Some(address) = depth.filter(|_| writes_depth) {
                descriptors.written(address, true);
            }
        }
    }

    #[cfg(test)]
    pub fn new(scene: &'a Scene, legacy_extent: (u32, u32), frame: [u32; 2]) -> Option<Self> {
        Self::with_targets(scene, legacy_extent, frame, &Default::default())
    }

    pub fn with_targets(
        scene: &'a Scene,
        legacy_extent: (u32, u32),
        frame: [u32; 2],
        descriptors: &super::framebuffers::targets::TargetDescriptors,
    ) -> Option<Self> {
        let mut descriptors = descriptors.clone();
        let mut diagnostics = Vec::new();
        let mut dropped_runs = 0;
        if (scene.draw_runs.is_empty() || scene.raw_pos.is_empty() || scene.indices.is_empty())
            && scene.framebuffer_pairs.is_empty()
        {
            return None;
        }
        let workload = super::workload::Workload::new(scene);
        let mut result = Self {
            diagnostics: Vec::new(),
            dropped_runs: 0,
            textures: scene.materials.iter().map(TextureInputs::from).collect(),
            rsp: RspInputs::new(scene),
            targets: workload
                .targets
                .iter()
                .map(|target| {
                    let extents = super::extents::TargetExtents::new(
                        target.id,
                        target.logical_extent,
                        legacy_extent,
                        1,
                    )
                    .expect("nonzero target extents at scale 1");
                    let (native_w, native_h) = extents.native;
                    let any_depth = target.uses_depth(scene);
                    let mut valid = true;
                    let pc = target.operations.first().and_then(|op| op.pc).unwrap_or(0);
                    if let Some(address) = target.depth_image {
                        if let Err(kind) = descriptors.record(
                            address,
                            super::framebuffers::ImageLayout {
                                width: native_w,
                                fmt: 0,
                                siz: 2,
                            },
                            native_h,
                            true,
                        ) {
                            diagnostics.push(crate::Diagnostic { at: pc, kind });
                            valid = false;
                        }
                    }
                    if let TargetId::Guest(address) = target.id {
                        let layout = super::framebuffers::ImageLayout {
                            width: native_w,
                            fmt: target.color_image.fmt,
                            siz: target.color_image.siz,
                        };
                        if !target.depth_clear {
                            if let Err(kind) = descriptors.record(address, layout, native_h, false)
                            {
                                diagnostics.push(crate::Diagnostic { at: pc, kind });
                                valid = false;
                            }
                        }
                    }
                    let mut inputs = TargetInputs {
                        color_image_epoch: target.color_image_epoch,
                        valid,
                        id: target.id,
                        color_image: target.color_image,
                        depth_image: target.depth_image,
                        pc: target.operations.first().and_then(|op| op.pc).unwrap_or(0),
                        extents,
                        any_depth,
                        depth_clear: target.depth_clear,
                        operations: Vec::new(),
                        logical_scissors: target
                            .operations
                            .iter()
                            .map(|operation| operation.presentation_scissor)
                            .collect(),
                        uniforms: vec![0; target.operations.len() * 256],
                        rectangles: Vec::new().into(),
                        rect_indices: Vec::new(),
                    };
                    {
                        for (slot, operation) in target.operations.iter().enumerate() {
                            inputs
                                .rect_indices
                                .push((inputs.rectangles.len() / 6) as u32);
                            let (draw, mut uniform) = match &operation.draw {
                                SceneOp::FramebufferCopy(copy) => {
                                    inputs.operations.push(OperationInputs {
                                        literal_fill: None,
                                        coverage_mode: None,
                                        coverage_rect: None,
                                        pc: 0,
                                        scissor: (0, 0, 0, 0),
                                        draw: DrawInputs::FramebufferCopy(copy.clone()),
                                    });
                                    continue;
                                }
                                SceneOp::Tris(run) => {
                                    let mat = &scene.materials[run.material_index as usize];
                                    let mode = &scene.render_modes[run.render_mode_index as usize];
                                    let mut uniform =
                                        CombinerUniform::from_run(mat, mode, run.fog_color);
                                    uniform.primitive_depth[..2].copy_from_slice(&[
                                        u32::from(mode.z_source_prim),
                                        u32::from(run.prim_depth.z),
                                    ]);
                                    uniform.inv_tex_size = super::triangle_inv_tex_size(mat);
                                    uniform.inv_tex_size[3] = u32::from(run.unclipped_depth) as f32;
                                    (
                                        DrawInputs::Tris {
                                            cull: run.cull,
                                            unclipped_depth: run.unclipped_depth,
                                            index_start: run.index_start,
                                            index_count: run.index_count,
                                            material_index: run.material_index,
                                            mode: DrawMode::new(mode, any_depth),
                                        },
                                        uniform,
                                    )
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
                                    fb_source,
                                    prim_depth,
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
                                    if !copy_mode {
                                        let mode = &scene.render_modes[*render_mode_index as usize];
                                        uniform.primitive_depth[..2].copy_from_slice(&[
                                            u32::from(mode.z_source_prim),
                                            u32::from(prim_depth.z),
                                        ]);
                                    }
                                    let fb_source = fb_source
                                        .and_then(|address| descriptors.get(address, false));
                                    // A load-free framebuffer view is bound at the source's own extent, so
                                    // texel coordinates must scale by that extent rather than the tile's.
                                    uniform.inv_tex_size = match fb_source {
                                        Some(source) => [
                                            1.0 / source.layout.width.max(1) as f32,
                                            1.0 / source.height.max(1) as f32,
                                            0.0,
                                            0.0,
                                        ],
                                        None => super::triangle_inv_tex_size(mat),
                                    };
                                    uniform.inv_tex_size[2] = 1.0;
                                    let (ds, dt) = super::texrect_steps((*dsdx, *dtdy), *copy_mode);
                                    uniform.raster[2] = u32::from(
                                        ds.unsigned_abs() == 1024 && dt.unsigned_abs() == 1024,
                                    );
                                    inputs.rectangles.extend_from_slice(&super::texrect_quad(
                                        rect,
                                        (*uls, *ult),
                                        (*dsdx, *dtdy),
                                        *flip,
                                        *copy_mode,
                                        extents.logical,
                                    ));
                                    (
                                        DrawInputs::Rectangle {
                                            material_index: Some(*material_index),
                                            fb_source,
                                            mode: if *copy_mode {
                                                DrawMode::default()
                                            } else {
                                                DrawMode::new(
                                                    &scene.render_modes
                                                        [*render_mode_index as usize],
                                                    any_depth,
                                                )
                                            },
                                        },
                                        uniform,
                                    )
                                }
                                SceneOp::FillRect {
                                    rect, color_raw, ..
                                } => {
                                    if target.depth_clear {
                                        let intersect = |scissor: crate::scene::Scissor| {
                                            crate::scene::Scissor {
                                                ulx: scissor.ulx.max(rect.ulx),
                                                uly: scissor.uly.max(rect.uly),
                                                lrx: scissor.lrx.min(rect.lrx.saturating_add(1)),
                                                lry: scissor.lry.min(rect.lry.saturating_add(1)),
                                                mode: scissor.mode,
                                            }
                                        };
                                        let scissor = intersect(operation.scissor);
                                        inputs.logical_scissors[slot] =
                                            intersect(operation.presentation_scissor);
                                        inputs.operations.push(OperationInputs {
                                            literal_fill: Some(*color_raw),
                                            coverage_mode: None,
                                            coverage_rect: None,
                                            pc: operation.pc.unwrap_or(0),
                                            scissor: super::clamp_scissor(
                                                &scissor, native_w, native_h,
                                            ),
                                            draw: DrawInputs::DepthFill { word: *color_raw },
                                        });
                                        continue;
                                    }
                                    inputs.rectangles.extend_from_slice(&super::rect_quad(
                                        rect,
                                        native_w,
                                        native_h,
                                        [1.0; 4],
                                        [[0.0; 2]; 4],
                                    ));
                                    (
                                        DrawInputs::Rectangle {
                                            material_index: None,
                                            fb_source: None,
                                            mode: DrawMode::default(),
                                        },
                                        CombinerUniform::fill_rect(
                                            *color_raw,
                                            target.color_image.siz,
                                        ),
                                    )
                                }
                                SceneOp::SetScissor(_) => {
                                    unreachable!("scissor is normalized onto draws")
                                }
                            };
                            let mut draw = draw;
                            if let SceneOp::TexRect {
                                fb_source: Some(address),
                                material_index,
                                ..
                            } = &operation.draw
                            {
                                let material = &scene.materials[*material_index as usize];
                                let kind = match descriptors.get(*address, false) {
                                    None => Some(crate::DiagKind::UnsupportedFramebufferAccess {
                                        address: *address,
                                        reason: crate::FramebufferAccess::MissingSource,
                                    }),
                                    Some(_) if material.tile_count > 1 => {
                                        Some(crate::DiagKind::UnsupportedFramebufferAccess {
                                            address: *address,
                                            reason: crate::FramebufferAccess::TextureMode,
                                        })
                                    }
                                    Some(source) => descriptors
                                        .source(
                                            *address,
                                            target.color_image.addr,
                                            super::framebuffers::ImageLayout {
                                                width: source.layout.width,
                                                fmt: material.fmt,
                                                siz: material.siz,
                                            },
                                            material.tex_h,
                                        )
                                        .err(),
                                };
                                if let Some(kind) = kind {
                                    diagnostics.push(crate::Diagnostic {
                                        at: operation.pc.unwrap_or(0),
                                        kind,
                                    });
                                    dropped_runs += 1;
                                    draw = DrawInputs::Rejected;
                                }
                            }
                            debug_assert_eq!(
                                uniform.is_rect != 0,
                                !matches!(operation.draw, SceneOp::Tris(_))
                            );
                            debug_assert!(native_w > 0 && native_h > 0);
                            uniform.frame = [frame[0], frame[1], native_w, native_h];
                            uniform.coverage_slot = slot as u32 + 1;
                            let bytes = bytemuck::bytes_of(&uniform);
                            inputs.uniforms[slot * 256..slot * 256 + bytes.len()]
                                .copy_from_slice(bytes);
                            let scissor = super::workload::output_scissor(
                                operation.scissor,
                                extents.logical,
                                extents.native,
                            );
                            let (literal_fill, coverage_mode, coverage_rect) = match &operation.draw
                            {
                                SceneOp::Tris(run) => (
                                    None,
                                    Some(scene.render_modes[run.render_mode_index as usize]),
                                    None,
                                ),
                                SceneOp::TexRect {
                                    rect,
                                    copy_mode: false,
                                    render_mode_index,
                                    ..
                                } => (
                                    None,
                                    Some(scene.render_modes[*render_mode_index as usize]),
                                    Some([
                                        rect.ulx as f32 / 4.0,
                                        rect.uly as f32 / 4.0,
                                        rect.lrx as f32 / 4.0,
                                        rect.lry as f32 / 4.0,
                                    ]),
                                ),
                                SceneOp::TexRect {
                                    rect,
                                    copy_mode: true,
                                    ..
                                } => (
                                    None,
                                    None,
                                    Some([
                                        (rect.ulx >> 2) as f32,
                                        (rect.uly >> 2) as f32,
                                        ((rect.lrx >> 2) + 1) as f32,
                                        ((rect.lry >> 2) + 1) as f32,
                                    ]),
                                ),
                                SceneOp::FillRect {
                                    color_raw, rect, ..
                                } => (
                                    Some(*color_raw),
                                    None,
                                    Some([
                                        rect.ulx as f32,
                                        rect.uly as f32,
                                        (rect.lrx + 1) as f32,
                                        (rect.lry + 1) as f32,
                                    ]),
                                ),
                                _ => (None, None, None),
                            };
                            inputs.operations.push(OperationInputs {
                                literal_fill,
                                coverage_mode,
                                coverage_rect,
                                pc: operation.pc.unwrap_or(0),
                                scissor: super::clamp_scissor(&scissor, native_w, native_h),
                                draw,
                            });
                        }
                    }
                    inputs
                })
                .collect(),
        };
        let mut uses = vec![(false, false); result.textures.len()];
        for op in result.targets.iter().flat_map(|target| &target.operations) {
            match op.draw {
                DrawInputs::Rectangle {
                    material_index: Some(index),
                    fb_source: Some(_),
                    ..
                } => uses[index as usize].0 = true,
                DrawInputs::Tris { material_index, .. }
                | DrawInputs::Rectangle {
                    material_index: Some(material_index),
                    fb_source: None,
                    ..
                } => uses[material_index as usize].1 = true,
                _ => {}
            }
        }
        for (texture, (framebuffer, ordinary)) in result.textures.iter_mut().zip(uses) {
            texture.framebuffer_only = framebuffer && !ordinary;
        }
        result.diagnostics = diagnostics;
        result.dropped_runs = dropped_runs;
        Some(result)
    }
}

#[derive(Debug)]
pub(super) struct BufferData<T>(Vec<T>);

impl<T: bytemuck::Pod> PartialEq for BufferData<T> {
    fn eq(&self, other: &Self) -> bool {
        bytemuck::cast_slice::<T, u8>(&self.0) == bytemuck::cast_slice::<T, u8>(&other.0)
    }
}

impl<T> From<Vec<T>> for BufferData<T> {
    fn from(data: Vec<T>) -> Self {
        Self(data)
    }
}

impl<T> std::ops::Deref for BufferData<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> std::ops::DerefMut for BufferData<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hle::{gbi::GbiUcode, interp::interpret, mem::GbiDataFormat};

    #[test]
    fn framebuffer_shortcut_preserves_shared_material_textures() {
        let scene = crate::tests::common::scene_from_fixture("offscreen-then-sample");
        let (pair, op, index) = scene
            .framebuffer_pairs
            .iter()
            .enumerate()
            .find_map(|(pair, target)| {
                target
                    .ops
                    .iter()
                    .enumerate()
                    .find_map(|(op, draw)| match draw {
                        SceneOp::TexRect {
                            material_index,
                            fb_source: Some(_),
                            ..
                        } => Some((pair, op, *material_index as usize)),
                        _ => None,
                    })
            })
            .unwrap();
        let inputs = RenderInputs::new(&scene, (320, 240), [0; 2]).unwrap();
        assert!(inputs.textures[index].framebuffer_only);
        for before in [false, true] {
            let mut shared = scene.clone();
            let mut ordinary = shared.framebuffer_pairs[pair].ops[op].clone();
            let SceneOp::TexRect { fb_source, .. } = &mut ordinary else {
                unreachable!()
            };
            *fb_source = None;
            shared.framebuffer_pairs[pair]
                .ops
                .insert(op + usize::from(!before), ordinary);
            let inputs = RenderInputs::new(&shared, (320, 240), [0; 2]).unwrap();
            assert!(!inputs.textures[index].framebuffer_only);
            assert_eq!(
                inputs.textures[index],
                TextureInputs::from(&scene.materials[index])
            );
        }
    }

    #[test]
    fn coverage_target_extent_and_draw_kind() {
        let mut seen = [false; 3];
        for name in [
            "flat-color--white1",
            "high-poly--empty-texture",
            "hud-over-3d",
            "fill-texrect",
            "offscreen-then-sample",
        ] {
            let (bytes, entry) = crate::tests::fixtures::fixture(name);
            let scene = interpret(
                crate::RdramImage::new(bytes),
                entry,
                GbiUcode::F3dex2,
                GbiDataFormat::Fixed,
                None,
            )
            .scene;
            if let Some(mat) = scene.materials.first() {
                assert_eq!(
                    CombinerUniform::from_rect(mat, &Default::default(), [0; 4]).is_rect,
                    1
                );
            }
            for extent in [(96, 64), (193, 132)] {
                let inputs = RenderInputs::new(&scene, extent, [17, 29]).unwrap();
                for target in &inputs.targets {
                    let (w, h) = target.extents.presentation;
                    let (native_w, native_h) = target.extents.native;
                    match target.id {
                        TargetId::Legacy => {
                            assert_eq!(target.extents.logical, (320, 240));
                            assert_eq!((w, h), extent);
                        }
                        TargetId::Guest(_) => {
                            assert_eq!((native_w, native_h), target.extents.logical);
                            assert_eq!((w, h), (native_w, native_h));
                        }
                    }
                    let mut descriptors = Default::default();
                    assert_eq!(target.record_descriptors(&mut descriptors), Ok(true));
                    if let TargetId::Guest(address) = target.id {
                        if !target.depth_clear {
                            let descriptor = descriptors.get(address, false).unwrap();
                            assert_eq!(descriptor.layout.width, native_w);
                            assert_eq!(descriptor.height, native_h);
                        }
                    }
                    for (slot, operation) in target.operations.iter().enumerate() {
                        let uniform: CombinerUniform = bytemuck::pod_read_unaligned(
                            &target.uniforms
                                [slot * 256..slot * 256 + std::mem::size_of::<CombinerUniform>()],
                        );
                        assert_eq!(uniform.frame, [17, 29, w, h], "{name}");
                        let kind = match operation.draw {
                            DrawInputs::Tris { .. } => 0,
                            DrawInputs::Rectangle {
                                material_index: Some(_),
                                ..
                            } => 1,
                            DrawInputs::Rectangle {
                                material_index: None,
                                ..
                            } => 2,
                            DrawInputs::DepthFill { .. } => continue,
                            DrawInputs::FramebufferCopy(_) => continue,
                            DrawInputs::Rejected => panic!("{name}: rejected draw"),
                        };
                        seen[kind] = true;
                        assert_eq!(uniform.is_rect, u32::from(kind != 0), "{name}");
                    }
                }
            }
        }
        assert_eq!(seen, [true; 3]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn guest_descriptors_and_write_bounds_use_native_extent() {
        let address = 0x10_0000;
        let mut scene = crate::tests::common::dl_2d_fill_rect_with_scissor_height(
            address,
            0xf801_f801,
            0,
            0,
            16,
            22,
            132,
        );
        scene.framebuffer_pairs[0].color_image.width = 193;
        scene.framebuffer_pairs[0].depth_image = Some(0x20_0000);
        let mut inputs = RenderInputs::new(&scene, (640, 480), [17, 29]).unwrap();
        let target = &mut inputs.targets[0];
        target.extents =
            super::super::extents::TargetExtents::new(target.id, (193, 132), (640, 480), 3)
                .unwrap();
        let mut descriptors = Default::default();
        assert_eq!(target.record_descriptors(&mut descriptors), Ok(true));
        let descriptor = descriptors.get(address, false).unwrap();
        assert_eq!(descriptor.layout.width, 193);
        assert_eq!(descriptor.height, 132);
        assert_eq!(
            descriptor.range().unwrap(),
            address..address + 193 * 132 * 2
        );
        let depth = descriptors.get(0x20_0000, true).unwrap();
        assert_eq!(depth.layout.width, 193);
        assert_eq!(depth.range().unwrap(), 0x20_0000..0x20_0000 + 193 * 132 * 2);
        assert_eq!(target.write_bounds(0), (0, 0, 18, 24));
    }
}
