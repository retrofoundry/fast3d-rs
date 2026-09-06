//! Position and texcoord are transformed on the GPU (renderer compute kernel -> OutVertex);
//! the CPU keeps conditional control positions, RSP/RDP state, and state-index tables.

use crate::hle::gbi::f3dex2::F3DEX2_CONSTS;
use crate::hle::math::{identity, mul4, mul_col_vec3, mul_row_vec4, Mat4};
use crate::hle::mem::Rdram;
pub use crate::scene::{
    ColorImage, CullKind, DrawRun, FramebufferPair, Rect, Scene, SceneOp, Scissor, TexRectBounds,
};

pub const RSP_MAX_VERTICES: usize = 256;
/// F3DEX2 max directional lights.
pub const RSP_MAX_LIGHTS: u32 = 7;
pub const DEPTH_RANGE: f32 = 1024.0;
/// Fixed framebuffer the viewport maps into. The real per-DL resolution comes from
/// gsDPSetColorImage / the VI, which we don't model yet — 320x240 is the classic N64 res.
pub const FB_WIDTH: f32 = 320.0;
pub const FB_HEIGHT: f32 = 240.0;
/// F3DEX2 modelview matrix stack size. Pushes past 32 are silently dropped.
pub const RSP_MATRIX_STACK_SIZE: usize = 32;

/// Texture scaling state set by gsSPTexture.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextureState {
    pub tile: u8,
    pub level: u8,
    pub on: bool,
    pub sc: u16,
    pub tc: u16,
}

pub struct Rsp {
    cache_global_index: [u32; RSP_MAX_VERTICES],
    loaded: [bool; RSP_MAX_VERTICES],
    clip_codes: [Option<u8>; RSP_MAX_VERTICES],
    screen_z: [Option<f64>; RSP_MAX_VERTICES],
    used: [bool; RSP_MAX_VERTICES],
    model_stack: [Mat4; RSP_MATRIX_STACK_SIZE],
    model_stack_size: usize,
    viewproj: Mat4,
    mvp: Mat4,
    vp_scale: [f32; 3],
    vp_trans: [f32; 3],
    geom: u32,
    consts: crate::hle::gbi::GbiConstants,
    data_format: crate::hle::mem::GbiDataFormat,
    pub texture_state: TextureState,
    // Compute-RSP state-index accumulation (flushed onto the Scene by `finish`).
    mvp_table: Vec<Mat4>,
    viewport_table: Vec<([f32; 3], [f32; 3])>,
    cur_mvp_index: u32,
    cur_viewport_index: u32,
    texcoord_table: Vec<[f32; 2]>,
    cur_texcoord_index: u32,
    modify_unit_texcoord_index: Option<u32>,
    // Light state (dir camera-space s8/127, col u8/255).
    pub lights: [([f32; 3], [f32; 3]); 8],
    pub ambient_col: [f32; 3],
    pub num_dir: u32,
    pub light_version: u64,
    // Dedup state for object-space light prefold.
    last_light_key: Option<(u64, Mat4)>,
    cur_light_index: u32,
    // LookAt axes (S=0, T=1) in eye space (s8/127), and texgen prefold dedup state.
    pub lookat_axes: [[f32; 3]; 2],
    pub lookat_version: u64,
    last_lookat_key: Option<(u64, Mat4)>,
    cur_lookat_index: u32,
    texgen_scale_table: Vec<[f32; 2]>,
    // Material/render-mode dirty flag and snapshot cache (A5). `material_dirty` is pub so
    // rdp.rs and rsp_f3dex2.rs handlers can set it without going through a method.
    pub material_dirty: bool,
    last_material: Option<crate::hle::combiner::Material>,
    last_material_index: Option<u32>,
    last_render_mode: Option<crate::hle::blender::RenderMode>,
    last_render_mode_index: Option<u32>,
}

impl Default for Rsp {
    fn default() -> Self {
        Rsp {
            cache_global_index: [0u32; RSP_MAX_VERTICES],
            loaded: [false; RSP_MAX_VERTICES],
            clip_codes: [None; RSP_MAX_VERTICES],
            screen_z: [None; RSP_MAX_VERTICES],
            used: [false; RSP_MAX_VERTICES],
            model_stack: [identity(); RSP_MATRIX_STACK_SIZE],
            model_stack_size: 1,
            viewproj: identity(),
            mvp: identity(),
            // Default: a full-screen FB_WIDTH x FB_HEIGHT viewport (half-extent = center).
            vp_scale: [FB_WIDTH / 2.0, FB_HEIGHT / 2.0, 511.0 / DEPTH_RANGE],
            vp_trans: [FB_WIDTH / 2.0, FB_HEIGHT / 2.0, 511.0 / DEPTH_RANGE],
            geom: F3DEX2_CONSTS.g_clipping,
            consts: F3DEX2_CONSTS,
            data_format: crate::hle::mem::GbiDataFormat::Fixed,
            texture_state: TextureState::default(),
            mvp_table: vec![identity()],
            viewport_table: vec![(
                [FB_WIDTH / 2.0, FB_HEIGHT / 2.0, 511.0 / DEPTH_RANGE],
                [FB_WIDTH / 2.0, FB_HEIGHT / 2.0, 511.0 / DEPTH_RANGE],
            )],
            cur_mvp_index: 0,
            cur_viewport_index: 0,
            // Default entry 0 = zero scale (sc=tc=0).
            texcoord_table: vec![[0.0, 0.0]],
            cur_texcoord_index: 0,
            modify_unit_texcoord_index: None,
            lights: [([0.0; 3], [0.0; 3]); 8],
            ambient_col: [0.0; 3],
            num_dir: 0,
            light_version: 0,
            last_light_key: None,
            cur_light_index: 0,
            lookat_axes: [[0.0; 3]; 2],
            lookat_version: 0,
            last_lookat_key: None,
            cur_lookat_index: 0,
            // Seed entry 0 = zero scale, mirroring texcoord_table so the two stay index-parallel.
            texgen_scale_table: vec![[0.0, 0.0]],
            material_dirty: true,
            last_material: None,
            last_material_index: None,
            last_render_mode: None,
            last_render_mode_index: None,
        }
    }
}

impl Rsp {
    pub(crate) fn new(
        consts: crate::hle::gbi::GbiConstants,
        data_format: crate::hle::mem::GbiDataFormat,
    ) -> Self {
        Rsp {
            geom: consts.g_clipping,
            consts,
            data_format,
            ..Rsp::default()
        }
    }

    pub fn geometry_mode(&self) -> u32 {
        self.geom
    }

    /// Flush the accumulated state tables onto the scene (call once after the DL walk).
    ///
    /// NOTE: mvp_table/viewport_table/texcoord_table are only copied onto the Scene here. A caller
    /// that drives set_vertex/draw_tri directly (instead of via interpret) must call this before
    /// reading those Scene tables.
    pub fn finish(&self, scene: &mut Scene) {
        scene.mvp_table = self.mvp_table.clone();
        scene.viewport_table = self.viewport_table.clone();
        scene.texcoord_table = self.texcoord_table.clone();
        scene.texgen_scale_table = self.texgen_scale_table.clone();
    }

    fn recompute_mvp(&mut self) {
        self.mvp = mul4(self.model_stack[self.model_stack_size - 1], self.viewproj);
        // Append a new MVP table entry (dedup against the last to avoid trivial growth).
        if self.mvp_table.last() != Some(&self.mvp) {
            self.mvp_table.push(self.mvp);
            self.cur_mvp_index = (self.mvp_table.len() - 1) as u32;
        }
    }

    pub fn matrix<M: Rdram>(&mut self, mem: &M, addr: u64, params: u8) {
        let m = mem.read_matrix(addr, self.data_format);
        let is_proj = params & self.consts.g_mtx_projection != 0;
        let is_load = params & self.consts.g_mtx_load != 0;
        let is_push = params & self.consts.g_mtx_push != 0;
        if is_proj {
            // Projection is single (no stack), per spec §3.
            self.viewproj = if is_load { m } else { mul4(m, self.viewproj) };
        } else {
            // Modelview stack: push copies the
            // current top, then LOAD/MUL always writes the (new) top — even when the push
            // was dropped at the 32 ceiling.
            if is_push && self.model_stack_size < RSP_MATRIX_STACK_SIZE {
                self.model_stack[self.model_stack_size] =
                    self.model_stack[self.model_stack_size - 1];
                self.model_stack_size += 1;
            }
            let top = self.model_stack_size - 1;
            self.model_stack[top] = if is_load {
                m
            } else {
                mul4(m, self.model_stack[top])
            };
        }
        self.recompute_mvp();
    }

