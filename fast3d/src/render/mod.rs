//! wgpu pass-through renderer for the walking skeleton, extended with texture support.
use bytemuck::{Pod, Zeroable};

#[cfg(test)]
mod texrect_tests;

/// The depth format the Z-buffer uses. `Depth32Float` is WebGL2-core (`DEPTH_COMPONENT32F`) and
/// matches `D32_FLOAT`. Callers that own the depth texture must use this format.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The background clear color used for BOTH the clear-only and draw passes. Lifted verbatim from
/// the web shell so the facade and the web consumer agree on the canvas background.
pub const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.05,
    g: 0.05,
    b: 0.08,
    a: 1.0,
};

/// Position+color+uv vertex stream (slot 0) — produced by the RSP-process compute pass.
/// std430/vertex stride 48: position @0, color @16, uv @32 (pad @40).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct OutVertex {
    pub position: [f32; 4],
    pub color: [f32; 4],
    pub uv: [f32; 2],
    pub _pad: [f32; 2],
}
const _: () = assert!(std::mem::size_of::<OutVertex>() == 48);
impl OutVertex {
    pub const ATTRS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x2];
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<OutVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// Maximum LOD level count bound as INDEPENDENT per-level textures (hardware-faithful; N64 hardware max
/// of 8 — the G_TEXTURE `level` field is 3 bits). Level 0 is `tex0` (@group(0) @binding 0); levels
/// 1..MAX_LOD occupy the fixed `tex_lod1..tex_lod7` slots at `LOD_BINDING_BASE..`. Kept in lockstep
/// with `hle::MAX_LOD_LEVELS` (asserted below).
///
/// Fragment-stage sampled-texture budget (wgpu 29 `Limits::default()`
/// `max_sampled_textures_per_shader_stage = 16`): `tex0` (1) + 7 LOD levels + `tex1` (1) +
/// `tex_detail` (1) = 10 ≤ 16. Samplers used: `samp0` (shared across ALL LOD levels), `samp1`,
/// `samp_detail` = 3 ≤ `max_samplers_per_shader_stage = 16`. NO binding_array / bindless (unavailable
/// on the WebGPU baseline).
const MAX_LOD: u32 = 8;
const _: () = assert!(MAX_LOD == crate::hle::MAX_LOD_LEVELS);
/// First `@group(0)` binding for the LOD-level textures 1..MAX_LOD (bindings 6..=12). Bindings 0..5
/// are tex0/samp0, tex1/samp1, tex_detail/samp_detail.
const LOD_BINDING_BASE: u32 = 6;

/// Clamp a declared LOD level count to the fixed per-level binding budget (`MAX_LOD`). LOD levels are
/// now independent textures (no halving mip chain), so the count is NOT bounded by the base dims —
/// only by how many `tex_lod*` bindings exist. Both the upload (`build_tex_entry`) and the shader
/// uniform (`CombinerUniform::from_run` → `lod_params.y`) share this helper so they can never drift.
fn uploaded_level_count(declared: u8) -> u32 {
    (declared.max(1) as u32).min(MAX_LOD)
}

/// The `MAX_LOD - 1` independent LOD-level texture bind-group entries (bindings 6..=12), all pointing
/// at `view`. Appended to EVERY `@group(0)` bind group: the material draw path overrides them with
/// real per-level textures (`build_tex_entry`); every other path (fill / present / fb-source / blit /
/// test harness) binds the shared 1×1 dummy here — those paths never sample a LOD level.
fn lod_level_entries(view: &wgpu::TextureView) -> impl Iterator<Item = wgpu::BindGroupEntry<'_>> {
    (0..MAX_LOD - 1).map(move |i| wgpu::BindGroupEntry {
        binding: LOD_BINDING_BASE + i,
        resource: wgpu::BindingResource::TextureView(view),
    })
}

const TILE_SAMPLING_BINDING: u32 = 13;
const TILE_SAMPLING_COUNT: usize = MAX_LOD as usize + 2;

type TileSamplingArray = [crate::hle::tile_sampling::TileSampling; TILE_SAMPLING_COUNT];

pub(crate) fn material_sampling(mat: &crate::hle::Material) -> TileSamplingArray {
    let mut tiles = [crate::hle::tile_sampling::TileSampling::default(); TILE_SAMPLING_COUNT];
    tiles[0] = mat.sampling;
    if let Some(tex) = &mat.tex1 {
        tiles[1] = tex.sampling;
    }
    if let Some(tex) = &mat.detail_tex {
        tiles[2] = tex.sampling;
    }
    for (i, level) in mat.mip_levels.iter().take(MAX_LOD as usize).enumerate() {
        tiles[if i == 0 { 0 } else { i + 2 }] = level.sampling;
    }
    tiles
}

pub(crate) fn sampling_buffer(device: &wgpu::Device, tiles: &TileSamplingArray) -> wgpu::Buffer {
    use wgpu::util::DeviceExt;

    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("tile-sampling"),
        contents: bytemuck::cast_slice(tiles),
        usage: wgpu::BufferUsages::UNIFORM,
    })
}

pub(crate) fn sampling_entry(buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding: TILE_SAMPLING_BINDING,
        resource: buffer.as_entire_binding(),
    }
}

pub(crate) fn image_sampling_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    let mut tile = crate::hle::tile_sampling::TileSampling::default();
    tile.image[2] = 2;
    sampling_buffer(device, &[tile; TILE_SAMPLING_COUNT])
}

/// The combiner uniform; field order matches WGSL `Combiner` and fits a 256-byte draw slot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct CombinerUniform {
    pub combine_l: u32,        // raw w0 (combine_l = w0)
    pub combine_h: u32,        // raw w1 (combine_h = w1)
    pub cycle_type: u32,       // 0 = 1-cycle, 1 = 2-cycle, 2 = COPY
    pub tex_enable: u32,       // 1 if texture is enabled, 0 otherwise
    pub blender_mux: u32,      // raw blender mux (other_mode_l bits [31:16])
    pub force_blend: u32,      // 1 if FORCE_BL is set, 0 otherwise
    pub alpha_flags: u32, // bits 0..1: alpha compare (0=off, 1=threshold, 3=dither); bit 2: coverage
    pub alpha_threshold: f32, // blend-color alpha threshold
    pub prim: [f32; 4],   // primitive color RGBA, normalized 0..1
    pub env: [f32; 4],    // environment color RGBA, normalized 0..1
    pub blend_color: [f32; 4], // blend color RGBA, normalized 0..1 (from mat.blend_color / G_SETBLENDCOLOR)
    pub fog_color: [f32; 4],   // normalized draw fog color RGBA
    /// `.xy` = 1/(tex_w, tex_h) for texel-space triangle and rectangle coordinates.
    /// `.z` = 1 for rectangles, 0 for triangles; `.w` is padding.
    pub inv_tex_size: [f32; 4],
    /// TEXEL1 mirror of `inv_tex_size` (second-texture params). `.xy` = 1/(tex1_w, tex1_h); `.z` =
    /// `tex_enable1` (1.0 when the material carries a second texture — `tile_count == 2` — else 0.0);
    /// `.w` = filter mode (0 = point, 2 = bilerp, 3 = average). Must stay in lockstep with
    /// the WGSL `Combiner.inv_tex1_size`.
    pub inv_tex1_size: [f32; 4],
    /// LOD / mipmapping parameters (hardware-faithful LOD). std140 layout:
    /// `.x` = lod_enable (0.0 = G_TL_LOD off; 1.0 when LOD sampling is active),
    /// `.y` = num_levels (declared mip level count; the shader independently re-clamps this to the
    /// REAL uploaded mip count — see `compute_lod`'s caller in `combiner_prelude.wgsl`),
    /// `.z` = prim_lod_frac (the primitive LOD fraction = lodFrac/256; drives the PRIM_LOD_FRAC selector),
    /// `.w` = lod_scale (resolution LOD scale; 1.0, native res).
    /// Grows the struct by exactly one std140 row (128 -> 144). Must stay in LOCKSTEP with the WGSL
    /// `Combiner.lod_params`.
    pub lod_params: [f32; 4],
    /// DETAIL-tile params (hardware-faithful LOD). std140 layout:
    /// `.xy` = 1/(detail_w, detail_h) when the material carries a DETAIL tile, else `[1, 1]`;
    /// `.z` = prim_lod_min (the primitive LOD minimum = lodMin/32; floors `maxDst` under DETAIL/SHARPEN —
    /// see `compute_lod`); `.w` = detail_mode bits (bit0 = SHARPEN, bit1 = DETAIL — DETAIL is set
    /// only when a real DETAIL tile was decoded, so a fallback-decoded material with the othermode
    /// bit set but no faithful tile never engages detail sampling). Non-detail, non-LOD draws leave
    /// this `[1, 1, 0, 0]`, byte-identical to the prior tail. Grows the struct by exactly one
    /// std140 row (144 -> 160). Must stay in LOCKSTEP with the WGSL `Combiner.inv_detail_size`.
    pub inv_detail_size: [f32; 4],
    /// Low 32 bits of frame serial, dither seed, framebuffer width and height.
    pub frame: [u32; 4],
}
const _: () = assert!(std::mem::size_of::<CombinerUniform>() == 176);

impl CombinerUniform {
    fn alpha_flags(rm: &crate::hle::RenderMode) -> u32 {
        let compare = match rm.alpha_compare {
            crate::hle::AlphaCompare::None => 0,
            crate::hle::AlphaCompare::Threshold => 1,
            crate::hle::AlphaCompare::Dither => 3,
        };
        compare | (u32::from(rm.cvg_x_alpha) << 2)
    }

