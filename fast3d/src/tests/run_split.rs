/// §11 run-split unit tests: A6 snapshot_run dedup / None-drop policy.
///
/// Each test builds a minimal DL with `crate::asm::encode` + `crate::hle::interpret_rdram`, then asserts
/// exact `draw_runs.len()` / `materials.len()` / `render_modes.len()`.
use crate::asm::encode::{
    gdp_set_combine_lerp, gdp_set_cycle_type, gdp_set_prim_color, gdp_set_render_mode,
    gsp_1triangle, gsp_clear_geometrymode, gsp_enddl, gsp_set_geometrymode, gsp_texture,
    gsp_vertex, gsp_viewport, CcPass, ZERO_A, ZERO_C,
};
use crate::hle::interpret_rdram;

/// Build a minimal RDRAM prelude: viewport at offset 0, 3 white verts, SHADE combiner,
/// G_RM_OPA_SURF render mode.  Returns (rdram, cmd_list).
fn prelude() -> (Vec<u8>, Vec<(u32, u32)>) {
    let mut rdram = Vec::new();
    // Viewport @0 (16 bytes: 8 i16 values, big-endian).
    for v in [640i16, 480, 511, 0, 640, 480, 511, 0] {
        rdram.extend_from_slice(&v.to_be_bytes());
    }
    let vp = 0u32;
    let vtx = rdram.len() as u32; // RDRAM offset of vertex data
    for _ in 0..3 {
        // 16-byte vertex: pos 0, color white
        rdram.extend_from_slice(&[0u8; 12]);
        rdram.extend_from_slice(&[255, 255, 255, 255]);
    }
    let mut cmds: Vec<(u32, u32)> = Vec::new();
    cmds.push(gsp_viewport(vp));
    cmds.push(gdp_set_render_mode(
        crate::hle::consts::G_RM_OPA_SURF,
        crate::hle::consts::G_RM_OPA_SURF2,
    ));
    cmds.push(gdp_set_cycle_type(0)); // 1-cycle
                                      // SHADE-only combiner (textureless): rgb = SHADE pass, alpha = SHADE pass.
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
    }; // d=4 → Shade
    cmds.push(gdp_set_combine_lerp(cc, ca, cc, ca));
    cmds.push(gsp_vertex(0, 3, vtx)); // dst=0, count=3, addr=vtx
    (rdram, cmds)
}

/// Append G_ENDDL, serialize all commands to bytes, append to rdram, run the interpreter.
fn finish(rdram: Vec<u8>, mut cmds: Vec<(u32, u32)>) -> crate::hle::InterpResult {
    cmds.push(gsp_enddl());
    let entry = rdram.len() as u32; // commands start here
    let mut all = rdram;
    for (w0, w1) in &cmds {
        all.extend_from_slice(&w0.to_be_bytes());
        all.extend_from_slice(&w1.to_be_bytes());
    }
    interpret_rdram(&all, entry)
}

// ---------------------------------------------------------------------------
// Test 1: two distinct combiners → 2 runs, 2 materials
// ---------------------------------------------------------------------------
#[test]
fn two_distinct_combiners_two_runs_two_materials() {
    let (rdram, mut cmds) = prelude();
    cmds.push(gsp_1triangle(0, 1, 2));
    // Change combiner → different material → new run.
    let cc = CcPass {
        a: 3,
        b: ZERO_C,
        c: ZERO_C,
        d: ZERO_C,
    }; // a=3 → PRIMITIVE
    let ca = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 3,
    }; // d=3 → PRIMITIVE
    cmds.push(gdp_set_prim_color(0, 0, 0xFF00_00FF));
    cmds.push(gdp_set_combine_lerp(cc, ca, cc, ca));
    cmds.push(gsp_1triangle(0, 1, 2));
    let r = finish(rdram, cmds);
    assert!(r.diags.is_empty(), "unexpected diags: {:?}", r.diags);
    assert_eq!(r.scene.draw_runs.len(), 2);
    assert_eq!(r.scene.materials.len(), 2);
}

// ---------------------------------------------------------------------------
// Test 2: redundant identical SetCombine → 1 coalesced run
// ---------------------------------------------------------------------------
#[test]
fn redundant_setcombine_coalesces_to_one_run() {
    let (rdram, mut cmds) = prelude();
    cmds.push(gsp_1triangle(0, 1, 2));
    // Re-issue the SAME combiner → material deduped → run extends (not split).
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
    cmds.push(gdp_set_combine_lerp(cc, ca, cc, ca));
    cmds.push(gsp_1triangle(0, 1, 2));
    let r = finish(rdram, cmds);
    assert_eq!(
        r.scene.draw_runs.len(),
        1,
        "identical material must coalesce"
    );
    assert_eq!(r.scene.materials.len(), 1);
}