    /// F3D G_MV_MATRIX_1: overwrite the current MVP with a full matrix loaded directly from
    /// RDRAM. The matrix stacks remain unchanged, so the next matrix operation recomputes from
    /// their state.
    pub fn force_matrix<M: Rdram>(&mut self, mem: &M, addr: u64) {
        self.mvp = mem.read_matrix(addr, self.data_format);
        if self.mvp_table.last() != Some(&self.mvp) {
            self.mvp_table.push(self.mvp);
        }
        self.cur_mvp_index = (self.mvp_table.len() - 1) as u32;
    }

    /// G_POPMTX: pop `count` modelview frames, never below 1.
    pub fn pop_matrix(&mut self, count: u32) {
        let old_size = self.model_stack_size;
        for _ in 0..count {
            if self.model_stack_size > 1 {
                self.model_stack_size -= 1;
            }
        }
        if self.model_stack_size != old_size {
            self.recompute_mvp();
        }
    }

    /// G_GEOMETRYMODE: mode = (mode & offMask) | onMask.
    pub fn modify_geometry_mode(&mut self, off_mask: u32, on_mask: u32) {
        let old = self.geom;
        self.geom = (old & off_mask) | on_mask;
        // Bump light_version when G_LIGHTING changes to prevent stale prefold cache.
        if (old ^ self.geom) & self.consts.g_lighting != 0 {
            self.light_version += 1;
        }
    }

    pub fn set_viewport<M: Rdram>(&mut self, mem: &M, addr: u64) {
        let vscale = [
            mem.read_i16(addr),
            mem.read_i16(addr.saturating_add(2)),
            mem.read_i16(addr.saturating_add(4)),
            mem.read_i16(addr.saturating_add(6)),
        ];
        let vtrans = [
            mem.read_i16(addr.saturating_add(8)),
            mem.read_i16(addr.saturating_add(10)),
            mem.read_i16(addr.saturating_add(12)),
            mem.read_i16(addr.saturating_add(14)),
        ];
        // Authentic libultra order: X=index0, Y=index1, Z=index2 / DepthRange.
        // Some ports read vscale[1] for X / vscale[0] for Y; that swap is a host-byteswap
        // artifact D1 removes — authentic libultra vscale[0]=X-scale.
        self.vp_scale = [
            vscale[0] as f32 / 4.0,
            vscale[1] as f32 / 4.0,
            vscale[2] as f32 / DEPTH_RANGE,
        ];
        self.vp_trans = [
            vtrans[0] as f32 / 4.0,
            vtrans[1] as f32 / 4.0,
            vtrans[2] as f32 / DEPTH_RANGE,
        ];
        let entry = (self.vp_scale, self.vp_trans);
        if self.viewport_table.last() != Some(&entry) {
            self.viewport_table.push(entry);
            self.cur_viewport_index = (self.viewport_table.len() - 1) as u32;
        }
    }

