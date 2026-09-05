use crate::hle::consts::rdp::G_SETTIMG;
use crate::hle::consts::rsp_f3d::{
    G_CLEARGEOMETRYMODE, G_CULLDL, G_DL, G_ENDDL, G_MOVEMEM, G_MOVEWORD, G_MTX, G_MV_LOOKATX,
    G_MV_LOOKATY, G_MV_MATRIX_1, G_MV_MATRIX_2, G_MV_MATRIX_3, G_MV_MATRIX_4, G_MV_TXTATT,
    G_MV_VIEWPORT, G_MW_CLIP, G_MW_FOG, G_MW_LIGHTCOL, G_MW_MATRIX, G_MW_NUMLIGHT, G_MW_PERSPNORM,
    G_MW_POINTS, G_MW_SEGMENT, G_POPMTX, G_QUAD, G_RDPHALF_1, G_RDPHALF_2, G_RDPNOOP,
    G_SETGEOMETRYMODE, G_SETOTHERMODE_H, G_SETOTHERMODE_L, G_SPNOOP, G_SPRITE2D_BASE, G_TEXTURE,
    G_TRI1, G_VTX,
};
use crate::hle::interp::{Cmd, Ctx, Handler};
use crate::hle::mem::Rdram;
use crate::{DiagKind, Diagnostic};

pub(crate) fn install_overrides<M: Rdram>(t: &mut [Handler<M>; 256]) {
    for op in [
        G_SPNOOP,
        G_MTX,
        G_MOVEMEM,
        G_VTX,
        G_DL,
        G_SPRITE2D_BASE,
        G_RDPHALF_2,
        G_RDPHALF_1,
        G_QUAD,
        G_CLEARGEOMETRYMODE,
        G_SETGEOMETRYMODE,
        G_ENDDL,
        G_SETOTHERMODE_L,
        G_SETOTHERMODE_H,
        G_TEXTURE,
        G_MOVEWORD,
        G_POPMTX,
        G_CULLDL,
        G_TRI1,
        G_RDPNOOP,
    ] {
        t[op as usize] = no_op::<M>;
    }
    t[G_MTX as usize] = matrix::<M>;
    t[G_VTX as usize] = vtx::<M>;
    t[G_TRI1 as usize] = tri1::<M>;
    t[G_QUAD as usize] = quad::<M>;
    t[G_SETGEOMETRYMODE as usize] = set_geometry_mode::<M>;
    t[G_CLEARGEOMETRYMODE as usize] = clear_geometry_mode::<M>;
    t[G_MOVEMEM as usize] = move_mem::<M>;
    t[G_TEXTURE as usize] = texture::<M>;
    t[G_SETOTHERMODE_H as usize] = set_other_mode_h::<M>;
    t[G_SETOTHERMODE_L as usize] = set_other_mode_l::<M>;
    t[G_MOVEWORD as usize] = move_word::<M>;
    t[G_POPMTX as usize] = pop_matrix::<M>;
    t[G_SETTIMG as usize] = set_texture_image::<M>;
}

fn no_op<M: Rdram>(_c: &Cmd, _cx: &mut Ctx<M>) {}

fn matrix<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let params = c.p0(16, 8) as u8;
    let addr = cx.mem.resolve_masked(c.w1_addr);
    cx.rsp.matrix(cx.mem, addr, params);
}

fn vtx<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let count = c.p0(20, 4) + 1;
    let dst = c.p0(16, 4);
    let end = dst + count;
    if end <= 16 {
        let addr = cx.mem.resolve_masked(c.w1_addr);
        cx.rsp
            .set_vertex(cx.mem, addr, count, dst, cx.rdp, cx.scene);
    } else {
        cx.diags.push(Diagnostic {
            at: cx.pc,
            kind: DiagKind::VtxOutOfRange { count, end },
        });
    }
}

fn tri1<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    if let Some((material_index, render_mode_index)) =
        crate::hle::rsp::snapshot_run(cx.rsp, cx.rdp, cx.diags, cx.scene, cx.pc)
    {
        crate::hle::rsp::record_tri(
            cx.rsp,
            cx.rdp,
            cx.scene,
            cx.rec,
            c.p1(16, 8) / 10,
            c.p1(8, 8) / 10,
            c.p1(0, 8) / 10,
            material_index,
            render_mode_index,
        );
    } else {
        *cx.dropped_runs += 1;
    }
}

fn quad<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    if let Some((material_index, render_mode_index)) =
        crate::hle::rsp::snapshot_run(cx.rsp, cx.rdp, cx.diags, cx.scene, cx.pc)
    {
        let v0 = c.p1(24, 8) / 10;
        let v1 = c.p1(16, 8) / 10;
        let v2 = c.p1(8, 8) / 10;
        let v3 = c.p1(0, 8) / 10;
        crate::hle::rsp::record_tri(
            cx.rsp,
            cx.rdp,
            cx.scene,
            cx.rec,
            v0,
            v1,
            v2,
            material_index,
            render_mode_index,
        );
        crate::hle::rsp::record_tri(
            cx.rsp,
            cx.rdp,
            cx.scene,
            cx.rec,
            v0,
            v2,
            v3,
            material_index,
            render_mode_index,
        );
    } else {
        *cx.dropped_runs += 1;
    }
}

fn set_geometry_mode<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    cx.rsp.modify_geometry_mode(u32::MAX, c.w1);
}

fn clear_geometry_mode<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    cx.rsp.modify_geometry_mode(!c.w1, 0);
}

fn move_mem<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let idx = c.p0(16, 8) as u8;
    match idx {
        G_MV_VIEWPORT => {
            let addr = cx.mem.resolve_masked(c.w1_addr);
            cx.rsp.set_viewport(cx.mem, addr);
        }
        G_MV_LOOKATY => {
            let addr = cx.mem.resolve_masked(c.w1_addr);
            cx.rsp.set_lookat(cx.mem, 1, addr);
        }
        G_MV_LOOKATX => {
            let addr = cx.mem.resolve_masked(c.w1_addr);
            cx.rsp.set_lookat(cx.mem, 0, addr);
        }
        0x86..=0x94 if idx & 1 == 0 => {
            let light_idx = ((idx - 0x86) / 2) as u32;
            let addr = cx.mem.resolve_masked(c.w1_addr);
            cx.rsp.set_light(cx.mem, light_idx, addr);
        }
        G_MV_MATRIX_1 => {
            let addr = cx.mem.resolve_masked(c.w1_addr);
            cx.rsp.force_matrix(cx.mem, addr);
        }
        G_MV_MATRIX_2 | G_MV_MATRIX_3 | G_MV_MATRIX_4 | G_MV_TXTATT => {}
        _ => cx.diags.push(Diagnostic {
            at: cx.pc,
            kind: DiagKind::UnhandledMovemem(idx),
        }),
    }
}