    /// Build a `CombinerUniform` from a material + run's render mode + draw fog color.
    ///
    /// The blender fields are wired from `rm`; `blend_color` is
    /// derived from `mat.blend_color` (set by G_SETBLENDCOLOR; default [0,0,0,255]); `fog_color`
    /// is captured by the draw.
    pub fn from_run(
        mat: &crate::hle::Material,
        rm: &crate::hle::RenderMode,
        fog_color: [u8; 4],
    ) -> Self {
        let prim = [
            mat.prim[0] as f32 / 255.0,
            mat.prim[1] as f32 / 255.0,
            mat.prim[2] as f32 / 255.0,
            mat.prim[3] as f32 / 255.0,
        ];
        let env = [
            mat.env[0] as f32 / 255.0,
            mat.env[1] as f32 / 255.0,
            mat.env[2] as f32 / 255.0,
            mat.env[3] as f32 / 255.0,
        ];
        let blend_color = [
            mat.blend_color[0] as f32 / 255.0,
            mat.blend_color[1] as f32 / 255.0,
            mat.blend_color[2] as f32 / 255.0,
            mat.blend_color[3] as f32 / 255.0,
        ];
        CombinerUniform {
            combine_l: mat.selectors.raw_l,
            combine_h: mat.selectors.raw_h,
            cycle_type: mat.cycle_type,
            tex_enable: if mat.tex_enable { 1 } else { 0 },
            blender_mux: rm.blender_mux as u32,
            force_blend: if rm.force_blend { 1 } else { 0 },
            alpha_flags: Self::alpha_flags(rm),
            alpha_threshold: blend_color[3],
            prim,
            env,
            blend_color,
            fog_color: fog_color.map(|c| c as f32 / 255.0),
            inv_tex_size: [1.0, 1.0, 0.0, 0.0],
            inv_tex1_size: match &mat.tex1 {
                Some(t) => [
                    1.0 / t.tex_w.max(1) as f32,
                    1.0 / t.tex_h.max(1) as f32,
                    1.0,
                    mat.filter_mode as f32,
                ],
                None => [1.0, 1.0, 0.0, mat.filter_mode as f32],
            },
            // LOD params: lod_enable = 1.0 when the material's per-level texture set is active
            // (`mat.lod`), else 0.0; num_levels is the REAL uploaded level count (clamped to MAX_LOD
            // the same way `build_tex_entry` clamps its uploads, so the shader never selects a level
            // past what was actually bound); prim_lod_frac from the material (the primitive LOD fraction);
            // lod_scale = 1. A non-LOD material keeps `[0, 1, prim_lod_frac, 1]` — byte-identical to
            // before (the shader gates all new consumption on `.x != 0`). No non-LOD golden sets
            // `mat.lod`.
            lod_params: [
                if mat.lod { 1.0 } else { 0.0 },
                uploaded_level_count(mat.num_levels) as f32,
                mat.prim_lod_frac,
                1.0,
            ],
            // DETAIL params: `.xy` = 1/(detail dims) when the material carries a DETAIL tile, else
            // `[1, 1]`. `.z` = prim_lod_min (the primitive LOD minimum, always threaded — only consumed under
            // DETAIL/SHARPEN). `.w` = detail_mode bits (bit0 SHARPEN from `mat.text_detail`; bit1
            // DETAIL gated on `mat.detail_tex.is_some()` so a fallback-decoded material with the
            // othermode bit set but no faithful tile never engages detail sampling).
            inv_detail_size: {
                let (dw, dh) = match &mat.detail_tex {
                    Some(d) => (1.0 / d.w.max(1) as f32, 1.0 / d.h.max(1) as f32),
                    None => (1.0, 1.0),
                };
                let sharpen_bit = if mat.text_detail & 0b01 != 0 {
                    1.0
                } else {
                    0.0
                };
                let detail_bit = if mat.detail_tex.is_some() { 2.0 } else { 0.0 };
                [dw, dh, mat.prim_min_level, sharpen_bit + detail_bit]
            },
            frame: [0; 4],
        }
    }

    fn from_rect(
        mat: &crate::hle::Material,
        rm: &crate::hle::RenderMode,
        fog_color: [u8; 4],
    ) -> Self {
        let mut uniform = Self::from_run(mat, rm, fog_color);
        uniform.inv_tex_size = triangle_inv_tex_size(mat);
        uniform.inv_tex_size[2] = 1.0;
        uniform
    }

    /// Synthesize a `CombinerUniform` for a `SceneOp::FillRect` (the 2D solid-rect path).
    ///
    /// The RDP fill-color register (`color_raw`) is resolved to normalized RGBA8 via the color
    /// image's pixel size (`siz`) and placed in the `prim` register (`tex_enable = 0`). The combine
    /// words encode a flat-PRIM passthrough so the fragment shader emits the fill color verbatim:
    /// 1-cycle color = (0 − 0)·0 + PRIMITIVE; alpha = (0 − 0)·0 + PRIM_ALPHA. `blend_color` is NOT
    /// a combiner input (it drives the blender), so it stays zeroed.
    pub fn fill_rect(color_raw: u32, siz: u8) -> Self {
        // Resolve the fill color to normalized RGBA8 by the color image's pixel size.
        let prim = if siz == 3 {
            // G_IM_SIZ_32b: color_raw is RGBA8888 directly.
            [
                ((color_raw >> 24) & 0xFF) as f32 / 255.0,
                ((color_raw >> 16) & 0xFF) as f32 / 255.0,
                ((color_raw >> 8) & 0xFF) as f32 / 255.0,
                (color_raw & 0xFF) as f32 / 255.0,
            ]
        } else {
            // G_IM_SIZ_16b (and the degenerate 4b/8b cases): the high 16 bits are an RGBA5551
            // pixel (the fill word replicates the pixel across both halves). Bit-replicate each
            // 5-bit channel to 8 bits — identical to `decode_rgba16`.
            let p = ((color_raw >> 16) & 0xFFFF) as u16;
            let r5 = ((p >> 11) & 0x1F) as u8;
            let g5 = ((p >> 6) & 0x1F) as u8;
            let b5 = ((p >> 1) & 0x1F) as u8;
            let a1 = (p & 0x1) as u8;
            [
                (((r5 << 3) | (r5 >> 2)) as f32) / 255.0,
                (((g5 << 3) | (g5 >> 2)) as f32) / 255.0,
                (((b5 << 3) | (b5 >> 2)) as f32) / 255.0,
                if a1 != 0 { 1.0 } else { 0.0 },
            ]
        };
        // Flat-PRIM passthrough combine words. Field positions mirror the 1-cycle (cycle-1) slots
        // the shader reads (combiner_prelude.wgsl eval_combiner): color a=L[5,4] b=H[24,4]
        // c=L[0,5] d=H[6,3]; alpha a=H[21,3] b=H[3,3] c=H[18,3] d=H[0,3]. Color a/b/c = G_CCMUX_0
        // (→ shader ZERO once masked to field width), d = G_CCMUX_PRIMITIVE. Alpha a/b/c = G_ACMUX_0
        // (→ shader ZERO), d = G_ACMUX_PRIMITIVE.
        const CC_0: u32 = 31; // G_CCMUX_0
        const CC_PRIM: u32 = 3; // G_CCMUX_PRIMITIVE
        const AC_0: u32 = 7; // G_ACMUX_0
        const AC_PRIM: u32 = 3; // G_ACMUX_PRIMITIVE
        let combine_l = ((CC_0 & 0xF) << 5) | (CC_0 & 0x1F);
        let combine_h = ((CC_0 & 0xF) << 24)
            | ((CC_PRIM & 0x7) << 6)
            | ((AC_0 & 0x7) << 21)
            | ((AC_0 & 0x7) << 3)
            | ((AC_0 & 0x7) << 18)
            | (AC_PRIM & 0x7);
        CombinerUniform {
            combine_l,
            combine_h,
            cycle_type: 0,
            tex_enable: 0,
            blender_mux: 0,
            force_blend: 0,
            alpha_flags: 0,
            alpha_threshold: 0.0,
            prim,
            env: [0.0; 4],
            blend_color: [0.0; 4],
            fog_color: [0.0; 4],
            inv_tex_size: [1.0, 1.0, 0.0, 0.0],
            // FillRect is untextured — no second texture (tex_enable1 = 0).
            inv_tex1_size: [1.0, 1.0, 0.0, 0.0],
            // Untextured solid rect — non-LOD defaults (no combiner LOD selector on this path).
            lod_params: [0.0, 1.0, 0.0, 1.0],
            // No DETAIL tile on the fill path.
            inv_detail_size: [1.0, 1.0, 0.0, 0.0],
            frame: [0; 4],
        }
    }

    /// COPY bypasses the combiner and blender, but still compares sampled texel alpha.
    /// Threshold compare and the coverage approximation use a half-alpha cutoff for RGBA/IA.
    pub fn tex_copy(rm: Option<&crate::hle::RenderMode>, fmt: u8) -> Self {
        let mut alpha_flags = rm.map_or(0, Self::alpha_flags);
        if fmt != 0 && fmt != 3 {
            alpha_flags &= !4;
            if alpha_flags & 3 == 1 {
                alpha_flags &= !3;
            }
        }
        CombinerUniform {
            combine_l: 0,
            combine_h: 0,
            cycle_type: 2,
            tex_enable: 1,
            blender_mux: 0,
            force_blend: 0,
            alpha_flags,
            alpha_threshold: 0.5,
            prim: [0.0; 4],
            env: [0.0; 4],
            blend_color: [0.0; 4],
            fog_color: [0.0; 4],
            inv_tex_size: [1.0, 1.0, 0.0, 0.0],
            // COPY-mode TexRect samples a single tile — no second texture (tex_enable1 = 0).
            inv_tex1_size: [1.0, 1.0, 0.0, 0.0],
            // COPY mode bypasses the combiner — non-LOD defaults.
            lod_params: [0.0, 1.0, 0.0, 1.0],
            // No DETAIL tile on the COPY-mode TexRect path.
            inv_detail_size: [1.0, 1.0, 0.0, 0.0],
            frame: [0; 4],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_sampling_uniform_and_shaders_fit_webgpu() {
        assert_eq!(std::mem::size_of::<CombinerUniform>(), 176);
        assert_eq!(std::mem::size_of::<TileSamplingArray>(), 960);
        let limits = wgpu::Limits::default();
        assert!(
            std::mem::size_of::<TileSamplingArray>()
                <= limits.max_uniform_buffer_binding_size as usize
        );
        assert!((TILE_SAMPLING_COUNT as u32) < limits.max_sampled_textures_per_shader_stage);
        assert!(TILE_SAMPLING_BINDING < limits.max_bindings_per_bind_group);
        for (prefix, body, decal) in [
            (
                "",
                include_str!("skeleton.wgsl"),
                include_str!("decal.wgsl"),
            ),
            (
                "enable dual_source_blending;\n",
                include_str!("blender_dualsrc.wgsl"),
                include_str!("decal_dual.wgsl"),
            ),
        ] {
            let source = format!(
                "{prefix}{}\n{body}\n{decal}",
                include_str!("combiner_prelude.wgsl")
            );
            let module = wgpu::naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|err| panic!("{}", err.emit_to_string(&source)));
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|err| panic!("{}", err.emit_to_string(&source)));
        }
    }

    fn test_material() -> crate::hle::Material {
        crate::hle::Material {
            convert: Default::default(),
            key: Default::default(),
            sampling: Default::default(),
            texture: vec![255u8; 4],
            tex_w: 1,
            tex_h: 1,
            selectors: crate::hle::combiner::decode_combine(0x00_00_00_00, 0x00_00_00_00),
            cycle_type: 0,
            filter_mode: 0,
            prim: [0, 0, 0, 255],
            env: [0, 0, 0, 255],
            blend_color: [0, 0, 0, 255],
            tex_enable: false,
            wrap_s: 2,
            wrap_t: 2,
            fmt: 0,
            siz: 0,
            tile_count: 0,
            tex1: None,
            prim_lod_frac: 0.0,
            prim_min_level: 0.0,
            lod: false,
            num_levels: 1,
            text_detail: 0,
            mip_levels: Vec::new(),
            detail_tex: None,
        }
    }

    #[test]
    fn combiner_uniform_from_run_carries_blender_mux() {
        let mat = test_material();
        // IMP5: renderer crate has no gbi-consts dep; use crate::hle::consts::rdp::* (gbi is a dev-dep
        // re-exporting gbi_consts as `consts`). `crate::hle::consts::rdp::…` would NOT resolve here.
        let rm = crate::hle::decode_render_mode(crate::hle::consts::rdp::G_RM_AA_ZB_XLU_SURF, 0, 0);
        let u = CombinerUniform::from_run(&mat, &rm, [0; 4]);
        assert_eq!(u.blender_mux, rm.blender_mux as u32);
        assert_eq!(u.force_blend, 1);
        assert!(std::mem::size_of::<CombinerUniform>() <= 256);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn headless_device_reports_dual_source_flag() {
        let (device, _q, dual) = headless_device();
        // Deterministic across probes.
        let (_d2, _q2, dual2) = headless_device();
        assert_eq!(dual, dual2);
        // MIN9: when the flag is true, the returned device must ACTUALLY have the feature enabled
        // (proves it was requested, not just advertised by the adapter).
        if dual {
            assert!(device
                .features()
                .contains(wgpu::Features::DUAL_SOURCE_BLENDING));
        }
    }

    /// Smoke test: `SceneRenderer::new` at both `Rgba8Unorm` and `Bgra8Unorm` must not panic.
    /// Verifies the dual draw-pipeline matrices (textured at color_format + textured_fb at
    /// Rgba8Unorm) and the present pipeline all compile successfully at both surface formats.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn scene_renderer_new_at_rgba8_and_bgra8() {
        let (device, _q, dual) = headless_device();
        let _ = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
        let _ = SceneRenderer::new(&device, wgpu::TextureFormat::Bgra8Unorm, 64, 64, dual);
    }
}

