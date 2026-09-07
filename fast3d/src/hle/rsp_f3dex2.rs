use crate::diag::{DiagKind, Diagnostic};
use crate::hle::consts::rdp::G_SETTIMG;
use crate::hle::consts::rsp_f3dex2::{
    G_DMA_IO, G_GEOMETRYMODE, G_LINE3D, G_MODIFYVTX, G_MOVEMEM, G_MOVEWORD, G_MTX, G_POPMTX,
    G_QUAD, G_SETOTHERMODE_H, G_SETOTHERMODE_L, G_SPECIAL_1, G_SPECIAL_2, G_SPECIAL_3, G_SPNOOP,
    G_TEXTURE, G_TRI1, G_TRI2, G_VTX,
};
use crate::hle::interp::memory_try;
use crate::hle::interp::{Cmd, Ctx, Handler};
use crate::hle::mem::Rdram;
use crate::hle::rsp::RSP_MAX_VERTICES;

pub(crate) fn install_overrides<M: Rdram>(t: &mut [Handler<M>; 256]) {
    t[G_VTX as usize] = vtx::<M>;
    t[G_MODIFYVTX as usize] = modify_vertex::<M>;
    t[G_QUAD as usize] = tri2::<M>;
    t[G_TRI1 as usize] = tri1::<M>;
    t[G_TRI2 as usize] = tri2::<M>;
    t[G_GEOMETRYMODE as usize] = geometry_mode::<M>;
    t[G_MTX as usize] = matrix::<M>;
    t[G_MOVEMEM as usize] = move_mem::<M>;
    t[G_TEXTURE as usize] = texture::<M>;
    t[G_SETOTHERMODE_H as usize] = set_other_mode_h::<M>;
    t[G_SETOTHERMODE_L as usize] = set_other_mode_l::<M>;
    t[G_SETTIMG as usize] = set_texture_image::<M>;
    t[G_MOVEWORD as usize] = move_word::<M>;
    t[G_POPMTX as usize] = pop_matrix::<M>;
    t[G_SPNOOP as usize] = spnoop::<M>;
    for opcode in [G_LINE3D, G_DMA_IO, G_SPECIAL_1, G_SPECIAL_2, G_SPECIAL_3] {
        t[opcode as usize] = unsupported::<M>;
    }
}

fn spnoop<M: Rdram>(_: &Cmd, _: &mut Ctx<M>) {}

fn unsupported<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let opcode = c.opcode();
    if opcode == G_LINE3D {
        *cx.dropped_runs += 1;
    }
    if !cx.diags.iter().any(
        |d| matches!(d.kind, DiagKind::UnsupportedCommand { opcode: seen, .. } if seen == opcode),
    ) {
        cx.diags.push(Diagnostic {
            at: cx.pc,
            kind: DiagKind::UnsupportedCommand {
                opcode,
                w0: c.w0,
                w1: c.w1_addr,
            },
        });
    }
}

fn vtx<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let count = c.p0(12, 8);
    let end = c.p0(1, 7);
    match end.checked_sub(count) {
        Some(dst) if (dst + count) as usize <= RSP_MAX_VERTICES => {
            let addr = memory_try!(cx, Vertex, cx.mem.resolve_masked(c.w1_addr));
            memory_try!(
                cx,
                Vertex,
                cx.rsp
                    .set_vertex(cx.mem, addr, count, dst, cx.rdp, cx.scene)
            );
        }
        _ => cx.diags.push(Diagnostic {
            at: cx.pc,
            kind: DiagKind::VtxOutOfRange { count, end },
        }),
    }
}

fn modify_vertex<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    if let Err(kind) = cx
        .rsp
        .modify_vertex(c.p0(1, 15), c.p0(16, 8), c.w1, cx.scene)
    {
        cx.diags.push(Diagnostic { at: cx.pc, kind });
    }
}

fn tri1<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    if let Some((mi, ri)) = crate::hle::rsp::snapshot_run(cx.rsp, cx.rdp, cx.diags, cx.scene, cx.pc)
    {
        crate::hle::rsp::record_tri(
            cx.rsp,
            cx.rdp,
            cx.scene,
            cx.rec,
            c.p0(17, 7),
            c.p0(9, 7),
            c.p0(1, 7),
            mi,
            ri,
        );
    } else {
        *cx.dropped_runs += 1;
    }
}