fn texture<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let tile = c.p0(8, 3) as u8;
    let level = c.p0(11, 3) as u8;
    let on = c.p0(0, 8) != 0;
    let sc = (c.w1 >> 16) as u16;
    let tc = c.w1 as u16;
    cx.rsp.set_texture(tile, level, on, sc, tc);
    cx.rsp.material_dirty = true;
}

fn set_other_mode_h<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    cx.rsp
        .set_other_mode_h_raw(c.p0(8, 8), c.p0(0, 8), c.w1, cx.rdp);
    cx.rsp.material_dirty = true;
}

fn set_other_mode_l<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    cx.rsp
        .set_other_mode_l_raw(c.p0(8, 8), c.p0(0, 8), c.w1, cx.rdp);
}

fn move_word<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let ty = c.p0(0, 8) as u8;
    match ty {
        G_MW_MATRIX | G_MW_CLIP | G_MW_PERSPNORM => {}
        G_MW_POINTS => {
            let offset = c.p0(8, 16);
            cx.rsp
                .modify_vertex(offset / 40, offset % 40, c.w1, cx.scene);
        }
        G_MW_NUMLIGHT => {
            let n = ((c.w1.wrapping_sub(0x8000_0000)) >> 5).wrapping_sub(1);
            cx.rsp.set_num_lights_direct(n);
        }
        G_MW_SEGMENT => Rdram::set_segment(cx.mem, c.p0(10, 4), c.w1_addr),
        G_MW_FOG => {
            cx.rdp.fog_mul = (c.w1 >> 16) as i16;
            cx.rdp.fog_offset = c.w1 as i16;
        }
        G_MW_LIGHTCOL => {
            let light_idx = c.p0(8, 16) / 32;
            cx.rsp.set_light_color(light_idx, c.w1);
        }
        _ => cx.diags.push(Diagnostic {
            at: cx.pc,
            kind: DiagKind::UnhandledMoveword(ty),
        }),
    }
}

fn pop_matrix<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    if c.w1 == 0 {
        cx.rsp.pop_matrix(1);
    }
}

fn set_texture_image<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let fmt = c.p0(21, 3) as u8;
    let siz = c.p0(19, 2) as u8;
    // The width field is (actual_width - 1), but tex_image stores the field value directly.
    let width = (c.w0 & 0xFFF) as u16;
    let addr = cx.mem.resolve(c.w1_addr);
    cx.rsp.set_texture_image(fmt, siz, width, addr, cx.rdp);
}

#[cfg(all(test, feature = "asm"))]
mod phase2_tests {
    use crate::hle::consts::{G_CULL_FRONT, G_FOG, G_RM_OPA_SURF, G_RM_OPA_SURF2};
    use crate::hle::gbi::GbiUcode;
    use crate::hle::interp::interpret;
    use crate::hle::mem::RdramImage;
    use crate::hle::{CullKind, DrawRun, InterpResult};
    use n64_gbi::encode::{
        gdp_load_texture_block, gdp_set_combine_lerp, gdp_set_cycle_type, gdp_set_cycle_type_f3d,
        gdp_set_render_mode, gdp_set_render_mode_f3d, gsp_1triangle_f3d, gsp_2triangles,
        gsp_clear_geometrymode_f3d, gsp_enddl, gsp_enddl_f3d, gsp_matrix, gsp_matrix_f3d,
        gsp_quad_f3d, gsp_set_geometrymode, gsp_set_geometrymode_f3d, gsp_texture, gsp_texture_f3d,
        gsp_vertex, gsp_vertex_f3d, gsp_viewport, gsp_viewport_f3d, mtx_to_bytes, CcPass,
        VtxColored, ZERO_A, ZERO_C,
    };

    const VTX_ADDR: u32 = 0x40;
    const VIEWPORT_ADDR: u32 = 0x80;
    const TEXTURE_ADDR: u32 = 0x100;
    const ENTRY_ADDR: u32 = 0x180;
    const G_CULL_FRONT_F3D: u32 = 0x0000_1000;

    fn push(buf: &mut Vec<u8>, (w0, w1): (u32, u32)) {
        buf.extend_from_slice(&w0.to_be_bytes());
        buf.extend_from_slice(&w1.to_be_bytes());
    }

    fn shaded_combine() -> (u32, u32) {
        let color = CcPass {
            a: ZERO_C,
            b: ZERO_C,
            c: ZERO_C,
            d: 4,
        };
        let alpha = CcPass {
            a: ZERO_A,
            b: ZERO_A,
            c: ZERO_A,
            d: 4,
        };
        gdp_set_combine_lerp(color, alpha, color, alpha)
    }

    fn textured_combine() -> (u32, u32) {
        let color = CcPass {
            a: 1,
            b: ZERO_C,
            c: 4,
            d: ZERO_C,
        };
        let alpha = CcPass {
            a: ZERO_A,
            b: ZERO_A,
            c: ZERO_A,
            d: 4,
        };
        gdp_set_combine_lerp(color, alpha, color, alpha)
    }

