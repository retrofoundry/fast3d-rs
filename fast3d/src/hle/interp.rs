//! F3DEX2 opcode dispatch loop. Reads big-endian w0/w1 command pairs from the image's
//! command stream and drives the RSP, producing a Scene + decode diagnostics.

use crate::diag::{DiagKind, Diagnostic};
use crate::hle::mem::{Rdram, RdramImage};
use crate::hle::rsp::Scene;

#[derive(Clone, Copy)]
pub(crate) struct Cmd {
    pub(crate) w0: u32,
    pub(crate) w1: u32,
    /// Full 64-bit address operand. For `RdramImage` equals `w1 as u64`;
    /// for `HostRam` carries the full host pointer (high bits don't fit in `w1`).
    pub(crate) w1_addr: u64,
}

impl Cmd {
    #[inline]
    pub(crate) fn p0(&self, pos: u8, bits: u8) -> u32 {
        (self.w0 >> pos) & ((1u32 << bits) - 1)
    }
    #[inline]
    pub(crate) fn p1(&self, pos: u8, bits: u8) -> u32 {
        (self.w1 >> pos) & ((1u32 << bits) - 1)
    }
    #[inline]
    pub(crate) fn opcode(&self) -> u8 {
        (self.w0 >> 24) as u8
    }
}

pub(crate) struct Ctx<'a, M: Rdram> {
    pub rsp: &'a mut crate::hle::rsp::Rsp,
    pub rdp: &'a mut crate::hle::rdp::Rdp,
    pub mem: &'a mut M,
    pub scene: &'a mut crate::hle::rsp::Scene,
    pub diags: &'a mut Vec<crate::diag::Diagnostic>,
    /// Byte address of the command currently being dispatched.
    pub pc: u64,
    pub gbi_consts: crate::hle::gbi::GbiConstants,
    /// 2D framebuffer-pair recorder walk-state (Task 3). Threaded so `draw_tri` (via `record_tri`)
    /// can route triangles into the current pair's ordered op-stream.
    pub rec: &'a mut crate::hle::rsp::PairRec,
    /// Runs discarded during the walk (unwired/no-texture/before-CIMG/truncated). Rolled into
    /// `DlSummary.dropped_runs` by `process_dl`.
    pub dropped_runs: &'a mut u32,
    /// Anti-flood set for `UnknownOpcode`: emit each distinct unknown opcode ONCE (spec §3.6).
    pub unknown_seen: &'a mut [bool; 256],
}

/// Sign-extend the low 24 bits of `w` (RDP float-GBI rect coords are s23 in the command word).
#[inline]
fn sext24(w: u32) -> i32 {
    ((w << 8) as i32) >> 8
}

pub(crate) type Handler<M> = fn(&Cmd, &mut Ctx<M>);

pub(crate) fn unknown<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let op = c.opcode();
    if cx.unknown_seen[op as usize] {
        return; // dedup (anti-flood, spec §3.6)
    }
    cx.unknown_seen[op as usize] = true;
    cx.diags.push(Diagnostic {
        at: cx.pc,
        kind: DiagKind::UnknownOpcode(op),
    });
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InterpResult {
    pub scene: Scene,
    pub diags: Vec<crate::diag::Diagnostic>,
    pub geometry_mode: u32,
    /// Final RDP state after the walk — exposes TLUT, tiles, combine, etc. for testing.
    pub rdp: crate::hle::rdp::Rdp,
    /// Total command dispatches (the `dispatched` counter).
    pub commands: u32,
    /// Draw runs discarded during the walk.
    pub dropped_runs: u32,
}

impl InterpResult {
    pub(crate) fn summary(&self, renderable: bool) -> crate::diag::DlSummary {
        let (mut warns, mut errors) = (0, 0);
        for d in &self.diags {
            match d.kind.severity() {
                crate::diag::Severity::Warn => warns += 1,
                crate::diag::Severity::Error => errors += 1,
            }
        }
        crate::diag::DlSummary {
            commands: self.commands,
            tris: (self.scene.indices.len() / 3) as u32,
            warns,
            errors,
            dropped_runs: self.dropped_runs,
            renderable,
        }
    }
}

/// Max command dispatches before the runaway guard fires. Needed because we run
/// user-authored DLs, not trusted hardware DLs.
/// 1 << 20 cannot be reached by any valid finite DL in RDRAM yet terminates a
/// self-branch loop quickly.
const DISPATCH_CAP: u64 = 1 << 20;

enum Control {
    Continue,
    Call(u64),
    Branch(u64),
    Return,
    Abort,
}

