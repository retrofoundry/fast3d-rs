use super::{rsp_buffers as rb, workload::TargetId, CombinerUniform, OutVertex};
use crate::hle::{BlendClass, Material, ZMode};
use crate::scene::{CullKind, Scene, SceneOp};

// GPU submission only receives these inputs. Capture compares the same preparation result.
#[derive(Debug, PartialEq)]
pub(crate) struct RenderInputs<'a> {
    pub(crate) diagnostics: Vec<crate::Diagnostic>,
    pub(crate) dropped_runs: u32,
    pub(super) textures: Vec<TextureInputs<'a>>,
    pub(super) rsp: Option<RspInputs<'a>>,
    pub(super) targets: Vec<TargetInputs>,
}

#[derive(Debug, PartialEq)]
pub(super) struct TextureInputs<'a> {
    pub sampling: super::TileSamplingArray,
    pub allocation_extent: [u32; 2],
    pub tex_w: u32,
    pub tex_h: u32,
    pub texture: &'a [u8],
    pub wrap_s: u8,
    pub wrap_t: u8,
    pub num_levels: u8,
    pub tex1: &'a Option<crate::hle::combiner::Tex1>,
    pub mip_levels: &'a [crate::hle::MipLevel],
    pub detail_tex: &'a Option<crate::hle::MipLevel>,
}