    fn scene_data() -> Vec<u8> {
        let mut bytes = vec![0; ENTRY_ADDR as usize];
        let matrix = [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [4.0, 5.0, 0.0, 1.0],
        ];
        bytes[..0x40].copy_from_slice(&mtx_to_bytes(matrix));
        let vertices = [
            VtxColored {
                x: -48,
                y: -48,
                z: 0,
                flag: 0,
                s: 0,
                t: 0,
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
            VtxColored {
                x: 48,
                y: -48,
                z: 0,
                flag: 0,
                s: 32,
                t: 0,
                r: 0,
                g: 255,
                b: 0,
                a: 255,
            },
            VtxColored {
                x: 48,
                y: 48,
                z: 0,
                flag: 0,
                s: 32,
                t: 32,
                r: 0,
                g: 0,
                b: 255,
                a: 255,
            },
            VtxColored {
                x: -48,
                y: 48,
                z: 0,
                flag: 0,
                s: 0,
                t: 32,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        ];
        for (i, vertex) in vertices.iter().enumerate() {
            let start = VTX_ADDR as usize + i * 16;
            bytes[start..start + 16].copy_from_slice(&vertex.to_bytes());
        }
        for (i, value) in [640i16, 480, 511, 0, 320, 240, 256, 0].iter().enumerate() {
            let start = VIEWPORT_ADDR as usize + i * 2;
            bytes[start..start + 2].copy_from_slice(&value.to_be_bytes());
        }
        bytes[TEXTURE_ADDR as usize..TEXTURE_ADDR as usize + 32].fill(0xFF);
        bytes
    }

    fn run(mut bytes: Vec<u8>, commands: &[(u32, u32)], ucode: GbiUcode) -> InterpResult {
        for &command in commands {
            push(&mut bytes, command);
        }
        interpret(
            RdramImage::new(&bytes),
            ENTRY_ADDR as u64,
            ucode,
            crate::hle::mem::GbiDataFormat::Fixed,
        )
    }

    #[test]
    fn f3d_textured_cull_front_quad_outputs_match_f3dex2() {
        let mut f3d_commands = vec![
            gsp_matrix_f3d(0, true, true, false),
            gsp_viewport_f3d(VIEWPORT_ADDR),
            gdp_set_cycle_type_f3d(0),
            gdp_set_render_mode_f3d(G_RM_OPA_SURF, G_RM_OPA_SURF2),
            textured_combine(),
        ];
        f3d_commands.extend(gdp_load_texture_block(0, 2, 4, 4, TEXTURE_ADDR, 2, 2, 2, 2));
        f3d_commands.extend([
            gsp_texture_f3d(0x8000, 0x4000, 0, 0, true),
            gsp_set_geometrymode_f3d(G_CULL_FRONT_F3D | G_FOG),
            gsp_vertex_f3d(5, 4, VTX_ADDR),
            gsp_quad_f3d(5, 6, 7, 8),
            gsp_enddl_f3d(),
        ]);

        let mut f3dex2_commands = vec![
            gsp_matrix(0, true, true, false),
            gsp_viewport(VIEWPORT_ADDR),
            gdp_set_cycle_type(0),
            gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
            textured_combine(),
        ];
        f3dex2_commands.extend(gdp_load_texture_block(0, 2, 4, 4, TEXTURE_ADDR, 2, 2, 2, 2));
        f3dex2_commands.extend([
            gsp_texture(0x8000, 0x4000, 0, 0, true),
            gsp_set_geometrymode(G_CULL_FRONT | G_FOG),
            gsp_vertex(5, 4, VTX_ADDR),
            gsp_2triangles(5, 6, 7, 5, 7, 8),
            gsp_enddl(),
        ]);

        let f3d = run(scene_data(), &f3d_commands, GbiUcode::F3d);
        let f3dex2 = run(scene_data(), &f3dex2_commands, GbiUcode::F3dex2);

        assert!(f3d.scene.materials[0].tex_enable);
        assert_eq!(f3d.scene.draw_runs[0].cull, CullKind::Cull);
        assert_eq!(f3d.scene.indices, vec![2, 1, 0, 3, 2, 0]);
        assert_eq!(f3d.scene.mtx_index, vec![1; 4]);
        assert_eq!(f3d.scene.fog, vec![1; 4]);

        assert_eq!(
            (
                &f3d.scene.draw_runs,
                &f3d.scene.indices,
                &f3d.scene.raw_pos,
                &f3d.scene.mvp_table,
                &f3d.scene.materials,
                &f3d.scene.render_modes,
                &f3d.scene.viewport_table,
                &f3d.scene.texcoord_table,
                &f3d.scene.mtx_index,
                &f3d.scene.light_index,
                &f3d.scene.fog,
            ),
            (
                &f3dex2.scene.draw_runs,
                &f3dex2.scene.indices,
                &f3dex2.scene.raw_pos,
                &f3dex2.scene.mvp_table,
                &f3dex2.scene.materials,
                &f3dex2.scene.render_modes,
                &f3dex2.scene.viewport_table,
                &f3dex2.scene.texcoord_table,
                &f3dex2.scene.mtx_index,
                &f3dex2.scene.light_index,
                &f3dex2.scene.fog,
            )
        );
    }

    #[test]
    fn f3d_vertex_tri1_and_quad_record_expected_corners() {
        let result = run(
            scene_data(),
            &[
                shaded_combine(),
                gsp_vertex_f3d(0, 4, VTX_ADDR),
                gsp_1triangle_f3d(0, 1, 2),
                gsp_quad_f3d(0, 1, 2, 3),
                gsp_enddl_f3d(),
            ],
            GbiUcode::F3d,
        );

        assert_eq!(
            result.scene.draw_runs,
            vec![DrawRun {
                fog_color: [0; 4],
                material_index: 0,
                render_mode_index: 0,
                cull: CullKind::None,
                index_count: 9,
                index_start: 0,
            }]
        );
        assert_eq!(result.scene.indices, vec![0, 1, 2, 0, 1, 2, 0, 2, 3]);
        assert_eq!(
            result.scene.raw_pos,
            vec![
                [-48.0, -48.0, 0.0],
                [48.0, -48.0, 0.0],
                [48.0, 48.0, 0.0],
                [-48.0, 48.0, 0.0],
            ]
        );
    }

    #[test]
    fn f3d_cull_front_swaps_indices_until_cleared() {
        let result = run(
            scene_data(),
            &[
                shaded_combine(),
                gsp_vertex_f3d(0, 3, VTX_ADDR),
                gsp_set_geometrymode_f3d(G_CULL_FRONT_F3D),
                gsp_1triangle_f3d(0, 1, 2),
                gsp_clear_geometrymode_f3d(G_CULL_FRONT_F3D),
                gsp_1triangle_f3d(0, 1, 2),
                gsp_enddl_f3d(),
            ],
            GbiUcode::F3d,
        );

        assert_eq!(
            result.scene.draw_runs,
            vec![
                DrawRun {
                    fog_color: [0; 4],
                    material_index: 0,
                    render_mode_index: 0,
                    cull: CullKind::Cull,
                    index_count: 3,
                    index_start: 0,
                },
                DrawRun {
                    fog_color: [0; 4],
                    material_index: 0,
                    render_mode_index: 0,
                    cull: CullKind::None,
                    index_count: 3,
                    index_start: 3,
                },
            ]
        );
        assert_eq!(result.scene.indices, vec![2, 1, 0, 0, 1, 2]);
        assert_eq!(result.geometry_mode & G_CULL_FRONT_F3D, 0);
    }
}

#[cfg(all(test, feature = "asm"))]
mod phase3_tests {
    use crate::hle::consts::rsp_f3d::{
        G_MOVEWORD, G_MV_MATRIX_2, G_MV_MATRIX_3, G_MV_MATRIX_4, G_MW_PERSPNORM, G_POPMTX,
    };
    use crate::hle::consts::{G_LIGHTING, G_RM_OPA_SURF, G_RM_OPA_SURF2, G_TEXTURE_GEN};
    use crate::hle::gbi::GbiUcode;
    use crate::hle::interp::{interpret, InterpResult};
    use crate::hle::math::mul4;
    use crate::hle::mem::RdramImage;
    use n64_gbi::encode::{
        gdp_load_texture_block, gdp_set_combine_lerp, gdp_set_cycle_type_f3d,
        gdp_set_render_mode_f3d, gsp_1triangle_f3d, gsp_enddl_f3d, gsp_forcematrix_f3d,
        gsp_light_f3d, gsp_lightcolor_f3d, gsp_lookat_f3d, gsp_matrix_f3d, gsp_modifyvertex_f3d,
        gsp_numlights_f3d, gsp_popmatrix_f3d, gsp_segment_f3d, gsp_set_geometrymode_f3d,
        gsp_texture_f3d, gsp_vertex_f3d, gsp_viewport_f3d, mtx_to_bytes, CcPass, VtxColored,
        ZERO_A, ZERO_C,
    };

    const ENTRY: usize = 0x200;
    const G_CYC_2CYCLE: u32 = 1;

    fn push(buf: &mut Vec<u8>, (w0, w1): (u32, u32)) {
        buf.extend_from_slice(&w0.to_be_bytes());
        buf.extend_from_slice(&w1.to_be_bytes());
    }

    fn run(mut bytes: Vec<u8>, commands: &[(u32, u32)]) -> InterpResult {
        bytes.resize(ENTRY, 0);
        for &command in commands {
            push(&mut bytes, command);
        }
        interpret(
            RdramImage::new(&bytes),
            ENTRY as u64,
            GbiUcode::F3d,
            crate::hle::mem::GbiDataFormat::Fixed,
        )
    }

    fn vertex() -> VtxColored {
        VtxColored {
            x: 7,
            y: 8,
            z: 9,
            flag: 0,
            s: 32,
            t: 64,
            r: 127,
            g: 0,
            b: 0,
            a: 255,
        }
    }

    fn shaded_combine() -> (u32, u32) {
        let color = CcPass {
            a: ZERO_C,
            b: ZERO_C,
            c: ZERO_C,
            d: 4,
        };
        let alpha = CcPass {
            a: ZERO_A,
            b: ZERO_A,
            c: ZERO_A,
            d: 4,
        };
        gdp_set_combine_lerp(color, alpha, color, alpha)
    }

    fn textured_combine() -> (u32, u32) {
        let color = CcPass {
            a: 1,
            b: ZERO_C,
            c: 4,
            d: ZERO_C,
        };
        let alpha = CcPass {
            a: ZERO_A,
            b: ZERO_A,
            c: ZERO_A,
            d: 4,
        };
        gdp_set_combine_lerp(color, alpha, color, alpha)
    }

    #[test]
    fn texture_handler_builds_scaled_textured_material() {
        const VTX: usize = 0x40;
        const TEX: usize = 0x100;
        let mut bytes = vec![0; ENTRY];
        bytes[VTX..VTX + 16].copy_from_slice(&vertex().to_bytes());
        bytes[TEX..TEX + 32].fill(0xFF);

        let mut commands = vec![
            gdp_set_cycle_type_f3d(0),
            gdp_set_render_mode_f3d(G_RM_OPA_SURF, G_RM_OPA_SURF2),
            textured_combine(),
        ];
        commands.extend(gdp_load_texture_block(0, 2, 4, 4, TEX as u32, 2, 2, 2, 2));
        let mut texture = gsp_texture_f3d(0x8000, 0x4000, 0, 0, true);
        texture.0 = (texture.0 & !0xFF) | 0x80;
        commands.extend([
            texture,
            gsp_vertex_f3d(0, 1, VTX as u32),
            gsp_1triangle_f3d(0, 0, 0),
            gsp_enddl_f3d(),
        ]);

        let result = run(bytes, &commands);
        assert!(
            result.diags.is_empty(),
            "unexpected diags: {:?}",
            result.diags
        );
        let material = &result.scene.materials[0];
        assert!(material.tex_enable);
        assert_eq!((material.tex_w, material.tex_h), (4, 4));
        let scale = result.scene.texcoord_table[result.scene.texcoord_index[0] as usize];
        assert_eq!(scale, [0.015625, 0.0078125]);
    }

    #[test]
    fn raw_othermode_handlers_publish_cycle_and_render_mode() {
        const VTX: usize = 0x40;
        let mut bytes = vec![0; ENTRY];
        bytes[VTX..VTX + 16].copy_from_slice(&vertex().to_bytes());
        let render_mode = G_RM_OPA_SURF | G_RM_OPA_SURF2;
        let result = run(
            bytes,
            &[
                gdp_set_cycle_type_f3d(G_CYC_2CYCLE),
                gdp_set_render_mode_f3d(G_RM_OPA_SURF, G_RM_OPA_SURF2),
                shaded_combine(),
                gsp_vertex_f3d(0, 1, VTX as u32),
                gsp_1triangle_f3d(0, 0, 0),
                gsp_enddl_f3d(),
            ],
        );

        assert_eq!(result.rdp.other_mode_h & (3 << 20), G_CYC_2CYCLE << 20);
        assert_eq!(result.rdp.other_mode_l, render_mode);
        assert_eq!(
            result.scene.render_modes[0],
            crate::hle::blender::decode_render_mode(
                render_mode,
                G_CYC_2CYCLE << 20,
                result.geometry_mode,
            )
        );
    }

    #[test]
    fn moveword_segment_resolves_vertex_and_perspnorm_is_silent() {
        const SEGMENT_BASE: u32 = 0x40;
        const VTX: usize = 0x80;
        let mut bytes = vec![0; ENTRY];
        bytes[VTX..VTX + 16].copy_from_slice(&vertex().to_bytes());
        let perspnorm = ((G_MOVEWORD as u32) << 24) | G_MW_PERSPNORM as u32;
        let result = run(
            bytes,
            &[
                gsp_segment_f3d(2, SEGMENT_BASE),
                (perspnorm, 129),
                gsp_vertex_f3d(0, 1, 0x0200_0040),
                gsp_enddl_f3d(),
            ],
        );

        assert_eq!(result.scene.raw_pos, vec![[7.0, 8.0, 9.0]]);
        assert!(
            result.diags.is_empty(),
            "unexpected diags: {:?}",
            result.diags
        );
    }

    #[test]
    fn moveword_and_movemem_publish_fog_lights_lookat_and_viewport() {
        const VTX: usize = 0x00;
        const LIGHT: usize = 0x10;
        const LOOKAT_X: usize = 0x20;
        const LOOKAT_Y: usize = 0x30;
        const VIEWPORT: usize = 0x40;
        let mut bytes = vec![0; ENTRY];
        bytes[VTX..VTX + 16].copy_from_slice(&vertex().to_bytes());
        bytes[LIGHT] = 10;
        bytes[LIGHT + 1] = 20;
        bytes[LIGHT + 2] = 30;
        bytes[LIGHT + 8] = 127;
        bytes[LOOKAT_X + 8] = 127;
        bytes[LOOKAT_Y + 9] = 127;
        for (i, value) in [640i16, 480, 511, 0, 320, 240, 256, 0].iter().enumerate() {
            bytes[VIEWPORT + i * 2..VIEWPORT + i * 2 + 2].copy_from_slice(&value.to_be_bytes());
        }

        let fog = n64_gbi::encode::gsp_fog_position_f3d(900, 1000);
        let result = run(
            bytes,
            &[
                gsp_numlights_f3d(1),
                gsp_light_f3d(0, LIGHT as u32),
                gsp_lightcolor_f3d(0, 0x1122_33FF),
                gsp_lookat_f3d(0, LOOKAT_X as u32),
                gsp_lookat_f3d(1, LOOKAT_Y as u32),
                gsp_viewport_f3d(VIEWPORT as u32),
                fog,
                gsp_set_geometrymode_f3d(G_LIGHTING | G_TEXTURE_GEN),
                gsp_vertex_f3d(0, 1, VTX as u32),
                gsp_enddl_f3d(),
            ],
        );

        assert_eq!(result.scene.light_count, vec![2]);
        assert_eq!(
            result.scene.lights_table[0],
            ([1.0, 0.0, 0.0], [17.0 / 255.0, 34.0 / 255.0, 51.0 / 255.0])
        );
        assert_eq!(
            result.scene.lookat_table[0],
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
        );
        assert_eq!(
            result.scene.viewport_table[1],
            ([160.0, 120.0, 511.0 / 1024.0], [80.0, 60.0, 0.25])
        );
        assert_eq!((result.rdp.fog_mul, result.rdp.fog_offset), (1280, -1024));
    }

    #[test]
    fn force_matrix_reaches_vertex_mvp_and_continuations_are_silent() {
        const MATRIX: usize = 0x00;
        const VTX: usize = 0x80;
        let forced = [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 4.0, 0.0],
            [5.0, 6.0, 7.0, 1.0],
        ];
        let mut bytes = vec![0; ENTRY];
        bytes[MATRIX..MATRIX + 64].copy_from_slice(&mtx_to_bytes(forced));
        bytes[VTX..VTX + 16].copy_from_slice(&vertex().to_bytes());
        let continuation = |selector: u8, addr: u32| {
            (
                ((crate::hle::consts::rsp_f3d::G_MOVEMEM as u32) << 24) | ((selector as u32) << 16),
                addr,
            )
        };
        let result = run(
            bytes,
            &[
                gsp_forcematrix_f3d(MATRIX as u32),
                continuation(G_MV_MATRIX_2, 0x10),
                continuation(G_MV_MATRIX_3, 0x20),
                continuation(G_MV_MATRIX_4, 0x30),
                gsp_vertex_f3d(0, 1, VTX as u32),
                gsp_enddl_f3d(),
            ],
        );

        let mvp_index = result.scene.mtx_index[0] as usize;
        assert_eq!(result.scene.mvp_table[mvp_index], forced);
        assert!(
            result.diags.is_empty(),
            "unexpected diags: {:?}",
            result.diags
        );
    }

    #[test]
    fn no_op_pop_preserves_forced_matrix_at_stack_depth_one() {
        const MATRIX: usize = 0x00;
        const VTX: usize = 0x80;
        let forced = [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 4.0, 0.0],
            [5.0, 6.0, 7.0, 1.0],
        ];
        let mut bytes = vec![0; ENTRY];
        bytes[MATRIX..MATRIX + 64].copy_from_slice(&mtx_to_bytes(forced));
        bytes[VTX..VTX + 16].copy_from_slice(&vertex().to_bytes());
        let result = run(
            bytes,
            &[
                gsp_forcematrix_f3d(MATRIX as u32),
                gsp_popmatrix_f3d(),
                gsp_vertex_f3d(0, 1, VTX as u32),
                gsp_enddl_f3d(),
            ],
        );

        assert_eq!(result.scene.mvp_table.last(), Some(&forced));
        assert_eq!(
            result.scene.mvp_table[result.scene.mtx_index[0] as usize],
            forced
        );
    }

