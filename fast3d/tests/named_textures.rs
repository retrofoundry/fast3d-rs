#![cfg(feature = "asm")]
use fast3d::asm::{
    analyze, assemble_at, assemble_at_with_textures, assemble_with_texture, TextureInput,
};

fn input<'a>(name: &'a str, rgba8: &'a [u8], width: u32, height: u32) -> TextureInput<'a> {
    TextureInput {
        name,
        rgba8,
        width,
        height,
    }
}

#[test]
fn discovers_named_textures_in_source_order() {
    let source = "Texture grass = { 32, 16, RGBA16 }\n\
                  invalid source\n\
                  Texture mask = { 8, 8, IA8 }\n";
    let out = analyze(source);
    assert_eq!(out.textures.len(), 2);
    assert_eq!(out.textures[0].name, "grass");
    assert_eq!(out.textures[0].width, 32);
    assert_eq!(out.textures[0].height, 16);
    assert_eq!(out.textures[0].format, "RGBA16");
    assert_eq!(out.textures[0].line, 1);
    assert_eq!(out.textures[1].name, "mask");
    assert_eq!(out.textures[1].line, 3);
    assert!(!out.diagnostics.is_empty());
}

#[test]
fn duplicate_declarations_are_diagnosed() {
    let out = analyze("Texture tex = { 1, 1, RGBA16 }\nTexture tex = { 1, 1, I8 }\n");
    assert!(out.diagnostics.iter().any(|diag| {
        diag.msg.contains("duplicate texture declaration") && diag.msg.contains("tex")
    }));
}

#[test]
fn named_inputs_get_distinct_addresses_and_formats() {
    let source = "Texture color = { 1, 1, RGBA16 }\n\
                  Texture intensity = { 1, 1, I8 }\n\
                  Gfx main[] = {\n\
                    gsDPSetTextureImage(G_IM_FMT_RGBA, G_IM_SIZ_16b, 1, color)\n\
                    gsDPSetTextureImage(G_IM_FMT_I, G_IM_SIZ_8b, 1, intensity)\n\
                    gsSPEndDisplayList()\n\
                  }\n";
    let red = [255, 0, 0, 255];
    let white = [255, 255, 255, 255];
    let assembled = assemble_at_with_textures(
        source,
        0.0,
        &[input("color", &red, 1, 1), input("intensity", &white, 1, 1)],
    )
    .unwrap();
    let commands = &assembled.rdram[assembled.entry_addr as usize..];
    let color_addr = u32::from_be_bytes(commands[4..8].try_into().unwrap());
    let intensity_addr = u32::from_be_bytes(commands[12..16].try_into().unwrap());
    assert_ne!(color_addr, intensity_addr);
    assert_eq!(
        &assembled.rdram[color_addr as usize..color_addr as usize + 2],
        &[0xf8, 0x01]
    );
    assert_eq!(assembled.rdram[intensity_addr as usize], 255);
    assert_eq!(assembled.tex_addr, color_addr);
}

#[test]
fn named_inputs_require_exact_names_dimensions_and_lengths() {
    let source = "Texture color = { 2, 1, RGBA16 }\ngsSPEndDisplayList()\n";
    let missing = assemble_at_with_textures(source, 0.0, &[]).unwrap_err();
    assert!(missing
        .iter()
        .any(|d| d.msg.contains("missing texture input") && d.msg.contains("color")));

    let extra_pixels = [255; 8];
    let pixel = [255, 0, 0, 255];
    let extra = assemble_at_with_textures(
        source,
        0.0,
        &[
            input("color", &extra_pixels, 2, 1),
            input("extra", &pixel, 1, 1),
        ],
    )
    .unwrap_err();
    assert!(extra
        .iter()
        .any(|d| d.msg.contains("undeclared texture input") && d.msg.contains("extra")));

    let mismatch =
        assemble_at_with_textures(source, 0.0, &[input("color", &pixel, 1, 1)]).unwrap_err();
    assert!(mismatch
        .iter()
        .any(|d| d.msg.contains("declares 2x1") && d.msg.contains("input is 1x1")));

    let bad =
        assemble_at_with_textures(source, 0.0, &[input("color", &[255, 0, 0], 2, 1)]).unwrap_err();
    assert!(bad
        .iter()
        .any(|d| { d.msg.contains("expected 8 RGBA8 bytes") && d.msg.contains("got 3") }));
}