impl<'a> From<&'a Material> for TextureInputs<'a> {
    fn from(mat: &'a Material) -> Self {
        Self {
            sampling: super::material_sampling(mat),
            allocation_extent: mat.sampling.allocation_extent(),
            tex_w: mat.tex_w,
            tex_h: mat.tex_h,
            texture: &mat.texture,
            wrap_s: mat.wrap_s,
            wrap_t: mat.wrap_t,
            num_levels: mat.num_levels,
            tex1: &mat.tex1,
            mip_levels: &mat.mip_levels,
            detail_tex: &mat.detail_tex,
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
    pub logical_extent: (u32, u32),
    pub output_extent: (u32, u32),
    pub any_depth: bool,
    pub depth_clear: bool,
    pub operations: Vec<OperationInputs>,
    pub uniforms: Vec<u8>,
    pub rectangles: BufferData<OutVertex>,
    pub rect_indices: Vec<u32>,
}

#[derive(Debug, PartialEq)]
pub(super) struct OperationInputs {
    pub pc: u64,
    pub scissor: (u32, u32, u32, u32),
    pub draw: DrawInputs,
}

#[derive(Debug, PartialEq)]
pub(super) enum DrawInputs {
    Rejected,
    DepthFill {
        word: u32,
    },
    Tris {
        cull: CullKind,
        index_start: u32,
        index_count: u32,
        material_index: u32,
        mode: TriangleMode,
    },
    Rectangle {
        material_index: Option<u32>,
        fb_source: Option<super::framebuffers::targets::ImageDescriptor>,
        blend_class: BlendClass,
    },
}

#[derive(Debug, PartialEq)]
pub(super) struct TriangleMode {
    pub z_test: bool,
    pub z_write: bool,
    pub read_depth: bool,
    pub fallback_class: BlendClass,
    pub blend_class: BlendClass,
}

impl OperationInputs {
    pub fn reads_depth(&self) -> bool {
        matches!(&self.draw, DrawInputs::Tris { mode, .. } if mode.read_depth)
    }
}

impl<'a> RenderInputs<'a> {
    #[cfg(any(test, feature = "capture"))]
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
                    let (w, h) = match target.id {
                        TargetId::Legacy => legacy_extent,
                        TargetId::Guest(_) => target.logical_extent,
                    };
                    let any_depth = target.uses_depth(scene);
                    let mut valid = true;
                    let pc = target.operations.first().and_then(|op| op.pc).unwrap_or(0);
                    if let Some(address) = target.depth_image {
                        if let Err(kind) = descriptors.record(
                            address,
                            super::framebuffers::ImageLayout {
                                width: w,
                                fmt: 0,
                                siz: 2,
                            },
                            h,
                            true,
                        ) {
                            diagnostics.push(crate::Diagnostic { at: pc, kind });
                            valid = false;
                        }
                    }
                    if let TargetId::Guest(address) = target.id {
                        let layout = super::framebuffers::ImageLayout {
                            width: w,
                            fmt: target.color_image.fmt,
                            siz: target.color_image.siz,
                        };
                        if !target.depth_clear {
                            if let Err(kind) = descriptors.record(address, layout, h, false) {
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
                        logical_extent: target.logical_extent,
                        output_extent: (w, h),
                        any_depth,
                        depth_clear: target.depth_clear,
                        operations: Vec::new(),
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
                                SceneOp::Tris(run) => {
                                    let mat = &scene.materials[run.material_index as usize];
                                    let mode = &scene.render_modes[run.render_mode_index as usize];
                                    let mut uniform =
                                        CombinerUniform::from_run(mat, mode, run.fog_color);
                                    uniform.inv_tex_size = super::triangle_inv_tex_size(mat);
                                    (
                                        DrawInputs::Tris {
                                            cull: run.cull,
                                            index_start: run.index_start,
                                            index_count: run.index_count,
                                            material_index: run.material_index,
                                            mode: TriangleMode {
                                                z_test: any_depth && mode.z_test,
                                                z_write: any_depth && mode.z_write,
                                                read_depth: any_depth
                                                    && mode.z_mode == ZMode::Decal,
                                                fallback_class: mode.fallback_class,
                                                blend_class: mode.blend_class,
                                            },
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
                                    uniform.inv_tex_size = super::triangle_inv_tex_size(mat);
                                    uniform.inv_tex_size[2] = 1.0;
                                    inputs.rectangles.extend_from_slice(&super::texrect_quad(
                                        rect,
                                        (*uls, *ult),
                                        (*dsdx, *dtdy),
                                        *flip,
                                        *copy_mode,
                                        target.logical_extent,
                                    ));
                                    (
                                        DrawInputs::Rectangle {
                                            material_index: Some(*material_index),
                                            fb_source: fb_source.and_then(|address| {
                                                descriptors.get(address, false)
                                            }),
                                            blend_class: if *copy_mode {
                                                BlendClass::Replace
                                            } else {
                                                scene.render_modes[*render_mode_index as usize]
                                                    .fallback_class
                                            },
                                        },
                                        uniform,
                                    )
                                }
                                SceneOp::FillRect {
                                    rect, color_raw, ..
                                } => {
                                    if target.depth_clear {
                                        let scissor = crate::scene::Scissor {
                                            ulx: operation.scissor.ulx.max(rect.ulx),
                                            uly: operation.scissor.uly.max(rect.uly),
                                            lrx: operation
                                                .scissor
                                                .lrx
                                                .min(rect.lrx.saturating_add(1)),
                                            lry: operation
                                                .scissor
                                                .lry
                                                .min(rect.lry.saturating_add(1)),
                                            mode: operation.scissor.mode,
                                        };
                                        inputs.operations.push(OperationInputs {
                                            pc: operation.pc.unwrap_or(0),
                                            scissor: super::clamp_scissor(&scissor, w, h),
                                            draw: DrawInputs::DepthFill { word: *color_raw },
                                        });
                                        continue;
                                    }
                                    inputs.rectangles.extend_from_slice(&super::rect_quad(
                                        rect,
                                        w,
                                        h,
                                        [1.0; 4],
                                        [[0.0; 2]; 4],
                                    ));
                                    (
                                        DrawInputs::Rectangle {
                                            material_index: None,
                                            fb_source: None,
                                            blend_class: BlendClass::Replace,
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
                            if !matches!(operation.draw, SceneOp::FillRect { .. }) {
                                uniform.frame = [frame[0], frame[1], w, h];
                            }
                            let bytes = bytemuck::bytes_of(&uniform);
                            inputs.uniforms[slot * 256..slot * 256 + bytes.len()]
                                .copy_from_slice(bytes);
                            let scissor = super::workload::output_scissor(
                                operation.scissor,
                                target.logical_extent,
                                (w, h),
                            );
                            inputs.operations.push(OperationInputs {
                                pc: operation.pc.unwrap_or(0),
                                scissor: super::clamp_scissor(&scissor, w, h),
                                draw,
                            });
                        }
                    }
                    inputs
                })
                .collect(),
        };
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