/// RGBA16 -> RGBA8 decode. The single implementation lives in `hle`; re-exported here so
/// the renderer's texture path and tests share one decoder (no drift).
#[cfg_attr(not(test), allow(unused_imports))]
pub use crate::hle::decode_rgba16;

fn rect_quad(
    rect: &crate::hle::Rect,
    fb_w: u32,
    fb_h: u32,
    color: [f32; 4],
    uv: [[f32; 2]; 4],
) -> [OutVertex; 6] {
    screen_quad(
        [rect.ulx, rect.uly, rect.lrx + 1, rect.lry + 1],
        (fb_w, fb_h),
        color,
        uv,
    )
}

fn screen_quad(
    bounds: [i32; 4],
    extent: (u32, u32),
    color: [f32; 4],
    uv: [[f32; 2]; 4],
) -> [OutVertex; 6] {
    let fw = extent.0.max(1) as f32;
    let fh = extent.1.max(1) as f32;
    let [left, top, right, bottom] = bounds.map(|v| v as f32);
    let ndc_x = |px: f32| px / fw * 2.0 - 1.0;
    let ndc_y = |py: f32| 1.0 - py / fh * 2.0; // Y-flip
    let v = |px: f32, py: f32, uv: [f32; 2]| OutVertex {
        position: [ndc_x(px), ndc_y(py), 0.0, 1.0],
        color,
        uv,
        _pad: [0.0; 2],
    };
    let tl = v(left, top, uv[0]);
    let tr = v(right, top, uv[1]);
    let br = v(right, bottom, uv[2]);
    let bl = v(left, bottom, uv[3]);
    // Two triangles covering the quad; winding is irrelevant (rects draw cull-disabled).
    [tl, bl, br, tl, br, tr]
}

/// Normalize texel-space triangle UVs using the tile dimensions captured at draw time.
pub fn triangle_inv_tex_size(mat: &crate::hle::Material) -> [f32; 4] {
    if mat.tex_enable || mat.lod {
        [
            1.0 / mat.tex_w.max(1) as f32,
            1.0 / mat.tex_h.max(1) as f32,
            0.0,
            0.0,
        ]
    } else {
        [1.0, 1.0, 0.0, 0.0]
    }
}

fn texrect_quad(
    rect: &crate::hle::TexRectBounds,
    base: (i16, i16),
    step: (i16, i16),
    flip: bool,
    copy_mode: bool,
    extent: (u32, u32),
) -> [OutVertex; 6] {
    let [left, top, right, bottom] = if copy_mode {
        [
            rect.ulx >> 2,
            rect.uly >> 2,
            (rect.lrx >> 2) + 1,
            (rect.lry >> 2) + 1,
        ]
    } else {
        [rect.ulx, rect.uly, rect.lrx, rect.lry].map(|v| (v + 3) >> 2)
    };
    let bounds = [left, top, right.max(left), bottom.max(top)];
    let width = i64::from(bounds[2] - left);
    let height = i64::from(bounds[3] - top);
    let ds = i64::from(if copy_mode { step.0 >> 2 } else { step.0 });
    let dt = i64::from(step.1);
    let (sw, th) = if flip {
        (height, width)
    } else {
        (width, height)
    };
    // rt64 drawRect truncates endpoints to S10.5 and advances T for any fractional upper Y.
    let endpoint = |base: i16, step: i64, pixels: i64| {
        (((i64::from(base) << 7) + step * (pixels << 2)) >> 7) as f32 / 32.0
    };
    let t_offset = if !copy_mode && rect.uly & 3 != 0 {
        (dt >> 5) as f32 / 32.0
    } else {
        0.0
    };
    let s0 = f32::from(base.0) / 32.0;
    let t0 = f32::from(base.1) / 32.0 + t_offset;
    let s1 = endpoint(base.0, ds, sw);
    let t1 = endpoint(base.1, dt, th) + t_offset;
    let uv = if flip {
        [[s0, t0], [s0, t1], [s1, t1], [s1, t0]]
    } else {
        [[s0, t0], [s1, t0], [s1, t1], [s0, t1]]
    };
    screen_quad(bounds, extent, [1.0; 4], uv)
}

/// Clamp an N64 `Scissor` (pixel coords, possibly negative or larger than the FB) to a
/// `(x, y, w, h)` rect that fits inside `fb_w × fb_h`. wgpu's `set_scissor_rect` PANICS if
/// `x + w > attachment_width` (or the Y analog), so every field is saturated into range first.
fn clamp_scissor(s: &crate::hle::Scissor, fb_w: u32, fb_h: u32) -> (u32, u32, u32, u32) {
    let x = (s.ulx.max(0) as u32).min(fb_w);
    let y = (s.uly.max(0) as u32).min(fb_h);
    let right = (s.lrx.max(0) as u32).min(fb_w);
    let bottom = (s.lry.max(0) as u32).min(fb_h);
    (x, y, right.saturating_sub(x), bottom.saturating_sub(y))
}

/// The textured rendering pipeline with split bind groups:
/// `@group(0)` carries the texture+sampler; `@group(1)` carries the combiner uniform
/// (with `has_dynamic_offset: true` so A8b can stride per-run offsets).
///
/// Each depth×cull combination has TWO pipeline variants — Replace and AlphaOver — selected
/// at draw time by `run.render_mode.fallback_class` (B3: fixed-function fallback path).
pub struct TexturedPipeline {
    // ── Replace blend (src_factor=One, dst_factor=Zero) ──────────────────────────────────────
    pipeline_no_depth_nocull: wgpu::RenderPipeline,
    pipeline_no_depth_cull: wgpu::RenderPipeline,
    /// Depth-format-compatible but depth-passthrough (Always compare, no write).
    /// Used when the render pass has a depth attachment but this run does not test/write depth.
    /// WebGPU requires all pipelines in a depth-attached pass to declare the same depth format.
    pipeline_depth_compat_nocull: wgpu::RenderPipeline,
    pipeline_depth_compat_cull: wgpu::RenderPipeline,
    pipeline_depth_test_write_nocull: wgpu::RenderPipeline,
    pipeline_depth_test_write_cull: wgpu::RenderPipeline,
    pipeline_depth_test_nowrite_nocull: wgpu::RenderPipeline,
    pipeline_depth_test_nowrite_cull: wgpu::RenderPipeline,
    // ── AlphaOver blend (color: SrcAlpha/OneMinusSrcAlpha, alpha: One/Zero) ─────────────────
    pipeline_ao_no_depth_nocull: wgpu::RenderPipeline,
    pipeline_ao_no_depth_cull: wgpu::RenderPipeline,
    pipeline_ao_depth_compat_nocull: wgpu::RenderPipeline,
    pipeline_ao_depth_compat_cull: wgpu::RenderPipeline,
    pipeline_ao_depth_test_write_nocull: wgpu::RenderPipeline,
    pipeline_ao_depth_test_write_cull: wgpu::RenderPipeline,
    pipeline_ao_depth_test_nowrite_nocull: wgpu::RenderPipeline,
    pipeline_ao_depth_test_nowrite_cull: wgpu::RenderPipeline,
    /// Dual-source primary blender pipelines (`(P·A+M·B)/(A+B)` via @blend_src). `Some` ONLY on a
    /// device with `DUAL_SOURCE_BLENDING` enabled — `None` on the fallback device, where DualSrc
    /// runs take the AlphaOver fallback path instead. Selected for runs whose `blend_class` is
    /// `DualSrc` (B4 primary path).
    dual: Option<DualSrcSet>,
    /// Decal pipelines, built for the distinct decal pipeline layout `(group0, group1, group2)`.
    /// Used for ZMODE_DEC runs in the SECOND (decal) render pass, which has NO depth attachment
    /// (so only no-depth variants exist) and binds the prior pass's depth as a sampled texture at
    /// `@group(2)` (E1). group0/group1 BGLs are byte-identical to the non-decal layout's, so
    /// group0/group1 bindings survive `set_pipeline` across the two passes.
    decal: DecalSet,
    group0_bgl: wgpu::BindGroupLayout,
    group1_bgl: wgpu::BindGroupLayout,
    group2_depth_bgl: wgpu::BindGroupLayout,
}

/// The no-depth decal pipelines (decal layout `g0+g1+g2`). The decal pass carries no depth
/// attachment, so only the no-depth Replace/AlphaOver (and, on a dual-source device, the
/// dual-source) variants are needed. Mirrors the no-depth slice of `TexturedPipeline::select`.
struct DecalSet {
    no_depth_nocull: wgpu::RenderPipeline,
    no_depth_cull: wgpu::RenderPipeline,
    ao_no_depth_nocull: wgpu::RenderPipeline,
    ao_no_depth_cull: wgpu::RenderPipeline,
    /// `(nocull, cull)` dual-source no-depth variants; `Some` only on a DUAL_SOURCE_BLENDING device.
    dual: Option<(wgpu::RenderPipeline, wgpu::RenderPipeline)>,
}

/// The eight depth×cull dual-source primary blender pipelines (mirrors the Replace/AlphaOver
/// matrices). Built only on a `DUAL_SOURCE_BLENDING` device; see `TexturedPipeline::dual`.
struct DualSrcSet {
    no_depth_nocull: wgpu::RenderPipeline,
    no_depth_cull: wgpu::RenderPipeline,
    depth_compat_nocull: wgpu::RenderPipeline,
    depth_compat_cull: wgpu::RenderPipeline,
    depth_test_write_nocull: wgpu::RenderPipeline,
    depth_test_write_cull: wgpu::RenderPipeline,
    depth_test_nowrite_nocull: wgpu::RenderPipeline,
    depth_test_nowrite_cull: wgpu::RenderPipeline,
}

impl DualSrcSet {
    /// Pick the dual-source pipeline variant for a run's cull/depth state (same matrix as
    /// `TexturedPipeline::select`, but always the dual-source blend).
    fn select(
        &self,
        cull: crate::hle::CullKind,
        z_test: bool,
        z_write: bool,
        any_depth: bool,
    ) -> &wgpu::RenderPipeline {
        match (cull, z_test, z_write) {
            (crate::hle::CullKind::None, false, _) => {
                if any_depth {
                    &self.depth_compat_nocull
                } else {
                    &self.no_depth_nocull
                }
            }
            (crate::hle::CullKind::Cull, false, _) => {
                if any_depth {
                    &self.depth_compat_cull
                } else {
                    &self.no_depth_cull
                }
            }
            (crate::hle::CullKind::None, true, true) => &self.depth_test_write_nocull,
            (crate::hle::CullKind::Cull, true, true) => &self.depth_test_write_cull,
            (crate::hle::CullKind::None, true, false) => &self.depth_test_nowrite_nocull,
            (crate::hle::CullKind::Cull, true, false) => &self.depth_test_nowrite_cull,
        }
    }
}

