use crate::hle::interpret_rdram;

/// Build a textured-quad DL that uses TEXEL1 in color-C (an unwired selector).
/// The combine words (0xFC00_0002, 0x001C_0000) set color-C cycle1 = TEXEL1 (index 2)
/// and alpha-C to ZERO (H[18,3]=7), so only color-C is unwired.
/// This builds a minimal RDRAM + command stream that loads a texture (populates TMEM),
/// draws one triangle, and refuses to produce a Material due to the unwired selector.
/// The diagnostic is emitted by `build_material` inside `snapshot_run` at triangle-time
/// (A7: post-walk `build_material` was removed; diagnostic now requires a triangle).
fn build_textured_quad_dl_with_texel1() -> (Vec<u8>, u32) {
    use n64_gbi::encode::*;

    let mut rdram: Vec<u8> = Vec::new();

    // 3 zero-position vertices (16 bytes each: 12 zeros + RGBA 255,255,255,255).
    let vtx_addr = rdram.len() as u32;
    for _ in 0..3 {
        rdram.extend_from_slice(&[0u8; 12]);
        rdram.extend_from_slice(&[255u8, 255, 255, 255]);
    }

    // Minimal RDRAM: texture data (32x32 RGBA16 all-white)
    while !rdram.len().is_multiple_of(8) {
        rdram.push(0);
    }
    let tex_addr = rdram.len() as u32;
    for _ in 0..(32 * 32) {
        rdram.extend_from_slice(&[0xFF, 0xFF]);
    }

    let mut cmds: Vec<u8> = Vec::new();
    let push = |cmds: &mut Vec<u8>, (w0, w1): (u32, u32)| {
        cmds.extend_from_slice(&w0.to_be_bytes());
        cmds.extend_from_slice(&w1.to_be_bytes());
    };

    push(&mut cmds, gdp_set_cycle_type(0));
    // TEXEL1 in color-C cycle1: (0xFC00_0002, 0x001C_0000)
    // (color-C cycle1 = L[0,5] = 2 = TEXEL1; H[18,3]=7 -> alpha-C = ZERO, not LOD_FRACTION)
    push(&mut cmds, (0xFC00_0002u32, 0x001C_0000u32));
    push(&mut cmds, gdp_set_prim_color(0, 0, 0xFFFF_FFFF));
    push(&mut cmds, gdp_set_env_color(0x0000_00FF));
    for cmd in gdp_load_texture_block(0, 2, 32, 32, tex_addr, 2, 5, 2, 5) {
        push(&mut cmds, cmd);
    }
    push(&mut cmds, gsp_texture(0xFFFF, 0xFFFF, 0, 0, true));
    // Draw a triangle so snapshot_run -> build_material is called (and emits the diag).
    push(&mut cmds, gsp_vertex(0, 3, vtx_addr));
    push(&mut cmds, gsp_1triangle(0, 1, 2));
    push(&mut cmds, gsp_enddl());

    while !rdram.len().is_multiple_of(8) {
        rdram.push(0);
    }
    let entry = rdram.len() as u32;
    rdram.extend_from_slice(&cmds);
    (rdram, entry)
}

#[test]
fn unwired_selector_yields_diag_and_no_material() {
    let (rdram, entry) = build_textured_quad_dl_with_texel1();
    let res = crate::hle::interpret_rdram(&rdram, entry);
    assert!(
        res.diags
            .iter()
            .any(|d| d.kind.to_string().contains("not implemented")),
        "expected 'not implemented' diagnostic, got: {:?}",
        res.diags
    );
    assert!(
        res.scene.materials.is_empty(),
        "expected no material when combiner has unwired selector"
    );
}

#[test]
fn enddl_stops_the_loop() {
    let mut cmds = Vec::new();
    cmds.extend_from_slice(&(0xDFu32 << 24).to_be_bytes()); // G_ENDDL at byte 0
    cmds.extend_from_slice(&0u32.to_be_bytes());
    cmds.extend_from_slice(&(0xFFu32 << 24).to_be_bytes()); // never reached
    cmds.extend_from_slice(&0u32.to_be_bytes());
    let r = interpret_rdram(&cmds, 0);
    assert!(r.diags.is_empty());
    assert!(r.scene.raw_pos.is_empty());
}