fn tri2<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    if let Some((mi, ri)) = crate::hle::rsp::snapshot_run(cx.rsp, cx.rdp, cx.diags, cx.scene, cx.pc)
    {
        crate::hle::rsp::record_tri(
            cx.rsp,
            cx.rdp,
            cx.scene,
            cx.rec,
            c.p0(17, 7),
            c.p0(9, 7),
            c.p0(1, 7),
            mi,
            ri,
        );
        crate::hle::rsp::record_tri(
            cx.rsp,
            cx.rdp,
            cx.scene,
            cx.rec,
            c.p1(17, 7),
            c.p1(9, 7),
            c.p1(1, 7),
            mi,
            ri,
        );
    } else {
        *cx.dropped_runs += 1;
    }
}

fn pop_matrix<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    cx.rsp.pop_matrix(c.w1 >> 6);
}

fn geometry_mode<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    cx.rsp.modify_geometry_mode(c.p0(0, 24), c.w1);
}

fn matrix<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let addr = memory_try!(cx, Matrix, cx.mem.resolve_masked(c.w1_addr));
    memory_try!(
        cx,
        Matrix,
        cx.rsp.matrix(
            cx.mem,
            addr,
            (c.p0(0, 8) ^ cx.gbi_consts.mtx_param_xor as u32) as u8,
        )
    );
}

fn move_mem<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let idx = c.p0(0, 8);
    if idx == cx.gbi_consts.g_mv_viewport as u32 {
        let addr = memory_try!(cx, Viewport, cx.mem.resolve_masked(c.w1_addr));
        memory_try!(cx, Viewport, cx.rsp.set_viewport(cx.mem, addr));
    } else if idx == cx.gbi_consts.g_mv_light as u32 {
        let byte_off = c.p0(8, 8) * 8;
        let light_idx = byte_off / 24;
        if light_idx >= 2 {
            let addr = memory_try!(cx, Light, cx.mem.resolve_masked(c.w1_addr));
            memory_try!(cx, Light, cx.rsp.set_light(cx.mem, light_idx - 2, addr));
        } else {
            let addr = memory_try!(cx, LookAt, cx.mem.resolve_masked(c.w1_addr));
            memory_try!(cx, LookAt, cx.rsp.set_lookat(cx.mem, light_idx, addr));
        }
    } else {
        cx.diags.push(Diagnostic {
            at: cx.pc,
            kind: DiagKind::UnhandledMovemem(idx as u8),
        });
    }
}

fn texture<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let on = (c.p0(1, 1)) != 0;
    let tile = c.p0(8, 3) as u8;
    let level = c.p0(11, 3) as u8;
    let sc = (c.w1 >> 16) as u16;
    let tc = (c.w1 & 0xFFFF) as u16;
    cx.rsp.set_texture(tile, level, on, sc, tc);
    cx.rsp.material_dirty = true;
}

/// gsDPSetOtherMode_H: update the RDP othermode.H field.
/// p0[8,8] = shift field (pos of the high bit); p0[0,8] = length-1.
fn set_other_mode_h<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let shift_field = c.p0(8, 8);
    let len_field = c.p0(0, 8);
    cx.rsp
        .set_other_mode_h(shift_field, len_field, c.w1, cx.rdp);
    cx.rsp.material_dirty = true;
}

fn set_other_mode_l<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let shift_field = c.p0(8, 8);
    let len_field = c.p0(0, 8);
    cx.rsp
        .set_other_mode_l(shift_field, len_field, c.w1, cx.rdp);
}

fn set_texture_image<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let fmt = c.p0(21, 3) as u8;
    let siz = c.p0(19, 2) as u8;
    // The width field is (actual_width - 1), but tex_image stores the field value directly.
    let width = (c.w0 & 0xFFF) as u16;
    let addr = memory_try!(cx, Texture, cx.mem.resolve(c.w1_addr));
    cx.rsp.set_texture_image(fmt, siz, width, addr, cx.rdp);
}

