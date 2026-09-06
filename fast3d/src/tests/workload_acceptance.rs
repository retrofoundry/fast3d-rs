use crate::render::workload::TargetId;
use crate::render::{headless_device_forced_fallback, SceneRenderer};
use crate::tests::common::pixels_from_render;
use crate::tests::dl_builder::{Built, Command, DlBuilder};
use crate::ClearPolicy;
use n64_gbi::{consts::*, encode::*};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const COLOR_ADDRESS: u32 = 0x0010_0000;
const DEPTH_ADDRESS: u32 = 0x0020_0000;
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const YELLOW: [u8; 4] = [255, 255, 0, 255];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];
const CYAN: [u8; 4] = [0, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];
const WHITE: [u8; 4] = [255; 4];
const BACKGROUND: [u8; 4] = [13, 13, 20, 255];
const POLICIES: [ClearPolicy; 2] = [ClearPolicy::PerFrame, ClearPolicy::Persist];
const OPAQUE: u32 = G_RM_OPA_SURF | Z_CMP | Z_UPD;
const DECAL: u32 = G_RM_OPA_SURF | Z_CMP | ZMODE_DEC;

fn viewport(b: &mut DlBuilder, far: bool) -> u32 {
    b.viewport(Vp {
        vscale: [1024, 1024, if far { 0 } else { 511 }, 0],
        vtrans: [640, 480, if far { 1024 } else { 511 }, 0],
    })
}

fn setup(b: &mut DlBuilder, paired: bool) -> Vec<Command> {
    let projection = b.matrix(n64_gbi::gu::gu_scale(1.0 / 256.0, 1.0 / 256.0, 1.0 / 128.0));
    let model = b.matrix(n64_gbi::gu::gu_scale(1.0, 1.0, 1.0));
    let viewport = viewport(b, false);
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
    let mut dl = vec![
        gdp_set_depth_image(DEPTH_ADDRESS),
        gdp_set_scissor(0, 0, 0, 1280, 960),
        gsp_clear_geometrymode(u32::MAX),
        gsp_set_geometrymode(G_CLIPPING | G_SHADE | G_SHADING_SMOOTH | G_ZBUFFER),
        gsp_matrix(projection, true, true, false),
        gsp_matrix(model, false, true, false),
        gsp_viewport(viewport),
        gdp_set_cycle_type(0),
        gdp_set_other_mode_h(4, 2, 3 << 4),
        gdp_set_other_mode_h(6, 2, 3 << 6),
        gdp_set_other_mode_l(0, 2, 0),
        gdp_set_combine_lerp(color, alpha, color, alpha),
    ];
    if paired {
        dl.push(gdp_set_color_image(0, 2, 320, COLOR_ADDRESS));
    }
    dl
}

fn quad(
    b: &mut DlBuilder,
    dl: &mut Vec<Command>,
    [left, top, right, bottom]: [i16; 4],
    z: i16,
    [r, g, blue, a]: [u8; 4],
    mode: u32,
) {
    let vertices = b.vertices(
        &[(left, top), (left, bottom), (right, bottom), (right, top)].map(|(x, y)| VtxColored {
            x: x - 160,
            y: 120 - y,
            z,
            flag: 0,
            s: 0,
            t: 0,
            r,
            g,
            b: blue,
            a,
        }),
    );
    dl.extend([
        gdp_pipe_sync(),
        gdp_set_render_mode(mode, G_RM_OPA_SURF2),
        gsp_vertex(0, 4, vertices),
        gsp_2triangles(0, 1, 2, 0, 2, 3),
    ]);
}

fn finish(mut b: DlBuilder, mut dl: Vec<Command>) -> Built {
    dl.extend([gdp_pipe_sync(), (0xe900_0000, 0), gsp_enddl()]);
    b.list("main", &dl);
    b.finish("main")
}

fn interpret(built: &Built) -> crate::hle::Scene {
    let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    result.scene
}

fn target(paired: bool) -> TargetId {
    if paired {
        TargetId::Guest(COLOR_ADDRESS.into())
    } else {
        TargetId::Legacy
    }
}

fn scanout(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &SceneRenderer,
    target: TargetId,
    width: u32,
    height: u32,
) -> Vec<u8> {
    pixels_from_render(device, queue, width, height, FORMAT, |view| {
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        renderer.scanout(&mut encoder, view, target);
        queue.submit(Some(encoder.finish()));
    })
}