    #[test]
    fn popmtx_only_pops_when_w1_is_zero() {
        const MATRIX_A: usize = 0x00;
        const MATRIX_B: usize = 0x40;
        const VTX_A: usize = 0x80;
        const VTX_B: usize = 0x90;
        let a = [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 2.0, 0.0, 0.0],
            [0.0, 0.0, 2.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let b = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [5.0, 6.0, 7.0, 1.0],
        ];
        let mut bytes = vec![0; ENTRY];
        bytes[MATRIX_A..MATRIX_A + 64].copy_from_slice(&mtx_to_bytes(a));
        bytes[MATRIX_B..MATRIX_B + 64].copy_from_slice(&mtx_to_bytes(b));
        bytes[VTX_A..VTX_A + 16].copy_from_slice(&vertex().to_bytes());
        bytes[VTX_B..VTX_B + 16].copy_from_slice(&vertex().to_bytes());
        let result = run(
            bytes,
            &[
                gsp_matrix_f3d(MATRIX_A as u32, false, true, false),
                gsp_matrix_f3d(MATRIX_B as u32, false, false, true),
                ((G_POPMTX as u32) << 24, 1),
                gsp_vertex_f3d(0, 1, VTX_A as u32),
                ((G_POPMTX as u32) << 24, 0),
                gsp_vertex_f3d(1, 1, VTX_B as u32),
                gsp_enddl_f3d(),
            ],
        );

        let before = result.scene.mtx_index[0] as usize;
        let after = result.scene.mtx_index[1] as usize;
        assert_eq!(result.scene.mvp_table[before], mul4(b, a));
        assert_eq!(result.scene.mvp_table[after], a);
    }