impl TexturedPipeline {
    pub fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        // The combiner prelude (structs, bindings, helpers, eval_combiner) is shared by the base
        // ubershader and the dual-source blender; each fragment entry is concatenated after it.
        const COMBINER_PRELUDE: &str = include_str!("combiner_prelude.wgsl");
        // The base module carries skeleton's non-decal `fs_main` AND decal.wgsl's `fs_decal`
        // (E2: combiner + in-shader Z occlusion/coplanar). decal.wgsl declares `@group(2)` depth,
        // which `fs_main` never references — so non-decal pipelines (layout g0+g1) stay valid while
        // the decal pipelines (decal layout g0+g1+g2) select the `fs_decal` entry.
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("skeleton-ubershader"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}",
                    COMBINER_PRELUDE,
                    include_str!("skeleton.wgsl"),
                    include_str!("decal.wgsl")
                )
                .into(),
            ),
        });
        // Ground truth for the dual-source path is the device's ENABLED feature set (equivalent to
        // `SceneRenderer.dual_source`): the headless/web device only carries DUAL_SOURCE_BLENDING
        // when the adapter advertised it AND it was requested. [§5.3] We must NOT pass a `@blend_src`
        // module to create_shader_module on a device lacking the feature (naga rejects it), so the
        // dual module + pipelines are built ONLY inside this guard.
        let dual_source = device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING);

        // group0_bgl: texture + sampler (@group(0)). Bindings 0/1 = tex0/samp0, 2/3 = tex1/samp1,
        // 4/5 = tex_detail/samp_detail, 6..=12 = the MAX_LOD-1 independent LOD-level textures
        // (`tex_lod1..tex_lod7`, appended below). All texture entries are Float{filterable} D2.
        let mut group0_entries: Vec<wgpu::BindGroupLayoutEntry> = vec![
            // binding 0: texture
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            // binding 1: sampler
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            // binding 2: tex1 (TEXEL1). Always present — single-texture draws bind a 1×1 dummy so
            // this layout is uniformly satisfied across every group(0) bind group (draw,
            // FB-as-texture alias, and present/scanout blits, which never sample it).
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            // binding 3: tex1 sampler.
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            // binding 4: tex_detail (the DETAIL tile). Always present — non-detail draws bind a
            // 1×1 dummy so the layout is uniformly satisfied across every group(0) bind group.
            // Sampled under DETAIL mode; uploaded regardless.
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            // binding 5: tex_detail sampler.
            wgpu::BindGroupLayoutEntry {
                binding: 5,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ];
        // bindings 6..=12: the MAX_LOD-1 independent LOD-level textures (`tex_lod1..tex_lod7`). All
        // share `samp0` (no extra samplers), so only texture entries are appended here.
        group0_entries.extend((0..MAX_LOD - 1).map(|i| wgpu::BindGroupLayoutEntry {
            binding: LOD_BINDING_BASE + i,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }));
        group0_entries.push(wgpu::BindGroupLayoutEntry {
            binding: TILE_SAMPLING_BINDING,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: std::num::NonZeroU64::new(
                    std::mem::size_of::<TileSamplingArray>() as u64,
                ),
            },
            count: None,
        });
        let group0_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("textured-group0-bgl"),
            entries: &group0_entries,
        });

        // group1_bgl: combiner uniform (@group(1)), dynamic offset for A8b per-run stride.
        let group1_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("textured-group1-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: std::num::NonZeroU64::new(
                        std::mem::size_of::<CombinerUniform>() as u64,
                    ),
                },
                count: None,
            }],
        });

        // group2_depth_bgl: the scene depth buffer bound as a SAMPLED `texture_depth_2d` (E1).
        // sample_type Depth, no sampler entry (depth is read via `textureLoad` in E2). Used only by
        // the decal pipeline layout; the decal pass binds the prior depth pass's output here.
        let group2_depth_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("textured-group2-depth-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skeleton-layout"),
            bind_group_layouts: &[Some(&group0_bgl), Some(&group1_bgl)],
            immediate_size: 0, // wgpu 29: replaces push_constant_ranges
        });
        // Distinct decal pipeline layout: group0 + group1 + group2(depth). group0/group1 are the
        // SAME descriptors as the non-decal layout (WebGPU layout-compatibility), so group0/group1
        // bindings set in the depth pass survive `set_pipeline` into the decal pass. ≤ max_bind_groups=4.
        let decal_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skeleton-decal-layout"),
            bind_group_layouts: &[
                Some(&group0_bgl),
                Some(&group1_bgl),
                Some(&group2_depth_bgl),
            ],
            immediate_size: 0,
        });
        let make = |label: &str,
                    depth_stencil: Option<wgpu::DepthStencilState>,
                    cull_mode: Option<wgpu::Face>,
                    blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[OutVertex::layout()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    // wgpu facing == NDC winding (Y-up) here: N64-front is CCW-in-NDC = wgpu-front,
                    // so cull Back to drop N64-back faces (matches culling.rs).
                    cull_mode,
                    ..Default::default()
                },
                depth_stencil,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        // B3: two blend states — Replace (opaque) and AlphaOver (src_alpha / 1-src_alpha).
        // Replace: color {One, Zero, Add}, alpha {One, Zero, Add} — unchanged from Phase A.
        let replace = wgpu::BlendState::REPLACE;
        // AlphaOver: color {SrcAlpha, OneMinusSrcAlpha, Add}, alpha {One, Zero, Add}.
        // Used for G_RM_AA_ZB_XLU_SURF and similar runs on adapters without DUAL_SOURCE_BLENDING.
        let alphaover = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::Zero,
                operation: wgpu::BlendOperation::Add,
            },
        };
        // depth-test + write (the classic Z-buffer variant, used when any run has z_test or z_write)
        let ds_write = || wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };
        // depth-test only, no write (A9 will wire this for decal/transparent runs)
        let ds_nowrite = || wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };
        // depth-format-compatible passthrough: declares the depth format so the pipeline can be
        // used in a depth-attached render pass, but uses Always compare + no write so it has no
        // effect on the depth buffer. Used for runs that don't test/write depth when the pass has
        // a depth attachment because OTHER runs in the same scene DO use depth (multi-material).
        let ds_compat = || wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };
        let back = Some(wgpu::Face::Back);
        // ── Replace pipelines (opaque / G_RM_OPA_SURF family) ─────────────────────────────────
        let pipeline_no_depth_nocull = make("tp-nodepth-nocull", None, None, replace);
        let pipeline_no_depth_cull = make("tp-nodepth-cull", None, back, replace);
        let pipeline_depth_compat_nocull =
            make("tp-depth-compat-nocull", Some(ds_compat()), None, replace);
        let pipeline_depth_compat_cull =
            make("tp-depth-compat-cull", Some(ds_compat()), back, replace);
        let pipeline_depth_test_write_nocull =
            make("tp-depth-write-nocull", Some(ds_write()), None, replace);
        let pipeline_depth_test_write_cull =
            make("tp-depth-write-cull", Some(ds_write()), back, replace);
        let pipeline_depth_test_nowrite_nocull =
            make("tp-depth-nowrite-nocull", Some(ds_nowrite()), None, replace);
        let pipeline_depth_test_nowrite_cull =
            make("tp-depth-nowrite-cull", Some(ds_nowrite()), back, replace);
        // ── AlphaOver pipelines (XLU / AA_ZB_XLU_SURF family, fallback path) ──────────────────
        let pipeline_ao_no_depth_nocull = make("tp-ao-nodepth-nocull", None, None, alphaover);
        let pipeline_ao_no_depth_cull = make("tp-ao-nodepth-cull", None, back, alphaover);
        let pipeline_ao_depth_compat_nocull = make(
            "tp-ao-depth-compat-nocull",
            Some(ds_compat()),
            None,
            alphaover,
        );
        let pipeline_ao_depth_compat_cull = make(
            "tp-ao-depth-compat-cull",
            Some(ds_compat()),
            back,
            alphaover,
        );
        let pipeline_ao_depth_test_write_nocull = make(
            "tp-ao-depth-write-nocull",
            Some(ds_write()),
            None,
            alphaover,
        );
        let pipeline_ao_depth_test_write_cull =
            make("tp-ao-depth-write-cull", Some(ds_write()), back, alphaover);
        let pipeline_ao_depth_test_nowrite_nocull = make(
            "tp-ao-depth-nowrite-nocull",
            Some(ds_nowrite()),
            None,
            alphaover,
        );
        let pipeline_ao_depth_test_nowrite_cull = make(
            "tp-ao-depth-nowrite-cull",
            Some(ds_nowrite()),
            back,
            alphaover,
        );
        // ── Decal pipelines (E1, decal layout g0+g1+g2; no depth attachment) ──────────────────
        // The decal pass has no depth-stencil attachment, so all decal pipelines are depth_stencil:
        // None. They use the base module's `fs_decal` entry (E2: combiner + in-shader Z occlusion +
        // coplanar discard). Built against `decal_layout` so `@group(2)` (sampled depth) is in scope.
        let make_decal = |label: &str, cull_mode: Option<wgpu::Face>, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&decal_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[OutVertex::layout()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_decal"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let decal_no_depth_nocull = make_decal("tp-decal-nodepth-nocull", None, replace);
        let decal_no_depth_cull = make_decal("tp-decal-nodepth-cull", back, replace);
        let decal_ao_no_depth_nocull = make_decal("tp-decal-ao-nodepth-nocull", None, alphaover);
        let decal_ao_no_depth_cull = make_decal("tp-decal-ao-nodepth-cull", back, alphaover);
        // ── Dual-source primary pipelines (B4) ────────────────────────────────────────────────
        // Built ONLY on a DUAL_SOURCE_BLENDING device: the dual module contains `@blend_src`, which
        // naga rejects at create_shader_module on a device without the feature [§5.3]. The whole
        // assembly + module + pipeline construction is guarded behind `dual_source`.
        let dual = dual_source.then(|| {
            let dual_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("dualsrc-blender"),
                source: wgpu::ShaderSource::Wgsl(
                    format!(
                        "enable dual_source_blending;\n{}\n{}\n{}",
                        COMBINER_PRELUDE,
                        include_str!("blender_dualsrc.wgsl"),
                        include_str!("decal_dual.wgsl")
                    )
                    .into(),
                ),
            });
            // Blend: color {src One, dst Src1, Add}, alpha {src One, dst Zero, Add}.
            // out = color0 + Src1*dst = P·A/denom + dst·B/denom = (P·A + M·B)/(A+B).
            let dualsrc = wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::Src1,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::Zero,
                    operation: wgpu::BlendOperation::Add,
                },
            };
            let make_dual = |label: &str,
                             depth_stencil: Option<wgpu::DepthStencilState>,
                             cull_mode: Option<wgpu::Face>| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &dual_shader,
                        entry_point: Some("vs_main"),
                        buffers: &[OutVertex::layout()],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &dual_shader,
                        entry_point: Some("fs_main"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: target_format,
                            blend: Some(dualsrc),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode,
                        ..Default::default()
                    },
                    depth_stencil,
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
            };
            // Dual-source DECAL pipelines (decal layout g0+g1+g2; no depth attachment).
            let make_dual_decal = |label: &str, cull_mode: Option<wgpu::Face>| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&decal_layout),
                    vertex: wgpu::VertexState {
                        module: &dual_shader,
                        entry_point: Some("vs_main"),
                        buffers: &[OutVertex::layout()],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &dual_shader,
                        entry_point: Some("fs_decal"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: target_format,
                            blend: Some(dualsrc),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
            };
            let decal_dual = (
                make_dual_decal("tp-ds-decal-nodepth-nocull", None),
                make_dual_decal("tp-ds-decal-nodepth-cull", back),
            );
            let dualset = DualSrcSet {
                no_depth_nocull: make_dual("tp-ds-nodepth-nocull", None, None),
                no_depth_cull: make_dual("tp-ds-nodepth-cull", None, back),
                depth_compat_nocull: make_dual(
                    "tp-ds-depth-compat-nocull",
                    Some(ds_compat()),
                    None,
                ),
                depth_compat_cull: make_dual("tp-ds-depth-compat-cull", Some(ds_compat()), back),
                depth_test_write_nocull: make_dual(
                    "tp-ds-depth-write-nocull",
                    Some(ds_write()),
                    None,
                ),
                depth_test_write_cull: make_dual("tp-ds-depth-write-cull", Some(ds_write()), back),
                depth_test_nowrite_nocull: make_dual(
                    "tp-ds-depth-nowrite-nocull",
                    Some(ds_nowrite()),
                    None,
                ),
                depth_test_nowrite_cull: make_dual(
                    "tp-ds-depth-nowrite-cull",
                    Some(ds_nowrite()),
                    back,
                ),
            };
            (dualset, decal_dual)
        });
        // Split the optional (DualSrcSet, decal-dual-pair) into the `dual` field + the decal set's
        // dual variants (both `Some` together, on a DUAL_SOURCE_BLENDING device, or both `None`).
        let (dual, decal_dual) = match dual {
            Some((ds, dd)) => (Some(ds), Some(dd)),
            None => (None, None),
        };
        let decal = DecalSet {
            no_depth_nocull: decal_no_depth_nocull,
            no_depth_cull: decal_no_depth_cull,
            ao_no_depth_nocull: decal_ao_no_depth_nocull,
            ao_no_depth_cull: decal_ao_no_depth_cull,
            dual: decal_dual,
        };
        Self {
            pipeline_no_depth_nocull,
            pipeline_no_depth_cull,
            pipeline_depth_compat_nocull,
            pipeline_depth_compat_cull,
            pipeline_depth_test_write_nocull,
            pipeline_depth_test_write_cull,
            pipeline_depth_test_nowrite_nocull,
            pipeline_depth_test_nowrite_cull,
            pipeline_ao_no_depth_nocull,
            pipeline_ao_no_depth_cull,
            pipeline_ao_depth_compat_nocull,
            pipeline_ao_depth_compat_cull,
            pipeline_ao_depth_test_write_nocull,
            pipeline_ao_depth_test_write_cull,
            pipeline_ao_depth_test_nowrite_nocull,
            pipeline_ao_depth_test_nowrite_cull,
            dual,
            decal,
            group0_bgl,
            group1_bgl,
            group2_depth_bgl,
        }
    }

    /// `@group(0)` layout: texture + sampler.  Used by callers building per-material bind groups.
    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.group0_bgl
    }

    /// `@group(1)` layout: combiner uniform with dynamic offset.
    /// Used by callers building the uniform bind group (one per scene, not per material).
    pub fn uniform_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.group1_bgl
    }

    /// `@group(2)` layout: the scene depth buffer as a sampled `texture_depth_2d` (E1). Used by the
    /// decal pass to build the bind group that exposes the prior depth pass's output to decal runs.
    pub fn depth_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.group2_depth_bgl
    }

    /// Pick the decal pipeline (decal layout, no depth attachment) for a run's cull/blend state.
    /// Mirrors the no-depth slice of `select`: dual-source primary on a capable device for DualSrc
    /// runs, else the Replace/AlphaOver fallback keyed on `fallback_class`.
    fn select_decal(
        &self,
        cull: crate::hle::CullKind,
        fallback_class: crate::hle::BlendClass,
        blend_class: crate::hle::BlendClass,
    ) -> &wgpu::RenderPipeline {
        if let Some((nocull, cull_pl)) = &self.decal.dual {
            if blend_class == crate::hle::BlendClass::DualSrc {
                return match cull {
                    crate::hle::CullKind::None => nocull,
                    crate::hle::CullKind::Cull => cull_pl,
                };
            }
        }
        let ao = fallback_class == crate::hle::BlendClass::AlphaOver;
        match (ao, cull) {
            (false, crate::hle::CullKind::None) => &self.decal.no_depth_nocull,
            (false, crate::hle::CullKind::Cull) => &self.decal.no_depth_cull,
            (true, crate::hle::CullKind::None) => &self.decal.ao_no_depth_nocull,
            (true, crate::hle::CullKind::Cull) => &self.decal.ao_no_depth_cull,
        }
    }

    /// Select the correct pipeline variant for a draw run.
    ///
    /// `any_depth`: true when the render pass itself has a depth attachment (because at least one
    /// run in the scene uses depth). When true and the run does not test/write depth, we use a
    /// "depth-compat" passthrough pipeline (Always compare, no write) so the pipeline depth format
    /// matches the pass. WebGPU requires all pipelines in a depth-attached pass to declare the
    /// same depth format — mixing a `depth_stencil: None` pipeline into a depth-attached pass is
    /// a validation error (caught when multi-material mixes OPA_SURF with AA_ZB_XLU_SURF runs).
    ///
    /// `fallback_class`: the run's `RenderMode.fallback_class` (B3). `AlphaOver` selects the
    /// SrcAlpha/OneMinusSrcAlpha pipeline; all other classes (Replace, DualSrc) use Replace.
    /// DualSrc runs arrive here only on adapters without `DUAL_SOURCE_BLENDING`; their
    /// `fallback_class` is already `AlphaOver` so they blend correctly on the fallback path.
    fn select(
        &self,
        cull: crate::hle::CullKind,
        z_test: bool,
        z_write: bool,
        any_depth: bool,
        fallback_class: crate::hle::BlendClass,
    ) -> &wgpu::RenderPipeline {
        let ao = fallback_class == crate::hle::BlendClass::AlphaOver;
        match (ao, cull, z_test, z_write) {
            // ── Replace ──────────────────────────────────────────────────────────────────
            (false, crate::hle::CullKind::None, false, _) => {
                if any_depth {
                    &self.pipeline_depth_compat_nocull
                } else {
                    &self.pipeline_no_depth_nocull
                }
            }
            (false, crate::hle::CullKind::Cull, false, _) => {
                if any_depth {
                    &self.pipeline_depth_compat_cull
                } else {
                    &self.pipeline_no_depth_cull
                }
            }
            (false, crate::hle::CullKind::None, true, true) => {
                &self.pipeline_depth_test_write_nocull
            }
            (false, crate::hle::CullKind::Cull, true, true) => &self.pipeline_depth_test_write_cull,
            (false, crate::hle::CullKind::None, true, false) => {
                &self.pipeline_depth_test_nowrite_nocull
            }
            (false, crate::hle::CullKind::Cull, true, false) => {
                &self.pipeline_depth_test_nowrite_cull
            }
            // ── AlphaOver ─────────────────────────────────────────────────────────────────
            (true, crate::hle::CullKind::None, false, _) => {
                if any_depth {
                    &self.pipeline_ao_depth_compat_nocull
                } else {
                    &self.pipeline_ao_no_depth_nocull
                }
            }
            (true, crate::hle::CullKind::Cull, false, _) => {
                if any_depth {
                    &self.pipeline_ao_depth_compat_cull
                } else {
                    &self.pipeline_ao_no_depth_cull
                }
            }
            (true, crate::hle::CullKind::None, true, true) => {
                &self.pipeline_ao_depth_test_write_nocull
            }
            (true, crate::hle::CullKind::Cull, true, true) => {
                &self.pipeline_ao_depth_test_write_cull
            }
            (true, crate::hle::CullKind::None, true, false) => {
                &self.pipeline_ao_depth_test_nowrite_nocull
            }
            (true, crate::hle::CullKind::Cull, true, false) => {
                &self.pipeline_ao_depth_test_nowrite_cull
            }
        }
    }

    /// Draws each `DrawRun` in a single render pass (clear once, one `draw_indexed` per run).
    ///
    /// `material_bind_groups` is indexed by `run.material_index` — one `@group(0)` bind group
    /// per distinct material (tex + sampler).  `uniform_bind_group` is the `@group(1)` bind
    /// group (combiner uniform with dynamic offset); `uniform_stride` is the per-run byte stride
    /// (0 for A8a — all runs share offset 0 into the same uniform slot).
    #[allow(clippy::too_many_arguments)]
    #[cfg(test)]
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        pos_buffer: &wgpu::Buffer,
        index_buffer: &wgpu::Buffer,
        scene: &crate::hle::Scene,
        clear: wgpu::Color,
        material_bind_groups: &[&wgpu::BindGroup],
        uniform_bind_group: &wgpu::BindGroup,
        uniform_stride: u32,
        depth: Option<&wgpu::TextureView>,
    ) {
        let depth_stencil_attachment = depth.map(|dv| wgpu::RenderPassDepthStencilAttachment {
            view: dv,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("skeleton-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_vertex_buffer(0, pos_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        let any_depth = depth.is_some();
        for (i, run) in scene.draw_runs.iter().enumerate() {
            let rm = &scene.render_modes[run.render_mode_index as usize];
            // Gate z_test/z_write by whether a depth buffer was actually provided:
            // a pipeline with depth_stencil cannot be used in a render pass without a depth attachment.
            let z_test = any_depth && rm.z_test;
            let z_write = any_depth && rm.z_write;
            // B4: on a dual-source device, DualSrc runs take the primary blender path; all other
            // runs (and every run on the fallback device) take B3's Replace/AlphaOver fallback by
            // fallback_class. Replace-class runs always use the fallback Replace pipeline.
            let pipeline = match &self.dual {
                Some(ds) if rm.blend_class == crate::hle::BlendClass::DualSrc => {
                    ds.select(run.cull, z_test, z_write, any_depth)
                }
                _ => self.select(run.cull, z_test, z_write, any_depth, rm.fallback_class),
            };
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, material_bind_groups[run.material_index as usize], &[]);
            pass.set_bind_group(1, uniform_bind_group, &[(i as u32) * uniform_stride]);
            pass.draw_indexed(run.index_start..run.index_start + run.index_count, 0, 0..1);
        }
    }
}

/// The per-vertex RSP position-transform compute pipeline (position-only).
pub struct RspProcessPipeline {
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

/// Parameters uniform for the RSP-process compute kernel (binding 0, 16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RspProcessParams {
    pub vertex_count: u32,
    pub _pad: u32,
    pub fb_width: f32,
    pub fb_height: f32,
}
const _: () = assert!(std::mem::size_of::<RspProcessParams>() == 16);

pub(crate) mod workload;

const PAIRLESS_LOGICAL_EXTENT: (u32, u32) = (320, 240);

fn pair_render_extent(pair: &crate::hle::FramebufferPair) -> (u32, u32) {
    (
        u32::from(pair.color_image.width).max(1),
        if pair.size_extent.1 == 0 {
            240
        } else {
            pair.size_extent.1
        },
    )
}

#[derive(Default)]
struct RspSceneBuffers {
    vertices: std::collections::HashMap<(u32, u32), wgpu::Buffer>,
    indices: Option<wgpu::Buffer>,
}

impl RspProcessPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rsp-process-cs"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rsp_process.wgsl").into()),
        });
        let uniform = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rsp-process-bgl"),
            entries: &[
                uniform(0),
                storage(1, true),  // source (SrcVertex)
                storage(2, true),  // mvp_table
                storage(3, true),  // viewport_table
                storage(4, true),  // texcoord_table
                storage(5, true),  // lights_table (GpuLight)
                storage(6, true),  // lookat_table (GpuLookAt)
                storage(7, false), // output (OutVertex) read_write
                storage(8, true),  // fog_table
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rsp-process-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("rsp-process-pipeline"),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            pipeline,
            bind_group_layout,
        }
    }

    fn process_scene(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        scene: &crate::hle::Scene,
        workload: &workload::Workload,
    ) -> RspSceneBuffers {
        use crate::render::rsp_buffers as rb;
        use wgpu::util::DeviceExt;
        if scene.raw_pos.is_empty() || scene.indices.is_empty() {
            return RspSceneBuffers::default();
        }
        let n = scene.raw_pos.len() as u32;
        let sb = |data: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: data,
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        let source = sb(bytemuck::cast_slice(&rb::src_vertices(scene)));
        let mvp_table = sb(bytemuck::cast_slice(&rb::mvp_table(scene)));
        let viewport_table = sb(bytemuck::cast_slice(&rb::viewport_table(scene)));
        let texcoord_table = sb(bytemuck::cast_slice(&rb::texcoord_table(scene)));
        let lights_table = sb(bytemuck::cast_slice(&rb::lights_table(scene)));
        let lookat_table = sb(bytemuck::cast_slice(&rb::lookat_table(scene)));
        let fog_table = sb(bytemuck::cast_slice(&rb::fog_table(scene)));

        let extents = workload
            .targets
            .iter()
            .filter(|target| !target.depth_clear)
            .map(|target| target.logical_extent);
        let mut vertices = std::collections::HashMap::new();
        for extent in extents {
            vertices.entry(extent).or_insert_with(|| {
                let dst = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("out-vertices"),
                    size: (n as u64) * 48,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
                    mapped_at_creation: false,
                });
                let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("rsp-params"),
                    contents: bytemuck::bytes_of(&RspProcessParams {
                        vertex_count: n,
                        _pad: 0,
                        fb_width: extent.0 as f32,
                        fb_height: extent.1 as f32,
                    }),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let rsp_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("rsp-bg"),
                    layout: self.bind_group_layout(),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: params.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: source.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: mvp_table.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: viewport_table.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: texcoord_table.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: lights_table.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: lookat_table.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 7,
                            resource: dst.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 8,
                            resource: fog_table.as_entire_binding(),
                        },
                    ],
                });
                self.dispatch(encoder, &rsp_bg, n);
                dst
            });
        }
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ibuf"),
            contents: bytemuck::cast_slice(&scene.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        RspSceneBuffers {
            vertices,
            indices: Some(indices),
        }
    }

    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_group_layout
    }

    /// Encode the dispatch. `bind_group` must bind 0..=8 per the layout; `vertex_count` drives
    /// both the uniform and the workgroup count.
    pub fn dispatch(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        bind_group: &wgpu::BindGroup,
        vertex_count: u32,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("rsp-process-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.dispatch_workgroups(vertex_count.div_ceil(64), 1, 1);
    }
}