fn move_word<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let ty = c.p0(16, 8);
    if ty == cx.gbi_consts.g_mw_segment as u32 {
        // seg = p0(2,4), value = w1 (stored RAW).
        Rdram::set_segment(cx.mem, c.p0(2, 4), c.w1_addr);
    } else if ty == cx.gbi_consts.g_mw_perspnorm as u32 {
        // perspNorm is the RSP's fixed-point W-normalization coefficient. Our f32 transform is
        // exact, so it does not change geometry; honor the command as a no-op (emitted for ROM
        // fidelity + the future Fog milestone). (spec §2)
    } else if ty == cx.gbi_consts.g_mw_clip as u32 {
        // clip ratio — RSP viewport state, no scene effect
    } else if ty == cx.gbi_consts.g_mw_numlight as u32 {
        cx.rsp.set_num_lights(c.w1);
    } else if ty == cx.gbi_consts.g_mw_fog as u32 {
        // G_MW_FOG data word: high i16 = fog multiplier (fm), low i16 = fog offset (fo).
        // The ucode already converted min/max to fm/fo; HLE reads them directly (no re-derive).
        cx.rdp.fog_mul = (c.w1 >> 16) as i16;
        cx.rdp.fog_offset = (c.w1 & 0xFFFF) as i16;
    } else {
        cx.diags.push(Diagnostic {
            at: cx.pc,
            kind: DiagKind::UnhandledMoveword(ty as u8),
        });
    }
}

#[cfg(test)]
mod wired_tests {
    use super::*;
    use crate::hle::gbi::{GbiConstants, GbiUcode};
    use crate::hle::interp::{Cmd, Ctx};
    use crate::hle::mem::RdramImage;
    use crate::hle::rdp::Rdp;
    use crate::hle::rsp::{Rsp, Scene};

    fn dispatch_movework_fog(consts: GbiConstants) -> i16 {
        // MOVEWORD: opcode in w0[24:32], type byte at w0[16:24]=G_MW_FOG(0x08); w1 = fog (mul<<16 | off).
        let w0 = (crate::hle::consts::G_MOVEWORD as u32) << 24
            | (crate::hle::consts::G_MW_FOG as u32) << 16;
        let w1 = 0x0002_0003; // fog_mul = 2
        let cmd = Cmd {
            w0,
            w1,
            w1_addr: w1 as u64,
        };
        let mut rsp = Rsp::new(
            GbiUcode::F3dex2.constants(),
            crate::hle::mem::GbiDataFormat::Fixed,
        );
        let mut rdp = Rdp::default();
        let mut scene = Scene::default();
        let mut diags = Vec::new();
        let mut mem = RdramImage::new(&[]);
        let mut rec = crate::hle::rsp::PairRec::default();
        let mut dropped = 0u32;
        let mut seen = [false; 256];
        let mut cx = Ctx {
            rsp: &mut rsp,
            rdp: &mut rdp,
            mem: &mut mem,
            scene: &mut scene,
            diags: &mut diags,
            pc: 0,
            gbi_consts: consts,
            rec: &mut rec,
            dropped_runs: &mut dropped,
            unknown_seen: &mut seen,
        };
        move_word(&cmd, &mut cx);
        rdp.fog_mul
    }

    #[test]
    fn move_word_reads_g_mw_fog_from_consts() {
        // Real g_mw_fog matches the command's type byte -> fog_mul updates.
        assert_eq!(dispatch_movework_fog(GbiUcode::F3dex2.constants()), 2);
        // Wrong g_mw_fog -> handler's compare misses -> fog_mul stays default (0).
        let wrong = GbiConstants {
            g_mw_fog: 0x7F,
            ..GbiUcode::F3dex2.constants()
        };
        assert_eq!(dispatch_movework_fog(wrong), 0);
    }
}

#[cfg(test)]
mod fog_tests {
    use crate::hle::interp::interpret_rdram;
    use n64_gbi::encode::*;

    #[test]
    fn fog_position_and_color_reach_rdp() {
        // fm/fo for min=900, max=1000: span=100, fm=128000/100=1280, fo=((500-900)*256)/100 = -1024.
        let (w0f, w1f) = gsp_fog_position(900, 1000);
        let (w0c, w1c) = gdp_set_fog_color(0x8090A0FF);
        let mut bytes = Vec::new();
        for (w0, w1) in [(w0f, w1f), (w0c, w1c), gsp_enddl()] {
            bytes.extend_from_slice(&w0.to_be_bytes());
            bytes.extend_from_slice(&w1.to_be_bytes());
        }
        let r = interpret_rdram(&bytes, 0);
        assert_eq!(r.rdp.fog_mul, 1280, "fog_mul mismatch");
        assert_eq!(r.rdp.fog_offset, -1024, "fog_offset mismatch");
        assert_eq!(
            r.rdp.fog_color,
            [0x80, 0x90, 0xA0, 0xFF],
            "fog_color mismatch"
        );
    }
}