    #[test]
    fn phase4_moveword_points_dispatches_modify_vertex() {
        const VTX: usize = 0x40;
        let mut bytes = vec![0; ENTRY];
        bytes[VTX..VTX + 16].copy_from_slice(&vertex().to_bytes());
        let result = run(
            bytes,
            &[
                gsp_vertex_f3d(0, 1, VTX as u32),
                gsp_modifyvertex_f3d(0, 0x10, 0x1122_3344),
                gsp_enddl_f3d(),
            ],
        );

        assert!(
            result.diags.is_empty(),
            "unexpected diags: {:?}",
            result.diags
        );
        assert_eq!(result.scene.cn, vec![0x4433_2211]);
    }

    #[test]
    fn moveword_points_without_vertex_load_is_ignored() {
        let result = run(
            vec![0; ENTRY],
            &[gsp_modifyvertex_f3d(0, 0x10, 0x1122_3344), gsp_enddl_f3d()],
        );

        assert!(result.scene.raw_pos.is_empty());
        assert!(result.scene.cn.is_empty());
        assert!(result.scene.indices.is_empty());
        assert!(result.scene.draw_runs.is_empty());
    }
}

#[cfg(all(test, feature = "asm"))]
mod phase5_tests {
    use crate::diag::DiagKind;
    use crate::hle::consts::rsp_f3d::{
        G_CULLDL, G_MOVEMEM, G_MOVEWORD, G_MV_MATRIX_2, G_MV_MATRIX_3, G_MV_MATRIX_4, G_MV_TXTATT,
        G_MW_CLIP, G_MW_MATRIX, G_MW_PERSPNORM, G_RDPHALF_1, G_RDPHALF_2, G_RDPNOOP, G_SPNOOP,
        G_SPRITE2D_BASE,
    };
    use crate::hle::consts::{G_RM_OPA_SURF, G_RM_OPA_SURF2};
    use crate::hle::gbi::GbiUcode;
    use crate::hle::interp::{interpret, InterpResult};
    use crate::hle::mem::RdramImage;
    use crate::hle::{Rect, SceneOp};
    use n64_gbi::encode::{
        gdp_fill_rectangle, gdp_load_texture_block, gdp_set_color_image, gdp_set_combine_lerp,
        gdp_set_fill_color, gdp_set_render_mode_f3d, gdp_set_scissor, gsp_1triangle_f3d,
        gsp_branchlist_f3d, gsp_displaylist_f3d, gsp_enddl_f3d, gsp_segment_f3d,
        gsp_texture_rectangle, gsp_vertex_f3d, CcPass, VtxColored, ZERO_A, ZERO_C,
    };