/// Maps an N64 wrap mode (cms/cmt field: 0=WRAP, 1=MIRROR, 2=CLAMP; ≥3 treated as CLAMP) to the
/// corresponding wgpu `AddressMode`.
fn address_mode(wrap: u8) -> wgpu::AddressMode {
    match wrap {
        0 => wgpu::AddressMode::Repeat,
        1 => wgpu::AddressMode::MirrorRepeat,
        _ => wgpu::AddressMode::ClampToEdge,
    }
}

struct TexCache {
    sampling: TileSamplingArray,
    w: u32,
    h: u32,
    bytes: Vec<u8>,
    wrap_s: u8,
    wrap_t: u8,
    /// The material's second texture (TEXEL1), part of the cache key so a change to the second
    /// tile's content/dims/wrap rebuilds the bind group. `None` for single-texture materials — the
    /// bind group then points binding 2/3 at the shared dummy.
    tex1: Option<crate::hle::combiner::Tex1>,
    /// The material's decoded mip chain (hardware-faithful LOD), part of the cache key so a change
    /// to any level's content/dims rebuilds the uploaded mip-level texture. Empty for non-LOD
    /// materials (a single level is uploaded from `bytes`).
    mip_levels: Vec<crate::hle::MipLevel>,
    /// The material's DETAIL tile, part of the cache key so a change rebuilds. `None` binds
    /// the shared 1×1 dummy at binding 4/5.
    detail_tex: Option<crate::hle::MipLevel>,
    bind_group: wgpu::BindGroup,
}