#[test]
fn duplicate_named_inputs_are_diagnosed() {
    let source = "Texture color = { 1, 1, RGBA16 }\ngsSPEndDisplayList()\n";
    let pixel = [255, 0, 0, 255];
    let err = assemble_at_with_textures(
        source,
        0.0,
        &[input("color", &pixel, 1, 1), input("color", &pixel, 1, 1)],
    )
    .unwrap_err();
    assert!(err
        .iter()
        .any(|d| d.msg.contains("duplicate texture input") && d.msg.contains("color")));
}

#[test]
fn zero_texture_dimensions_are_diagnosed() {
    let pixel = [255, 0, 0, 255];
    let declared_zero = assemble_at_with_textures(
        "Texture color = { 0, 1, RGBA16 }\ngsSPEndDisplayList()\n",
        0.0,
        &[input("color", &[], 0, 1)],
    )
    .unwrap_err();
    assert!(declared_zero
        .iter()
        .any(|d| d.msg.contains("zero dimensions") && d.msg.contains("color")));

    let input_zero = assemble_at_with_textures(
        "Texture color = { 1, 1, RGBA16 }\ngsSPEndDisplayList()\n",
        0.0,
        &[input("color", &pixel, 0, 1)],
    )
    .unwrap_err();
    assert!(input_zero
        .iter()
        .any(|d| d.msg.contains("zero dimensions") && d.msg.contains("color")));
}

#[test]
fn named_ci_textures_get_independent_palettes() {
    let source = "Texture red = { 1, 1, CI8 }\n\
                  Texture green = { 1, 1, CI8 }\n\
                  gsDPLoadTextureBlock(red, G_IM_FMT_CI, G_IM_SIZ_8b, 1, 1)\n\
                  gsDPLoadTextureBlock(green, G_IM_FMT_CI, G_IM_SIZ_8b, 1, 1)\n\
                  gsSPEndDisplayList()\n";
    let red = [255, 0, 0, 255];
    let green = [0, 255, 0, 255];
    let assembled = assemble_at_with_textures(
        source,
        0.0,
        &[input("red", &red, 1, 1), input("green", &green, 1, 1)],
    )
    .unwrap();
    let commands = &assembled.rdram[assembled.entry_addr as usize..];
    let red_palette = u32::from_be_bytes(commands[4..8].try_into().unwrap()) as usize;
    let green_palette =
        u32::from_be_bytes(commands[11 * 8 + 4..12 * 8].try_into().unwrap()) as usize;
    assert_ne!(red_palette, green_palette);
    assert_eq!(
        &assembled.rdram[red_palette..red_palette + 2],
        &[0xf8, 0x01]
    );
    assert_eq!(
        &assembled.rdram[green_palette..green_palette + 2],
        &[0x07, 0xc1]
    );
}

#[test]
fn named_ci_encoding_errors_use_the_declaration_line() {
    let source = "\nTexture crowded = { 257, 1, CI8 }\ngsSPEndDisplayList()\n";
    let rgba8: Vec<_> = (0..257u16)
        .flat_map(|value| [value as u8, (value >> 8) as u8, 0, 255])
        .collect();
    let err =
        assemble_at_with_textures(source, 0.0, &[input("crowded", &rgba8, 257, 1)]).unwrap_err();
    assert!(err
        .iter()
        .any(|diag| diag.line == 2 && diag.msg.contains("more than 256 distinct colors")));
}

#[test]
fn invalid_named_format_is_diagnosed_but_legacy_still_falls_back() {
    let source = "Texture color = { 1, 1, NOPE }\ngsSPEndDisplayList()\n";
    let red = [255, 0, 0, 255];
    let named = assemble_at_with_textures(source, 0.0, &[input("color", &red, 1, 1)]).unwrap_err();
    assert!(named
        .iter()
        .any(|d| d.msg.contains("unknown texture format") && d.msg.contains("NOPE")));

    let legacy = assemble_with_texture(source, &red, 1, 1).unwrap();
    assert_eq!(
        &legacy.rdram[legacy.tex_addr as usize..legacy.tex_addr as usize + 2],
        &[0xf8, 0x01]
    );
}