    const ENTRY: usize = 0x80;
    const TARGET: usize = 0x100;
    const VERTEX_ADDR: u32 = 0;

    fn put(bytes: &mut [u8], offset: usize, (w0, w1): (u32, u32)) {
        bytes[offset..offset + 4].copy_from_slice(&w0.to_be_bytes());
        bytes[offset + 4..offset + 8].copy_from_slice(&w1.to_be_bytes());
    }

    fn push(bytes: &mut Vec<u8>, (w0, w1): (u32, u32)) {
        bytes.extend_from_slice(&w0.to_be_bytes());
        bytes.extend_from_slice(&w1.to_be_bytes());
    }

    fn shaded_combine() -> (u32, u32) {
        let color = CcPass {
            a: ZERO_C,
            b: ZERO_C,
            c: ZERO_C,
            d: 4,
        };
        let alpha = CcPass {
            a: ZERO_A,
            b: ZERO_A,
            c: ZERO_A,
            d: 4,
        };
        gdp_set_combine_lerp(color, alpha, color, alpha)
    }

    fn triangle_rdram() -> Vec<u8> {
        let mut bytes = vec![0; TARGET + 0x20];
        let vertices = [
            VtxColored {
                x: -1,
                y: -1,
                z: 0,
                flag: 0,
                s: 0,
                t: 0,
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
            VtxColored {
                x: 1,
                y: -1,
                z: 0,
                flag: 0,
                s: 0,
                t: 0,
                r: 0,
                g: 255,
                b: 0,
                a: 255,
            },
            VtxColored {
                x: 0,
                y: 1,
                z: 0,
                flag: 0,
                s: 0,
                t: 0,
                r: 0,
                g: 0,
                b: 255,
                a: 255,
            },
        ];
        for (index, vertex) in vertices.iter().enumerate() {
            let offset = index * 16;
            bytes[offset..offset + 16].copy_from_slice(&vertex.to_bytes());
        }
        bytes
    }

    fn put_triangle_setup(bytes: &mut [u8]) {
        put(bytes, ENTRY, shaded_combine());
        put(
            bytes,
            ENTRY + 8,
            gdp_set_render_mode_f3d(G_RM_OPA_SURF, G_RM_OPA_SURF2),
        );
        put(bytes, ENTRY + 16, gsp_vertex_f3d(0, 3, VERTEX_ADDR));
    }

    fn run(bytes: &[u8], entry: usize) -> InterpResult {
        interpret(
            RdramImage::new(bytes),
            entry as u64,
            GbiUcode::F3d,
            crate::hle::mem::GbiDataFormat::Fixed,
        )
    }

    #[test]
    fn f3d_displaylist_call_returns_and_continues() {
        let mut bytes = triangle_rdram();
        put_triangle_setup(&mut bytes);
        put(&mut bytes, ENTRY + 24, gsp_displaylist_f3d(TARGET as u32));
        put(&mut bytes, ENTRY + 32, gsp_1triangle_f3d(0, 1, 2));
        put(&mut bytes, ENTRY + 40, gsp_enddl_f3d());
        put(&mut bytes, TARGET, gsp_1triangle_f3d(0, 1, 2));
        put(&mut bytes, TARGET + 8, gsp_enddl_f3d());

        let result = run(&bytes, ENTRY);

        assert_eq!(result.scene.indices, vec![0, 1, 2, 0, 1, 2]);
        assert_eq!(result.commands, 8);
        assert!(
            result.diags.is_empty(),
            "unexpected diags: {:?}",
            result.diags
        );
    }

    #[test]
    fn f3d_branchlist_never_returns_to_caller() {
        let mut bytes = triangle_rdram();
        put_triangle_setup(&mut bytes);
        put(&mut bytes, ENTRY + 24, gsp_branchlist_f3d(TARGET as u32));
        put(&mut bytes, ENTRY + 32, (0xAB00_0000, 0));
        put(&mut bytes, ENTRY + 40, gsp_enddl_f3d());
        put(&mut bytes, TARGET, gsp_1triangle_f3d(0, 1, 2));
        put(&mut bytes, TARGET + 8, gsp_enddl_f3d());

        let result = run(&bytes, ENTRY);

        assert_eq!(result.scene.indices, vec![0, 1, 2]);
        assert_eq!(result.commands, 6);
        assert!(result
            .diags
            .iter()
            .all(|diag| diag.at != (ENTRY + 32) as u64));
        assert!(
            result.diags.is_empty(),
            "unexpected diags: {:?}",
            result.diags
        );
    }

    #[test]
    fn shared_2d_commands_record_framebuffer_pair_under_f3d() {
        let mut bytes = vec![0xFF; ENTRY];
        for command in gdp_load_texture_block(0, 2, 4, 4, 0, 2, 2, 2, 2) {
            push(&mut bytes, command);
        }
        push(&mut bytes, gdp_set_color_image(0, 2, 64, 0x0001_0000));
        push(&mut bytes, gdp_set_scissor(0, 0, 0, 64 * 4, 64 * 4));
        push(&mut bytes, gdp_set_fill_color(0xCAFE_F00D));
        push(&mut bytes, gdp_fill_rectangle(4 * 4, 6 * 4, 20 * 4, 22 * 4));
        let mut texrect =
            gsp_texture_rectangle(8 * 4, 12 * 4, 31 * 4, 35 * 4, 0, 11, 13, 1024, 512, false);
        texrect[1].0 = (G_RDPHALF_1 as u32) << 24;
        texrect[2].0 = (G_RDPHALF_2 as u32) << 24;
        for command in texrect {
            push(&mut bytes, command);
        }
        push(&mut bytes, gsp_enddl_f3d());

        let result = run(&bytes, ENTRY);

        assert_eq!(result.scene.framebuffer_pairs.len(), 1);
        let pair = &result.scene.framebuffer_pairs[0];
        assert_eq!(pair.color_image.width, 64);
        assert_eq!(pair.color_image.addr, 0x0001_0000);
        match pair.ops.as_slice() {
            [SceneOp::FillRect { rect, color_raw }, SceneOp::TexRect {
                rect: tex_rect,
                uls,
                ult,
                dsdx,
                dtdy,
                flip,
                copy_mode,
                fb_source,
                ..
            }] => {
                assert_eq!(
                    *rect,
                    Rect {
                        ulx: 4,
                        uly: 6,
                        lrx: 20,
                        lry: 22,
                    }
                );
                assert_eq!(*color_raw, 0xCAFE_F00D);
                assert_eq!(
                    *tex_rect,
                    Rect {
                        ulx: 8,
                        uly: 12,
                        lrx: 31,
                        lry: 35,
                    }
                );
                assert_eq!((*uls, *ult, *dsdx, *dtdy), (11, 13, 1024, 512));
                assert!(!flip);
                assert!(!copy_mode);
                assert_eq!(*fb_source, None);
            }
            other => panic!("expected ordered FILLRECT + TEXRECT operations, got {other:?}"),
        }
        assert!(
            result.diags.is_empty(),
            "unexpected diags: {:?}",
            result.diags
        );
    }

    #[test]
    fn realistic_f3d_stub_mix_has_no_structural_diagnostics() {
        let opcode = |op: u8| ((op as u32) << 24, 0);
        let moveword = |selector: u8| (((G_MOVEWORD as u32) << 24) | selector as u32, 0);
        let movemem = |selector: u8| (((G_MOVEMEM as u32) << 24) | ((selector as u32) << 16), 0);
        let commands = vec![
            opcode(G_SPNOOP),
            gsp_segment_f3d(1, 0),
            opcode(G_CULLDL),
            opcode(G_SPRITE2D_BASE),
            opcode(G_RDPHALF_1),
            opcode(G_RDPHALF_2),
            opcode(G_RDPNOOP),
            moveword(G_MW_MATRIX),
            moveword(G_MW_CLIP),
            moveword(G_MW_PERSPNORM),
            movemem(G_MV_MATRIX_2),
            movemem(G_MV_MATRIX_3),
            movemem(G_MV_MATRIX_4),
            movemem(G_MV_TXTATT),
            gsp_enddl_f3d(),
        ];
        let mut bytes = Vec::new();
        for &command in &commands {
            push(&mut bytes, command);
        }

        let result = run(&bytes, 0);
        let structural_diags: Vec<_> = result
            .diags
            .iter()
            .filter(|diag| {
                matches!(
                    diag.kind,
                    DiagKind::UnknownOpcode(_)
                        | DiagKind::UnhandledMovemem(_)
                        | DiagKind::UnhandledMoveword(_)
                )
            })
            .collect();

        assert!(
            structural_diags.is_empty(),
            "stub mix emitted structural diagnostics: {structural_diags:?}"
        );
        assert_eq!(result.commands as usize, commands.len());
    }
}

#[cfg(all(test, feature = "asm"))]
mod phase6_tests {
    use crate::diag::DiagKind;
    use crate::hle::consts::rsp_f3d::{
        G_CULL_BACK, G_LIGHTING, G_SHADE, G_SHADING_SMOOTH, G_TEXTURE_GEN, G_TEXTURE_GEN_LINEAR,
    };
    use crate::hle::consts::{G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2};
    use crate::hle::gbi::GbiUcode;
    use crate::hle::interp::interpret;
    use crate::hle::mem::RdramImage;
    use n64_gbi::encode::{
        gdp_load_texture_block, gdp_set_combine_lerp, gdp_set_cycle_type_f3d,
        gdp_set_render_mode_f3d, gsp_1triangle_f3d, gsp_clear_geometrymode_f3d, gsp_enddl_f3d,
        gsp_matrix_f3d, gsp_set_geometrymode_f3d, gsp_texture_f3d, gsp_vertex_f3d, mtx_to_bytes,
        CcPass, VtxColored, ZERO_A, ZERO_C,
    };