fn assert_pixels(pixels: &[u8], width: u32, height: u32, expected: impl Fn(u32, u32) -> [u8; 4]) {
    assert_eq!(pixels.len(), (width * height * 4) as usize);
    for (y, row) in pixels.chunks((width * 4) as usize).enumerate() {
        for (x, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
            assert_eq!(
                *pixel,
                expected(x as u32, y as u32),
                "({x}, {y}) at {width}x{height}"
            );
        }
    }
}

fn in_rect(x: u32, y: u32, [left, top, right, bottom]: [u32; 4]) -> bool {
    (left..right).contains(&x) && (top..bottom).contains(&y)
}

fn color_task(paired: bool, task: u8) -> crate::hle::Scene {
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, paired);
    match task {
        0 => {
            quad(&mut b, &mut dl, [0, 0, 320, 240], 0, RED, G_RM_OPA_SURF);
            quad(
                &mut b,
                &mut dl,
                [160, 0, 320, 240],
                0,
                YELLOW,
                G_RM_OPA_SURF,
            );
        }
        1 => quad(&mut b, &mut dl, [24, 40, 88, 96], 0, BLUE, G_RM_OPA_SURF),
        2 => quad(
            &mut b,
            &mut dl,
            [208, 144, 280, 208],
            0,
            GREEN,
            G_RM_OPA_SURF,
        ),
        _ => unreachable!(),
    }
    interpret(&finish(b, dl))
}

#[test]
fn paired_pairless_clear_policy_equivalence() {
    let (device, queue) = headless_device_forced_fallback();
    for policy in POLICIES {
        let mut outputs = Vec::new();
        for paired in [false, true] {
            let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
            renderer.begin_frame();
            for task in 0..3 {
                if task == 2 {
                    renderer.begin_frame();
                }
                let scene = color_task(paired, task);
                assert_eq!(
                    renderer.render_into_store(&device, &queue, &scene, policy),
                    Some(target(paired))
                );
                let pixels = scanout(&device, &queue, &renderer, target(paired), 320, 240);
                assert_pixels(&pixels, 320, 240, |x, y| {
                    if task == 2 && in_rect(x, y, [208, 144, 280, 208]) {
                        GREEN
                    } else if task == 2 && policy == ClearPolicy::PerFrame {
                        BACKGROUND
                    } else if task >= 1 && in_rect(x, y, [24, 40, 88, 96]) {
                        BLUE
                    } else if x < 160 {
                        RED
                    } else {
                        YELLOW
                    }
                });
                outputs.push(pixels);
            }
        }
        assert_eq!(outputs[..3], outputs[3..], "{policy:?}");
    }
}

fn scissor_scene(paired: bool) -> crate::hle::Scene {
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, paired);
    quad(&mut b, &mut dl, [0, 0, 320, 240], 0, BLACK, G_RM_OPA_SURF);
    dl.push(gdp_set_scissor(0, 24 * 4, 32 * 4, 136 * 4, 104 * 4));
    quad(&mut b, &mut dl, [0, 0, 320, 240], 0, RED, G_RM_OPA_SURF);
    dl.push(gdp_set_scissor(0, 96 * 4, 80 * 4, 224 * 4, 176 * 4));
    quad(&mut b, &mut dl, [0, 0, 320, 240], 0, GREEN, G_RM_OPA_SURF);
    dl.push(gdp_set_scissor(0, 280 * 4, 200 * 4, 300 * 4, 220 * 4));
    interpret(&finish(b, dl))
}

#[test]
fn paired_pairless_scissor_equivalence() {
    let (device, queue) = headless_device_forced_fallback();
    for policy in POLICIES {
        let mut outputs = Vec::new();
        for paired in [false, true] {
            let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
            let scene = scissor_scene(paired);
            assert_eq!(
                renderer.render_into_store(&device, &queue, &scene, policy),
                Some(target(paired))
            );
            let pixels = scanout(&device, &queue, &renderer, target(paired), 320, 240);
            assert_pixels(&pixels, 320, 240, |x, y| {
                if in_rect(x, y, [96, 80, 224, 176]) {
                    GREEN
                } else if in_rect(x, y, [24, 32, 136, 104]) {
                    RED
                } else {
                    BLACK
                }
            });
            outputs.push(pixels);
        }
        assert_eq!(outputs[0], outputs[1], "{policy:?}");
    }
}