/// Upload `mat`'s texture to the GPU and build a `@group(0)` (tex + sampler) bind group.
///
/// Standalone (not a `SceneRenderer` method) so callers can hold immutable borrows of
/// `self.textured` and `self.samplers` while mutably updating `self.tex_caches`, without
/// triggering a split-borrow conflict.
fn build_tex_entry(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bgl: &wgpu::BindGroupLayout,
    samplers: &[[wgpu::Sampler; 3]; 3],
    dummy_view: &wgpu::TextureView,
    mat: &crate::hle::Material,
) -> TexCache {
    // Upload one decoded RGBA8 texture (tex0, or the tex1 second texture) to a fresh GPU texture,
    // returning its view. The view keeps the texture alive via the bind group's strong ref.
    let upload = |label: &str, w: u32, h: u32, bytes: &[u8]| -> wgpu::TextureView {
        let size = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            size,
        );
        tex.create_view(&wgpu::TextureViewDescriptor::default())
    };
    // tex0: LOD level 0 as its OWN single-level texture (`upload` uses mip_level_count = 1). Non-LOD
    // materials upload only this from `mat.texture` — byte-identical to the pre-LOD single
    // `write_texture`. When LOD is active, level 0 is `mat.texture` (== `mip_levels[0]`).
    let [w, h] = mat.sampling.allocation_extent();
    let tex_view = upload("n64-tex", w, h, &mat.texture);
    // Levels 1..MAX_LOD as INDEPENDENT per-level textures (hardware-faithful — NO halving constraint),
    // bound at bindings 6..=12. `uploaded_level_count` caps the real count at MAX_LOD. A slot beyond
    // the uploaded count (or a non-LOD material) binds the shared 1×1 dummy; it is never sampled
    // because the shader clamps the selected level to the uploaded count. Views are held in
    // `level_views` so their textures stay alive for the bind group's strong refs.
    let num_levels = uploaded_level_count(mat.num_levels);
    let level_views: Vec<wgpu::TextureView> = (1..MAX_LOD)
        .map(|k| {
            if k < num_levels {
                if let Some(lvl) = mat.mip_levels.get(k as usize) {
                    let [w, h] = lvl.sampling.allocation_extent();
                    return upload(&format!("n64-tex-lod{k}"), w, h, &lvl.texture);
                }
            }
            dummy_view.clone()
        })
        .collect();
    // Binding 2/3: the second texture (TEXEL1) when the material carries one (`tile_count == 2`),
    // else the shared 1×1 dummy. tex1 wrap comes from the second tile's cms/cmt.
    let tex1_view = match &mat.tex1 {
        Some(t) => {
            let [w, h] = t.sampling.allocation_extent();
            upload("n64-tex1", w, h, &t.texture)
        }
        None => dummy_view.clone(),
    };
    let samp1 = match &mat.tex1 {
        Some(t) => &samplers[(t.wrap_s as usize).min(2)][(t.wrap_t as usize).min(2)],
        None => &samplers[2][2],
    };
    // Binding 4/5: the DETAIL tile when present (LOD DETAIL mode), else the shared 1×1 dummy.
    // ClampToEdge/Linear sampler for the detail tile.
    let detail_view = match &mat.detail_tex {
        Some(d) => {
            let [w, h] = d.sampling.allocation_extent();
            upload("n64-tex-detail", w, h, &d.texture)
        }
        None => dummy_view.clone(),
    };
    let sampling = material_sampling(mat);
    let sampling_buffer = sampling_buffer(device, &sampling);
    let entries: Vec<wgpu::BindGroupEntry> = [
        sampling_entry(&sampling_buffer),
        wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&tex_view),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(
                &samplers[(mat.wrap_s as usize).min(2)][(mat.wrap_t as usize).min(2)],
            ),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::TextureView(&tex1_view),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::Sampler(samp1),
        },
        wgpu::BindGroupEntry {
            binding: 4,
            resource: wgpu::BindingResource::TextureView(&detail_view),
        },
        wgpu::BindGroupEntry {
            binding: 5,
            resource: wgpu::BindingResource::Sampler(&samplers[2][2]),
        },
    ]
    .into_iter()
    .chain(
        level_views
            .iter()
            .enumerate()
            .map(|(i, v)| wgpu::BindGroupEntry {
                binding: LOD_BINDING_BASE + i as u32,
                resource: wgpu::BindingResource::TextureView(v),
            }),
    )
    .collect();
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("n64-bg"),
        layout: bgl,
        entries: &entries,
    });
    TexCache {
        sampling,
        w: mat.tex_w,
        h: mat.tex_h,
        bytes: mat.texture.clone(),
        wrap_s: mat.wrap_s,
        wrap_t: mat.wrap_t,
        tex1: mat.tex1.clone(),
        mip_levels: mat.mip_levels.clone(),
        detail_tex: mat.detail_tex.clone(),
        bind_group,
    }
}

struct Framebuffer {
    color: wgpu::Texture,      // Rgba8Unorm | RENDER_ATTACHMENT | TEXTURE_BINDING
    attach: wgpu::TextureView, // color attachment for the RCP pass
    sampled: wgpu::TextureView,
    present_bg: wgpu::BindGroup, // @group(0): sampled(color) + Clamp/Linear sampler, for `scanout`
}