    const MATRIX_ADDR: usize = 0x000;
    const VERTEX_ADDR: usize = 0x040;
    const TEXTURE_ADDR: usize = 0x080;
    const DL_ENTRY: usize = 0x100;
    const EXPECTED_TRIANGLES: usize = 2;

    /// Raw big-endian Gfx words for a small SM64-style static F3D display list.
    const SM64_STYLE_F3D_DL: &[u8] = &[
        0x01, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // gsSPMatrix
        0xB6, 0x00, 0x00, 0x00, 0x00, 0x0E, 0x00, 0x00, // gsSPClearGeometryMode
        0xB7, 0x00, 0x00, 0x00, 0x00, 0x00, 0x22, 0x04, // gsSPSetGeometryMode
        0xBA, 0x00, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, // one-cycle mode
        0xB9, 0x00, 0x03, 0x1D, 0x00, 0x50, 0x20, 0x78, // opaque render mode
        0xFD, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, // gsDPSetTextureImage
        0xF5, 0x10, 0x00, 0x00, 0x07, 0x08, 0x82, 0x20, // load tile
        0xE6, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // gsDPLoadSync
        0xF3, 0x00, 0x00, 0x00, 0x07, 0x00, 0xF8, 0x00, // gsDPLoadBlock
        0xE7, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // gsDPPipeSync
        0xF5, 0x10, 0x02, 0x00, 0x00, 0x08, 0x82, 0x20, // render tile
        0xF2, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x0C, // gsDPSetTileSize
        0xBB, 0x00, 0x00, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, // gsSPTexture
        0xFC, 0x12, 0x7E, 0x24, 0xFF, 0xFF, 0xF9, 0xFC, // gsDPSetCombine
        0x04, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, // gsSPVertex
        0xBF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0A, 0x14, // gsSP1Triangle
        0xBF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x14, 0x1E, // gsSP1Triangle
        0xB8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // gsSPEndDisplayList
    ];