fn fill(dl: &mut Vec<Command>, [left, top, right, bottom]: [u32; 4], color: u32) {
    dl.extend([
        gdp_pipe_sync(),
        gdp_set_cycle_type(3),
        gdp_set_fill_color(color),
        gdp_fill_rectangle(left * 4, top * 4, (right - 1) * 4, (bottom - 1) * 4),
        gdp_pipe_sync(),
        gdp_set_cycle_type(0),
    ]);
}

fn interleaving_scene(paired: bool) -> Built {
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, paired);
    if paired {
        dl.push(gdp_set_color_image(0, 2, 320, DEPTH_ADDRESS));
        fill(&mut dl, [0, 0, 320, 240], 0xfffc_fffc);
        dl.push(gdp_set_color_image(0, 2, 320, COLOR_ADDRESS));
        fill(&mut dl, [0, 0, 320, 240], 0x0001_0001);
    } else {
        quad(&mut b, &mut dl, [0, 0, 320, 240], 0, BLACK, G_RM_OPA_SURF);
    }
    quad(&mut b, &mut dl, [32, 32, 288, 208], 32, BLUE, OPAQUE);
    quad(&mut b, &mut dl, [48, 48, 160, 160], 32, RED, DECAL);
    if paired {
        fill(&mut dl, [112, 64, 224, 112], 0x07c1_07c1);
    } else {
        quad(
            &mut b,
            &mut dl,
            [112, 64, 224, 112],
            0,
            GREEN,
            G_RM_OPA_SURF,
        );
    }
    quad(&mut b, &mut dl, [144, 80, 272, 192], 0, YELLOW, OPAQUE);
    quad(&mut b, &mut dl, [128, 128, 240, 200], 0, MAGENTA, DECAL);
    if paired {
        fill(&mut dl, [192, 144, 256, 176], 0x07ff_07ff);
    } else {
        quad(
            &mut b,
            &mut dl,
            [192, 144, 256, 176],
            0,
            CYAN,
            G_RM_OPA_SURF,
        );
    }
    let transparent_mode = Z_CMP | Z_UPD | IM_RD | FORCE_BL | gbl_c1(CLR_IN, A_IN, CLR_MEM, B_1MA);
    quad(
        &mut b,
        &mut dl,
        [64, 64, 96, 96],
        0,
        [0; 4],
        transparent_mode,
    );
    quad(&mut b, &mut dl, [72, 72, 88, 88], 0, WHITE, DECAL);
    finish(b, dl)
}

#[path = "../../tests/common/workload_semantics.rs"]
mod workload_semantics;
use workload_semantics::expected as interleaving_expected;

#[test]
fn opaque_decal_rect_order_is_preserved() {
    let (device, queue) = headless_device_forced_fallback();
    for policy in POLICIES {
        let mut outputs = Vec::new();
        for paired in [false, true] {
            let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
            let scene = interpret(&interleaving_scene(paired));
            assert_eq!(
                renderer.render_into_store(&device, &queue, &scene, policy),
                Some(target(paired))
            );
            let pixels = scanout(&device, &queue, &renderer, target(paired), 320, 240);
            assert_pixels(&pixels, 320, 240, interleaving_expected);
            outputs.push(pixels);
        }
        assert_eq!(outputs[0], outputs[1], "{policy:?}");
    }
}

#[test]
fn legacy_draws_before_cimg_are_retained() {
    let (device, queue) = headless_device_forced_fallback();
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, false);
    quad(&mut b, &mut dl, [24, 32, 136, 176], 0, RED, G_RM_OPA_SURF);
    dl.push(gdp_set_color_image(0, 2, 320, 0));
    quad(&mut b, &mut dl, [184, 64, 296, 208], 0, BLUE, G_RM_OPA_SURF);
    let scene = interpret(&finish(b, dl));
    for policy in POLICIES {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
        assert_eq!(
            renderer.render_into_store(&device, &queue, &scene, policy),
            Some(TargetId::Guest(0))
        );
        let legacy = scanout(&device, &queue, &renderer, TargetId::Legacy, 320, 240);
        assert_pixels(&legacy, 320, 240, |x, y| {
            if in_rect(x, y, [24, 32, 136, 176]) {
                RED
            } else {
                BACKGROUND
            }
        });
        let guest = scanout(&device, &queue, &renderer, TargetId::Guest(0), 320, 240);
        assert_pixels(&guest, 320, 240, |x, y| {
            if in_rect(x, y, [184, 64, 296, 208]) {
                BLUE
            } else {
                BACKGROUND
            }
        });
    }
}

