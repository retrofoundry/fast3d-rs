//! The flat Scene draw-data types the renderer consumes: the vertex/index/material SoA plus the
//! 2D framebuffer op-list. Extracted from `hle/rsp.rs` in the P2 crate merge; re-exported from both
//! the crate root and `hle::rsp` so every pre-merge path keeps resolving.

/// Per-draw cull state. Binary rasterizer cull (FRONT when any cull bit is
/// set, NONE otherwise). The FRONT/BACK distinction is the CPU
/// a<->c swap in `draw_tri`, not a third state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CullKind {
    None,
    Cull,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrimitiveDepth {
    pub z: u16,
    pub dz: u16,
}

/// A contiguous run of `scene.indices` sharing one cull state, emitted as one `draw_indexed`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawRun {
    pub material_index: u32,
    pub render_mode_index: u32,
    pub fog_color: [u8; 4],
    pub prim_depth: PrimitiveDepth,
    pub cull: CullKind,
    pub index_count: u32,
    pub index_start: u32,
}

/// Color (or z) framebuffer pointer decoded from G_SETCIMG / G_SETZIMG.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColorImage {
    pub fmt: u8,
    pub siz: u8,
    pub width: u16, // actual pixels (raw field + 1)
    pub addr: u64,
}
/// Axis-aligned rectangle in pixel coordinates (decoded from 10.2 fixed point).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub ulx: i32,
    pub uly: i32,
    pub lrx: i32,
    pub lry: i32,
}
/// TexRect command bounds in raw 10.2 fixed point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TexRectBounds {
    pub ulx: i32,
    pub uly: i32,
    pub lrx: i32,
    pub lry: i32,
}
/// Scissor rectangle in pixel coordinates with mode.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scissor {
    pub ulx: i32,
    pub uly: i32,
    pub lrx: i32,
    pub lry: i32,
    pub mode: u8,
}
/// An op in the 2D/3D scene op-list inside a `FramebufferPair`.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneOp {
    Tris(DrawRun),
    FillRect {
        rect: Rect,
        color_raw: u32,
        convert: [i16; 6],
        key: [crate::hle::rdp::KeyChannel; 3],
    },
    TexRect {
        rect: TexRectBounds,
        tile: u8,
        uls: i16,
        ult: i16,
        dsdx: i16,
        dtdy: i16,
        flip: bool,
        copy_mode: bool,
        material_index: u32,
        render_mode_index: u32,
        fog_color: [u8; 4],
        prim_depth: PrimitiveDepth,
        fb_source: Option<u64>,
    },
    SetScissor(Scissor),
}
/// One color+depth framebuffer pass: a list of scene ops bounded by SetColorImage.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FramebufferPair {
    pub color_image: ColorImage,
    pub depth_image: Option<u64>,
    pub ops: Vec<SceneOp>,
    pub active_scissor: Scissor,
    pub size_extent: (u32, u32),
    pub is_depth_clear: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawOrigin {
    pub pc: u64,
    pub scissor: Scissor,
    pub indices: std::ops::Range<u32>,
    pub rectangle: Option<(usize, usize)>,
}

/// The flat scene the renderer consumes: vertex buffer + triangle index buffer + per-run materials.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub(crate) draw_origins: Vec<DrawOrigin>,
    pub indices: Vec<u32>,
    pub materials: Vec<crate::hle::combiner::Material>,
    pub render_modes: Vec<crate::hle::blender::RenderMode>,
    // --- Compute-RSP SoA inputs (consumed by the GPU RSP-process pass) ---
    /// Object-space position per output vertex (untransformed).
    pub raw_pos: Vec<[f32; 3]>,
    /// gSPModifyVertex screen override bits, index-parallel to `raw_pos`: bit 0 = XY, bit 1 = Z.
    pub modify_flags: Vec<u32>,
    /// gSPModifyVertex final screen target `[x_px, y_px, z_ndc, unused]`, index-parallel to `raw_pos`.
    pub modify_screen: Vec<[f32; 4]>,
    /// Index into `mvp_table` of the MVP active when this vertex was loaded.
    pub mtx_index: Vec<u32>,
    /// Index into `viewport_table` of the viewport active when this vertex was loaded.
    pub viewport_index: Vec<u32>,
    /// MVP state table (one entry per distinct MVP during the walk).
    pub mvp_table: Vec<crate::hle::math::Mat4>,
    /// Viewport state table: (scale, trans), each `[f32;3]`.
    pub viewport_table: Vec<([f32; 3], [f32; 3])>,
    /// Raw s,t per output vertex (i16 -> f32), consumed by the GPU texcoord stage.
    pub raw_st: Vec<[f32; 2]>,
    /// Index into `texcoord_table` of the texcoord state active when this vertex was loaded.
    pub texcoord_index: Vec<u32>,
    /// Prefolded per-axis texcoord scale [scale_s, scale_t] = sc/(DIVISOR*tile_w), tc/(DIVISOR*tile_h), f64-computed.
    pub texcoord_table: Vec<[f32; 2]>,
    /// Cull-keyed draw batches partitioning `indices` in order (binary cull).
    pub draw_runs: Vec<DrawRun>,
    /// 2D framebuffer passes: one per SetColorImage boundary (empty until Task-3 recording).
    pub framebuffer_pairs: Vec<FramebufferPair>,
    /// The final color image (`G_SETCIMG`) observed during the walk — the pair-less scanout FB key
    /// (spec §4). Defaults (addr 0) for a flat-3D scene that never sets a color image.
    pub color_image: ColorImage,
    /// Raw Vtx bytes 12..15 per vertex (normal-or-color), little-endian: cn = b12|b13<<8|b14<<16|b15<<24.
    pub cn: Vec<u32>,
    /// Start index into `lights_table` of the light set active at this vertex (0 if unlit).
    pub light_index: Vec<u32>,
    /// Light count (num_dir + 1 ambient) for this vertex, or 0 when G_LIGHTING is off.
    pub light_count: Vec<u32>,
    /// Concatenated object-space light sets: (dir_obj, col); the last entry of each set is ambient.
    pub lights_table: Vec<([f32; 3], [f32; 3])>,
    /// Texgen mode per vertex: 0 = off, 1 = spherical, 2 = linear (gated on G_LIGHTING).
    pub texgen_mode: Vec<u32>,
    /// Fog state at vertex load: 0 = disabled, n + 1 = `fog_table[n]`.
    pub fog: Vec<u32>,
    /// Raw signed G_MW_FOG multiplier and offset pairs.
    pub fog_table: Vec<[i16; 2]>,
    /// Index into `lookat_table` of the (S,T) axis pair active at this vertex (0 if no texgen).
    pub lookat_index: Vec<u32>,
    /// Concatenated object-space lookat pairs (axis_S_obj, axis_T_obj), deduped by (version, modelview).
    pub lookat_table: Vec<([f32; 3], [f32; 3])>,
    /// Texgen ST-scale `[texgen_scale_s, texgen_scale_t]`, index-parallel to `texcoord_table`
    /// (same dedup, same `texcoord_index`). Separate from `texcoord_table` so this stays additive.
    pub texgen_scale_table: Vec<[f32; 2]>,
}
