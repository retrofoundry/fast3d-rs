use crate::asm::asm::{
    assemble, assemble_with_texture, encode_ci8, encode_i4_pair, encode_i8_texel,
    encode_ia16_texel, encode_ia4_nibble, encode_ia4_pair, encode_ia8_texel, encode_rgba16_texel,
    Image,
};

#[test]
fn parser_accepts_gsdp_set_render_mode_preset() {
    let img = crate::asm::assemble(
        "gsDPSetRenderMode(G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2)\ngsSPEndDisplayList()",
    )
    .expect("assemble");
    // first command opcode byte is G_SETOTHERMODE_L (0xE2).
    assert_eq!(img.rdram[img.entry_addr as usize], 0xE2);
}

const SRC: &str = "\
Mtx p = scale(0.015625)
Mtx m = identity()
Vp { 640, 480, 511, 511, 320, 240, 0, 511 }
Vtx { -48, -48, 0, 0, 0, 0, 255, 0, 0, 255 }
Vtx {  48, -48, 0, 0, 0, 0, 0, 255, 0, 255 }
Vtx {   0,  48, 0, 0, 0, 0, 0, 0, 255, 255 }
gsSPMatrix(p, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(m, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPClearGeometryMode(G_LIGHTING, G_CULL_BACK)
gsSPSetGeometryMode(G_SHADE, G_SHADING_SMOOTH)
gsSPVertex(verts, 3, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSPEndDisplayList()
";

const SOURCE_MAP_SRC: &str = "\
Mtx p = scale(0.015625)
Mtx m = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0, 0, 0, 255, 0, 0, 255 }
Vtx {  48, -48, 0, 0, 0, 0, 0, 255, 0, 255 }
Vtx {   0,  48, 0, 0, 0, 0, 0, 0, 255, 255 }

gsSPMatrix(p, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(m, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE)
gsSPVertex(verts, 3, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSPEndDisplayList()
";

#[test]
fn source_map_tracks_command_lines_and_excludes_data() {
    let img = crate::asm::assemble_at_with_textures(SOURCE_MAP_SRC, 0.0, &[]).expect("assemble");
    assert_eq!(img.source_map.len(), 9);
    assert_eq!(img.rdram.len() - img.entry_addr as usize, 9 * 8);
    for (index, line) in (8..=16).enumerate() {
        let addr = img.entry_addr + index as u32 * 8;
        assert_eq!(img.source_map[index], (addr, line));
        assert_eq!(img.line_at(u64::from(addr)), Some(line));
    }
    assert_eq!(img.line_at(u64::from(img.vtx_addr)), None);
    assert_eq!(img.line_at(u64::from(img.vp_addr)), None);
    assert_eq!(img.line_at(u64::from(img.entry_addr) + 4), None);
    assert_eq!(img.line_at(img.rdram.len() as u64), None);
    assert_eq!(img.line_at((1u64 << 32) + u64::from(img.entry_addr)), None);
    assert_eq!(img.line_at(u64::MAX), None);
}

#[test]
fn source_map_tracks_every_texture_macro_word() {
    for (format, gbi_format, size, words) in [("RGBA16", "RGBA", "16b", 7), ("CI8", "CI", "8b", 11)]
    {
        let source = format!(
            "Texture tex = {{ 2, 2, {format} }}\n\n\
gsDPLoadTextureBlock(tex, G_IM_FMT_{gbi_format}, G_IM_SIZ_{size}, 2, 2)\n\
gsSPEndDisplayList()\n"
        );
        let rgba8 = [255; 16];
        let img = crate::asm::assemble_at_with_textures(
            &source,
            0.0,
            &[crate::asm::TextureInput {
                name: "tex",
                rgba8: &rgba8,
                width: 2,
                height: 2,
            }],
        )
        .expect("assemble texture macro");
        assert_eq!(img.source_map.len(), words + 1, "{format}");
        assert_eq!(img.rdram.len() - img.entry_addr as usize, (words + 1) * 8);
        for index in 0..words {
            let addr = img.entry_addr + index as u32 * 8;
            assert_eq!(img.source_map[index], (addr, 3));
            assert_eq!(img.line_at(u64::from(addr)), Some(3));
        }
        assert_eq!(
            img.source_map[words],
            (img.entry_addr + words as u32 * 8, 4)
        );
        assert_eq!(img.line_at(u64::from(img.tex_addr)), None);
    }
}

#[test]
fn source_map_tracks_named_blocks_in_address_order() {
    let source = "\
Gfx sub[] = {
  gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
  gsSPEndDisplayList()
}
Gfx main[] = {
  gsSPDisplayList(sub)
  gsSPEndDisplayList()
}
";
    let img = assemble(source).expect("assemble named blocks");
    assert_eq!(img.source_map.len(), 4);
    for (index, line) in [6, 7, 2, 3].into_iter().enumerate() {
        let addr = img.entry_addr + index as u32 * 8;
        assert_eq!(img.source_map[index], (addr, line));
        assert_eq!(img.line_at(u64::from(addr)), Some(line));
    }
    assert_eq!(img.line_at(u64::from(img.entry_addr)), Some(6));
}

#[test]
fn source_map_resolves_missing_render_mode_diagnostic() {
    let source = SOURCE_MAP_SRC.replace("gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)", "");
    let img = crate::asm::assemble_at_with_textures(&source, 0.0, &[]).expect("assemble");
    let result = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
    assert_eq!(result.diags.len(), 1, "{:?}", result.diags);
    let diag = result
        .diags
        .iter()
        .find(|diag| diag.kind == crate::diag::DiagKind::RenderModeNeverSet)
        .expect("missing render mode diagnostic");
    assert_eq!(img.line_at(diag.at), Some(16));
}

#[test]
fn assembles_minimal_dl_layout() {
    let img: Image = assemble(SRC).expect("assemble ok");
    // 1 Vp (16) + 3 Vtx (48) + 2 Mtx (128) = 192 bytes of data, then 8-aligned commands.
    assert_eq!(img.rdram.len() % 8, 0);
    assert!(img.rdram.len() >= 192);
    let e = img.entry_addr as usize;
    assert_eq!(e % 8, 0);
    let cmds = &img.rdram[e..];
    assert_eq!(cmds.len() % 8, 0);
    assert_eq!(cmds[0], 0xDA); // first command is G_MTX (projection)
    let last_w0_off = cmds.len() - 8;
    assert_eq!(cmds[last_w0_off], 0xDF); // last command is G_ENDDL

    let mut found_vp = false;
    let mut off = 0usize;
    while off < cmds.len() {
        if cmds[off] == 0xDC {
            let w1 =
                u32::from_be_bytes([cmds[off + 4], cmds[off + 5], cmds[off + 6], cmds[off + 7]]);
            assert_eq!(w1, img.vp_addr);
            found_vp = true;
        }
        off += 8;
    }
    assert!(found_vp);
}

#[test]
fn first_matrix_command_is_projection_with_length_field() {
    let img = assemble(SRC).expect("assemble ok");
    let e = img.entry_addr as usize;
    let w0 = u32::from_be_bytes([
        img.rdram[e],
        img.rdram[e + 1],
        img.rdram[e + 2],
        img.rdram[e + 3],
    ]);
    // proj+load+nopush -> 0xDA380007.
    assert_eq!(w0, 0xDA38_0007);
}

#[test]
fn unknown_matrix_name_is_diagnosed() {
    // A comment and data declarations precede the bad command, so the reported line
    // is the TRUE source line (4), not the statement index.
    let bad = "\
// unknown-matrix fixture
Mtx p = identity()
Vtx { 0, 0, 0, 0, 0, 0, 0, 0, 0, 255 }
gsSPMatrix(zzz, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPEndDisplayList()
";
    let err = assemble(bad).unwrap_err();
    assert_eq!(err.len(), 1);
    assert_eq!(err[0].line, 4);
    assert!(err[0].msg.contains("zzz"));
}

#[test]
fn assemble_surfaces_parse_diagnostics() {
    let err = assemble("gsSPBogus()\n").unwrap_err();
    assert_eq!(err.len(), 1);
    assert_eq!(err[0].line, 1);
}

// ── Tier-1 texture format round-trip tests ───────────────────────────────────────────────────────

/// I8 encoder → I8 decoder round-trip: encode mid-gray as I8, decode back.
/// Gates the assembler's I8 branch: encode_i8_texel must produce a byte that decode_i8 restores.
#[test]
fn i8_texel_roundtrips_through_decode() {
    let enc = encode_i8_texel(128, 128, 128, 255);
    assert_eq!(
        crate::hle::texdec::decode_i8(&[enc], 1, 1),
        vec![128, 128, 128, 128] // I-format: alpha = intensity
    );
}

/// I4 pack → I4 decode round-trip: even=0xF (full), odd=0x0 (zero).
/// High nibble = even column convention: decode_i4 must read 0xF from the high nibble.
#[test]
fn i4_pair_packs_high_nibble_even_then_decodes() {
    let byte = encode_i4_pair(0xF, 0x0); // even=full, odd=zero
    assert_eq!(
        crate::hle::texdec::decode_i4(&[byte], 2, 1),
        vec![255, 255, 255, 255, 0, 0, 0, 0] // I-format: alpha = intensity
    );
}

/// IA16 encoder → decoder round-trip: encode white (255,255,255,128) as IA16.
#[test]
fn ia16_texel_roundtrips_through_decode() {
    let [hi, lo] = encode_ia16_texel(255, 255, 255, 128);
    assert_eq!(hi, 255); // intensity = (255+255+255)/3 = 255
    assert_eq!(lo, 128); // alpha passed through
    let decoded = crate::hle::texdec::decode_ia16(&[hi, lo], 1, 1);
    assert_eq!(decoded, vec![255, 255, 255, 128]);
}

/// IA8 encoder → decoder round-trip: encode mid-gray (128,128,128,192) as IA8.
#[test]
fn ia8_texel_roundtrips_through_decode() {
    let enc = encode_ia8_texel(128, 128, 128, 192);
    // i4 = 128>>4 = 8, a4 = 192>>4 = 12; packed = 0x8C
    assert_eq!(enc, 0x8C);
    let decoded = crate::hle::texdec::decode_ia8(&[enc], 1, 1);
    // i8 = (8<<4)|8 = 136; a8 = (12<<4)|12 = 204
    assert_eq!(decoded, vec![136, 136, 136, 204]);
}

/// IA4 pack → decode round-trip: even=full-bright (255,255,255,255), odd=zero (0,0,0,0).
#[test]
fn ia4_pair_packs_high_nibble_even_then_decodes() {
    let n0 = encode_ia4_nibble(255, 255, 255, 255); // i3=7, a1=1 -> 0xF
    let n1 = encode_ia4_nibble(0, 0, 0, 0); // i3=0, a1=0 -> 0x0
    assert_eq!(n0, 0xF);
    assert_eq!(n1, 0x0);
    let byte = encode_ia4_pair(n0, n1); // 0xF0
    assert_eq!(byte, 0xF0);
    let decoded = crate::hle::texdec::decode_ia4(&[byte], 2, 1);
    // texel0: nibble=0xF -> i8=255, a=255; texel1: nibble=0x0 -> i8=0, a=0
    assert_eq!(decoded, vec![255, 255, 255, 255, 0, 0, 0, 0]);
}

// ── CI8 assembler ↔ HLE end-to-end TLUT contract test ────────────────────────────────────────────

/// Assembler → HLE TLUT e2e: assemble a 3-color CI8 texture, interpret through the HLE, and
/// verify that the TLUT loaded by G_LOADTLUT contains exactly `palette_count * 8` bytes in
/// stride-8 RDRAM layout with the correct big-endian RGBA16 content for each color.
///
/// This test would FAIL before the encode.rs fix because `shiftl(lrt, 12, 12)` placed lrt in
/// bits[23:12], but the HLE reads it at bits[11:0] — so `count` was always 1 regardless of
/// the real palette size.  The fix `(lrt & 0xFFF)` at bits[11:0] makes the round-trip correct.
#[test]
fn ci8_assembler_hle_tlut_roundtrip_correct_count_and_content() {
    let src_n64 = "\
Texture tex = { 3, 1, CI8 }
gsDPLoadTextureBlock(tex, G_IM_FMT_CI, G_IM_SIZ_8b, 3, 1)
gsSPEndDisplayList()
";
    // 3 distinct colors → 3-entry palette.
    let rgba: [u8; 12] = [
        0, 0, 0, 255, // black  → index 0 → RGBA16 0x0001
        255, 0, 0, 255, // red    → index 1 → RGBA16 0xF801
        0, 255, 0, 255, // green  → index 2 → RGBA16 0x07C1
    ];
    let palette_count: usize = 3;

    let img = assemble_with_texture(src_n64, &rgba, 3, 1).expect("CI8 assembly must succeed");

    // Interpret through the full HLE pipeline.
    let result = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
    // Allow the known "combiner selector not implemented" diagnostic (non-fatal).
    // Any other diagnostic would indicate a structural problem with the DL.
    let fatal_diags: Vec<_> = result
        .diags
        .iter()
        .filter(|d| {
            !d.kind
                .to_string()
                .contains("combiner selector not implemented")
        })
        .collect();
    assert!(
        fatal_diags.is_empty(),
        "unexpected HLE diags: {:?}",
        fatal_diags
    );

    // G_LOADTLUT → write_tlut expands the packed RDRAM palette (stride-8: entry i at byte i<<3)
    // into the faithful bank's palette region (upper 2 KiB at PALETTE_BASE). decode_ci8 reads entry
    // i at that stride, so the first palette_count entries must hold the big-endian RGBA16 content.
    // Before the encode.rs fix, lrt landed in bits[23:12] but HLE reads bits[11:0], so count was
    // always 1 and only entry 0 was populated; here we verify all `palette_count` entries loaded.
    let tlut = result.rdp.tmem_bank.palette();
    assert!(
        tlut.len() >= palette_count * 8,
        "palette region must cover all {palette_count} stride-8 entries"
    );

    // Verify palette entries have correct big-endian RGBA16 content (stride-8: entry i at [i*8]).
    // Entry 0 = black [0,0,0,255] → RGBA16 r5=0,g5=0,b5=0,a1=1 → 0x0001.
    assert_eq!(tlut[0], 0x00, "entry 0 hi should be 0x00 (black)");
    assert_eq!(tlut[1], 0x01, "entry 0 lo should be 0x01 (alpha=1)");
    // Entry 1 = red [255,0,0,255] → RGBA16 r5=31,g5=0,b5=0,a1=1 → 0xF801.
    assert_eq!(tlut[8], 0xF8, "entry 1 hi should be 0xF8 (red)");
    assert_eq!(tlut[9], 0x01, "entry 1 lo should be 0x01 (alpha=1)");
    // Entry 2 = green [0,255,0,255] → RGBA16 r5=0,g5=31,b5=0,a1=1 → 0x07C1.
    assert_eq!(tlut[16], 0x07, "entry 2 hi should be 0x07 (green)");
    assert_eq!(tlut[17], 0xC1, "entry 2 lo should be 0xC1 (green+alpha)");
    // Trailing palette slots (beyond the loaded entries) must remain zero.
    assert_eq!(
        &tlut[palette_count * 8..palette_count * 8 + 8],
        &[0u8; 8],
        "slot after last loaded entry must be zero"
    );
}

// ── Tier-1 CI8 round-trip + assembler verify ─────────────────────────────────────────────────────

/// CI8 encode → decode round-trip: encode_ci8 produces the right palette indices; decode_ci8
/// with the emitted RGBA16 TLUT (stride 8: entry i at offset i<<3) reproduces the source colors
/// within ±5/channel (5-bit RGBA16 quantization error).
#[test]
fn ci8_index_roundtrips_through_decode() {
    // 3-color palette; source pixels choose indices 0, 1, 2 respectively.
    let pal: [[u8; 4]; 3] = [[0, 0, 0, 255], [255, 0, 0, 255], [0, 255, 0, 255]];
    let src = [0u8, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255]; // 3×1 RGBA8
    let idx = encode_ci8(&src, &pal);
    assert_eq!(idx, vec![0, 1, 2]);
    // Build a stride-8 TLUT buffer (decode_ci8 reads entry i at offset i<<3).
    let mut tlut = vec![0u8; pal.len() * 8];
    for (i, c) in pal.iter().enumerate() {
        let e = encode_rgba16_texel(c[0], c[1], c[2], c[3]);
        tlut[i * 8] = e[0];
        tlut[i * 8 + 1] = e[1];
    }
    let out = crate::hle::texdec::decode_ci8(&idx, 3, 1, &tlut, 2 /* RGBA16 */);
    // index 2 → green: g5=31 → decode (31<<3)|(31>>2) = 248|7 = 255. R=0, B=0.
    assert_eq!(
        &out[8..11],
        &[0, 255, 0],
        "index 2 must decode to green (RGBA16 5-bit expansion)"
    );
}

/// CI8 assembler verify: `assemble_with_texture` for a CI8 test scene must produce an RDRAM image that
/// contains (a) the correct CI8 palette (RGBA16 entries at stride 8), (b) the CI8 index data
/// at tex_addr, and (c) a G_LOADTLUT command (opcode 0xF0) in the display list.
#[test]
fn ci8_assembles_palette_and_loadtlut_command() {
    let src_n64 = "\
Texture tex = { 3, 1, CI8 }
gsDPLoadTextureBlock(tex, G_IM_FMT_CI, G_IM_SIZ_8b, 3, 1)
gsSPEndDisplayList()
";
    // 3×1 RGBA8: black, red, green — 3 distinct colors → 3-entry palette, indices [0,1,2].
    let rgba = [
        0u8, 0, 0, 255, // black  → index 0
        255, 0, 0, 255, // red    → index 1
        0, 255, 0, 255, // green  → index 2
    ];
    let img = assemble_with_texture(src_n64, &rgba, 3, 1)
        .expect("CI8 assembly must succeed for 3 distinct colors");

    // (a) CI8 index bytes at tex_addr: [0, 1, 2].
    let ta = img.tex_addr as usize;
    assert_eq!(&img.rdram[ta..ta + 3], &[0u8, 1, 2], "CI8 index bytes");

    // (b) Palette at tex_addr + 8 (3 index bytes + 5 pad bytes = 8-byte-aligned block).
    //     PACKED layout (hardware-accurate RDRAM): entry i at offset pal_addr + i*2 (2 bytes each,
    //     no stride-8 padding). load_tlut DMA-expands packed→stride-8 on load.
    let pal_base = ta + 8;
    // Entry 0 = black [0,0,0,255] → RGBA16 r5=0,g5=0,b5=0,a1=1 → 0x0001
    assert_eq!(img.rdram[pal_base], 0x00, "palette[0] hi");
    assert_eq!(img.rdram[pal_base + 1], 0x01, "palette[0] lo");
    // Entry 1 = red [255,0,0,255] → RGBA16 r5=31→ 0xF801 (packed: offset +2, not +8)
    assert_eq!(img.rdram[pal_base + 2], 0xF8, "palette[1] hi");
    assert_eq!(img.rdram[pal_base + 3], 0x01, "palette[1] lo");
    // Entry 2 = green [0,255,0,255] → RGBA16 g5=31 → 0x07C1 (packed: offset +4, not +16)
    assert_eq!(img.rdram[pal_base + 4], 0x07, "palette[2] hi");
    assert_eq!(img.rdram[pal_base + 5], 0xC1, "palette[2] lo");

    // (c) G_LOADTLUT command (opcode 0xF0) must appear in the display list before G_ENDDL.
    let e = img.entry_addr as usize;
    let mut found_tlut = false;
    let mut off = e;
    while off + 8 <= img.rdram.len() {
        let w0 = u32::from_be_bytes(img.rdram[off..off + 4].try_into().unwrap());
        if w0 >> 24 == 0xF0 {
            found_tlut = true;
            break;
        }
        if w0 >> 24 == 0xDF {
            break; // G_ENDDL
        }
        off += 8;
    }
    assert!(
        found_tlut,
        "G_LOADTLUT (opcode 0xF0) not found in display list"
    );
}

// ── CI4 assembler — nibble-order + round-trip test ───────────────────────────────────────────────

/// CI4 encode nibble-order: even column → high nibble. Two-color palette (black=0, red=1).
/// Source: texel0=red(index1), texel1=black(index0) → packed byte must be 0x10 (high=1, low=0).
#[test]
fn ci4_pair_packs_high_nibble_even() {
    let pal = [[0, 0, 0, 255u8], [255, 0, 0, 255]]; // index0 black, index1 red
    let src = [255, 0, 0, 255, 0, 0, 0, 255]; // texel0 red(1), texel1 black(0)
    let packed = crate::asm::asm::encode_ci4(&src, &pal); // expect [0x10]
    assert_eq!(packed, vec![0x10]);
}

/// CI4 assembler end-to-end: assemble a 2-color CI4 texture, verify RDRAM layout and TLUT command.
/// Palette must be PACKED (count*2 bytes, no stride-8 gap). G_LOADTLUT (0xF0) must appear in DL.
#[test]
fn ci4_assembles_palette_and_loadtlut_command() {
    let src_n64 = "\
Texture tex = { 2, 1, CI4 }
gsDPLoadTextureBlock(tex, G_IM_FMT_CI, G_IM_SIZ_4b, 2, 1)
gsSPEndDisplayList()
";
    // 2×1 RGBA8: black, red — 2 distinct colors → 2-entry palette, packed nibble [0x01].
    let rgba = [
        0u8, 0, 0, 255, // black → index 0
        255, 0, 0, 255, // red   → index 1
    ];
    let img = assemble_with_texture(src_n64, &rgba, 2, 1)
        .expect("CI4 assembly must succeed for 2 distinct colors");

    // (a) CI4 packed nibble byte at tex_addr: [0x01] (texel0=black=0 high nibble, texel1=red=1 low).
    let ta = img.tex_addr as usize;
    assert_eq!(img.rdram[ta], 0x01, "CI4 packed byte: black=0 hi, red=1 lo");

    // (b) Palette at tex_addr + 8 (1 byte index + 7 pad bytes = 8-byte-aligned block).
    //     PACKED layout: entry i at pal_addr + i*2 (2 bytes each, no stride-8 padding).
    let pal_base = ta + 8;
    // Entry 0 = black [0,0,0,255] → RGBA16 0x0001.
    assert_eq!(img.rdram[pal_base], 0x00, "CI4 palette[0] hi (black)");
    assert_eq!(img.rdram[pal_base + 1], 0x01, "CI4 palette[0] lo (alpha=1)");
    // Entry 1 = red [255,0,0,255] → RGBA16 0xF801 (PACKED: offset +2, not +8).
    assert_eq!(img.rdram[pal_base + 2], 0xF8, "CI4 palette[1] hi (red)");
    assert_eq!(img.rdram[pal_base + 3], 0x01, "CI4 palette[1] lo (alpha=1)");

    // (c) G_LOADTLUT command (opcode 0xF0) must appear in the display list before G_ENDDL.
    let e = img.entry_addr as usize;
    let mut found_tlut = false;
    let mut off = e;
    while off + 8 <= img.rdram.len() {
        let w0 = u32::from_be_bytes(img.rdram[off..off + 4].try_into().unwrap());
        if w0 >> 24 == 0xF0 {
            found_tlut = true;
            break;
        }
        if w0 >> 24 == 0xDF {
            break; // G_ENDDL
        }
        off += 8;
    }
    assert!(
        found_tlut,
        "G_LOADTLUT (opcode 0xF0) not found in CI4 display list"
    );
}