pub fn interpret<M: Rdram>(
    mem: M,
    entry: u64,
    ucode: crate::hle::gbi::GbiUcode,
    data_format: crate::hle::mem::GbiDataFormat,
) -> InterpResult {
    let mut mem = mem;
    let gbi = crate::hle::gbi::Gbi::<M>::new(ucode, data_format);
    let mut rsp = crate::hle::rsp::Rsp::new(gbi.consts, gbi.data_format);
    let mut rdp = crate::hle::rdp::Rdp::default();
    let mut scene = Scene::default();
    let mut diags = Vec::new();
    let mut dropped_runs: u32 = 0;
    let mut unknown_seen = [false; 256];
    let mut rdphalf_1 = None;
    let mut rejected = false;

    let mut pc: u64 = entry;
    let mut return_stack: Vec<u64> = Vec::new();
    let mut dispatched: u64 = 0;
    let mut rec = crate::hle::rsp::PairRec::default();

    loop {
        // Runaway guard: dispatch cap + per-read bounds check.
        if dispatched >= DISPATCH_CAP {
            diags.push(Diagnostic {
                at: pc,
                kind: DiagKind::RunawayDl { cap: DISPATCH_CAP },
            });
            break;
        }
        let stride = mem.command_stride();
        if !mem.in_bounds(pc, stride) {
            diags.push(Diagnostic {
                at: pc,
                kind: DiagKind::DlPastRdram,
            });
            break;
        }
        dispatched += 1;

        let cmd = mem.read_command(pc);
        let c = Cmd {
            w0: cmd.w0,
            w1: cmd.w1,
            w1_addr: cmd.w1_addr,
        };
        let op = c.opcode();

        let control = if op == gbi.consts.g_dl {
            let target = mem.resolve_masked(c.w1_addr);
            if c.p0(16, 1) == 0 {
                Control::Call(target)
            } else {
                Control::Branch(target)
            }
        } else if op == gbi.consts.g_enddl {
            Control::Return
        } else if ucode == crate::hle::gbi::GbiUcode::F3dex2
            && matches!(
                op,
                crate::hle::consts::G_CULLDL | crate::hle::consts::G_BRANCH_Z
            )
        {
            let conditional = if op == crate::hle::consts::G_CULLDL {
                rsp.cull_display_list(c.p0(1, 15), c.p1(1, 15))
                    .map(|culled| {
                        if culled {
                            Control::Return
                        } else {
                            Control::Continue
                        }
                    })
            } else {
                rdphalf_1
                    .ok_or(DiagKind::MissingBranchTarget)
                    .and_then(|target| {
                        rsp.branch_z(c.p0(1, 11), c.w1).map(|taken| {
                            if taken {
                                Control::Branch(mem.resolve_masked(target))
                            } else {
                                Control::Continue
                            }
                        })
                    })
            };
            match conditional {
                Ok(control) => control,
                Err(kind) => {
                    diags.push(Diagnostic { at: pc, kind });
                    Control::Abort
                }
            }
        } else if ucode == crate::hle::gbi::GbiUcode::F3dex2
            && op == crate::hle::consts::G_LOAD_UCODE
        {
            diags.push(Diagnostic {
                at: pc,
                kind: DiagKind::UnsupportedMicrocodeLoad {
                    w0: c.w0,
                    w1: c.w1_addr,
                    data_address: rdphalf_1,
                },
            });
            Control::Abort
        } else {
            Control::Continue
        };
        match control {
            Control::Call(target) | Control::Branch(target) => {
                if matches!(control, Control::Call(_)) {
                    return_stack.push(pc + stride);
                }
                pc = target;
                continue;
            }
            Control::Return => match return_stack.pop() {
                Some(ret) => {
                    pc = ret;
                    continue;
                }
                None => break,
            },
            Control::Abort => {
                rejected = true;
                break;
            }
            Control::Continue => {}
        }
        if ucode == crate::hle::gbi::GbiUcode::F3dex2 && op == crate::hle::consts::G_RDPHALF_1 {
            rdphalf_1 = Some(c.w1_addr);
            pc += stride;
            continue;
        }
        if ucode == crate::hle::gbi::GbiUcode::F3dex2
            && matches!(
                op,
                crate::hle::consts::G_CULLDL | crate::hle::consts::G_BRANCH_Z
            )
        {
            pc += stride;
            continue;
        }

        // First G_SETCIMG → the scene becomes "paired" (subsequent draws record into ordered
        // FramebufferPairs). The CIMG itself still dispatches below to update RDP state.
        if op == crate::hle::consts::G_SETCIMG {
            rec.have_seen_cimg = true;
        }

        // --- 2D inline rect decode + pair recording (Task 3). Multi-word commands a `Handler`
        // cannot express (it cannot advance `pc`); decoded here from RAW locals, gated on the
        // opcode. Each continuation read is bounds-checked (mirrors the loop-top guard). ---
        if op == crate::hle::consts::G_TEXRECT || op == crate::hle::consts::G_TEXRECTFLIP {
            // cmd0 == c (already bounds-checked at loop top). Read the two continuation words.
            if !mem.in_bounds(pc + stride, stride) {
                dropped_runs += 1;
                diags.push(Diagnostic {
                    at: pc,
                    kind: DiagKind::TruncatedRect { fill: false },
                });
                break;
            }
            let cmd1 = mem.read_command(pc + stride);
            if !mem.in_bounds(pc + 2 * stride, stride) {
                dropped_runs += 1;
                diags.push(Diagnostic {
                    at: pc,
                    kind: DiagKind::TruncatedRect { fill: false },
                });
                break;
            }
            let cmd2 = mem.read_command(pc + 2 * stride);

            let (lrx, lry, tile, ulx, uly) = match gbi.data_format {
                crate::hle::mem::GbiDataFormat::Fixed => (
                    c.p0(12, 12) as i32,
                    c.p0(0, 12) as i32,
                    c.p1(24, 3) as u8,
                    c.p1(12, 12) as i32,
                    c.p1(0, 12) as i32,
                ),
                crate::hle::mem::GbiDataFormat::Float => (
                    sext24(c.w0),
                    sext24(c.w1),
                    c.p1(24, 3) as u8,
                    sext24(cmd1.w0),
                    sext24(cmd2.w0),
                ),
            };
            // uls/ult/dsdx/dtdy occupy identical positions in both formats.
            let uls = (cmd1.w1 >> 16) as i16;
            let ult = cmd1.w1 as i16;
            let dsdx = (cmd2.w1 >> 16) as i16;
            let dtdy = cmd2.w1 as i16;
            let flip = op == crate::hle::consts::G_TEXRECTFLIP;
            let copy_mode = ((rdp.other_mode_h >> 20) & 3) == crate::hle::consts::G_CYC_COPY;
            let rect = crate::hle::TexRectBounds { ulx, uly, lrx, lry };

            if !rec.have_seen_cimg {
                // A 2D op needs a framebuffer target; one before the first CIMG is malformed → drop.
                dropped_runs += 1;
                diags.push(Diagnostic {
                    at: pc,
                    kind: DiagKind::DrawBeforeCimg,
                });
                pc += 3 * stride;
                continue;
            }

            let Some((material_index, render_mode_index)) =
                crate::hle::rsp::snapshot_rect_run(&rsp, &rdp, tile, &mut diags, &mut scene, pc)
            else {
                dropped_runs += 1;
                pc += 3 * stride;
                continue;
            };
            crate::hle::rsp::ensure_pair_open(&mut scene, &mut rdp, &mut rec);
            crate::hle::rsp::record_scissor_if_changed(&mut scene, &rdp, &mut rec);

            // fb_source: the latest PRIOR pair whose framebuffer byte-range contains the texture
            // image address (a framebuffer-as-texture read-back). The current pair is excluded
            // (it is not yet recorded as a finished framebuffer).
            let tex_addr = rdp.tex_image.3;
            let cur = rec.cur_pair;
            let fb_source = scene.framebuffer_pairs[..cur]
                .iter()
                .rev()
                .filter(|p| !p.is_depth_clear)
                .find(|p| {
                    let start = p.color_image.addr;
                    let end = start
                        + (p.color_image.width as u64)
                            * (p.size_extent.1 as u64)
                            * crate::hle::rsp::bpp(p.color_image.siz);
                    (start..end).contains(&tex_addr)
                })
                .map(|p| p.color_image.addr);

            scene.framebuffer_pairs[cur]
                .ops
                .push(crate::hle::rsp::SceneOp::TexRect {
                    rect,
                    tile,
                    uls,
                    ult,
                    dsdx,
                    dtdy,
                    flip,
                    copy_mode,
                    material_index,
                    render_mode_index,
                    fog_color: rdp.fog_color,
                    prim_depth: rdp.prim_depth,
                    fb_source,
                });
            pc += 3 * stride;
            continue;
        }
        if op == crate::hle::consts::G_FILLRECT {
            // Fixed: 1 word (lrx/lry in w0, ulx/uly in w1). Float: 2 words (F6 + E1), all coords
            // sign-extended 24-bit across cmd0.w0/w1 and cmd1.w0/w1.
            let (rect, words) = match gbi.data_format {
                crate::hle::mem::GbiDataFormat::Fixed => (
                    crate::hle::rsp::Rect {
                        lrx: (c.p0(12, 12) as i32) >> 2,
                        lry: (c.p0(0, 12) as i32) >> 2,
                        ulx: (c.p1(12, 12) as i32) >> 2,
                        uly: (c.p1(0, 12) as i32) >> 2,
                    },
                    1u64,
                ),
                crate::hle::mem::GbiDataFormat::Float => {
                    if !mem.in_bounds(pc + stride, stride) {
                        dropped_runs += 1;
                        diags.push(Diagnostic {
                            at: pc,
                            kind: DiagKind::TruncatedRect { fill: true },
                        });
                        break;
                    }
                    let cmd1 = mem.read_command(pc + stride);
                    (
                        crate::hle::rsp::Rect {
                            lrx: sext24(c.w0) >> 2,
                            lry: sext24(c.w1) >> 2,
                            ulx: sext24(cmd1.w0) >> 2,
                            uly: sext24(cmd1.w1) >> 2,
                        },
                        2u64,
                    )
                }
            };

            if !rec.have_seen_cimg {
                dropped_runs += 1;
                diags.push(Diagnostic {
                    at: pc,
                    kind: DiagKind::DrawBeforeCimg,
                });
                pc += words * stride;
                continue;
            }

            crate::hle::rsp::ensure_pair_open(&mut scene, &mut rdp, &mut rec);
            crate::hle::rsp::record_scissor_if_changed(&mut scene, &rdp, &mut rec);
            let color_raw = rdp.fill_color_raw;
            scene.framebuffer_pairs[rec.cur_pair]
                .ops
                .push(crate::hle::rsp::SceneOp::FillRect { rect, color_raw });
            pc += words * stride;
            continue;
        }

        let mut cx = Ctx {
            rsp: &mut rsp,
            rdp: &mut rdp,
            mem: &mut mem,
            scene: &mut scene,
            diags: &mut diags,
            pc,
            gbi_consts: gbi.consts,
            rec: &mut rec,
            dropped_runs: &mut dropped_runs,
            unknown_seen: &mut unknown_seen,
        };
        gbi.table[op as usize](&c, &mut cx);
        if diags.last().is_some_and(|d| {
            matches!(
                d.kind,
                DiagKind::DlPastRdram
                    | DiagKind::UnhandledMovemem(_)
                    | DiagKind::UnhandledMoveword(_)
                    | DiagKind::UnsupportedCommand {
                        opcode: 0xd3..=0xd5,
                        ..
                    }
            )
        }) {
            rejected = true;
            break;
        }
        pc += stride;
    }

    if rejected {
        dropped_runs += scene.draw_runs.len() as u32
            + scene
                .framebuffer_pairs
                .iter()
                .flat_map(|pair| &pair.ops)
                .filter(|op| {
                    matches!(
                        op,
                        crate::hle::rsp::SceneOp::Tris(_)
                            | crate::hle::rsp::SceneOp::TexRect { .. }
                            | crate::hle::rsp::SceneOp::FillRect { .. }
                    )
                })
                .count() as u32;
        return InterpResult {
            scene: Scene::default(),
            diags,
            geometry_mode: rsp.geometry_mode(),
            rdp,
            commands: dispatched as u32,
            dropped_runs,
        };
    }

    let geometry_mode = rsp.geometry_mode();
    // Per-run materials/render_modes/draw_runs are populated DURING the walk (snapshot_run).
    // A9: emit a loud diagnostic if geometry was drawn without a render mode being set. Covers both
    // the flat 3D path (draw_runs) and the paired 2D path (a pair with a recorded Tris op).
    let paired_has_tris = scene.framebuffer_pairs.iter().any(|p| {
        p.ops
            .iter()
            .any(|op| matches!(op, crate::hle::rsp::SceneOp::Tris(_)))
    });
    if (!scene.draw_runs.is_empty() || paired_has_tris) && rdp.other_mode_l == 0 {
        diags.push(Diagnostic {
            at: pc,
            kind: DiagKind::RenderModeNeverSet,
        });
    }
    rsp.finish(&mut scene);
    // The final color image — the pair-less renderer's internal-framebuffer key (spec §4).
    scene.color_image = rdp.color_image;
    InterpResult {
        scene,
        diags,
        geometry_mode,
        rdp,
        commands: dispatched as u32,
        dropped_runs,
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn interpret_rdram(bytes: &[u8], entry_addr: u32) -> InterpResult {
    interpret(
        RdramImage::new(bytes),
        entry_addr as u64,
        crate::hle::gbi::GbiUcode::F3dex2,
        crate::hle::mem::GbiDataFormat::Fixed,
    )
}

#[cfg(test)]
mod task_a7_tests {
    use super::*;
    use crate::hle::consts::{G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2, G_ZBUFFER};
    use n64_gbi::encode::*;

    fn push(buf: &mut Vec<u8>, (w0, w1): (u32, u32)) {
        buf.extend_from_slice(&w0.to_be_bytes());
        buf.extend_from_slice(&w1.to_be_bytes());
    }

    #[test]
    fn interpret_populates_per_run_model_and_keeps_zbuffer_shim() {
        let mut rdram: Vec<u8> = Vec::new();
        for v in [640i16, 480, 511, 0, 640, 480, 511, 0] {
            rdram.extend_from_slice(&v.to_be_bytes());
        }
        let vtx = rdram.len() as u32;
        for _ in 0..3 {
            rdram.extend_from_slice(&[0u8; 12]);
            rdram.extend_from_slice(&[255, 255, 255, 255]);
        }
        let entry = rdram.len() as u32;
        let mut cmds: Vec<u8> = Vec::new();
        push(&mut cmds, gsp_viewport(0));
        push(&mut cmds, gsp_set_geometrymode(G_ZBUFFER));
        push(
            &mut cmds,
            gdp_set_render_mode(G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2),
        );
        let cc = CcPass {
            a: ZERO_C,
            b: ZERO_C,
            c: ZERO_C,
            d: 4,
        };
        let ca = CcPass {
            a: ZERO_A,
            b: ZERO_A,
            c: ZERO_A,
            d: 4,
        };
        push(&mut cmds, gdp_set_combine_lerp(cc, ca, cc, ca));
        push(&mut cmds, gsp_vertex(0, 3, vtx));
        push(&mut cmds, gsp_1triangle(0, 1, 2));
        push(&mut cmds, gsp_enddl());
        let mut full = rdram.clone();
        full.extend_from_slice(&cmds);
        let r = interpret_rdram(&full, entry);
        // Per-run model populated during the walk:
        assert_eq!(r.scene.draw_runs.len(), 1);
        assert_eq!(r.scene.render_modes.len(), 1);
        assert!(!r.scene.materials.is_empty());
        // A9: depth is render-mode driven (G_RM_AA_ZB_OPA_SURF has z_test=true).
        assert!(r
            .scene
            .render_modes
            .iter()
            .any(|rm| rm.z_test || rm.z_write));
        // Render mode was set, so no un-migrated diag.
        assert!(r.diags.is_empty(), "{:?}", r.diags);
    }
}

#[cfg(test)]
mod table_tests {
    use super::*;
    use crate::diag::{DiagKind, Diagnostic};
    #[test]
    fn unknown_opcode_is_diagnosed_at_its_byte_address() {
        // Unknown opcode 0xAB at entry_addr 0, then G_ENDDL at byte 8.
        let mut rdram = vec![0u8; 16];
        rdram[0] = 0xAB;
        rdram[8] = crate::hle::consts::G_ENDDL; // 0xDF terminates the walk
        let r = interpret_rdram(&rdram, 0);
        assert_eq!(
            r.diags,
            vec![Diagnostic {
                at: 0,
                kind: DiagKind::UnknownOpcode(0xAB),
            }]
        );
    }
}

#[cfg(test)]
mod task8_tests {
    use super::*;
    use crate::hle::consts::{G_RM_OPA_SURF, G_RM_OPA_SURF2, G_SHADE, G_SHADING_SMOOTH};
    use n64_gbi::encode::*;

    /// One vertex specification: (x, y, z, s, t, r, g, b, a).
    struct VtxSpec {
        x: i16,
        y: i16,
        z: i16,
        s: i16,
        t: i16,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    }

    fn encode_vtx(v: &VtxSpec) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0..2].copy_from_slice(&v.x.to_be_bytes());
        buf[2..4].copy_from_slice(&v.y.to_be_bytes());
        buf[4..6].copy_from_slice(&v.z.to_be_bytes());
        // flag = 0 at [6..8]
        buf[8..10].copy_from_slice(&v.s.to_be_bytes());
        buf[10..12].copy_from_slice(&v.t.to_be_bytes());
        buf[12] = v.r;
        buf[13] = v.g;
        buf[14] = v.b;
        buf[15] = v.a;
        buf
    }

    /// Build a complete textured-quad display list (unified RDRAM with commands appended).
    /// 32x32 RGBA16 texture, 4 corner vertices with S10.5 texcoords, 2 triangles.
    /// SetOtherModeH(1-cycle), SetCombineLERP(MODULATE), SetPrimColor(white),
    /// SetEnvColor(0,0,0,255), LoadTextureBlock, SPTexture, SPVertex, SP1Triangle x2, SPEndDL.
    fn build_textured_quad_dl() -> (Vec<u8>, u32) {
        let mut rdram: Vec<u8> = Vec::new();

        let vp_addr = 0u32;
        let vscale: [i16; 4] = [640, 480, 511, 511];
        let vtrans: [i16; 4] = [320, 240, 0, 511];
        for v in &vscale {
            rdram.extend_from_slice(&v.to_be_bytes());
        }
        for v in &vtrans {
            rdram.extend_from_slice(&v.to_be_bytes());
        }

        let proj_addr = rdram.len() as u32;
        rdram.extend_from_slice(&n64_gbi::encode::mtx_identity_bytes());

        let model_addr = rdram.len() as u32;
        rdram.extend_from_slice(&n64_gbi::encode::mtx_identity_bytes());

        // Vertex data: 4 corners, s/t in S10.5 (0 and 1024 = 32<<5)
        let vtx_addr = rdram.len() as u32;
        let verts = [
            VtxSpec {
                x: -48,
                y: -48,
                z: 0,
                s: 0,
                t: 0,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            VtxSpec {
                x: 48,
                y: -48,
                z: 0,
                s: 1024,
                t: 0,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            VtxSpec {
                x: 48,
                y: 48,
                z: 0,
                s: 1024,
                t: 1024,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            VtxSpec {
                x: -48,
                y: 48,
                z: 0,
                s: 0,
                t: 1024,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        ];
        for v in &verts {
            rdram.extend_from_slice(&encode_vtx(v));
        }

        // Align to 8 bytes before texture data
        while !rdram.len().is_multiple_of(8) {
            rdram.push(0);
        }

        // Texture data: 32x32 RGBA16 all-white (0xFFFF each)
        let tex_addr = rdram.len() as u32;
        for _ in 0..(32 * 32) {
            rdram.extend_from_slice(&[0xFF, 0xFF]);
        }

        while !rdram.len().is_multiple_of(8) {
            rdram.push(0);
        }
        let entry = rdram.len() as u32;

        let mut cmds: Vec<u8> = Vec::new();
        let push = |cmds: &mut Vec<u8>, (w0, w1): (u32, u32)| {
            cmds.extend_from_slice(&w0.to_be_bytes());
            cmds.extend_from_slice(&w1.to_be_bytes());
        };

        push(&mut cmds, gsp_matrix(proj_addr, true, true, false));
        push(&mut cmds, gsp_matrix(model_addr, false, true, false));
        push(&mut cmds, gsp_viewport(vp_addr));
        push(&mut cmds, gsp_set_geometrymode(G_SHADE | G_SHADING_SMOOTH));
        push(&mut cmds, gdp_set_cycle_type(0)); // 1-cycle
        push(
            &mut cmds,
            gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
        );
        let crgb = CcPass {
            a: 1,
            b: ZERO_C,
            c: 4,
            d: ZERO_C,
        };
        let calpha = CcPass {
            a: ZERO_A,
            b: ZERO_A,
            c: ZERO_A,
            d: 4,
        };
        push(&mut cmds, gdp_set_combine_lerp(crgb, calpha, crgb, calpha));
        push(&mut cmds, gdp_set_prim_color(0, 0, 0xFFFF_FFFF));
        push(&mut cmds, gdp_set_env_color(0x0000_00FF));
        for cmd in gdp_load_texture_block(0, 2, 32, 32, tex_addr, 2, 5, 2, 5) {
            push(&mut cmds, cmd);
        }
        push(&mut cmds, gsp_texture(0xFFFF, 0xFFFF, 0, 0, true));
        push(&mut cmds, gsp_vertex(0, 4, vtx_addr));
        push(&mut cmds, gsp_1triangle(0, 1, 2));
        push(&mut cmds, gsp_1triangle(0, 2, 3));
        push(&mut cmds, gsp_enddl());

        rdram.extend_from_slice(&cmds);
        (rdram, entry)
    }

    /// Dispatch a single SetOtherModeH command and return `(other_mode_h >> 20) & 3`.
    fn cycle_type_after(w0: u32, w1: u32) -> u32 {
        let mut rdram2 = crate::hle::mem::RdramImage::new(&[]);
        let mut rdp = crate::hle::rdp::Rdp::default();
        let mut rsp = crate::hle::rsp::Rsp::default();
        let mut scene = crate::hle::rsp::Scene::default();
        let mut diags = Vec::new();
        let mut rec = crate::hle::rsp::PairRec::default();
        let mut dropped = 0u32;
        let mut seen = [false; 256];
        let cmd = Cmd {
            w0,
            w1,
            w1_addr: w1 as u64,
        };
        let table = crate::hle::gbi::Gbi::<crate::hle::mem::RdramImage>::new(
            crate::hle::gbi::GbiUcode::F3dex2,
            crate::hle::mem::GbiDataFormat::Fixed,
        )
        .table;
        let mut cx = Ctx {
            rsp: &mut rsp,
            rdp: &mut rdp,
            mem: &mut rdram2,
            scene: &mut scene,
            diags: &mut diags,
            pc: 0,
            gbi_consts: crate::hle::gbi::GbiUcode::F3dex2.constants(),
            rec: &mut rec,
            dropped_runs: &mut dropped,
            unknown_seen: &mut seen,
        };
        table[cmd.opcode() as usize](&cmd, &mut cx);
        (rdp.other_mode_h >> 20) & 3
    }

    #[test]
    fn textured_quad_produces_material_and_corner_uvs() {
        let (rdram, entry) = build_textured_quad_dl();
        let res = interpret_rdram(&rdram, entry);
        assert!(res.diags.is_empty(), "unexpected diags: {:?}", res.diags);
        let m = &res.scene.materials[0];
        assert_eq!(m.cycle_type, 0); // 1-cycle
        assert_eq!(m.prim, [255, 255, 255, 255]);
        assert_eq!(m.env, [0, 0, 0, 255]);
        assert_eq!(m.tex_w, 32);
        assert_eq!(m.tex_h, 32);
        assert_eq!(m.texture.len(), 32 * 32 * 4); // decoded RGBA8
        assert!(m.tex_enable);
        // FOUR corners: s/t in S10.5 (0 and 1024 = 32<<5), sc=tc=0xFFFF. No V-flip.
        // uv now lives on the GPU OutVertex; rederive from raw s/t + texcoord table for the assert.
        // The texcoord table is TEXEL-space (tile-size normalization is deferred to the fragment
        // shader), so divide by the material's tile dims to recover the normalized UV the sampler
        // sees (renderer applies `inv_tex_size` = 1/(tex_w, tex_h)).
        let uv = |i: usize| {
            let st = res.scene.raw_st[i];
            let s = res.scene.texcoord_table[res.scene.texcoord_index[i] as usize];
            [st[0] * s[0] / m.tex_w as f32, st[1] * s[1] / m.tex_h as f32]
        };
        assert!((uv(0)[0] - 0.0).abs() < 1e-3 && (uv(0)[1] - 0.0).abs() < 1e-3);
        assert!((uv(1)[0] - 1.0).abs() < 2e-3 && (uv(1)[1] - 0.0).abs() < 1e-3);
        assert!((uv(2)[0] - 1.0).abs() < 2e-3 && (uv(2)[1] - 1.0).abs() < 2e-3);
        assert!((uv(3)[0] - 0.0).abs() < 1e-3 && (uv(3)[1] - 1.0).abs() < 2e-3);
    }

    #[test]
    fn other_mode_h_decodes_cycle_type_for_both_words() {
        // 1CYCLE word 0xE3000A01 / 0x00000000 -> cycle_type 0
        // 2CYCLE word 0xE3000A01 / 0x00100000 -> cycle_type 1
        assert_eq!(cycle_type_after(0xE300_0A01, 0x0000_0000), 0);
        assert_eq!(cycle_type_after(0xE300_0A01, 0x0010_0000), 1);
    }
}

#[cfg(test)]
mod slice2_tests {
    use super::*;
    use n64_gbi::encode::*;

    fn push(buf: &mut Vec<u8>, (w0, w1): (u32, u32)) {
        buf.extend_from_slice(&w0.to_be_bytes());
        buf.extend_from_slice(&w1.to_be_bytes());
    }

    #[test]
    fn persp_normalize_is_a_silent_no_op() {
        // A bare gsSPPerspNormalize + EndDL must produce NO diagnostic (the RSP no-ops it).
        let mut rdram = Vec::new();
        push(&mut rdram, gsp_persp_normalize(129));
        push(&mut rdram, gsp_enddl());
        let r = interpret_rdram(&rdram, 0);
        assert!(
            r.diags.is_empty(),
            "perspNorm must be a no-op, got: {:?}",
            r.diags
        );
    }
}

#[cfg(test)]
mod task_a9_tests {
    use super::*;
    use n64_gbi::encode::*;

    fn push(buf: &mut Vec<u8>, (w0, w1): (u32, u32)) {
        buf.extend_from_slice(&w0.to_be_bytes());
        buf.extend_from_slice(&w1.to_be_bytes());
    }

    #[test]
    fn geometry_without_render_mode_emits_loud_diag() {
        let mut rdram: Vec<u8> = Vec::new();
        for v in [640i16, 480, 511, 0, 640, 480, 511, 0] {
            rdram.extend_from_slice(&v.to_be_bytes());
        }
        let vtx = rdram.len() as u32;
        for _ in 0..3 {
            rdram.extend_from_slice(&[0u8; 12]);
            rdram.extend_from_slice(&[255, 255, 255, 255]);
        }
        let entry = rdram.len() as u32;
        let mut cmds: Vec<u8> = Vec::new();
        push(&mut cmds, gsp_viewport(0));
        let cc = CcPass {
            a: ZERO_C,
            b: ZERO_C,
            c: ZERO_C,
            d: 4,
        };
        let ca = CcPass {
            a: ZERO_A,
            b: ZERO_A,
            c: ZERO_A,
            d: 4,
        };
        push(&mut cmds, gdp_set_combine_lerp(cc, ca, cc, ca));
        push(&mut cmds, gsp_vertex(0, 3, vtx));
        push(&mut cmds, gsp_1triangle(0, 1, 2)); // NO gsDPSetRenderMode → other_mode_l == 0
        push(&mut cmds, gsp_enddl());
        let mut full = rdram.clone();
        full.extend_from_slice(&cmds);
        let r = interpret_rdram(&full, entry);
        assert!(
            r.diags
                .iter()
                .any(|d| d.kind == crate::diag::DiagKind::RenderModeNeverSet),
            "{:?}",
            r.diags
        );
    }
}

#[cfg(test)]
mod rect_encoding_tests {
    //! Round-trip tests for the inline TEXRECT/FILLRECT decode + pair recording (Task 3).
    //!
    //! Every DL is CIMG-first and shaped `[G_SETCIMG, rect-op(s), SENTINEL, G_ENDDL]`. The SENTINEL
    //! is a single-word `gsDPSetPrimColor` carrying 0xDEADBEEF: if the rect word-count were wrong
    //! the walk would land mid-stream and misread it, so asserting `rdp.prim` is the desync guard
    //! (interp.rs exposes no `pc`). Well-formed rect DLs consume their continuation words inline, so
    //! the RDPHALF canary diag must be ABSENT. Float DLs use `GbiUcode::F3dex2` + `GbiDataFormat::Float` over `RdramImage`
    //! (a rect-only DL never trips RdramImage's Fixed-only assert).
    use super::*;
    use crate::hle::consts::{
        G_ENDDL, G_FILLRECT, G_RDPHALF_1, G_RDPHALF_2, G_SETCIMG, G_SETFILLCOLOR, G_SETPRIMCOLOR,
        G_SETSCISSOR, G_TEXRECT, G_TEXRECTFLIP,
    };
    use crate::hle::gbi::GbiUcode;
    use crate::hle::rsp::{ColorImage, Rect, SceneOp, Scissor};

    const SENTINEL: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

    fn push(buf: &mut Vec<u8>, (w0, w1): (u32, u32)) {
        buf.extend_from_slice(&w0.to_be_bytes());
        buf.extend_from_slice(&w1.to_be_bytes());
    }
    /// G_SETCIMG: fmt=RGBA(0), siz=16b(2), width=320 (field 319), at `addr`.
    fn cimg(addr: u32) -> (u32, u32) {
        ((G_SETCIMG as u32) << 24 | (2 << 19) | 319, addr)
    }
    fn prim_sentinel() -> (u32, u32) {
        ((G_SETPRIMCOLOR as u32) << 24, u32::from_be_bytes(SENTINEL))
    }
    fn fill_color(rgba: u32) -> (u32, u32) {
        ((G_SETFILLCOLOR as u32) << 24, rgba)
    }
    fn enddl() -> (u32, u32) {
        ((G_ENDDL as u32) << 24, 0)
    }
    fn scissor(ulx: u32, uly: u32, lrx: u32, lry: u32, mode: u32) -> (u32, u32) {
        (
            (G_SETSCISSOR as u32) << 24 | ((ulx << 2) << 12) | (uly << 2),
            (mode << 24) | ((lrx << 2) << 12) | (lry << 2),
        )
    }
    fn run(buf: &[u8], ucode: GbiUcode) -> InterpResult {
        interpret(
            crate::hle::mem::RdramImage::new(buf),
            0,
            ucode,
            crate::hle::mem::GbiDataFormat::Fixed,
        )
    }
    /// `GBI_FLOATS` variant of `run`: F3DEX2 command table read with the float data layout —
    /// the sm64/wafel PC-port path (formerly selected via the removed `F3dex2e` ucode).
    fn run_float(buf: &[u8]) -> InterpResult {
        interpret(
            crate::hle::mem::RdramImage::new(buf),
            0,
            GbiUcode::F3dex2,
            crate::hle::mem::GbiDataFormat::Float,
        )
    }

    #[test]
    fn fixed_texrect_records_op_and_sentinel_decodes() {
        let mut b = Vec::new();
        push(&mut b, cimg(0x10000));
        // cmd0: lrx=320 (raw 1280), lry=240 (raw 960), tile=0, ulx=5 (raw 20), uly=7 (raw 28).
        // Distinct values so any field transposition (ulx↔uly, lrx↔lry) fails the assertion.
        push(
            &mut b,
            (
                (G_TEXRECT as u32) << 24 | (1280 << 12) | 960,
                (20u32 << 12) | 28,
            ),
        );
        push(&mut b, ((G_RDPHALF_1 as u32) << 24, (11u32 << 16) | 13)); // uls=11, ult=13
        push(&mut b, ((G_RDPHALF_2 as u32) << 24, (1024u32 << 16) | 512)); // dsdx=1024, dtdy=512
        push(&mut b, prim_sentinel());
        push(&mut b, enddl());
        let r = run(&b, GbiUcode::F3dex2);
        assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
        assert_eq!(r.scene.framebuffer_pairs.len(), 1);
        let p = &r.scene.framebuffer_pairs[0];
        assert_eq!(
            p.color_image,
            ColorImage {
                fmt: 0,
                siz: 2,
                width: 320,
                addr: 0x10000
            }
        );
        assert_eq!(
            p.ops,
            vec![SceneOp::TexRect {
                rect: crate::hle::TexRectBounds {
                    ulx: 20,
                    uly: 28,
                    lrx: 1280,
                    lry: 960
                },
                tile: 0,
                uls: 11,
                ult: 13,
                dsdx: 1024,
                dtdy: 512,
                flip: false,
                copy_mode: false,
                material_index: 0,
                render_mode_index: 0,
                fog_color: [0; 4],
                prim_depth: Default::default(),
                fb_source: None,
            }]
        );
        assert_eq!(r.rdp.prim, SENTINEL, "3-word TEXRECT → sentinel decodes");
        assert!(
            r.scene.draw_runs.is_empty(),
            "paired scene → flat draw_runs empty"
        );
    }

    #[test]
    fn fixed_texrectflip_sets_flip() {
        let mut b = Vec::new();
        push(&mut b, cimg(0x10000));
        // Distinct non-zero coords: ulx=5 (raw 20), uly=7 (raw 28), lrx=320, lry=240.
        push(
            &mut b,
            (
                (G_TEXRECTFLIP as u32) << 24 | (1280 << 12) | 960,
                (20u32 << 12) | 28,
            ),
        );
        push(&mut b, ((G_RDPHALF_1 as u32) << 24, (11u32 << 16) | 13)); // uls=11, ult=13
        push(&mut b, ((G_RDPHALF_2 as u32) << 24, (1024u32 << 16) | 512)); // dsdx=1024, dtdy=512
        push(&mut b, prim_sentinel());
        push(&mut b, enddl());
        let r = run(&b, GbiUcode::F3dex2);
        assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
        match &r.scene.framebuffer_pairs[0].ops[..] {
            [SceneOp::TexRect {
                flip,
                rect,
                uls,
                ult,
                dsdx,
                dtdy,
                ..
            }] => {
                assert!(*flip, "TEXRECTFLIP must set flip");
                assert_eq!(
                    *rect,
                    crate::hle::TexRectBounds {
                        ulx: 20,
                        uly: 28,
                        lrx: 1280,
                        lry: 960
                    }
                );
                assert_eq!((*uls, *ult, *dsdx, *dtdy), (11, 13, 1024, 512));
            }
            other => panic!("expected one TexRect, got {other:?}"),
        }
        assert_eq!(r.rdp.prim, SENTINEL);
    }

    #[test]
    fn fixed_fillrect_one_word_records_fill_color() {
        let mut b = Vec::new();
        push(&mut b, cimg(0x10000));
        push(&mut b, fill_color(0xCAFE_F00D));
        // 1-word FILLRECT: lrx=320, lry=240, ulx=10, uly=20.
        push(
            &mut b,
            (
                (G_FILLRECT as u32) << 24 | (1280 << 12) | 960,
                ((10u32 << 2) << 12) | (20u32 << 2),
            ),
        );
        push(&mut b, prim_sentinel());
        push(&mut b, enddl());
        let r = run(&b, GbiUcode::F3dex2);
        assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
        assert_eq!(
            r.scene.framebuffer_pairs[0].ops,
            vec![SceneOp::FillRect {
                rect: Rect {
                    ulx: 10,
                    uly: 20,
                    lrx: 320,
                    lry: 240
                },
                color_raw: 0xCAFE_F00D,
            }]
        );
        assert_eq!(r.rdp.prim, SENTINEL, "1-word FILLRECT → sentinel decodes");
    }

    #[test]
    fn float_texrect_records_op_and_sentinel_decodes() {
        let mut b = Vec::new();
        push(&mut b, cimg(0x10000));
        // Float: lrx=sext24(cmd0.w0)>>2, lry=sext24(cmd0.w1)>>2,
        //        ulx=sext24(cmd1.w0)>>2, uly=sext24(cmd2.w0)>>2.
        // Distinct non-zero values: ulx=5 (raw 20), uly=7 (raw 28), lrx=320, lry=240.
        // Any source-word swap (ulx@cmd1 vs uly@cmd2, or lrx@cmd0.w0 vs lry@cmd0.w1) fails.
        push(&mut b, ((G_TEXRECT as u32) << 24 | 1280, 960)); // lrx=320 (raw 1280), lry=240 (raw 960)
        push(
            &mut b,
            ((G_RDPHALF_1 as u32) << 24 | 20, (11u32 << 16) | 13),
        ); // ulx=5 (raw 20); uls=11, ult=13
        push(
            &mut b,
            ((G_RDPHALF_2 as u32) << 24 | 28, (1024u32 << 16) | 512),
        ); // uly=7 (raw 28); dsdx=1024, dtdy=512
        push(&mut b, prim_sentinel());
        push(&mut b, enddl());
        let r = run_float(&b);
        assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
        assert_eq!(
            r.scene.framebuffer_pairs[0].ops,
            vec![SceneOp::TexRect {
                rect: crate::hle::TexRectBounds {
                    ulx: 20,
                    uly: 28,
                    lrx: 1280,
                    lry: 960
                },
                tile: 0,
                uls: 11,
                ult: 13,
                dsdx: 1024,
                dtdy: 512,
                flip: false,
                copy_mode: false,
                material_index: 0,
                render_mode_index: 0,
                fog_color: [0; 4],
                prim_depth: Default::default(),
                fb_source: None,
            }]
        );
        assert_eq!(
            r.rdp.prim, SENTINEL,
            "3-word Float TEXRECT → sentinel decodes"
        );
    }

    #[test]
    fn float_fillrect_two_words_records_fill_color() {
        let mut b = Vec::new();
        push(&mut b, cimg(0x10000));
        push(&mut b, fill_color(0x1234_5678));
        // Float FILLRECT = F6 + E1: lrx/lry in cmd0.w0/w1, ulx/uly in cmd1.w0/w1 (all sext24).
        push(&mut b, ((G_FILLRECT as u32) << 24 | 1280, 960));
        push(&mut b, ((G_RDPHALF_1 as u32) << 24, 0));
        push(&mut b, prim_sentinel());
        push(&mut b, enddl());
        let r = run_float(&b);
        assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
        assert_eq!(
            r.scene.framebuffer_pairs[0].ops,
            vec![SceneOp::FillRect {
                rect: Rect {
                    ulx: 0,
                    uly: 0,
                    lrx: 320,
                    lry: 240
                },
                color_raw: 0x1234_5678,
            }]
        );
        assert_eq!(
            r.rdp.prim, SENTINEL,
            "2-word Float FILLRECT → sentinel decodes"
        );
    }

    #[test]
    fn mid_pair_scissor_change_emits_setscissor_op() {
        // Two FILLRECTs in one pair with a scissor change between → ops = [Fill, SetScissor, Fill].
        let mut b = Vec::new();
        push(&mut b, cimg(0x10000));
        push(&mut b, scissor(0, 0, 320, 240, 0)); // S1 (becomes active_scissor)
        push(&mut b, fill_color(0x1111_1111));
        push(
            &mut b,
            ((G_FILLRECT as u32) << 24 | (400 << 12) | 320, 0), // rect_a: lrx=100,lry=80,ulx=0,uly=0
        );
        push(&mut b, scissor(10, 20, 300, 220, 1)); // S2 (mid-pair change)
        push(
            &mut b,
            (
                (G_FILLRECT as u32) << 24 | (800 << 12) | 600, // rect_b: lrx=200,lry=150
                ((50u32 << 2) << 12) | (40u32 << 2),           // ulx=50,uly=40
            ),
        );
        push(&mut b, prim_sentinel());
        push(&mut b, enddl());
        let r = run(&b, GbiUcode::F3dex2);
        assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
        assert_eq!(r.scene.framebuffer_pairs.len(), 1);
        let p = &r.scene.framebuffer_pairs[0];
        assert_eq!(
            p.active_scissor,
            Scissor {
                ulx: 0,
                uly: 0,
                lrx: 320,
                lry: 240,
                mode: 0
            },
            "active_scissor is snapshotted at pair open (S1)"
        );
        assert_eq!(
            p.ops,
            vec![
                SceneOp::FillRect {
                    rect: Rect {
                        ulx: 0,
                        uly: 0,
                        lrx: 100,
                        lry: 80
                    },
                    color_raw: 0x1111_1111,
                },
                SceneOp::SetScissor(Scissor {
                    ulx: 10,
                    uly: 20,
                    lrx: 300,
                    lry: 220,
                    mode: 1
                }),
                SceneOp::FillRect {
                    rect: Rect {
                        ulx: 50,
                        uly: 40,
                        lrx: 200,
                        lry: 150
                    },
                    color_raw: 0x1111_1111,
                },
            ]
        );
        assert_eq!(r.rdp.prim, SENTINEL);
    }

    #[test]
    fn truncated_texrect_diagnoses_without_panic() {
        // DL ends right after the E4 command word → the continuation bounds-check must fire.
        let mut b = Vec::new();
        push(&mut b, cimg(0x10000));
        push(&mut b, ((G_TEXRECT as u32) << 24 | (1280 << 12) | 960, 0)); // cmd0 only; buffer ends
        let r = run(&b, GbiUcode::F3dex2);
        assert!(
            r.diags
                .iter()
                .any(|d| d.kind == crate::diag::DiagKind::TruncatedRect { fill: false }),
            "expected a truncation diag, got {:?}",
            r.diags
        );
        assert!(r.scene.framebuffer_pairs.is_empty());
    }

    #[test]
    fn rect_before_first_cimg_is_dropped_with_diag() {
        // No CIMG → a rect is malformed: diag + drop (no pair, no flat draw run).
        let mut b = Vec::new();
        push(&mut b, ((G_TEXRECT as u32) << 24 | (1280 << 12) | 960, 0));
        push(&mut b, ((G_RDPHALF_1 as u32) << 24, 0));
        push(&mut b, ((G_RDPHALF_2 as u32) << 24, (1024u32 << 16) | 1024));
        push(&mut b, prim_sentinel());
        push(&mut b, enddl());
        let r = run(&b, GbiUcode::F3dex2);
        assert!(
            r.diags
                .iter()
                .any(|d| d.kind == crate::diag::DiagKind::DrawBeforeCimg),
            "expected pre-CIMG diag, got {:?}",
            r.diags
        );
        assert!(r.scene.framebuffer_pairs.is_empty());
        assert!(r.scene.draw_runs.is_empty());
        assert_eq!(
            r.rdp.prim, SENTINEL,
            "sentinel still decodes after the dropped rect"
        );
    }

    #[test]
    fn interpret_stashes_final_color_image_on_scene() {
        // A DL that only sets the color image (no draws) still stamps scene.color_image from the
        // final RDP snapshot — the pair-less internal-FB key (spec §4).
        let mut b = Vec::new();
        push(&mut b, cimg(0x0010_0000)); // fmt=RGBA(0), siz=16b(2), width=320, addr=0x100000
        push(&mut b, enddl());
        let r = run(&b, GbiUcode::F3dex2);
        assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
        assert_eq!(
            r.scene.color_image,
            ColorImage {
                fmt: 0,
                siz: 2,
                width: 320,
                addr: 0x0010_0000
            }
        );
    }

    #[test]
    fn interpret_color_image_defaults_to_sentinel_when_no_cimg() {
        // A pair-less DL that never sets a color image leaves scene.color_image at the default
        // sentinel (addr 0) — the normal case for flat-3D scenes.
        let mut b = Vec::new();
        push(&mut b, enddl());
        let r = run(&b, GbiUcode::F3dex2);
        assert_eq!(r.scene.color_image, ColorImage::default());
        assert_eq!(r.scene.color_image.addr, 0);
    }
}

#[cfg(test)]
mod structured_diag_tests {
    use super::*;
    use crate::diag::{DiagKind, Diagnostic, Severity};
    use crate::hle::consts::{G_ENDDL, G_SETCIMG, G_TEXRECT};
    use crate::hle::gbi::GbiUcode;
    use crate::hle::mem::RdramImage;

    fn push(buf: &mut Vec<u8>, (w0, w1): (u32, u32)) {
        buf.extend_from_slice(&w0.to_be_bytes());
        buf.extend_from_slice(&w1.to_be_bytes());
    }

    #[test]
    fn unknown_opcode_maps_to_structured_kind() {
        let mut rdram = vec![0u8; 16];
        rdram[0] = 0xAB;
        rdram[8] = G_ENDDL;
        let r = interpret(
            RdramImage::new(&rdram),
            0,
            GbiUcode::F3dex2,
            crate::hle::mem::GbiDataFormat::Fixed,
        );
        assert_eq!(
            r.diags,
            vec![Diagnostic {
                at: 0,
                kind: DiagKind::UnknownOpcode(0xAB)
            }]
        );
        assert_eq!(r.diags[0].kind.severity(), Severity::Error);
    }

    #[test]
    fn repeated_unknown_opcode_is_deduped_by_value() {
        // 100 identical 0xAB opcode words then ENDDL: anti-flood emits ONE UnknownOpcode(0xAB).
        let mut rdram = Vec::new();
        for _ in 0..100 {
            push(&mut rdram, (0xABu32 << 24, 0));
        }
        push(&mut rdram, ((G_ENDDL as u32) << 24, 0));
        let r = interpret(
            RdramImage::new(&rdram),
            0,
            GbiUcode::F3dex2,
            crate::hle::mem::GbiDataFormat::Fixed,
        );
        let n = r
            .diags
            .iter()
            .filter(|d| matches!(d.kind, DiagKind::UnknownOpcode(0xAB)))
            .count();
        assert_eq!(
            n, 1,
            "repeated identical unknown opcode must dedup: {:?}",
            r.diags
        );
    }

    #[test]
    fn commands_counts_dispatches() {
        let mut b = Vec::new();
        push(
            &mut b,
            ((G_SETCIMG as u32) << 24 | (2 << 19) | 319, 0x10000),
        );
        push(&mut b, ((G_ENDDL as u32) << 24, 0));
        let r = interpret(
            RdramImage::new(&b),
            0,
            GbiUcode::F3dex2,
            crate::hle::mem::GbiDataFormat::Fixed,
        );
        assert_eq!(r.commands, 2, "SETCIMG + ENDDL == 2 dispatches");
    }

    #[test]
    fn draw_before_cimg_counts_a_dropped_run() {
        let mut b = Vec::new();
        push(&mut b, ((G_TEXRECT as u32) << 24 | (1280 << 12) | 960, 0));
        push(&mut b, (0, 0)); // TEXRECT continuation word 1
        push(&mut b, (0, 0)); // TEXRECT continuation word 2
        push(&mut b, ((G_ENDDL as u32) << 24, 0));
        let r = interpret(
            RdramImage::new(&b),
            0,
            GbiUcode::F3dex2,
            crate::hle::mem::GbiDataFormat::Fixed,
        );
        assert!(r.diags.iter().any(|d| d.kind == DiagKind::DrawBeforeCimg));
        assert_eq!(r.dropped_runs, 1);
    }
}