#[test]
fn decal_first_initializes_depth() {
    let (device, queue) = headless_device_forced_fallback();
    for policy in POLICIES {
        for paired in [false, true] {
            let mut b = DlBuilder::new();
            let mut dl = setup(&mut b, paired);
            dl.push(gsp_viewport(viewport(&mut b, true)));
            quad(&mut b, &mut dl, [32, 32, 112, 112], 0, RED, DECAL);
            dl.push(gsp_viewport(viewport(&mut b, false)));
            quad(&mut b, &mut dl, [144, 64, 240, 192], 0, GREEN, OPAQUE);
            quad(&mut b, &mut dl, [176, 80, 224, 128], 0, BLUE, DECAL);
            let scene = interpret(&finish(b, dl));
            let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
            assert_eq!(
                renderer.render_into_store(&device, &queue, &scene, policy),
                Some(target(paired))
            );
            let pixels = scanout(&device, &queue, &renderer, target(paired), 320, 240);
            assert_pixels(&pixels, 320, 240, |x, y| {
                if in_rect(x, y, [176, 80, 224, 128]) {
                    BLUE
                } else if in_rect(x, y, [144, 64, 240, 192]) {
                    GREEN
                } else if in_rect(x, y, [32, 32, 112, 112]) {
                    RED
                } else {
                    BACKGROUND
                }
            });
        }
    }
}

#[test]
fn logical_extent_survives_canvas_resize() {
    let (device, queue) = headless_device_forced_fallback();
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, false);
    dl.push(gdp_set_scissor(0, 64 * 4, 24 * 4, 192 * 4, 144 * 4));
    quad(&mut b, &mut dl, [32, 40, 208, 168], 32, RED, OPAQUE);
    dl.push(gdp_set_scissor(0, 128 * 4, 112 * 4, 256 * 4, 192 * 4));
    quad(&mut b, &mut dl, [144, 96, 272, 216], 0, GREEN, OPAQUE);
    let scene = interpret(&finish(b, dl));
    for policy in POLICIES {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
        for (width, height) in [(320, 240), (640, 480), (640, 240), (320, 480), (320, 240)] {
            renderer.resize(&device, width, height);
            renderer.begin_frame();
            assert_eq!(
                renderer.render_into_store(&device, &queue, &scene, policy),
                Some(TargetId::Legacy)
            );
            let pixels = scanout(&device, &queue, &renderer, TargetId::Legacy, width, height);
            assert_pixels(&pixels, width, height, |x, y| {
                let (x, y) = (x * 320 / width, y * 240 / height);
                if in_rect(x, y, [144, 112, 256, 192]) {
                    GREEN
                } else if in_rect(x, y, [64, 40, 192, 144]) {
                    RED
                } else {
                    BACKGROUND
                }
            });
        }
    }
}

#[cfg(feature = "capture")]
fn interleaving_fixture() -> crate::capture::Fixture {
    let built = interleaving_scene(true);
    super::capture_fixture::make_image(built.rdram, built.entry, crate::Microcode::F3dex2, 320, 240, crate::capture::Provenance {
        decomp_revision: "libultra gbi.h; authored library-contract PR 6".into(),
        source_symbols: "gsSP2Triangles, gsDPFillRectangle, gsDPSetRenderMode, gsDPSetColorImage".into(),
        command_vector: "Blue opaque [32,288)x[32,208) z=32; red decal [48,160)x[48,160) z=32; green fill [112,224)x[64,112); yellow opaque [144,272)x[80,192) z=0; magenta decal [128,240)x[128,200) z=0; cyan fill [192,256)x[144,176); alpha-zero depth writer [64,96)x[64,96) z=0; white decal [72,88)x[72,88) z=0. Projection Z=1/128, viewport Z scale/translation=511. No draw may read later depth writes.".into(),
        synthetic_data: "IMAGE BE F3DEX2 commands, authored integer-edge geometry and flat primary colors, dither disabled; RGBA16 color at 0x100000, cleared depth at 0x200000. Expected RGB and overlap mask are exported from literal rectangular coverage, independently of rendering. No game capture.".into(),
    })
}