    pub fn set_vertex<M: Rdram>(
        &mut self,
        mem: &M,
        addr: u64,
        count: u32,
        dst: u32,
        rdp: &crate::hle::rdp::Rdp,
        scene: &mut Scene,
    ) {
        // NOTE: mtx_index/viewport_index/texcoord_index pushed per-vertex here only resolve to
        // their respective tables after `finish()` copies them onto the Scene.
        // Texel-space texcoord scale: sc/(65536*32) with NO tile-size division. Tile normalization
        // is a rasterization-time property, so it is deferred to the fragment shader (draw-time tile
        // dims via `CombinerUniform.inv_tex_size`). Keeping the vertex texcoord tile-INDEPENDENT lets
        // a DL that issues G_SETTILESIZE *after* gsSPVertex (sm64's power-meter frame loads 8 verts
        // before sizing the 32×64 tile) still sample at the correct scale, not a stale tile size.
        // Prefold in f64 so the kernel does one f32 multiply (WGSL has no f64).
        const TC_DIVISOR: f64 = 65536.0 * 32.0;
        let tc_entry = [
            (self.texture_state.sc as f64 / TC_DIVISOR) as f32,
            (self.texture_state.tc as f64 / TC_DIVISOR) as f32,
        ];
        let tg_entry = [
            (self.texture_state.sc as f64 / 65536.0) as f32,
            (self.texture_state.tc as f64 / 65536.0) as f32,
        ];
        if self.texcoord_table.last() != Some(&tc_entry) {
            self.texcoord_table.push(tc_entry);
            self.texgen_scale_table.push(tg_entry);
            self.cur_texcoord_index = (self.texcoord_table.len() - 1) as u32;
        }
        // Object-space light set for these vertices: bring each eye-space light dir INTO object
        // space via the INVERSE modelview rotation (mul_col_vec3 = M·v), so the lit region stays
        // world-fixed as the model spins. (mul_row_vec4 here would apply the FORWARD rotation and
        // co-rotate the light with the geometry — see math::mul_col_vec3.)
        let lit = (self.geom & self.consts.g_lighting) != 0;
        let light_count = if lit { self.num_dir + 1 } else { 0 };
        if lit {
            let mv = self.model_stack[self.model_stack_size - 1];
            let key = (self.light_version, mv);
            if self.last_light_key != Some(key) {
                self.cur_light_index = scene.lights_table.len() as u32;
                for k in 0..self.num_dir as usize {
                    let (dir, col) = self.lights[k];
                    let o = mul_col_vec3(mv, dir);
                    let len = (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt();
                    let inv = if len > 0.0 { 1.0 / len } else { 0.0 };
                    scene
                        .lights_table
                        .push(([o[0] * inv, o[1] * inv, o[2] * inv], col));
                }
                scene.lights_table.push(([0.0, 0.0, 0.0], self.ambient_col)); // ambient last
                self.last_light_key = Some(key);
            }
        }
        // Object-space lookat prefold (mirrors lights): bring each eye-space lookat axis INTO object
        // space via mul_col_vec3 (inverse rotation) so texgen stays world-fixed as the model spins.
        // Texgen rides the normal lighting datapath, so it is gated on G_LIGHTING.
        let texgen_mode: u32 = if lit && (self.geom & self.consts.g_texture_gen) != 0 {
            if (self.geom & self.consts.g_texture_gen_linear) != 0 {
                2
            } else {
                1
            }
        } else {
            0
        };
        if texgen_mode != 0 {
            let mv = self.model_stack[self.model_stack_size - 1];
            let key = (self.lookat_version, mv);
            if self.last_lookat_key != Some(key) {
                self.cur_lookat_index = scene.lookat_table.len() as u32;
                let nz = |o: [f32; 3]| {
                    let l = (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt();
                    let i = if l > 0.0 { 1.0 / l } else { 0.0 };
                    [o[0] * i, o[1] * i, o[2] * i]
                };
                let s = nz(mul_col_vec3(mv, self.lookat_axes[0]));
                let t = nz(mul_col_vec3(mv, self.lookat_axes[1]));
                scene.lookat_table.push((s, t));
                self.last_lookat_key = Some(key);
            }
        }
        // Vertex stride + field layout are backend-decided (fixed-point vs GBI_FLOATS) — see
        // `Rdram::read_vertex`. Decoding a float-GBI vertex as s16 misreads every position.
        let stride = mem.vertex_stride(self.data_format);
        let fog_index = if (self.geom & self.consts.g_fog_geom) != 0 {
            let factors = [rdp.fog_mul, rdp.fog_offset];
            let index = scene.fog_table.iter().position(|&entry| entry == factors);
            index.unwrap_or_else(|| {
                scene.fog_table.push(factors);
                scene.fog_table.len() - 1
            }) as u32
                + 1
        } else {
            0
        };
        for i in 0..count {
            let o = addr.saturating_add((i as u64) * stride);
            let v = mem.read_vertex(o, self.data_format);
            let slot = (dst + i) as usize;
            let gi = scene.raw_pos.len() as u32;
            self.cache_global_index[slot] = gi;
            self.loaded[slot] = true;
            self.used[slot] = false;
            let [x, y, z] = v.pos;
            let clip = mul_row_vec4([x, y, z, 1.0], self.mvp);
            self.clip_codes[slot] = clip.iter().all(|v| v.is_finite()).then(|| {
                let [x, y, z, w] = clip;
                [x < -w, x > w, y < -w, y > w, z < -w, z > w]
                    .into_iter()
                    .enumerate()
                    .fold(0, |code, (plane, outside)| {
                        code | (u8::from(outside) << plane)
                    })
            });
            self.screen_z[slot] = self.clip_codes[slot].and_then(|_| {
                if clip[3] == 0.0 {
                    return None;
                }
                let z = (f64::from(clip[2]) / f64::from(clip[3]) * f64::from(self.vp_scale[2])
                    + f64::from(self.vp_trans[2]))
                    * f64::from(DEPTH_RANGE);
                z.is_finite().then_some(z)
            });
            scene.raw_pos.push(v.pos);
            scene.modify_flags.push(0);
            scene.modify_screen.push([0.0; 4]);
            scene.mtx_index.push(self.cur_mvp_index);
            scene.viewport_index.push(self.cur_viewport_index);
            scene.raw_st.push([v.st[0] as f32, v.st[1] as f32]);
            scene.texcoord_index.push(self.cur_texcoord_index);
            let cn = u32::from_le_bytes(v.rgba); // color (unlit) or packed s8 normal (lit)
            scene.cn.push(cn);
            scene
                .light_index
                .push(if lit { self.cur_light_index } else { 0 });
            scene.light_count.push(light_count);
            scene.texgen_mode.push(texgen_mode);
            scene.fog.push(fog_index);
            scene.lookat_index.push(if texgen_mode != 0 {
                self.cur_lookat_index
            } else {
                0
            });
        }
    }

    fn modify_unit_texcoord_index(&mut self) -> u32 {
        if let Some(index) = self.modify_unit_texcoord_index {
            return index;
        }
        self.texcoord_table.push([1.0, 1.0]);
        self.texgen_scale_table.push([0.0, 0.0]);
        let index = (self.texcoord_table.len() - 1) as u32;
        self.modify_unit_texcoord_index = Some(index);
        index
    }

    pub(crate) fn cull_display_list(
        &self,
        first: u32,
        last: u32,
    ) -> Result<bool, crate::diag::DiagKind> {
        use crate::diag::DiagKind;
        if first > last || last as usize >= RSP_MAX_VERTICES {
            return Err(DiagKind::InvalidCullRange { first, last });
        }
        // RSP CULLDL's z convention is unverified; ignore those codes until a reference
        // establishes it, since a false cull would discard visible geometry.
        let mut common = 0x0f;
        for index in first..=last {
            if !self.loaded[index as usize] {
                return Err(DiagKind::InvalidConditionalVertex {
                    opcode: crate::hle::consts::G_CULLDL,
                    index,
                });
            }
            common &= self.clip_codes[index as usize]
                .ok_or(DiagKind::InvalidVertexTransform { index })?;
        }
        Ok(common != 0)
    }

    pub(crate) fn branch_z(
        &self,
        index: u32,
        threshold: u32,
    ) -> Result<bool, crate::diag::DiagKind> {
        use crate::diag::DiagKind;
        if index as usize >= RSP_MAX_VERTICES || !self.loaded[index as usize] {
            return Err(DiagKind::InvalidConditionalVertex {
                opcode: crate::hle::consts::G_BRANCH_Z,
                index,
            });
        }
        let z = self.screen_z[index as usize].ok_or(DiagKind::InvalidVertexTransform { index })?;
        Ok(z <= f64::from(threshold) / 65536.0)
    }

    /// Modify a loaded cache slot without changing vertices recorded by earlier draws.
    pub fn modify_vertex(
        &mut self,
        dst_index: u32,
        attr: u32,
        value: u32,
        scene: &mut Scene,
    ) -> Result<(), crate::diag::DiagKind> {
        let slot = dst_index as usize;
        if slot >= RSP_MAX_VERTICES
            || !self.loaded[slot]
            || !matches!(attr, 0x10 | 0x14 | 0x18 | 0x1C)
        {
            return Err(crate::diag::DiagKind::InvalidModifyVertex {
                index: dst_index,
                attribute: attr,
            });
        }
        let mut gi = self.cache_global_index[slot] as usize;

        // RT64 copy-on-use: a modify must not edit a vertex row already recorded by a draw.
        // Intentional RT64 divergence: carry modify state into the clone, matching in-place DMEM writes.
        if self.used[slot] {
            let next = scene.raw_pos.len();
            scene.raw_pos.push(scene.raw_pos[gi]);
            scene.raw_st.push(scene.raw_st[gi]);
            scene.mtx_index.push(scene.mtx_index[gi]);
            scene.viewport_index.push(scene.viewport_index[gi]);
            scene.texcoord_index.push(scene.texcoord_index[gi]);
            scene.cn.push(scene.cn[gi]);
            scene.light_index.push(scene.light_index[gi]);
            scene.light_count.push(scene.light_count[gi]);
            scene.texgen_mode.push(scene.texgen_mode[gi]);
            scene.fog.push(scene.fog[gi]);
            scene.lookat_index.push(scene.lookat_index[gi]);
            scene.modify_flags.push(scene.modify_flags[gi]);
            scene.modify_screen.push(scene.modify_screen[gi]);
            self.cache_global_index[slot] = next as u32;
            self.used[slot] = false;
            gi = next;
        }

        match attr {
            0x10 => {
                scene.cn[gi] = u32::from_le_bytes(value.to_be_bytes());
                scene.light_index[gi] = 0;
                scene.light_count[gi] = 0;
                scene.fog[gi] = 0;
            }
            0x14 => {
                scene.raw_st[gi] = [
                    (value >> 16) as i16 as f32 / 32.0,
                    value as i16 as f32 / 32.0,
                ];
                scene.texcoord_index[gi] = self.modify_unit_texcoord_index();
                scene.texgen_mode[gi] = 0;
                scene.lookat_index[gi] = 0;
            }
            0x18 => {
                scene.modify_screen[gi][0] = (value >> 16) as i16 as f32 / 4.0;
                scene.modify_screen[gi][1] = value as i16 as f32 / 4.0;
                scene.modify_flags[gi] |= 1;
            }
            0x1C => {
                scene.modify_screen[gi][2] = value as f32 / 65536.0;
                self.screen_z[slot] = self.screen_z[slot].map(|_| f64::from(value) / 65536.0);
                scene.modify_flags[gi] |= 2;
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    /// Record a triangle. `pair_target` routes the per-triangle `DrawRun`:
    /// - `None`  → the flat `scene.draw_runs` (the 3D / pair-less path, byte-identical to before);
    /// - `Some(p)` → `scene.framebuffer_pairs[p].ops` as a `SceneOp::Tris` (the 2D-recording path).
    ///
    /// Either way the three indices are pushed onto the shared `scene.indices` buffer in draw order,
    /// and coalescing only extends the LAST run/op when it is a `Tris` with a matching
    /// `(cull, material_index, render_mode_index, fog_color)` key.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_tri(
        &mut self,
        a: u32,
        b: u32,
        c: u32,
        material_index: u32,
        render_mode_index: u32,
        fog_color: [u8; 4],
        scene: &mut Scene,
        pair_target: Option<usize>,
    ) {
        let cull = self.geom & self.consts.g_cull_both;
        // Cull-both: draw nothing.
        if cull == self.consts.g_cull_both {
            return;
        }
        for slot in [a, b, c] {
            self.used[slot as usize] = true;
        }
        let (mut a, b, mut c) = (a, b, c);
        // Cull-front: swap a<->c so the single binary cull state culls front faces.
        // The GPU rasterizer does the area test.
        if cull == self.consts.g_cull_front {
            std::mem::swap(&mut a, &mut c);
        }
        let kind = if cull != 0 {
            CullKind::Cull
        } else {
            CullKind::None
        };
        let coalesces = |run: &DrawRun| {
            run.cull == kind
                && run.material_index == material_index
                && run.render_mode_index == render_mode_index
                && run.fog_color == fog_color
        };
        let index_start = scene.indices.len() as u32;
        match pair_target {
            Some(p) => match scene.framebuffer_pairs[p].ops.last_mut() {
                Some(SceneOp::Tris(run)) if coalesces(run) => run.index_count += 3,
                _ => scene.framebuffer_pairs[p].ops.push(SceneOp::Tris(DrawRun {
                    material_index,
                    render_mode_index,
                    fog_color,
                    cull: kind,
                    index_count: 3,
                    index_start,
                })),
            },
            None => match scene.draw_runs.last_mut() {
                Some(run) if coalesces(run) => run.index_count += 3,
                _ => scene.draw_runs.push(DrawRun {
                    material_index,
                    render_mode_index,
                    fog_color,
                    cull: kind,
                    index_count: 3,
                    index_start,
                }),
            },
        }
        scene.indices.push(self.cache_global_index[a as usize]);
        scene.indices.push(self.cache_global_index[b as usize]);
        scene.indices.push(self.cache_global_index[c as usize]);
    }

    /// gsSPTexture: set the texture scaling state.
    pub fn set_texture(&mut self, tile: u8, level: u8, on: bool, sc: u16, tc: u16) {
        self.texture_state = TextureState {
            tile,
            level,
            on,
            sc,
            tc,
        };
    }

    /// SetOtherMode_L bit-field update: length = p0_len_field+1; off = 32 - p0_shift_field - length.
    pub fn set_other_mode_l(
        &mut self,
        p0_shift_field: u32,
        p0_len_field: u32,
        data: u32,
        rdp: &mut crate::hle::rdp::Rdp,
    ) {
        let length = p0_len_field + 1;
        let shift = 32 - p0_shift_field - length;
        self.set_other_mode_l_raw(shift, length, data, rdp);
    }

    /// SetOtherMode_L bit-field update with a raw least-significant-bit shift and bit length.
    pub fn set_other_mode_l_raw(
        &mut self,
        shift: u32,
        length: u32,
        data: u32,
        rdp: &mut crate::hle::rdp::Rdp,
    ) {
        if length == 0 || length > 32 || shift > 32 - length {
            return;
        }
        let mask = if length == 32 {
            u32::MAX
        } else {
            ((1u32 << length) - 1) << shift
        };
        rdp.other_mode_l = (rdp.other_mode_l & !mask) | data;
    }

    /// SetOtherMode_H bit-field update: length = p0_len_field+1; off = 32 - p0_shift_field - length.
    pub fn set_other_mode_h(
        &mut self,
        p0_shift_field: u32,
        p0_len_field: u32,
        data: u32,
        rdp: &mut crate::hle::rdp::Rdp,
    ) {
        let length = p0_len_field + 1;
        let shift = 32 - p0_shift_field - length;
        self.set_other_mode_h_raw(shift, length, data, rdp);
    }

    /// SetOtherMode_H bit-field update with a raw least-significant-bit shift and bit length.
    pub fn set_other_mode_h_raw(
        &mut self,
        shift: u32,
        length: u32,
        data: u32,
        rdp: &mut crate::hle::rdp::Rdp,
    ) {
        if length == 0 || length > 32 || shift > 32 - length {
            return;
        }
        let mask = if length == 32 {
            u32::MAX
        } else {
            ((1u32 << length) - 1) << shift
        };
        rdp.other_mode_h = (rdp.other_mode_h & !mask) | data;
    }

    pub fn set_texture_image(
        &mut self,
        fmt: u8,
        siz: u8,
        width: u16,
        addr: u64,
        rdp: &mut crate::hle::rdp::Rdp,
    ) {
        rdp.tex_image = (fmt, siz, width, addr);
    }

    /// G_MW_NUMLIGHT: w1 = n * 24 where n is the number of directional lights.
    pub fn set_num_lights(&mut self, w1: u32) {
        self.set_num_lights_direct(w1 / 24);
    }

    /// Set the directional-light count without applying a microcode-specific word transform.
    pub fn set_num_lights_direct(&mut self, n: u32) {
        self.num_dir = n.min(RSP_MAX_LIGHTS);
        self.light_version += 1;
    }

    /// G_MV_LIGHT: load a directional or ambient light from RDRAM.
    /// `light_idx`: slot index (0-based, after removing lookat slots 0/1).
    /// `addr`: pre-resolved physical address of the Light_t or Ambient_t struct.
    pub fn set_light<M: Rdram>(&mut self, mem: &M, light_idx: u32, addr: u64) {
        if light_idx == self.num_dir {
            // ambient (8B Ambient_t: col@0..2, no dir)
            self.ambient_col = [
                mem.read_u8(addr) as f32 / 255.0,
                mem.read_u8(addr.saturating_add(1)) as f32 / 255.0,
                mem.read_u8(addr.saturating_add(2)) as f32 / 255.0,
            ];
        } else if (light_idx as usize) < self.lights.len() {
            let col = [
                mem.read_u8(addr) as f32 / 255.0,
                mem.read_u8(addr.saturating_add(1)) as f32 / 255.0,
                mem.read_u8(addr.saturating_add(2)) as f32 / 255.0,
            ];
            let dir = [
                mem.read_i8(addr.saturating_add(8)) as f32 / 127.0,
                mem.read_i8(addr.saturating_add(9)) as f32 / 127.0,
                mem.read_i8(addr.saturating_add(10)) as f32 / 127.0,
            ];
            self.lights[light_idx as usize] = (dir, col);
        }
        self.light_version += 1;
    }

    /// F3D G_MW_LIGHTCOL: update one light's color from packed 0xRRGGBBAA while preserving its
    /// direction. The ambient light occupies slot `num_dir`, matching `set_light`.
    pub fn set_light_color(&mut self, light_idx: u32, rgba: u32) {
        let col = [
            ((rgba >> 24) & 0xFF) as f32 / 255.0,
            ((rgba >> 16) & 0xFF) as f32 / 255.0,
            ((rgba >> 8) & 0xFF) as f32 / 255.0,
        ];
        if light_idx == self.num_dir {
            self.ambient_col = col;
        } else if (light_idx as usize) < self.lights.len() {
            self.lights[light_idx as usize].1 = col;
        }
        self.light_version += 1;
    }

    /// G_MV_LIGHT DMEM slot 0/1: load the s8 lookat axis (S=0, T=1) into object-relative eye space.
    pub fn set_lookat<M: Rdram>(&mut self, mem: &M, slot: u32, addr: u64) {
        if (slot as usize) < self.lookat_axes.len() {
            self.lookat_axes[slot as usize] = [
                mem.read_i8(addr.saturating_add(8)) as f32 / 127.0,
                mem.read_i8(addr.saturating_add(9)) as f32 / 127.0,
                mem.read_i8(addr.saturating_add(10)) as f32 / 127.0,
            ];
        }
        self.lookat_version += 1;
    }
}

/// Snapshot the current run's material + render mode at a triangle boundary.
///
/// Returns `(material_index, render_mode_index)`, or `None` to DROP this run's triangles
/// (build_material returned None — diag already pushed by the callee).
///
/// Material is rebuilt only when `rsp.material_dirty` (or no prior build succeeded);
/// render mode is decoded + deduped every call (cheap, no texture state involved).
/// When a NEW render mode with `non_canonical_blend` is pushed, one Diagnostic is emitted
/// (the §4.4/§9 additive-clamp diagnostic [IMP12]).
pub fn snapshot_run(
    rsp: &mut Rsp,
    rdp: &crate::hle::rdp::Rdp,
    diags: &mut Vec<crate::diag::Diagnostic>,
    scene: &mut Scene,
    pc: u64,
) -> Option<(u32, u32)> {
    // --- Material ---
    let material_index = if rsp.material_dirty {
        // Rebuild: build_material borrows rdp + rsp immutably; ? early-exits (None) on failure.
        let m = crate::hle::combiner::build_material(rdp, rsp, diags, pc)?;
        rsp.material_dirty = false;
        if rsp.last_material.as_ref() == Some(&m) {
            // Identical material: reuse the existing index (dedup).
            rsp.last_material_index.unwrap()
        } else {
            let idx = scene.materials.len() as u32;
            scene.materials.push(m.clone());
            rsp.last_material = Some(m);
            rsp.last_material_index = Some(idx);
            idx
        }
    } else if let Some(idx) = rsp.last_material_index {
        // Not dirty and already have a cached index: short-circuit.
        idx
    } else {
        // Dirty flag was false but no cached index yet (first tri ever); build once.
        let m = crate::hle::combiner::build_material(rdp, rsp, diags, pc)?;
        rsp.material_dirty = false;
        let idx = scene.materials.len() as u32;
        scene.materials.push(m.clone());
        rsp.last_material = Some(m);
        rsp.last_material_index = Some(idx);
        idx
    };

    // --- Render mode (decoded every call; cheap, no texture) ---
    let rm = crate::hle::blender::decode_render_mode(rdp.other_mode_l, rdp.other_mode_h, rsp.geom);
    let render_mode_index = if rsp.last_render_mode == Some(rm) {
        // Same render mode: reuse the existing index (dedup).
        rsp.last_render_mode_index.unwrap()
    } else {
        // §4.4/§9 [IMP12]: new non-canonical blended mode → additive-clamp diagnostic.
        if rm.non_canonical_blend {
            diags.push(crate::diag::Diagnostic {
                at: pc,
                kind: crate::diag::DiagKind::NonCanonicalBlend,
            });
        }
        let idx = scene.render_modes.len() as u32;
        scene.render_modes.push(rm);
        rsp.last_render_mode = Some(rm);
        rsp.last_render_mode_index = Some(idx);
        idx
    };

    Some((material_index, render_mode_index))
}

/// Walk-state for the 2D framebuffer-pair recorder (spec §1.1). Lives as a local in `interpret`
/// and is threaded through `Ctx` so both the inline rect slot and `draw_tri` (via `record_tri`)
/// share it. Pair-less scenes (no G_SETCIMG) leave this all-default → no pairs, flat `draw_runs`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PairRec {
    /// Set once the first G_SETCIMG is observed; gates the paired vs flat draw routing.
    pub have_seen_cimg: bool,
    /// True once at least one `FramebufferPair` has been opened.
    pub paired: bool,
    /// Index of the currently-open pair in `scene.framebuffer_pairs`.
    pub cur_pair: usize,
    /// Scissor currently in effect for the open pair (drives mid-pair `SetScissor` ops).
    pub last_scissor: Scissor,
}

/// Bytes per pixel for an RDP image size code (G_IM_SIZ_*): 4b→0, 8b→1, 16b→2, 32b→4.
pub(crate) fn bpp(siz: u8) -> u64 {
    (1u64 << siz) >> 1
}

/// Open a new `FramebufferPair` lazily on the first draw after a color/depth `changed` delta
/// (spec §1.1). Snapshots the color/depth image, scissor (→ `active_scissor`), framebuffer extent,
/// and the depth-clear flag, then clears the `changed` flags. No empty-pair reuse: only a draw
/// opens a pair, so an unused CIMG never produces an empty pair.
pub(crate) fn ensure_pair_open(
    scene: &mut Scene,
    rdp: &mut crate::hle::rdp::Rdp,
    rec: &mut PairRec,
) {
    if !rec.paired || rdp.color_changed || rdp.depth_changed {
        let depth_image = (rdp.depth_image != 0).then_some(rdp.depth_image);
        let is_depth_clear = depth_image == Some(rdp.color_image.addr);
        scene.framebuffer_pairs.push(FramebufferPair {
            color_image: rdp.color_image,
            depth_image,
            ops: Vec::new(),
            active_scissor: rdp.scissor,
            size_extent: (rdp.color_image.width as u32, rdp.scissor.lry.max(0) as u32),
            is_depth_clear,
        });
        rec.cur_pair = scene.framebuffer_pairs.len() - 1;
        rec.paired = true;
        rec.last_scissor = rdp.scissor;
        rdp.color_changed = false;
        rdp.depth_changed = false;
    }
}

/// Push a `SceneOp::SetScissor` into the current pair when the scissor changed mid-pair (i.e. since
/// the pair opened or the last recorded scissor). Called at every draw site after `ensure_pair_open`.
pub(crate) fn record_scissor_if_changed(
    scene: &mut Scene,
    rdp: &crate::hle::rdp::Rdp,
    rec: &mut PairRec,
) {
    if rdp.scissor != rec.last_scissor {
        scene.framebuffer_pairs[rec.cur_pair]
            .ops
            .push(SceneOp::SetScissor(rdp.scissor));
        rec.last_scissor = rdp.scissor;
    }
}

/// Snapshot a valid TexRect material and render mode, deduped against the last scene entry.
pub(crate) fn snapshot_rect_run(
    rsp: &Rsp,
    rdp: &crate::hle::rdp::Rdp,
    tile: u8,
    diags: &mut Vec<crate::diag::Diagnostic>,
    scene: &mut Scene,
    pc: u64,
) -> Option<(u32, u32)> {
    let m = crate::hle::combiner::build_rect_material(rdp, rsp, tile, diags, pc)?;
    let material_index = match scene.materials.last() {
        Some(last) if *last == m => (scene.materials.len() - 1) as u32,
        _ => {
            scene.materials.push(m);
            (scene.materials.len() - 1) as u32
        }
    };
    let rm = crate::hle::blender::decode_render_mode(rdp.other_mode_l, rdp.other_mode_h, rsp.geom);
    let render_mode_index = match scene.render_modes.last() {
        Some(last) if *last == rm => (scene.render_modes.len() - 1) as u32,
        _ => {
            scene.render_modes.push(rm);
            (scene.render_modes.len() - 1) as u32
        }
    };
    Some((material_index, render_mode_index))
}

/// Record a triangle through the pair recorder. When a CIMG has been seen the tri is routed into the
/// current `FramebufferPair`'s ordered op-stream (opening a pair / emitting a `SetScissor` as needed);
/// otherwise it falls through to the flat `draw_runs` path SILENTLY — a pure-3D DL never emits a CIMG,
/// so this keeps pair-less scenes byte-identical (NO "pre-CIMG" diagnostic for triangles).
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_tri(
    rsp: &mut Rsp,
    rdp: &mut crate::hle::rdp::Rdp,
    scene: &mut Scene,
    rec: &mut PairRec,
    a: u32,
    b: u32,
    c: u32,
    material_index: u32,
    render_mode_index: u32,
) {
    if rec.have_seen_cimg {
        ensure_pair_open(scene, rdp, rec);
        record_scissor_if_changed(scene, rdp, rec);
        rsp.draw_tri(
            a,
            b,
            c,
            material_index,
            render_mode_index,
            rdp.fog_color,
            scene,
            Some(rec.cur_pair),
        );
    } else {
        rsp.draw_tri(
            a,
            b,
            c,
            material_index,
            render_mode_index,
            rdp.fog_color,
            scene,
            None,
        );
    }
}

#[cfg(test)]
mod clip_code_tests {
    use super::*;
    use crate::hle::mem::RdramImage;

    #[test]
    fn unverified_z_codes_still_computed_but_unused_by_culldl() {
        for (z, code) in [(-3i16, 0x10), (-2, 0), (0, 0), (2, 0), (3, 0x20)] {
            let mut projection = crate::hle::math::identity();
            projection[3][3] = 2.0;
            let mut bytes = n64_gbi::encode::mtx_to_bytes(projection).to_vec();
            let mut vertex = [0u8; 16];
            vertex[4..6].copy_from_slice(&z.to_be_bytes());
            bytes.extend(vertex);
            let mem = RdramImage::new(&bytes);
            let mut rsp = Rsp::default();
            let mut scene = Scene::default();
            rsp.matrix(
                &mem,
                0,
                crate::hle::consts::G_MTX_PROJECTION | crate::hle::consts::G_MTX_LOAD,
            );
            rsp.set_vertex(&mem, 64, 1, 0, &Default::default(), &mut scene);
            assert_eq!(rsp.clip_codes[0], Some(code), "z={z}, w=2");
            assert_eq!(rsp.cull_display_list(0, 0), Ok(false), "z={z}, w=2");
        }
    }
}

#[cfg(test)]
mod consts_wired_tests {
    use super::*;
    use crate::hle::gbi::{GbiConstants, GbiUcode};

    // T-wired: the initial geometry-mode seed must come FROM consts.g_clipping, not a
    // hardcode. Flipping the field changes the seeded geom — proving the value flows.
    #[test]
    fn rsp_seeds_initial_geom_from_consts_clipping() {
        let base = GbiUcode::F3dex2.constants();
        let tweaked = GbiConstants {
            g_clipping: 0x1234,
            ..base
        };
        assert_eq!(
            Rsp::new(tweaked, crate::hle::mem::GbiDataFormat::Fixed).geometry_mode(),
            0x1234
        );
        assert_eq!(
            Rsp::new(base, crate::hle::mem::GbiDataFormat::Fixed).geometry_mode(),
            crate::hle::consts::G_CLIPPING
        );
    }

    // T-rsp-seed: Rsp::new carries the descriptor's data_format, distinct from the Default Fixed
    // seed — guards a future-ucode bug where construction forgets to apply the descriptor.
    #[test]
    fn rsp_new_carries_data_format() {
        use crate::hle::mem::GbiDataFormat;
        assert_eq!(
            Rsp::new(GbiUcode::F3dex2.constants(), GbiDataFormat::Float).data_format,
            GbiDataFormat::Float
        );
        assert_eq!(Rsp::default().data_format, GbiDataFormat::Fixed);
    }
}

#[cfg(test)]
mod lights_load_tests {
    use super::*;
    use crate::hle::mem::RdramImage;

    #[test]
    fn set_num_lights_clamps_to_rsp_max_lights() {
        // 9 directional lights (9*24=216) would exceed lights[8] and panic in the prefold loop.
        // After the clamp, num_dir must be RSP_MAX_LIGHTS (7).
        let mut rsp = Rsp::default();
        rsp.set_num_lights(9 * 24);
        assert_eq!(
            rsp.num_dir, RSP_MAX_LIGHTS,
            "num_dir must clamp to RSP_MAX_LIGHTS"
        );
    }

    #[test]
    fn direct_num_lights_matches_f3dex2_encoded_path() {
        let mut direct = Rsp::default();
        let mut encoded = Rsp::default();

        direct.set_num_lights_direct(3);
        encoded.set_num_lights(3 * 24);

        assert_eq!(direct.num_dir, encoded.num_dir);
        assert_eq!(direct.light_version, encoded.light_version);
    }

    #[test]
    fn set_num_lights_clamped_set_vertex_does_not_panic() {
        // Regression: with num_dir clamped to 7 via set_num_lights(9*24),
        // a lit set_vertex must NOT panic (no out-of-bounds on self.lights[0..7]).
        let mut rsp = Rsp::default();
        rsp.set_num_lights(9 * 24); // clamps to 7
        rsp.modify_geometry_mode(!0, crate::hle::consts::G_LIGHTING);
        // 8 bytes: minimal vertex bytes (all zeros, position 0,0,0, normal 0,0,0).
        // set_vertex reads 16 bytes per vertex so we need 16 bytes.
        let bytes = vec![0u8; 16];
        let rdram = RdramImage::new(&bytes);
        let mut scene = Scene::default();
        rsp.set_vertex(&rdram, 0, 1, 0, &Default::default(), &mut scene);
        // light_count = num_dir + 1 ambient = 8
        assert_eq!(scene.light_count[0], RSP_MAX_LIGHTS + 1);
    }

    #[test]
    fn set_light_reads_dir_color_and_ambient_8b() {
        let mut rsp = Rsp::default();
        rsp.set_num_lights(24); // 1 directional
                                // Directional Light_t (16B): col@0..2, pad, colc@4..6, pad, dir s8 @8..10, pad.
        let mut dir_l = vec![0u8; 16];
        dir_l[0] = 255;
        dir_l[1] = 128;
        dir_l[2] = 64;
        dir_l[8] = 127i8 as u8;
        dir_l[9] = 0;
        dir_l[10] = 0;
        let rd = RdramImage::new(&dir_l);
        rsp.set_light(&rd, 0, 0);
        assert_eq!(rsp.lights[0].0, [1.0, 0.0, 0.0]); // dir 127/127
        assert_eq!(rsp.lights[0].1, [1.0, 128.0 / 255.0, 64.0 / 255.0]); // col
                                                                         // Ambient_t (8B): col@0..2 only (no dir). Loaded as light_idx == num_dir (==1).
        let amb = vec![16u8, 32, 48, 0, 0, 0, 0, 0];
        let rda = RdramImage::new(&amb);
        rsp.set_light(&rda, 1, 0);
        assert_eq!(rsp.ambient_col, [16.0 / 255.0, 32.0 / 255.0, 48.0 / 255.0]);
    }

    #[test]
    fn set_light_color_preserves_direction_and_updates_ambient() {
        let mut rsp = Rsp::default();
        rsp.set_num_lights_direct(1);
        rsp.lights[0] = ([1.0, -0.5, 0.25], [0.0; 3]);
        let version = rsp.light_version;

        rsp.set_light_color(0, 0x1122_33FF);
        assert_eq!(rsp.lights[0].0, [1.0, -0.5, 0.25]);
        assert_eq!(rsp.lights[0].1, [17.0 / 255.0, 34.0 / 255.0, 51.0 / 255.0]);
        assert_eq!(rsp.light_version, version + 1);

        rsp.set_light_color(1, 0xA0B0_C0FF);
        assert_eq!(
            rsp.ambient_col,
            [160.0 / 255.0, 176.0 / 255.0, 192.0 / 255.0]
        );
    }
}

#[cfg(test)]
mod phase3_state_tests {
    use super::*;
    use crate::hle::mem::RdramImage;
    use crate::hle::rdp::Rdp;
    use n64_gbi::encode::mtx_to_bytes;

    #[test]
    fn raw_and_transformed_other_mode_paths_are_equivalent() {
        let mut raw_rsp = Rsp::default();
        let mut transformed_rsp = Rsp::default();
        let mut raw_rdp = Rdp::default();
        let mut transformed_rdp = Rdp::default();
        let cycle_type = 1u32 << 20;

        raw_rsp.set_other_mode_h_raw(20, 2, cycle_type, &mut raw_rdp);
        transformed_rsp.set_other_mode_h(10, 1, cycle_type, &mut transformed_rdp);
        assert_eq!(raw_rdp.other_mode_h, transformed_rdp.other_mode_h);

        let render_mode = 0x4411_2230;
        raw_rsp.set_other_mode_l_raw(3, 29, render_mode, &mut raw_rdp);
        transformed_rsp.set_other_mode_l(0, 28, render_mode, &mut transformed_rdp);
        assert_eq!(raw_rdp.other_mode_l, transformed_rdp.other_mode_l);
    }

    #[test]
    fn force_matrix_publishes_the_loaded_matrix() {
        let forced = [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 4.0, 0.0],
            [5.0, 6.0, 7.0, 1.0],
        ];
        let bytes = mtx_to_bytes(forced);
        let mem = RdramImage::new(&bytes);
        let mut rsp = Rsp::default();
        let mut scene = Scene::default();

        rsp.force_matrix(&mem, 0);
        rsp.finish(&mut scene);

        assert_eq!(scene.mvp_table.last(), Some(&forced));
    }
}

#[cfg(test)]
mod draw_runs_tests {
    use super::*;
    use crate::hle::consts::{G_CULL_BACK, G_CULL_FRONT};

    fn rsp_three() -> (Rsp, Scene) {
        let mut rsp = Rsp::default();
        for slot in 0..3u32 {
            rsp.cache_global_index[slot as usize] = slot;
        }
        (rsp, Scene::default())
    }

    #[test]
    fn cull_off_records_none_run_in_order() {
        let (mut rsp, mut scene) = rsp_three();
        rsp.draw_tri(0, 1, 2, 0, 0, [0; 4], &mut scene, None);
        assert_eq!(scene.indices, vec![0, 1, 2]);
        assert_eq!(
            scene.draw_runs,
            vec![DrawRun {
                fog_color: [0; 4],
                material_index: 0,
                render_mode_index: 0,
                cull: CullKind::None,
                index_count: 3,
                index_start: 0,
            }]
        );
    }

    #[test]
    fn cull_front_swaps_a_c_and_marks_cull() {
        let (mut rsp, mut scene) = rsp_three();
        rsp.modify_geometry_mode(!0, G_CULL_FRONT);
        rsp.draw_tri(0, 1, 2, 0, 0, [0; 4], &mut scene, None);
        assert_eq!(scene.indices, vec![2, 1, 0]);
        assert_eq!(
            scene.draw_runs,
            vec![DrawRun {
                fog_color: [0; 4],
                material_index: 0,
                render_mode_index: 0,
                cull: CullKind::Cull,
                index_count: 3,
                index_start: 0,
            }]
        );
    }

    #[test]
    fn runs_coalesce_and_split_on_cull_change() {
        let (mut rsp, mut scene) = rsp_three();
        rsp.draw_tri(0, 1, 2, 0, 0, [0; 4], &mut scene, None);
        rsp.modify_geometry_mode(!0, G_CULL_BACK);
        rsp.draw_tri(0, 1, 2, 0, 0, [0; 4], &mut scene, None);
        rsp.draw_tri(0, 1, 2, 0, 0, [0; 4], &mut scene, None);
        assert_eq!(
            scene.draw_runs,
            vec![
                DrawRun {
                    fog_color: [0; 4],
                    material_index: 0,
                    render_mode_index: 0,
                    cull: CullKind::None,
                    index_count: 3,
                    index_start: 0,
                },
                DrawRun {
                    fog_color: [0; 4],
                    material_index: 0,
                    render_mode_index: 0,
                    cull: CullKind::Cull,
                    index_count: 6,
                    index_start: 3,
                },
            ]
        );
    }

    #[test]
    fn drawrun_carries_material_and_render_mode_index() {
        let (mut rsp, mut scene) = rsp_three();
        rsp.draw_tri(0, 1, 2, 5, 7, [0; 4], &mut scene, None);
        assert_eq!(
            scene.draw_runs,
            vec![DrawRun {
                fog_color: [0; 4],
                material_index: 5,
                render_mode_index: 7,
                cull: CullKind::None,
                index_count: 3,
                index_start: 0,
            }]
        );
    }

    #[test]
    fn run_splits_on_material_index_change() {
        let (mut rsp, mut scene) = rsp_three();
        rsp.draw_tri(0, 1, 2, 0, 0, [0; 4], &mut scene, None);
        rsp.draw_tri(0, 1, 2, 1, 0, [0; 4], &mut scene, None);
        assert_eq!(scene.draw_runs.len(), 2);
    }

    #[test]
    fn run_coalesces_across_texgen_boundary() {
        for pair_target in [None, Some(0)] {
            let bytes = vec![0u8; 6 * 16];
            let rd = crate::hle::mem::RdramImage::new(&bytes);
            let mut rsp = Rsp::default();
            let mut scene = Scene::default();
            scene.framebuffer_pairs.push(Default::default());
            rsp.modify_geometry_mode(
                !0,
                crate::hle::consts::G_LIGHTING | crate::hle::consts::G_TEXTURE_GEN,
            );
            rsp.set_vertex(&rd, 0, 3, 0, &Default::default(), &mut scene);
            rsp.draw_tri(0, 1, 2, 0, 0, [0; 4], &mut scene, pair_target);
            rsp.modify_geometry_mode(!crate::hle::consts::G_TEXTURE_GEN, 0);
            rsp.set_vertex(&rd, 0, 3, 3, &Default::default(), &mut scene);
            rsp.draw_tri(3, 4, 5, 0, 0, [0; 4], &mut scene, pair_target);
            rsp.draw_tri(0, 4, 2, 0, 0, [0; 4], &mut scene, pair_target);

            let runs: Vec<_> = match pair_target {
                None => scene.draw_runs.iter().collect(),
                Some(p) => scene.framebuffer_pairs[p]
                    .ops
                    .iter()
                    .filter_map(|op| match op {
                        SceneOp::Tris(run) => Some(run),
                        _ => None,
                    })
                    .collect(),
            };
            assert_eq!(runs.len(), 1, "texgen must share a run: {pair_target:?}");
            assert_eq!(runs[0].index_count, 9);
            assert_eq!(scene.texgen_mode, [1, 1, 1, 0, 0, 0]);
        }
    }
}

#[cfg(test)]
mod lights_table_tests {
    use super::*;
    use crate::hle::mem::RdramImage;

    #[test]
    fn prefold_transforms_dir_by_modelview_transpose_and_marks_ambient_last() {
        let mut rsp = Rsp::default();
        // One light pointing +X (camera space), white; ambient grey.
        rsp.lights[0] = ([1.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        rsp.ambient_col = [0.1, 0.1, 0.1];
        rsp.num_dir = 1;
        rsp.light_version += 1;
        rsp.modify_geometry_mode(!0, crate::hle::consts::G_LIGHTING);
        // identity modelview (default) — dir_obj == normalize([1,0,0]) == [1,0,0].
        let bytes = vec![0u8; 16];
        let rdram = RdramImage::new(&bytes);
        let mut scene = Scene::default();
        rsp.set_vertex(&rdram, 0, 1, 0, &Default::default(), &mut scene);
        rsp.finish(&mut scene);
        assert_eq!(scene.light_count, vec![2]); // 1 dir + ambient
        assert_eq!(scene.light_index, vec![0]);
        // identity modelview -> dir_obj == normalize([1,0,0]) == [1,0,0]
        assert_eq!(scene.lights_table[0].0, [1.0, 0.0, 0.0]); // dir_obj
        assert_eq!(scene.lights_table[1].1, [0.1, 0.1, 0.1]); // ambient col, last
    }

    /// FIX 2: non-identity modelview test so transpose(MV) is actually exercised.
    ///
    /// Modelview = 90°-CCW rotation about Z (row-vector convention):
    ///   row 0: [ 0,  1, 0, 0]
    ///   row 1: [-1,  0, 0, 0]
    ///   row 2: [ 0,  0, 1, 0]
    ///   row 3: [ 0,  0, 0, 1]
    ///
    /// mul_row_vec4([1,0,0,0], MV) = MV[0] = [0,1,0,0] → dir_obj = normalize([0,1,0]) = [0,1,0].
    ///
    /// A wrong transform (using MV instead of transpose(MV), or using MVP) would yield a
    /// different result, so this test distinguishes them.
    #[test]
    fn prefold_light_dir_is_world_fixed_under_modelview() {
        // Build 90°-CCW about Z rotation matrix bytes for RDRAM (G_MTX format).
        // Row-vector 90°-CCW about Z: v * R maps (1,0,0) → (0,1,0).
        let rot90z: crate::hle::math::Mat4 = [
            [0.0, 1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let mtx_bytes = n64_gbi::encode::mtx_to_bytes(rot90z);

        // Build RDRAM: 64-byte matrix followed by enough zeros for the vertex read (16B).
        let mut rdram_bytes = mtx_bytes.to_vec();
        rdram_bytes.extend(vec![0u8; 16]); // vertex data at offset 64
        let rdram = RdramImage::new(&rdram_bytes);

        let mut rsp = Rsp::default();
        // Load the rotation as modelview (G_MTX_MODELVIEW=0x00 | G_MTX_LOAD=0x02 | G_MTX_NOPUSH=0x00).
        // `matrix` reads at from_segmented_masked(seg_addr) which masks to &0x00FFFFF8; addr=0 is fine.
        rsp.matrix(&rdram, 0, crate::hle::consts::G_MTX_LOAD);

        // One directional light pointing +X (camera space).
        rsp.lights[0] = ([1.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        rsp.ambient_col = [0.2, 0.2, 0.2];
        rsp.set_num_lights(24);
        rsp.modify_geometry_mode(!0, crate::hle::consts::G_LIGHTING);

        // set_vertex reads from rdram at the address masked from seg_addr=64.
        // rdram.from_segmented_masked(64) = 64 & 0x00FFFFF8 = 64. Vertex bytes are all zeros.
        let mut scene = Scene::default();
        rsp.set_vertex(&rdram, 64, 1, 0, &Default::default(), &mut scene);
        rsp.finish(&mut scene);

        // WORLD-FIXEDNESS INVARIANT (convention-independent): the light is world-fixed iff
        // forward-transforming the object-space dir back through the modelview recovers the
        // original eye-space dir. forward(point) == mul_row_vec4(·, mv); applying it to dir_obj
        // must give the input dir [1,0,0]. (The co-rotating bug — using mul_row_vec4 in the
        // prefold — yields dir_obj=[0,1,0] here, whose forward transform is [-1,0,0] ≠ [1,0,0],
        // so this assertion FAILS on the bug. With the correct mul_col_vec3 prefold, dir_obj is
        // [0,-1,0] and its forward transform is [1,0,0].)
        let dir_obj = scene.lights_table[0].0;
        let back =
            crate::hle::math::mul_row_vec4([dir_obj[0], dir_obj[1], dir_obj[2], 0.0], rot90z);
        let tol = 1e-5f32;
        assert!(
            (back[0] - 1.0).abs() < tol && back[1].abs() < tol && back[2].abs() < tol,
            "light must be world-fixed: forward(dir_obj) should recover eye dir [1,0,0], got [{}, {}, {}] (dir_obj=[{}, {}, {}])",
            back[0], back[1], back[2], dir_obj[0], dir_obj[1], dir_obj[2]
        );
        // Ambient last.
        assert_eq!(scene.lights_table[1].1, [0.2, 0.2, 0.2]);
    }

    #[test]
    fn lighting_off_yields_zero_light_count() {
        let mut rsp = Rsp::default(); // G_LIGHTING not set
        let bytes = vec![0u8; 16];
        let rdram = RdramImage::new(&bytes);
        let mut scene = Scene::default();
        rsp.set_vertex(&rdram, 0, 1, 0, &Default::default(), &mut scene);
        assert_eq!(scene.light_count, vec![0]);
    }
}

#[cfg(test)]
mod texcoord_table_tests {
    use super::*;
    use crate::hle::mem::RdramImage;
    use crate::hle::rdp::Rdp;

    // 16-byte N64 vertex, big-endian. set_vertex reads s at o+8, t at o+10 (read_i16, BE).
    // RdramImage borrows its bytes, so the buffer is owned by the test body (not a helper return).
    fn vtx_bytes(s: i16, t: i16) -> Vec<u8> {
        let mut b = vec![0u8; 16];
        b[8..10].copy_from_slice(&s.to_be_bytes());
        b[10..12].copy_from_slice(&t.to_be_bytes());
        b
    }

    #[test]
    fn texcoord_scale_is_tile_independent() {
        // The vertex texcoord scale is sc/(65536*32) in TEXEL space with NO tile-size division (tile
        // normalization is deferred to the fragment shader at draw time). So a gsDPSetTileSize
        // between vertex loads must NOT fork a new texcoord entry — the scale is identical regardless
        // of tile dims. (Fixes sm64's power-meter frame, which sizes its 32×64 tile AFTER loading the
        // frame verts.)
        let mut rsp = Rsp::default();
        let mut rdp = Rdp::default();
        rsp.set_texture(0, 0, true, 0x8000, 0x8000);
        rdp.tiles[0].width = 32;
        rdp.tiles[0].height = 32;
        let bytes = vtx_bytes(0, 0);
        let rdram = RdramImage::new(&bytes);
        let mut scene = Scene::default();

        rsp.set_vertex(&rdram, 0, 1, 0, &rdp, &mut scene);
        rsp.set_vertex(&rdram, 0, 1, 1, &rdp, &mut scene);
        assert_eq!(scene.texcoord_index, vec![1, 1]); // entry 0 is default [0,0]
        assert_eq!(scene.raw_st, vec![[0.0, 0.0], [0.0, 0.0]]);

        rdp.tiles[0].width = 64; // tile-size change no longer affects the scale -> SAME entry
        rsp.set_vertex(&rdram, 0, 1, 2, &rdp, &mut scene);
        assert_eq!(scene.texcoord_index, vec![1, 1, 1]);

        rsp.finish(&mut scene);
        // 0x8000 / (65536*32) = 2^15 / 2^21 = 2^-6 — tile-independent (both loads, both tile sizes).
        assert_eq!(scene.texcoord_table.len(), 2); // default [0,0] + the single tile-independent entry
        assert_eq!(scene.texcoord_table[1], [2.0f32.powi(-6), 2.0f32.powi(-6)]);
        // Parallel texgen fold: tg = sc/65536 = 0x8000/65536 = 0.5.
        assert_eq!(scene.texgen_scale_table[1], [0.5, 0.5]);
    }
}

#[cfg(test)]
mod lookat_tests {
    use super::*;
    use crate::hle::mem::RdramImage;

    #[test]
    fn set_lookat_reads_s8_axes() {
        let mut rsp = Rsp::default();
        // Two Light_t-shaped 16B entries; s8 axis @ +8/9/10.
        let mut b = vec![0u8; 32];
        b[8] = 127;
        b[9] = 0;
        b[10] = 0; // S = +X
        b[16 + 8] = 0;
        b[16 + 9] = 127;
        b[16 + 10] = 0; // T = +Y
        let rd = RdramImage::new(&b);
        rsp.set_lookat(&rd, 0, 0);
        rsp.set_lookat(&rd, 1, 16);
        assert_eq!(rsp.lookat_axes[0], [1.0, 0.0, 0.0]);
        assert_eq!(rsp.lookat_axes[1], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn texgen_prefold_axis_is_world_fixed_and_mode_set() {
        // 90deg-about-Z modelview loaded via the public matrix path.
        let rot90z: crate::hle::math::Mat4 = [
            [0.0, 1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let mut bytes = n64_gbi::encode::mtx_to_bytes(rot90z).to_vec();
        bytes.extend(vec![0u8; 16]);
        let rd = RdramImage::new(&bytes);
        let mut rsp = Rsp::default();
        rsp.matrix(&rd, 0, crate::hle::consts::G_MTX_LOAD);
        rsp.lookat_axes[0] = [1.0, 0.0, 0.0]; // S = eye +X
        rsp.lookat_version += 1;
        rsp.modify_geometry_mode(
            !0,
            crate::hle::consts::G_LIGHTING | crate::hle::consts::G_TEXTURE_GEN,
        );
        let mut scene = Scene::default();
        rsp.set_vertex(&rd, 64, 1, 0, &Default::default(), &mut scene);
        rsp.finish(&mut scene);
        assert_eq!(scene.texgen_mode, vec![1]); // spherical
                                                // World-fixed invariant: forward-transform of axis_obj recovers the eye-space axis [1,0,0].
        let a = scene.lookat_table[scene.lookat_index[0] as usize].0;
        let back = crate::hle::math::mul_row_vec4([a[0], a[1], a[2], 0.0], rot90z);
        assert!(
            (back[0] - 1.0).abs() < 1e-5 && back[1].abs() < 1e-5 && back[2].abs() < 1e-5,
            "lookat must be world-fixed, got back=[{},{},{}]",
            back[0],
            back[1],
            back[2]
        );
    }

    #[test]
    fn texgen_off_without_lighting() {
        let mut rsp = Rsp::default();
        rsp.modify_geometry_mode(!0, crate::hle::consts::G_TEXTURE_GEN); // G_LIGHTING NOT set
        let bytes = vec![0u8; 16];
        let rd = RdramImage::new(&bytes);
        let mut scene = Scene::default();
        rsp.set_vertex(&rd, 0, 1, 0, &Default::default(), &mut scene);
        assert_eq!(scene.texgen_mode, vec![0]); // gated on G_LIGHTING
    }

    #[test]
    fn texgen_linear_mode_two() {
        let mut rsp = Rsp::default();
        let bytes = vec![0u8; 64];
        let rd = RdramImage::new(&bytes);
        rsp.matrix(&rd, 0, crate::hle::consts::G_MTX_LOAD);
        rsp.lookat_axes[0] = [1.0, 0.0, 0.0];
        rsp.lookat_version += 1;
        rsp.modify_geometry_mode(
            !0,
            crate::hle::consts::G_LIGHTING
                | crate::hle::consts::G_TEXTURE_GEN
                | crate::hle::consts::G_TEXTURE_GEN_LINEAR,
        );
        let mut scene = Scene::default();
        rsp.set_vertex(&rd, 0, 1, 0, &Default::default(), &mut scene);
        assert_eq!(scene.texgen_mode, vec![2]);
    }
}