#[test]
fn legacy_texture_assembly_keeps_golden_bytes() {
    let source = "Texture tex = { 2, 1, RGBA16 }\ngsSPEndDisplayList()\n";
    let rgba8 = [255, 0, 0, 255, 0, 0, 255, 255];
    let assembled = assemble_at(source, 0.0, Some((&rgba8, 2, 1))).unwrap();
    let expected = vec![
        0x01, 0xe0, 0x02, 0x80, 0x01, 0xff, 0x01, 0xff, 0x01, 0xe0, 0x02, 0x80, 0x00, 0x00, 0x01,
        0xff, 0xf8, 0x01, 0x00, 0x3f, 0x00, 0x00, 0x00, 0x00, 0xdf, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];
    assert_eq!(assembled.rdram, expected);
    assert_eq!(assembled.tex_addr, 16);
    assert_eq!(assembled.entry_addr, 24);
    assert_eq!(
        assemble_with_texture(source, &rgba8, 2, 1).unwrap().rdram,
        expected
    );
}

#[test]
fn legacy_four_bit_inputs_shorter_than_dimensions_still_encode_flat() {
    let pixel = [0x00, 0x00, 0x00, 0xff];
    for (format, expected) in [("I4", 0x00), ("IA4", 0x10), ("CI4", 0x00)] {
        let source = format!("Texture tex = {{ 3, 2, {format} }}\ngsSPEndDisplayList()\n");
        let image = assemble_with_texture(&source, &pixel, 3, 2).unwrap();
        assert_eq!(image.rdram[image.tex_addr as usize], expected, "{format}");
    }
}

#[test]
fn legacy_four_bit_inputs_longer_than_dimensions_are_not_truncated() {
    let rgba = [
        0x00, 0x00, 0x00, 0xff, 0x11, 0x11, 0x11, 0xff, 0x22, 0x22, 0x22, 0xff,
    ];
    for (format, expected) in [
        ("I4", [0x01, 0x20]),
        ("IA4", [0x11, 0x30]),
        ("CI4", [0x01, 0x20]),
    ] {
        let source = format!("Texture tex = {{ 1, 1, {format} }}\ngsSPEndDisplayList()\n");
        let image = assemble_with_texture(&source, &rgba, 1, 1).unwrap();
        let start = image.tex_addr as usize;
        assert_eq!(&image.rdram[start..start + 2], &expected, "{format}");
    }
}

#[test]
fn i4_packs_each_odd_width_row_on_a_new_byte() {
    let source = "Texture tex = { 3, 2, I4 }\ngsSPEndDisplayList()\n";
    let rgba = [
        0x00, 0x00, 0x00, 0xff, 0x11, 0x11, 0x11, 0xff, 0x22, 0x22, 0x22, 0xff, 0x33, 0x33, 0x33,
        0xff, 0x44, 0x44, 0x44, 0xff, 0x55, 0x55, 0x55, 0xff,
    ];
    let image = assemble_at_with_textures(source, 0.0, &[input("tex", &rgba, 3, 2)]).unwrap();
    let start = image.tex_addr as usize;
    assert_eq!(&image.rdram[start..start + 4], &[0x01, 0x20, 0x34, 0x50]);
}

#[test]
fn ia4_packs_each_odd_width_row_on_a_new_byte() {
    let source = "Texture tex = { 3, 2, IA4 }\ngsSPEndDisplayList()\n";
    let rgba = [
        0x00, 0x00, 0x00, 0xff, 0x11, 0x11, 0x11, 0xff, 0x22, 0x22, 0x22, 0xff, 0x33, 0x33, 0x33,
        0xff, 0x44, 0x44, 0x44, 0xff, 0x55, 0x55, 0x55, 0xff,
    ];
    let image = assemble_at_with_textures(source, 0.0, &[input("tex", &rgba, 3, 2)]).unwrap();
    let start = image.tex_addr as usize;
    assert_eq!(&image.rdram[start..start + 4], &[0x11, 0x30, 0x35, 0x50]);
}

#[test]
fn ci4_packs_each_odd_width_row_on_a_new_byte() {
    let source = "Texture tex = { 3, 2, CI4 }\ngsSPEndDisplayList()\n";
    let rgba = [
        0x00, 0x00, 0x00, 0xff, 0x11, 0x11, 0x11, 0xff, 0x22, 0x22, 0x22, 0xff, 0x33, 0x33, 0x33,
        0xff, 0x44, 0x44, 0x44, 0xff, 0x55, 0x55, 0x55, 0xff,
    ];
    let image = assemble_at_with_textures(source, 0.0, &[input("tex", &rgba, 3, 2)]).unwrap();
    let start = image.tex_addr as usize;
    assert_eq!(&image.rdram[start..start + 4], &[0x01, 0x20, 0x34, 0x50]);
}
