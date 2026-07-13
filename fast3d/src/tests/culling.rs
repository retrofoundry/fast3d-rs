use crate::asm::encode::{
    gdp_set_combine_lerp, gdp_set_cycle_type, gdp_set_render_mode, gsp_1triangle, gsp_enddl,
    gsp_set_geometrymode, gsp_vertex, CcPass, Vp, VtxColored, ZERO_A, ZERO_C,
};
use crate::hle::consts::{G_RM_OPA_SURF, G_RM_OPA_SURF2};
use crate::hle::{interpret_rdram, CullKind, DrawRun};

fn put(rdram: &mut [u8], off: usize, w0: u32, w1: u32) {
    rdram[off..off + 4].copy_from_slice(&w0.to_be_bytes());
    rdram[off + 4..off + 8].copy_from_slice(&w1.to_be_bytes());
}

/// Build a DL: set viewport, set geometry mode `geom`, load 3 verts (0,0)/(20,0)/(0,-20),
/// draw one triangle with winding (i0,i1,i2). Returns the resulting scene.
fn run_tri(geom: u32, i0: u8, i1: u8, i2: u8) -> crate::hle::Scene {
    let mut rdram = vec![0u8; 0x100];
    let vp = Vp {
        vscale: [320, 240, 511, 0],
        vtrans: [320, 240, 511, 0],
    };
    rdram[0x00..0x10].copy_from_slice(&vp.to_bytes());
    let verts = [(0i16, 0i16), (20, 0), (0, -20)];
    for (k, (x, y)) in verts.iter().enumerate() {
        let v = VtxColored {
            x: *x,
            y: *y,
            z: 0,
            flag: 0,
            s: 0,
            t: 0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        };
        let o = 0x10 + k * 16;
        rdram[o..o + 16].copy_from_slice(&v.to_bytes());
    }
    let mut pc = 0x40;
    let mut emit = |rdram: &mut Vec<u8>, w: (u32, u32)| {
        put(rdram, pc, w.0, w.1);
        pc += 8;
    };
    // SHADE-only combiner (textureless): required by snapshot_run to produce a material.
    let cc = CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: 4,
    }; // d=4 → Shade
    let ca = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 4,
    };
    emit(&mut rdram, crate::asm::encode::gsp_viewport(0x00));
    emit(&mut rdram, gsp_set_geometrymode(geom));
    emit(&mut rdram, gdp_set_cycle_type(0)); // 1-cycle
    emit(
        &mut rdram,
        gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
    );
    emit(&mut rdram, gdp_set_combine_lerp(cc, ca, cc, ca));
    emit(&mut rdram, gsp_vertex(0, 3, 0x10));
    emit(&mut rdram, gsp_1triangle(i0, i1, i2));
    emit(&mut rdram, gsp_enddl());
    let r = interpret_rdram(&rdram, 0x40);
    assert!(r.diags.is_empty(), "unexpected diag: {:?}", r.diags);
    r.scene
}

#[test]
fn cull_back_marks_cull_run_no_swap() {
    let s = run_tri(crate::hle::consts::G_CULL_BACK, 0, 1, 2);
    assert_eq!(
        s.draw_runs,
        vec![DrawRun {
            material_index: 0,
            render_mode_index: 0,
            cull: CullKind::Cull,
            index_count: 3,
            index_start: 0,
        }]
    );
    assert_eq!(s.indices, vec![0, 1, 2]); // back-cull does not swap
}

#[test]
fn cull_front_marks_cull_run_and_swaps() {
    let s = run_tri(crate::hle::consts::G_CULL_FRONT, 0, 1, 2);
    assert_eq!(
        s.draw_runs,
        vec![DrawRun {
            material_index: 0,
            render_mode_index: 0,
            cull: CullKind::Cull,
            index_count: 3,
            index_start: 0,
        }]
    );
    assert_eq!(s.indices, vec![2, 1, 0]); // front-cull swaps a<->c
}

#[test]
fn cull_both_draws_nothing() {
    let s = run_tri(crate::hle::consts::G_CULL_BOTH, 0, 1, 2);
    assert!(s.indices.is_empty());
    assert!(s.draw_runs.is_empty());
}

#[test]
fn no_cull_bits_records_none_run() {
    let s = run_tri(0, 0, 1, 2);
    assert_eq!(
        s.draw_runs,
        vec![DrawRun {
            material_index: 0,
            render_mode_index: 0,
            cull: CullKind::None,
            index_count: 3,
            index_start: 0,
        }]
    );
    assert_eq!(s.indices, vec![0, 1, 2]);
}

// Real-toy lock for spec Testing item 2 part 1: the two culling toys each emit one Cull run.
#[test]
fn cull_toys_emit_single_cull_run() {
    let toys = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/toys");
    let white = vec![255u8; 32 * 32 * 4];
    for name in ["backface-culling.n64", "segmented-sub-dl.n64"] {
        let src = std::fs::read_to_string(toys.join(name)).unwrap();
        let img = crate::asm::assemble_with_texture(&src, &white, 32, 32).unwrap();
        let r = interpret_rdram(&img.rdram, img.entry_addr);
        assert!(r.diags.is_empty(), "{name}: {:?}", r.diags);
        assert_eq!(r.scene.draw_runs.len(), 1, "{name}: expected one run");
        assert_eq!(r.scene.draw_runs[0].cull, CullKind::Cull, "{name}");
        assert_eq!(
            r.scene.draw_runs[0].index_count as usize,
            r.scene.indices.len(),
            "{name}"
        );
    }
}
