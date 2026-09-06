use n64_gbi::encode::*;

fn interpret_commands(commands: impl IntoIterator<Item = (u32, u32)>) -> crate::hle::InterpResult {
    let rdram: Vec<u8> = commands
        .into_iter()
        .flat_map(|(w0, w1)| w0.to_be_bytes().into_iter().chain(w1.to_be_bytes()))
        .collect();
    crate::hle::interpret_rdram(&rdram, 0)
}

#[test]
fn roundtrip_fill_rectangle() {
    let r = interpret_commands([
        gdp_set_color_image(0, 2, 320, 0x00100000),
        gdp_set_fill_color(0xCAFECAFE),
        gdp_fill_rectangle(0, 0, 1280, 960),
        gsp_enddl(),
    ]);
    assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
    let pairs = &r.scene.framebuffer_pairs;
    assert_eq!(pairs.len(), 1, "expected 1 framebuffer pair");
    let pair = &pairs[0];

    assert_eq!(pair.color_image.fmt, 0, "fmt should be RGBA(0)");
    assert_eq!(pair.color_image.siz, 2, "siz should be 16b(2)");
    assert_eq!(pair.color_image.width, 320);
    assert_eq!(pair.color_image.addr, 0x00100000);

    assert_eq!(pair.ops.len(), 1);
    match &pair.ops[0] {
        crate::hle::SceneOp::FillRect { rect, color_raw } => {
            assert_eq!(*color_raw, 0xCAFECAFE);
            assert_eq!(rect.ulx, 0);
            assert_eq!(rect.uly, 0);
            assert_eq!(rect.lrx, 320);
            assert_eq!(rect.lry, 240);
        }
        other => panic!("expected FillRect, got {:?}", other),
    }
}

#[test]
fn roundtrip_texture_rectangle() {
    let r = interpret_commands(
        [gdp_set_color_image(0, 2, 320, 0x00100000)]
            .into_iter()
            .chain(gsp_texture_rectangle(
                0, 0, 1280, 960, 0, 44, 52, 1024, 512, false,
            ))
            .chain([gsp_enddl()]),
    );
    assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
    let pairs = &r.scene.framebuffer_pairs;
    assert_eq!(pairs.len(), 1);
    match &pairs[0].ops[0] {
        crate::hle::SceneOp::TexRect {
            rect,
            uls,
            ult,
            dsdx,
            dtdy,
            flip,
            ..
        } => {
            assert_eq!(rect.lrx, 1280);
            assert_eq!(rect.lry, 960);
            assert_eq!(rect.ulx, 0);
            assert_eq!(rect.uly, 0);
            assert_eq!(*uls, 44);
            assert_eq!(*ult, 52);
            assert_eq!(*dsdx, 1024);
            assert_eq!(*dtdy, 512);
            assert!(!*flip);
        }
        other => panic!("expected TexRect, got {:?}", other),
    }
}

#[test]
fn roundtrip_texture_rectangle_flip() {
    let r = interpret_commands(
        [gdp_set_color_image(0, 2, 320, 0x00100000)]
            .into_iter()
            .chain(gsp_texture_rectangle(
                0, 0, 1280, 960, 0, 11, 13, 1024, 512, true,
            ))
            .chain([gsp_enddl()]),
    );
    assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
    match &r.scene.framebuffer_pairs[0].ops[0] {
        crate::hle::SceneOp::TexRect {
            flip,
            uls,
            ult,
            dsdx,
            dtdy,
            ..
        } => {
            assert!(*flip, "flip should be true for TextureRectangleFlip");
            assert_eq!(*uls, 11);
            assert_eq!(*ult, 13);
            assert_eq!(*dsdx, 1024);
            assert_eq!(*dtdy, 512);
        }
        other => panic!("expected TexRect (flip), got {:?}", other),
    }
}

#[test]
fn roundtrip_set_color_and_depth_image() {
    let r = interpret_commands([
        gdp_set_color_image(0, 2, 320, 0x00100000),
        gdp_set_depth_image(0x00200000),
        gdp_set_fill_color(0xFFFFFFFF),
        gdp_fill_rectangle(0, 0, 1280, 960),
        gsp_enddl(),
    ]);
    assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
    let pair = &r.scene.framebuffer_pairs[0];
    assert_eq!(pair.color_image.addr, 0x00100000);
    assert_eq!(pair.depth_image, Some(0x00200000));
}

#[test]
fn roundtrip_set_scissor() {
    let r = interpret_commands([
        gdp_set_color_image(0, 2, 320, 0x00100000),
        gdp_set_scissor(0, 0, 0, 1280, 960),
        gdp_set_fill_color(0xCAFECAFE),
        gdp_fill_rectangle(0, 0, 1280, 960),
        gsp_enddl(),
    ]);
    assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
    let pair = &r.scene.framebuffer_pairs[0];

    assert_eq!(pair.active_scissor.lrx, 320);
    assert_eq!(pair.active_scissor.lry, 240);
    assert_eq!(pair.active_scissor.mode, 0);

    assert_eq!(pair.ops.len(), 1);
    assert!(matches!(pair.ops[0], crate::hle::SceneOp::FillRect { .. }));
}