    fn textured_combine() -> (u32, u32) {
        let color = CcPass {
            a: 1,
            b: ZERO_C,
            c: 4,
            d: ZERO_C,
        };
        let alpha = CcPass {
            a: ZERO_A,
            b: ZERO_A,
            c: ZERO_A,
            d: 4,
        };
        gdp_set_combine_lerp(color, alpha, color, alpha)
    }

    fn encoded_fixture() -> Vec<u8> {
        let clear = G_LIGHTING | G_TEXTURE_GEN | G_TEXTURE_GEN_LINEAR;
        let set = G_SHADE | G_SHADING_SMOOTH | G_CULL_BACK;
        let mut commands = vec![
            gsp_matrix_f3d(MATRIX_ADDR as u32, true, true, false),
            gsp_clear_geometrymode_f3d(clear),
            gsp_set_geometrymode_f3d(set),
            gdp_set_cycle_type_f3d(0),
            gdp_set_render_mode_f3d(G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2),
        ];
        commands.extend(gdp_load_texture_block(
            0,
            2,
            4,
            4,
            TEXTURE_ADDR as u32,
            2,
            2,
            2,
            2,
        ));
        commands.extend([
            gsp_texture_f3d(u16::MAX, u16::MAX, 0, 0, true),
            textured_combine(),
            gsp_vertex_f3d(0, 4, VERTEX_ADDR as u32),
            gsp_1triangle_f3d(0, 1, 2),
            gsp_1triangle_f3d(0, 2, 3),
            gsp_enddl_f3d(),
        ]);

        commands
            .into_iter()
            .flat_map(|(w0, w1)| w0.to_be_bytes().into_iter().chain(w1.to_be_bytes()))
            .collect()
    }

    fn fixture_rdram() -> Vec<u8> {
        let mut bytes = vec![0; DL_ENTRY];
        bytes[MATRIX_ADDR..MATRIX_ADDR + 64].copy_from_slice(&mtx_to_bytes([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]));
        let vertices = [
            VtxColored {
                x: -16,
                y: -16,
                z: 0,
                flag: 0,
                s: 0,
                t: 0,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            VtxColored {
                x: 16,
                y: -16,
                z: 0,
                flag: 0,
                s: 96,
                t: 0,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            VtxColored {
                x: 16,
                y: 16,
                z: 0,
                flag: 0,
                s: 96,
                t: 96,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            VtxColored {
                x: -16,
                y: 16,
                z: 0,
                flag: 0,
                s: 0,
                t: 96,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        ];
        for (index, vertex) in vertices.into_iter().enumerate() {
            let offset = VERTEX_ADDR + index * 16;
            bytes[offset..offset + 16].copy_from_slice(&vertex.to_bytes());
        }
        bytes[TEXTURE_ADDR..TEXTURE_ADDR + 32].fill(0xFF);
        bytes.extend_from_slice(SM64_STYLE_F3D_DL);
        bytes
    }

    #[test]
    fn sm64_style_f3d_fixture_walks_without_structural_diagnostics() {
        let encoded = encoded_fixture();
        assert_eq!(
            SM64_STYLE_F3D_DL, encoded,
            "committed fixture bytes must match the F3D/RDP encoders"
        );

        let bytes = fixture_rdram();
        let result = interpret(
            RdramImage::new(&bytes),
            DL_ENTRY as u64,
            GbiUcode::F3d,
            crate::hle::mem::GbiDataFormat::Fixed,
        );
        let structural_diags: Vec<_> = result
            .diags
            .iter()
            .filter(|diag| {
                matches!(
                    diag.kind,
                    DiagKind::UnknownOpcode(_)
                        | DiagKind::UnhandledMovemem(_)
                        | DiagKind::UnhandledMoveword(_)
                )
            })
            .collect();

        assert!(
            structural_diags.is_empty(),
            "fixture emitted structural diagnostics: {structural_diags:?}"
        );
        assert_eq!(result.scene.indices.len() / 3, EXPECTED_TRIANGLES);
        assert!(!result.scene.draw_runs.is_empty());
        assert!(result.scene.materials.iter().any(|mat| mat.tex_enable));
    }

    fn env_hex_u32(name: &str) -> u32 {
        let value = std::env::var(name).unwrap_or_else(|_| "0".to_owned());
        let digits = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .unwrap_or(&value);
        u32::from_str_radix(digits, 16)
            .unwrap_or_else(|error| panic!("{name} must be a hexadecimal byte offset: {error}"))
    }

    /// Walks the US SM64 Bob-omb Battlefield display list `bob_seg7_dl_07004390`.
    ///
    /// `RdramImage` does not emulate PI DMA or MIO0 decompression. This harness therefore assumes
    /// `FAST3D_SM64_ROM` names a prepared big-endian US image in which decompressed BOB segment 7
    /// is directly addressable. `FAST3D_SM64_SEGMENT_07` is that segment's hexadecimal byte base
    /// (default `0`); an untouched `sm64.us.z64` does not satisfy this assumption. The segmented
    /// entry `0x07004390` is documented by the SM64 decomp symbol named above rather than inferred
    /// from the input bytes.
    ///
    /// Run with:
    /// `FAST3D_SM64_ROM=/path/to/prepared-sm64.us.z64 FAST3D_SM64_SEGMENT_07=0xBASE cargo test -p fast3d sm64_us_bob_rom_display_list -- --ignored`
    #[test]
    #[ignore = "requires a non-redistributable, prepared US SM64 image"]
    fn sm64_us_bob_rom_display_list() {
        const BOB_DL_ENTRY: u32 = 0x0700_4390;

        let Some(path) = std::env::var_os("FAST3D_SM64_ROM") else {
            eprintln!("FAST3D_SM64_ROM is unset; skipping opt-in ROM validation");
            return;
        };
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("failed to read FAST3D_SM64_ROM at {path:?}: {error}"));
        let mut mem = RdramImage::new(&bytes);
        mem.set_segment(7, env_hex_u32("FAST3D_SM64_SEGMENT_07"));
        let entry = mem.from_segmented_masked(BOB_DL_ENTRY);
        assert!(
            entry as usize + 8 <= bytes.len(),
            "resolved BOB DL entry {entry:#010X} lies outside the prepared image"
        );

        let result = interpret(
            mem,
            entry as u64,
            GbiUcode::F3d,
            crate::hle::mem::GbiDataFormat::Fixed,
        );
        let unknown_diags: Vec<_> = result
            .diags
            .iter()
            .filter(|diag| matches!(diag.kind, DiagKind::UnknownOpcode(_)))
            .collect();
        let triangles = result.scene.indices.len() / 3;

        assert!(
            unknown_diags.is_empty(),
            "shipping F3D display list emitted unknown opcodes: {unknown_diags:?}"
        );
        assert!(
            (1..=100_000).contains(&triangles),
            "expected a plausible non-zero triangle count, got {triangles}"
        );
    }
}