#[cfg(feature = "capture")]
#[test]
fn workload_interleaving_fixture_pixels() {
    let fixture = interleaving_fixture();
    assert_eq!(
        fixture.to_bytes().unwrap(),
        include_bytes!("../../tests/fixtures/workload-interleaving.f3dcap")
    );
    assert_eq!(
        fixture.tasks[0].source.memory,
        crate::capture::MemoryLayout::IMAGE
    );
    assert!(fixture.tasks[0].entry.is_multiple_of(8));
    let (device, queue) = headless_device_forced_fallback();
    let output = pollster::block_on(fixture.replay(device, queue)).unwrap();
    assert!(
        output.diagnostics.iter().all(Vec::is_empty),
        "{:?}",
        output.diagnostics
    );
    assert_pixels(&output.rgba8, 320, 240, interleaving_expected);
}

#[cfg(feature = "capture")]
#[test]
#[ignore = "writes an RT64 oracle fixture and independent expected pixels to FAST3D_WRITE_FIXTURES"]
fn write_rt64_workload_interleaving_fixture() {
    let directory = std::env::var_os("FAST3D_WRITE_FIXTURES").expect("set FAST3D_WRITE_FIXTURES");
    let directory = std::path::Path::new(&directory);
    std::fs::create_dir_all(directory).unwrap();
    std::fs::write(
        directory.join("workload-interleaving.f3dcap"),
        interleaving_fixture().to_bytes().unwrap(),
    )
    .unwrap();
    let mut expected = Vec::with_capacity(320 * 240 * 4);
    let mut overlap = Vec::with_capacity(320 * 240 * 4);
    for y in 0..240 {
        for x in 0..320 {
            expected.extend(interleaving_expected(x, y));
            let covered = [
                [48, 48, 160, 160],
                [112, 64, 224, 112],
                [144, 80, 272, 192],
                [144, 128, 240, 192],
                [192, 144, 256, 176],
                [64, 64, 96, 96],
                [72, 72, 88, 88],
            ]
            .into_iter()
            .filter(|&rect| in_rect(x, y, rect))
            .count();
            let value = if covered >= 2 { 255 } else { 0 };
            overlap.extend([value, value, value, 255]);
        }
    }
    std::fs::write(
        directory.join("workload-interleaving.expected.rgba8"),
        expected,
    )
    .unwrap();
    std::fs::write(
        directory.join("workload-interleaving.overlap.rgba8"),
        overlap,
    )
    .unwrap();
}

#[test]
fn culled_target_applies_first_touch_clear() {
    let (device, queue) = headless_device_forced_fallback();
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, true);
    dl.push(gsp_set_geometrymode(G_CULL_BOTH));
    quad(&mut b, &mut dl, [0, 0, 320, 240], 0, GREEN, OPAQUE);
    let culled = interpret(&finish(b, dl));
    assert!(culled.indices.is_empty());
    assert_eq!(culled.framebuffer_pairs.len(), 1);
    for policy in POLICIES {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
        renderer.render_into_store(&device, &queue, &culled, policy);
        let pixels = scanout(&device, &queue, &renderer, target(true), 320, 240);
        assert_pixels(&pixels, 320, 240, |_, _| BACKGROUND);
        renderer.render_into_store(&device, &queue, &color_task(true, 0), policy);
        renderer.begin_frame();
        renderer.render_into_store(&device, &queue, &culled, policy);
        renderer.render_into_store(&device, &queue, &color_task(true, 1), policy);
        let pixels = scanout(&device, &queue, &renderer, target(true), 320, 240);
        assert_pixels(&pixels, 320, 240, |x, y| {
            if in_rect(x, y, [24, 40, 88, 96]) {
                BLUE
            } else if policy == ClearPolicy::PerFrame {
                BACKGROUND
            } else if x < 160 {
                RED
            } else {
                YELLOW
            }
        });
    }
}