#[test]
fn unknown_opcode_is_diagnosed_by_byte_address() {
    let mut cmds = Vec::new();
    cmds.extend_from_slice(&(0xABu32 << 24).to_be_bytes()); // unknown, at byte 0
    cmds.extend_from_slice(&0u32.to_be_bytes());
    cmds.extend_from_slice(&(0xDFu32 << 24).to_be_bytes());
    cmds.extend_from_slice(&0u32.to_be_bytes());
    let r = interpret_rdram(&cmds, 0);
    assert_eq!(r.diags.len(), 1);
    assert_eq!(r.diags[0].at, 0); // byte address 0 == first command
    assert!(r.diags[0].kind.to_string().contains("0xAB"));
}

#[test]
fn segment_store_then_resolve_masked_and_unmasked() {
    use crate::hle::mem::RdramImage;
    let bytes = vec![0u8; 0x40];
    let mut rd = RdramImage::new(&bytes);
    rd.set_segment(3, 0x40);
    // UNMASKED (SETTIMG path): segments[3] + 0 = 0x40.
    assert_eq!(rd.from_segmented(0x0300_0000).unwrap(), 0x40);
    // segments[7] = 0x05; probe with offset 3: unmasked = 0x08, masked = 0x08 (8-aligned).
    rd.set_segment(7, 0x05);
    assert_eq!(rd.from_segmented(0x0700_0003).unwrap(), 0x08);
    // Plan specified 0x00 (arithmetic error: 0x08 & 0x00FFFFF8 = 0x08, not 0x00).
    assert_eq!(rd.from_segmented_masked(0x0700_0003).unwrap(), 0x08);
    // Segment 0 zero-init is an identity map (preserves the existing sample).
    assert_eq!(rd.from_segmented(0x0000_0040).unwrap(), 0x40);
    assert_eq!(rd.from_segmented_masked(0x0000_0040).unwrap(), 0x40);
}

#[test]
fn move_word_segment_sets_base_no_diag() {
    use n64_gbi::encode::{gsp_enddl, gsp_segment};
    let mut rdram = vec![0u8; 0x40];
    let (sw0, sw1) = gsp_segment(3, 0x0000_0008);
    rdram[0..4].copy_from_slice(&sw0.to_be_bytes());
    rdram[4..8].copy_from_slice(&sw1.to_be_bytes());
    let (ew0, ew1) = gsp_enddl();
    rdram[8..12].copy_from_slice(&ew0.to_be_bytes());
    rdram[12..16].copy_from_slice(&ew1.to_be_bytes());
    let r = interpret_rdram(&rdram, 0);
    assert!(
        r.diags
            .iter()
            .all(|d| !d.kind.to_string().contains("G_MOVEWORD")),
        "unexpected MOVEWORD diag: {:?}",
        r.diags
    );
}

#[test]
fn move_word_non_segment_type_is_diagnosed() {
    // G_MOVEWORD with an unknown type -> "unhandled G_MOVEWORD type" diag.
    // type 0x10 is not any known G_MW_* constant (not SEGMENT/NUMLIGHT/PERSPNORM).
    let mut rdram = vec![0u8; 16];
    let w0 = ((0xDBu32) << 24) | (0x10u32 << 16);
    rdram[0..4].copy_from_slice(&w0.to_be_bytes());
    let (ew0, ew1) = n64_gbi::encode::gsp_enddl();
    rdram[8..12].copy_from_slice(&ew0.to_be_bytes());
    rdram[12..16].copy_from_slice(&ew1.to_be_bytes());
    let r = interpret_rdram(&rdram, 0);
    assert!(
        r.diags
            .iter()
            .any(|d| d.kind.to_string().contains("unhandled G_MOVEWORD type")),
        "expected unhandled-MOVEWORD-type diag: {:?}",
        r.diags
    );
}