pub struct SceneRenderer {
    pub(crate) frame_serial: u64,
    pub(crate) dither_seed: u32,
    textured: TexturedPipeline,
    textured_fb: TexturedPipeline,
    /// Fullscreen-triangle blit pipeline built at the surface `color_format`. Reads from an
    /// `Rgba8Unorm` intermediate FB (produced by `textured_fb` passes) via `group0_bgl`.
    present: wgpu::RenderPipeline,
    rsp: RspProcessPipeline,
    samplers: [[wgpu::Sampler; 3]; 3],
    /// Content-keyed GPU texture + `@group(0)` bind group — one per `scene.materials[i]`.
    /// Rebuilt only when a material's texture bytes, dims, or wrap mode change.
    tex_caches: Vec<TexCache>,
    /// A 1×1 white `@group(0)` (tex + sampler) bind group used as the texture binding for
    /// `FillRect` draws (which carry no material, but the pipeline layout still requires group 0).
    /// The fill combine has `tex_enable = 0`, so this texture is never actually sampled.
    fill_bind_group: wgpu::BindGroup,
    /// A shared 1×1 dummy texture view bound at `@group(0) @binding(2)` (the TEXEL1 slot) on every
    /// group(0) bind group that carries no real second texture — single-texture draws, the FB-as-
    /// texture alias, and the present/scanout blits. Keeps the layout uniformly satisfied;
    /// `tex_enable1 = 0` (or a non-TEXEL1 shader) means it is never sampled meaningfully.
    dummy_view: wgpu::TextureView,
    /// Internal-framebuffer dimensions (= the surface size passed to `new`/`resize`). The pair-less
    /// flat-3D path renders into an Rgba8Unorm FB of this size (spec §4), decoupled from the
    /// caller's `target` (the present blit scales FB→target).
    fb_w: u32,
    fb_h: u32,
    framebuffers: std::collections::HashMap<workload::TargetId, Framebuffer>,
    first_touch: std::collections::HashSet<workload::TargetId>,
    /// Descriptor array for bind groups that sample plain images (fill, scanout, FB alias).
    image_sampling: wgpu::Buffer,
}

impl SceneRenderer {
    /// Create the scene depth texture and return `(attachment_view, sample_view)`. The texture
    /// carries `RENDER_ATTACHMENT | TEXTURE_BINDING` (E1) so the SAME `Depth32Float` buffer that
    /// pass 1 writes as a depth attachment can be SAMPLED (`texture_depth_2d`) by the decal pass.
    /// Both views are over the same texture; they are used in distinct, sequential passes.
    fn make_depth_view(
        device: &wgpu::Device,
        w: u32,
        h: u32,
    ) -> (wgpu::TextureView, wgpu::TextureView) {
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("n64-depth"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let attachment = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sample = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
        (attachment, sample)
    }

    pub fn new(
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        w: u32,
        h: u32,
        _dual_source: bool,
    ) -> Self {
        let textured = TexturedPipeline::new(device, color_format, DEPTH_FORMAT);
        // Second draw-pipeline (Rgba8Unorm): the internal-framebuffer draw target — used by both
        // the per-pair FB passes and the pair-less flat-3D path (which renders into an internal FB
        // and blits it to the caller's target).
        let textured_fb =
            TexturedPipeline::new(device, wgpu::TextureFormat::Rgba8Unorm, DEPTH_FORMAT);
        // Present (blit) pipeline: fullscreen triangle at surface color_format, reads from an
        // Rgba8Unorm intermediate via group0_bgl (tex+sampler). Blits both the per-pair FB passes
        // and the pair-less internal FB to the caller's target.
        let present = {
            let present_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("present-shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("present.wgsl").into()),
            });
            let present_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("present-layout"),
                bind_group_layouts: &[Some(textured.bind_group_layout())],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("present-pipeline"),
                layout: Some(&present_layout),
                vertex: wgpu::VertexState {
                    module: &present_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &present_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let rsp = RspProcessPipeline::new(device);
        // Build a 3×3 sampler pool indexed [cms][cmt] (0=WRAP, 1=MIRROR, 2=CLAMP).
        let samplers: [[wgpu::Sampler; 3]; 3] = std::array::from_fn(|s| {
            std::array::from_fn(|t| {
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("n64-sampler"),
                    address_mode_u: address_mode(s as u8),
                    address_mode_v: address_mode(t as u8),
                    address_mode_w: wgpu::AddressMode::ClampToEdge,
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: wgpu::FilterMode::Linear,
                    mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                    ..Default::default()
                })
            })
        });
        // Shared 1×1 dummy texture: bound at the TEXEL1 slot (binding 2) of every group(0) bind
        // group that has no real second texture. Also reused as the FillRect texture binding
        // (binding 0) below — the fill combine has `tex_enable = 0`, so it is never sampled.
        let dummy_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("dummy-1x1"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let dummy_view = dummy_tex.create_view(&wgpu::TextureViewDescriptor::default());
        // 1×1 `@group(0)` bind group used as the FillRect texture binding. The pipeline layout
        // requires group 0, but the fill combine has `tex_enable = 0`, so binding 0 is never sampled;
        // bindings 2/3 (TEXEL1) point at the same dummy and are likewise never read (tex_enable1 = 0).
        let image_sampling = image_sampling_buffer(device);
        let fill_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fill-bg"),
            layout: textured_fb.bind_group_layout(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&dummy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&samplers[2][2]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&dummy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&samplers[2][2]),
                },
                // DETAIL slot (bindings 4/5): the fill path is untextured — shared dummy satisfies
                // the group(0) layout; never sampled.
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&dummy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&samplers[2][2]),
                },
            ]
            .into_iter()
            .chain(lod_level_entries(&dummy_view))
            .chain([sampling_entry(&image_sampling)])
            .collect::<Vec<_>>(),
        });
        Self {
            textured,
            textured_fb,
            present,
            rsp,
            samplers,
            tex_caches: Vec::new(),
            fill_bind_group,
            dummy_view,
            fb_w: w,
            fb_h: h,
            framebuffers: std::collections::HashMap::new(),
            first_touch: std::collections::HashSet::new(),
            frame_serial: 0,
            dither_seed: 0,
            image_sampling,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn resize(&mut self, _device: &wgpu::Device, w: u32, h: u32) {
        self.fb_w = w;
        self.fb_h = h;
    }

    /// Blit `src_bg` (an `@group(0)` bind group: texture + sampler) to `dst_view` via the
    /// fullscreen-triangle present pipeline. The pass uses `LoadOp::Load` (the triangle covers
    /// every pixel, so clearing first would waste a tile-flush).
    fn blit_to(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        dst_view: &wgpu::TextureView,
        src_bg: &wgpu::BindGroup,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("present-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dst_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.present);
        pass.set_bind_group(0, src_bg, &[]);
        pass.draw(0..3, 0..1);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_rect_op(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        device: &wgpu::Device,
        target: &workload::TargetWorkload,
        op: &crate::hle::SceneOp,
        scene: &crate::hle::Scene,
        material_bgs: &[&wgpu::BindGroup],
        rect_vbuf: Option<&wgpu::Buffer>,
        uniform_bg: Option<&wgpu::BindGroup>,
        slot: u32,
        rect_idx: u32,
        any_depth: bool,
    ) {
        // Step 1 (spec §2.4): FB-as-texture alias — if this TexRect carries a `fb_source`, bind the
        // prior pair's SAMPLED color view as @group(0) instead of the RDRAM-decoded material
        // texture. The pool's sampled view is row-0-at-top (GPU-native); no re-flip needed. The
        // source pair is PRIOR (ordered loop guarantees it was rendered first).
        let opt_fb_bg: Option<wgpu::BindGroup> = if let crate::hle::SceneOp::TexRect {
            fb_source: Some(src_addr),
            ..
        } = op
        {
            assert_ne!(
                workload::TargetId::Guest(*src_addr),
                target.id,
                "fb_source cannot reference the current pair (same-pair is invalid)"
            );
            let sampled = &self
                .framebuffers
                .get(&workload::TargetId::Guest(*src_addr))
                .expect("framebuffer source must precede its consumer")
                .sampled;
            Some(
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("fb-source-bg"),
                    layout: self.textured_fb.bind_group_layout(),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(sampled),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            // ClampToEdge + Linear; identity at 1:1.
                            resource: wgpu::BindingResource::Sampler(&self.samplers[2][2]),
                        },
                        // TEXEL1 slot: never sampled on the FB-as-texture alias path (its combine is a
                        // TEXEL0 copy) — bind the shared dummy to satisfy the group(0) layout.
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&self.dummy_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::Sampler(&self.samplers[2][2]),
                        },
                        // DETAIL slot (4/5): unused on the FB-as-texture alias path; shared dummy.
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(&self.dummy_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::Sampler(&self.samplers[2][2]),
                        },
                    ]
                    .into_iter()
                    .chain(lod_level_entries(&self.dummy_view))
                    .chain([sampling_entry(&self.image_sampling)])
                    .collect::<Vec<_>>(),
                }),
            )
        } else {
            None
        };
        let group0: &wgpu::BindGroup = match (opt_fb_bg.as_ref(), op) {
            (Some(bg), _) => bg,
            (None, crate::hle::SceneOp::TexRect { material_index, .. }) => {
                material_bgs[*material_index as usize]
            }
            _ => &self.fill_bind_group,
        };
        // FillRect and COPY-mode TexRect → Replace.
        // Non-COPY TexRect → render mode's fallback_class (may be AlphaOver).
        let blend_class = match op {
            crate::hle::SceneOp::TexRect {
                render_mode_index,
                copy_mode,
                ..
            } if !*copy_mode => {
                let rm = &scene.render_modes[*render_mode_index as usize];
                rm.fallback_class
            }
            _ => crate::hle::BlendClass::Replace,
        };
        let pipeline = self.textured_fb.select(
            crate::hle::CullKind::None,
            false,
            false,
            any_depth,
            blend_class,
        );
        pass.set_pipeline(pipeline);
        if let Some(rv) = rect_vbuf {
            pass.set_vertex_buffer(0, rv.slice(..));
        }
        pass.set_bind_group(0, group0, &[]);
        if let Some(ubg) = uniform_bg {
            pass.set_bind_group(1, ubg, &[slot * 256]);
        }
        pass.draw((rect_idx * 6)..(rect_idx * 6 + 6), 0..1);
    }

    fn ensure_fb(
        &mut self,
        device: &wgpu::Device,
        addr: workload::TargetId,
        w: u32,
        h: u32,
    ) -> bool {
        let need = match self.framebuffers.get(&addr) {
            Some(fb) => fb.color.width() != w || fb.color.height() != h,
            None => true,
        };
        if !need {
            return false;
        }
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fb-store-color"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let attach = color.create_view(&wgpu::TextureViewDescriptor::default());
        let sampled = color.create_view(&wgpu::TextureViewDescriptor::default());
        let present_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fb-store-present-bg"),
            layout: self.textured.bind_group_layout(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&sampled),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.samplers[2][2]), // Clamp/Linear
                },
                // TEXEL1 slot: the present pipeline never samples it (present.wgsl uses only
                // bindings 0/1) — bind the shared dummy to satisfy the shared group(0) layout.
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.dummy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.samplers[2][2]),
                },
                // DETAIL slot (4/5): the present pipeline never samples it; shared dummy.
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.dummy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&self.samplers[2][2]),
                },
            ]
            .into_iter()
            .chain(lod_level_entries(&self.dummy_view))
            .chain([sampling_entry(&self.image_sampling)])
            .collect::<Vec<_>>(),
        });
        self.framebuffers.insert(
            addr,
            Framebuffer {
                color,
                attach,
                sampled,
                present_bg,
            },
        );
        true
    }

    /// True if the store holds a framebuffer for `addr` (the VI source-selection guard `present` uses).
    pub fn has_fb(&self, addr: impl Into<workload::TargetId>) -> bool {
        self.framebuffers.contains_key(&addr.into())
    }

    /// VI scanout (D2): blit the stored FB at `src_addr` to `target` via the present pipeline
    /// (Clamp/Linear — identity at 1:1, sampler-stretch otherwise). Records into the CALLER's
    /// `encoder` (present owns acquire+submit). No device handle needed (uses the prebuilt
    /// `present_bg`). Panics on a missing key — callers gate on `has_fb`.
    pub fn scanout(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        src_addr: impl Into<workload::TargetId>,
    ) {
        let fb = self
            .framebuffers
            .get(&src_addr.into())
            .expect("scanout: src_addr not in the store (gate on has_fb)");
        self.blit_to(encoder, target, &fb.present_bg);
    }

    /// Explicit frame boundary (D2): reset the per-frame first-touch-clear set. Does NOT drop the
    /// textures (cross-frame persistence). `Renderer::begin_frame` delegates here.
    pub fn begin_frame(&mut self) {
        self.frame_serial = self.frame_serial.wrapping_add(1);
        self.first_touch.clear();
    }

    /// The color LoadOp for a store FB this frame under `clear_policy`. Mutates the per-frame
    /// first-touch set, so it MUST be called once per pair in DL order.
    fn fb_clear_op(
        &mut self,
        addr: workload::TargetId,
        created_or_resized: bool,
        clear_policy: crate::ClearPolicy,
    ) -> wgpu::LoadOp<wgpu::Color> {
        let first_this_frame = self.first_touch.insert(addr);
        let clear = created_or_resized
            || match clear_policy {
                crate::ClearPolicy::PerFrame => first_this_frame,
                crate::ClearPolicy::Persist => created_or_resized,
            };
        if clear {
            wgpu::LoadOp::Clear(CLEAR_COLOR)
        } else {
            wgpu::LoadOp::Load
        }
    }
}