// ---------------------------------------------------------------------------
// Test 3: render-mode change, same material → 2 runs, 1 material, 2 render modes
// ---------------------------------------------------------------------------
#[test]
fn render_mode_change_same_material_two_runs() {
    let (rdram, mut cmds) = prelude();
    cmds.push(gsp_1triangle(0, 1, 2));
    cmds.push(gdp_set_render_mode(
        crate::hle::consts::G_RM_AA_ZB_XLU_SURF,
        crate::hle::consts::G_RM_AA_ZB_XLU_SURF2,
    ));
    cmds.push(gsp_1triangle(0, 1, 2));
    let r = finish(rdram, cmds);
    assert_eq!(r.scene.draw_runs.len(), 2);
    assert_eq!(r.scene.materials.len(), 1, "material unchanged → deduped");
    assert_eq!(r.scene.render_modes.len(), 2);
}

// ---------------------------------------------------------------------------
// Test 4: None material → run dropped, sibling run still renders  [IMP15-adjacent]
// ---------------------------------------------------------------------------
#[test]
fn none_material_run_dropped_siblings_render() {
    // First run: TEXEL0 combiner with NO texture loaded → build_material None → dropped.
    let (rdram, mut cmds) = prelude();
    let cc = CcPass {
        a: 1,
        b: ZERO_C,
        c: 4,
        d: ZERO_C,
    }; // a=1→TEXEL0, c=4→SHADE
    let ca = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 4,
    };
    cmds.push(gdp_set_combine_lerp(cc, ca, cc, ca));
    cmds.push(gsp_texture(0xFFFF, 0xFFFF, 0, 0, true));
    cmds.push(gsp_1triangle(0, 1, 2)); // dropped (diag pushed by build_material)
                                       // Second run: textureless SHADE combiner → renders.
    let cc2 = CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: 4,
    };
    cmds.push(gdp_set_combine_lerp(cc2, ca, cc2, ca));
    cmds.push(gsp_texture(0, 0, 0, 0, false));
    cmds.push(gsp_1triangle(0, 1, 2)); // renders
    let r = finish(rdram, cmds);
    assert!(!r.diags.is_empty(), "expected a None diag for the bad run");
    assert_eq!(r.scene.draw_runs.len(), 1, "only the sibling run survives");
}

// ---------------------------------------------------------------------------
// Test 5: interleaved cull + material changes split on (cull, mi, rmi) key  [IMP15]
// ---------------------------------------------------------------------------
#[test]
fn interleaved_cull_and_material_changes_split_on_combined_key() {
    // §11(d) [IMP15]: the run key is (cull, material_index, render_mode_index).
    // Interleave a cull toggle and a combiner change across 4 triangles and assert
    // exact split boundaries.
    let (rdram, mut cmds) = prelude();
    let cc_prim = CcPass {
        a: 3,
        b: ZERO_C,
        c: ZERO_C,
        d: ZERO_C,
    }; // a=3→PRIMITIVE
    let ca = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 4,
    };
    // Tri 1: SHADE, no cull → run 1.
    cmds.push(gsp_1triangle(0, 1, 2));
    // Tri 2: cull change only (same material) → split on cull → run 2.
    cmds.push(gsp_set_geometrymode(crate::hle::consts::G_CULL_BACK));
    cmds.push(gsp_1triangle(0, 1, 2));
    // Tri 3: material change while still culling → split on material → run 3.
    cmds.push(gdp_set_prim_color(0, 0, 0xFF00_00FF));
    cmds.push(gdp_set_combine_lerp(cc_prim, ca, cc_prim, ca));
    cmds.push(gsp_1triangle(0, 1, 2));
    // Tri 4: cull off, same (prim) material → split back on cull → run 4.
    cmds.push(gsp_clear_geometrymode(crate::hle::consts::G_CULL_BACK));
    cmds.push(gsp_1triangle(0, 1, 2));
    let r = finish(rdram, cmds);
    assert!(r.diags.is_empty(), "unexpected diags: {:?}", r.diags);
    assert_eq!(
        r.scene.draw_runs.len(),
        4,
        "each (cull,material) transition opens a new run"
    );
    assert_eq!(
        r.scene.materials.len(),
        2,
        "only two distinct materials (shade, prim)"
    );
}