/// Headless device acquisition (native tests; no surface). Native-only: pollster cannot
/// block on wasm, and the web crate uses its own async path.
/// Uses Limits::default() (WebGPU) so compute + storage buffers are available for the RSP-process pass.
/// Returns `(device, queue, dual_source)` where `dual_source` is true when the adapter advertised
/// `DUAL_SOURCE_BLENDING` and it was successfully requested (B3/B4 use this to select pipelines).
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(not(test), allow(dead_code))]
pub fn headless_device() -> (wgpu::Device, wgpu::Queue, bool) {
    #[cfg(test)]
    let _ = env_logger::try_init();
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("no adapter");
    let dual = adapter
        .features()
        .contains(wgpu::Features::DUAL_SOURCE_BLENDING);
    let required_features = if dual {
        wgpu::Features::DUAL_SOURCE_BLENDING
    } else {
        wgpu::Features::empty()
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("headless"),
        required_features,
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        trace: wgpu::Trace::Off,
    }))
    .expect("no device");
    (device, queue, dual)
}

/// Headless device with dual-source blending forcibly disabled — mirrors the §11 forced-fallback
/// CI mode. Requests `Features::empty()` even when the adapter supports `DUAL_SOURCE_BLENDING`,
/// so the fallback blender path (B3) can be exercised deterministically.
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(not(test), allow(dead_code))]
pub fn headless_device_forced_fallback() -> (wgpu::Device, wgpu::Queue) {
    #[cfg(test)]
    let _ = env_logger::try_init();
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("no adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("headless-fallback"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        trace: wgpu::Trace::Off,
    }))
    .expect("no device");
    (device, queue)
}

/// Flattens `crate::hle::Scene` SoA fields into the exact GPU storage-buffer byte layouts the
/// `rsp_process.wgsl` compute kernel expects (see that shader for the binding contract).
pub mod rsp_buffers {
    use bytemuck::{Pod, Zeroable};

    /// Viewport entry, vec4-padded to a stable 32-byte WGSL std430 layout
    /// (`struct { scale: vec4<f32>, trans: vec4<f32> }`); only xyz are used.
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Pod, Zeroable)]
    pub struct GpuViewport {
        pub scale: [f32; 4],
        pub trans: [f32; 4],
    }
    const _: () = assert!(std::mem::size_of::<GpuViewport>() == 32);

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Pod, Zeroable)]
    pub struct SrcVertex {
        pub pos: [f32; 3],
        pub _pad0: f32,
        pub st: [f32; 2],
        pub mtx_index: u32,
        pub viewport_index: u32,
        pub texcoord_index: u32,
        pub cn: u32,
        pub light_index: u32,
        pub light_count: u32,
        pub lookat_index: u32,
        pub texgen_mode: u32,
        pub fog: u32,
        pub modify_flags: u32,
        pub modify_screen: [f32; 4],
    }
    const _: () = assert!(std::mem::size_of::<SrcVertex>() == 80);

    /// Per-light GPU entry: 32 bytes (dir + col each vec4).
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Pod, Zeroable)]
    pub struct GpuLight {
        pub dir: [f32; 4],
        pub col: [f32; 4],
    }
    const _: () = assert!(std::mem::size_of::<GpuLight>() == 32);

    /// Per-lookat GPU entry: 32 bytes (object-space S + T axis, each vec4; only xyz used).
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Pod, Zeroable)]
    pub struct GpuLookAt {
        pub axis_s: [f32; 4],
        pub axis_t: [f32; 4],
    }
    const _: () = assert!(std::mem::size_of::<GpuLookAt>() == 32);

    /// Prefolded per-axis texcoord scale + texgen ST-fold, 16-byte std430.
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Pod, Zeroable)]
    pub struct GpuTexcoord {
        pub scale_s: f32,
        pub scale_t: f32,
        pub texgen_scale_s: f32,
        pub texgen_scale_t: f32,
    }
    const _: () = assert!(std::mem::size_of::<GpuTexcoord>() == 16);

    pub fn src_vertices(scene: &crate::hle::Scene) -> Vec<SrcVertex> {
        (0..scene.raw_pos.len())
            .map(|i| SrcVertex {
                pos: scene.raw_pos[i],
                _pad0: 0.0,
                st: scene.raw_st[i],
                mtx_index: scene.mtx_index[i],
                viewport_index: scene.viewport_index[i],
                texcoord_index: scene.texcoord_index[i],
                cn: scene.cn[i],
                light_index: scene.light_index[i],
                light_count: scene.light_count[i],
                // Length-safe: the real HLE path keeps these index-parallel to raw_pos, but
                // hand-built test Scenes may omit the texgen SoA Vecs (they then default to 0).
                lookat_index: scene.lookat_index.get(i).copied().unwrap_or(0),
                texgen_mode: scene.texgen_mode.get(i).copied().unwrap_or(0),
                fog: scene.fog.get(i).copied().unwrap_or(0),
                modify_flags: scene.modify_flags.get(i).copied().unwrap_or(0),
                modify_screen: scene.modify_screen.get(i).copied().unwrap_or([0.0; 4]),
            })
            .collect()
    }

    pub fn fog_table(scene: &crate::hle::Scene) -> Vec<[f32; 2]> {
        if scene.fog_table.is_empty() {
            vec![[0.0; 2]]
        } else {
            scene
                .fog_table
                .iter()
                .map(|pair| pair.map(f32::from))
                .collect()
        }
    }

    pub fn lookat_table(scene: &crate::hle::Scene) -> Vec<GpuLookAt> {
        let v: Vec<GpuLookAt> = scene
            .lookat_table
            .iter()
            .map(|(s, t)| GpuLookAt {
                axis_s: [s[0], s[1], s[2], 0.0],
                axis_t: [t[0], t[1], t[2], 0.0],
            })
            .collect();
        if v.is_empty() {
            vec![GpuLookAt::zeroed()]
        } else {
            v
        }
    }

    pub fn lights_table(scene: &crate::hle::Scene) -> Vec<GpuLight> {
        let v: Vec<GpuLight> = scene
            .lights_table
            .iter()
            .map(|(d, c)| GpuLight {
                dir: [d[0], d[1], d[2], 0.0],
                col: [c[0], c[1], c[2], 0.0],
            })
            .collect();
        if v.is_empty() {
            vec![GpuLight::zeroed()]
        } else {
            v
        }
    }

    pub fn mvp_table(scene: &crate::hle::Scene) -> Vec<f32> {
        scene
            .mvp_table
            .iter()
            .flat_map(|m| m.iter().flatten().copied())
            .collect()
    }

    pub fn viewport_table(scene: &crate::hle::Scene) -> Vec<GpuViewport> {
        scene
            .viewport_table
            .iter()
            .map(|(s, t)| GpuViewport {
                scale: [s[0], s[1], s[2], 0.0],
                trans: [t[0], t[1], t[2], 0.0],
            })
            .collect()
    }

    pub fn texcoord_table(scene: &crate::hle::Scene) -> Vec<GpuTexcoord> {
        scene
            .texcoord_table
            .iter()
            .enumerate()
            .map(|(i, e)| {
                // texgen_scale_table is index-parallel to texcoord_table in the real HLE path;
                // length-safe for hand-built test Scenes that omit it.
                let tg = scene
                    .texgen_scale_table
                    .get(i)
                    .copied()
                    .unwrap_or([0.0, 0.0]);
                GpuTexcoord {
                    scale_s: e[0],
                    scale_t: e[1],
                    texgen_scale_s: tg[0],
                    texgen_scale_t: tg[1],
                }
            })
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn address_mode_maps_gbi_wrap_consts() {
            // N64 cms/cmt: 0=WRAP, 1=MIRROR, 2=CLAMP (3 is 2-bit overflow → CLAMP).
            // Pins the production mapping (the golden harness carries a separate copy).
            use crate::render::address_mode;
            assert_eq!(address_mode(0), wgpu::AddressMode::Repeat);
            assert_eq!(address_mode(1), wgpu::AddressMode::MirrorRepeat);
            assert_eq!(address_mode(2), wgpu::AddressMode::ClampToEdge);
            assert_eq!(address_mode(3), wgpu::AddressMode::ClampToEdge);
        }

        #[test]
        fn packs_source_and_texcoord_tables() {
            let scene = crate::hle::Scene {
                raw_pos: vec![[1.0, 2.0, 3.0]],
                raw_st: vec![[10.0, 20.0]],
                mtx_index: vec![1],
                viewport_index: vec![0],
                texcoord_index: vec![1],
                cn: vec![0],
                light_index: vec![0],
                light_count: vec![0],
                modify_flags: vec![3],
                modify_screen: vec![[32.0, 16.0, 0.5, 0.0]],
                mvp_table: vec![crate::hle::math::identity()],
                viewport_table: vec![([160.0, 120.0, 0.5], [160.0, 120.0, 0.5])],
                texcoord_table: vec![[0.0, 0.0], [0.25, 0.5]],
                ..Default::default()
            };
            let sv = src_vertices(&scene);
            assert_eq!(sv.len(), 1);
            assert_eq!(sv[0].pos, [1.0, 2.0, 3.0]);
            assert_eq!(sv[0].st, [10.0, 20.0]);
            assert_eq!(sv[0].texcoord_index, 1);
            assert_eq!(sv[0].modify_flags, 3);
            assert_eq!(sv[0].modify_screen, [32.0, 16.0, 0.5, 0.0]);
            assert_eq!(std::mem::size_of::<SrcVertex>(), 80);
            assert_eq!(texcoord_table(&scene)[1].scale_s, 0.25);
            assert_eq!(texcoord_table(&scene)[1].scale_t, 0.5);
            assert_eq!(std::mem::size_of::<GpuViewport>(), 32);
        }

        #[test]
        fn phase4_rsp_process_shader_parses_and_validates() {
            let module = wgpu::naga::front::wgsl::parse_str(include_str!("rsp_process.wgsl"))
                .expect("rsp_process.wgsl must parse");
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("rsp_process.wgsl must validate");
        }
    }
}
