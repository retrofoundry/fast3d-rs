use crate::render::workload::TargetId;
use crate::render::{headless_device_forced_fallback, SceneRenderer};
use crate::tests::common::pixels_from_render;
use crate::tests::dl_builder::{Built, Command, DlBuilder};
use crate::ClearPolicy;
use n64_gbi::{consts::*, encode::*};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const A: u32 = 0x0010_0000;
const B: u32 = 0x0030_0000;
const Z: u32 = 0x0020_0000;
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];
const BACKGROUND: [u8; 4] = [13, 13, 20, 255];
const POLICIES: [ClearPolicy; 2] = [ClearPolicy::PerFrame, ClearPolicy::Persist];

fn setup(b: &mut DlBuilder, color: u32, depth: u32) -> Vec<Command> {
    let projection = b.matrix(n64_gbi::gu::gu_scale(1.0 / 256.0, 1.0 / 256.0, 1.0 / 128.0));
    let model = b.matrix(n64_gbi::gu::gu_scale(1.0, 1.0, 1.0));
    let viewport = b.viewport(Vp {
        vscale: [1024, 1024, 511, 0],
        vtrans: [640, 480, 511, 0],
    });
    let color_pass = CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: 4,
    };
    let alpha_pass = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 4,
    };
    vec![
        gdp_set_depth_image(depth),
        gdp_set_color_image(0, 2, 320, color),
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
        gdp_set_combine_lerp(color_pass, alpha_pass, color_pass, alpha_pass),
    ]
}

fn quad(b: &mut DlBuilder, dl: &mut Vec<Command>, rect: [i16; 4], z: i16, rgba: [u8; 4]) {
    let [left, top, right, bottom] = rect;
    let [r, g, blue, a] = rgba;
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
        gdp_set_cycle_type(0),
        gdp_set_render_mode(G_RM_OPA_SURF | Z_CMP | Z_UPD, G_RM_OPA_SURF2),
        gsp_vertex(0, 4, vertices),
        gsp_2triangles(0, 1, 2, 0, 2, 3),
    ]);
}

fn fill(dl: &mut Vec<Command>, word: u32, [left, top, right, bottom]: [u32; 4]) {
    dl.extend([
        gdp_pipe_sync(),
        gdp_set_cycle_type(3),
        gdp_set_fill_color(word),
        gdp_fill_rectangle(left * 4, top * 4, right * 4, bottom * 4),
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

fn scanout(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut SceneRenderer,
    address: u32,
) -> Vec<u8> {
    pixels_from_render(device, queue, 320, 240, FORMAT, |view| {
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        renderer.scanout(
            device,
            &mut encoder,
            view,
            TargetId::Guest(address.into()),
            None,
        );
        queue.submit(Some(encoder.finish()));
    })
}

fn assert_pixels(pixels: &[u8], expected: impl Fn(u32, u32) -> [u8; 4]) {
    assert_eq!(pixels.len(), 320 * 240 * 4);
    for (i, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (i as u32 % 320, i as u32 / 320);
        assert_eq!(*pixel, expected(x, y), "({x}, {y})");
    }
}

fn inside(x: u32, y: u32, [left, top, right, bottom]: [u32; 4]) -> bool {
    (left..right).contains(&x) && (top..bottom).contains(&y)
}

fn shared_depth_scene() -> Built {
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, Z, Z);
    fill(&mut dl, 0xfffc_fffc, [0, 0, 319, 239]);
    dl.push(gdp_set_color_image(0, 2, 320, A));
    fill(&mut dl, 0x0001_0001, [0, 0, 319, 239]);
    quad(&mut b, &mut dl, [32, 32, 144, 208], 0, RED);
    dl.push(gdp_set_color_image(0, 2, 320, B));
    fill(&mut dl, 0x0001_0001, [0, 0, 319, 239]);
    quad(&mut b, &mut dl, [176, 32, 288, 208], 0, GREEN);
    quad(&mut b, &mut dl, [32, 32, 144, 208], 64, BLUE);
    dl.push(gdp_set_color_image(0, 2, 320, A));
    quad(&mut b, &mut dl, [32, 32, 288, 208], 64, BLUE);
    finish(b, dl)
}

#[path = "../../tests/common/depth_semantics.rs"]
mod depth_semantics;
use depth_semantics::expected as shared_depth_expected;

#[test]
fn depth_shared_across_color_switches() {
    let (device, queue) = headless_device_forced_fallback();
    let scene = interpret(&shared_depth_scene());
    for policy in POLICIES {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
        renderer.render_into_store(&device, &queue, &scene, policy);
        assert_pixels(
            &scanout(&device, &queue, &mut renderer, A),
            shared_depth_expected,
        );
        assert_pixels(&scanout(&device, &queue, &mut renderer, B), |x, y| {
            if inside(x, y, [176, 32, 288, 208]) {
                GREEN
            } else {
                BLACK
            }
        });
    }
}

fn draw_task(color: u32, depth: u32, z: i16, rgba: [u8; 4]) -> crate::hle::Scene {
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, color, depth);
    quad(&mut b, &mut dl, [32, 32, 288, 208], z, rgba);
    interpret(&finish(b, dl))
}

#[test]
fn depth_survives_task_boundary() {
    let (device, queue) = headless_device_forced_fallback();
    for policy in POLICIES {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
        renderer.render_into_store(&device, &queue, &draw_task(A, Z, 0, RED), policy);
        renderer.render_into_store(&device, &queue, &draw_task(B, Z, 64, BLUE), policy);
        assert_pixels(&scanout(&device, &queue, &mut renderer, B), |_, _| {
            BACKGROUND
        });
    }
}

#[test]
fn depth_legacy_opaque_write_reaches_later_guest_task() {
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, A, Z);
    dl.retain(|&(w0, _)| w0 >> 24 != 0xff);
    quad(&mut b, &mut dl, [32, 32, 288, 208], 0, RED);
    let scene = interpret(&finish(b, dl));
    assert!(scene.draw_runs.iter().all(|run| {
        let mode = &scene.render_modes[run.render_mode_index as usize];
        mode.z_test && mode.z_write && mode.z_mode != crate::hle::ZMode::Decal
    }));
    let (device, queue) = headless_device_forced_fallback();
    for policy in POLICIES {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
        renderer.render_into_store(&device, &queue, &scene, policy);
        assert_depth(&device, &queue, &renderer, Z.into(), [320, 240], |x, y| {
            if inside(x, y, [32, 32, 288, 208]) {
                511.0 / 1024.0
            } else {
                1.0
            }
        });
        renderer.render_into_store(&device, &queue, &draw_task(B, Z, 64, BLUE), policy);
        assert_pixels(&scanout(&device, &queue, &mut renderer, B), |_, _| {
            BACKGROUND
        });
    }
}

#[test]
fn depth_persist_vs_perframe() {
    let (device, queue) = headless_device_forced_fallback();
    for policy in POLICIES {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
        renderer.begin_frame();
        renderer.render_into_store(&device, &queue, &draw_task(A, Z, 0, RED), policy);
        renderer.begin_frame();
        renderer.render_into_store(&device, &queue, &draw_task(B, Z, 64, BLUE), policy);
        assert_pixels(&scanout(&device, &queue, &mut renderer, B), |x, y| {
            if policy == ClearPolicy::PerFrame && inside(x, y, [32, 32, 288, 208]) {
                BLUE
            } else {
                BACKGROUND
            }
        });
        renderer.render_into_store(&device, &queue, &draw_task(A, Z, 96, GREEN), policy);
        assert_pixels(&scanout(&device, &queue, &mut renderer, A), |x, y| {
            if policy == ClearPolicy::Persist && inside(x, y, [32, 32, 288, 208]) {
                RED
            } else {
                BACKGROUND
            }
        });
    }
}

fn depth_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &SceneRenderer,
    address: u64,
) -> (u32, u32, Vec<f32>) {
    let texture = &renderer.depthbuffers[&TargetId::Guest(address)].texture;
    let (width, height) = (texture.width(), texture.height());
    let bytes_per_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("depth-acceptance-readback"),
        size: u64::from(bytes_per_row * height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::DepthOnly,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        texture.size(),
    );
    queue.submit(Some(encoder.finish()));
    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let data = slice.get_mapped_range();
    let pixels = data
        .chunks(bytes_per_row as usize)
        .flat_map(|row| {
            row[..width as usize * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| f32::from_le_bytes(*bytes))
        })
        .collect();
    drop(data);
    readback.unmap();
    (width, height, pixels)
}

fn assert_depth(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &SceneRenderer,
    address: u64,
    extent: [u32; 2],
    expected: impl Fn(u32, u32) -> f32,
) {
    let (width, height, pixels) = depth_pixels(device, queue, renderer, address);
    assert_eq!([width, height], extent);
    for (i, actual) in pixels.into_iter().enumerate() {
        let (x, y) = (i as u32 % width, i as u32 / width);
        let expected = expected(x, y);
        assert!(
            (actual - expected).abs() < 0.000001,
            "Z {address:#x} ({x}, {y}): {actual} != {expected}"
        );
    }
}

fn depth_fill_scene(
    address: u64,
    width: u32,
    height: u32,
    scissor: [u32; 4],
    rect: [u32; 4],
    word: u32,
) -> crate::hle::Scene {
    let b = DlBuilder::new();
    let [left, top, right, bottom] = scissor;
    let mut dl = vec![
        gdp_set_depth_image(Z),
        gdp_set_color_image(0, 2, width, Z),
        gdp_set_scissor(0, 0, 0, width * 4, height * 4),
        gdp_set_scissor(0, left * 4, top * 4, right * 4, bottom * 4),
    ];
    fill(&mut dl, word, rect);
    let mut scene = interpret(&finish(b, dl));
    for pair in &mut scene.framebuffer_pairs {
        pair.color_image.addr = address;
        pair.depth_image = Some(address);
    }
    scene
}

#[test]
fn depth_addresses_are_independent() {
    let (device, queue) = headless_device_forced_fallback();
    for policy in POLICIES {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
        for (address, word) in [
            (0x0000_0001_0020_0000, 0x4000_4000),
            (0x0000_0002_0020_0000, 0x8000_8000),
        ] {
            renderer.render_into_store(
                &device,
                &queue,
                &depth_fill_scene(address, 7, 5, [0, 0, 7, 5], [0, 0, 6, 4], word),
                policy,
            );
        }
        assert_depth(
            &device,
            &queue,
            &renderer,
            0x0000_0001_0020_0000,
            [7, 5],
            |_, _| 196608.0 / 262144.0,
        );
        assert_depth(
            &device,
            &queue,
            &renderer,
            0x0000_0002_0020_0000,
            [7, 5],
            |_, _| 245760.0 / 262144.0,
        );
    }
}

#[test]
fn depth_address_zero_is_valid() {
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
    renderer.render_into_store(
        &device,
        &queue,
        &draw_task(A, 0, 0, RED),
        ClearPolicy::Persist,
    );
    renderer.render_into_store(
        &device,
        &queue,
        &draw_task(B, 0, 64, BLUE),
        ClearPolicy::Persist,
    );
    assert_pixels(&scanout(&device, &queue, &mut renderer, B), |_, _| {
        BACKGROUND
    });
    assert!(renderer.depthbuffers.contains_key(&TargetId::Guest(0)));
    renderer.render_into_store(
        &device,
        &queue,
        &depth_fill_scene(0, 320, 240, [0, 0, 320, 240], [0, 0, 319, 239], 0x4000_4000),
        ClearPolicy::Persist,
    );
    assert_depth(&device, &queue, &renderer, 0, [320, 240], |_, _| {
        196608.0 / 262144.0
    });
}

#[test]
fn depth_clear_hits_selected_storage() {
    let (device, queue) = headless_device_forced_fallback();
    for policy in POLICIES {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
        for (address, word) in [(u64::from(Z), 0x4000_4000), (u64::from(B), 0x8000_8000)] {
            renderer.render_into_store(
                &device,
                &queue,
                &depth_fill_scene(address, 7, 5, [0, 0, 7, 5], [0, 0, 6, 4], word),
                policy,
            );
        }
        renderer.render_into_store(
            &device,
            &queue,
            &depth_fill_scene(Z.into(), 7, 5, [0, 0, 7, 5], [2, 1, 4, 3], 0xfffc_fffc),
            policy,
        );
        assert_depth(&device, &queue, &renderer, Z.into(), [7, 5], |x, y| {
            if inside(x, y, [2, 1, 5, 4]) {
                262143.0 / 262144.0
            } else {
                196608.0 / 262144.0
            }
        });
        assert_depth(&device, &queue, &renderer, B.into(), [7, 5], |_, _| {
            245760.0 / 262144.0
        });
    }
}

#[test]
fn depth_clear_scissor_and_fill_parity() {
    let (device, queue) = headless_device_forced_fallback();
    for width in [6, 7] {
        for policy in POLICIES {
            let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
            renderer.render_into_store(
                &device,
                &queue,
                &depth_fill_scene(
                    Z.into(),
                    width,
                    5,
                    [0, 0, width, 5],
                    [0, 0, width - 1, 4],
                    0xfffc_fffc,
                ),
                policy,
            );
            renderer.render_into_store(
                &device,
                &queue,
                &depth_fill_scene(Z.into(), width, 5, [1, 1, 6, 4], [2, 0, 6, 2], 0x4000_8000),
                policy,
            );
            assert_depth(&device, &queue, &renderer, Z.into(), [width, 5], |x, y| {
                if inside(x, y, [2, 1, 6, 3]) {
                    if (y * width + x).is_multiple_of(2) {
                        196608.0 / 262144.0
                    } else {
                        245760.0 / 262144.0
                    }
                } else {
                    262143.0 / 262144.0
                }
            });
        }
    }
}

#[test]
fn depth_fill_packed_exponent_ranges() {
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
    let cases = [
        (0x0000_0000, 0.0),
        (0x2000_2000, 131072.0),
        (0x4000_4000, 196608.0),
        (0x6000_6000, 229376.0),
        (0x8000_8000, 245760.0),
        (0xa000_a000, 253952.0),
        (0xc000_c000, 258048.0),
        (0xe000_e000, 260096.0),
        (0xfffc_fffc, 262143.0),
        (0x4007_4007, 196624.0),
    ];
    for (word, fixed_depth) in cases {
        renderer.render_into_store(
            &device,
            &queue,
            &depth_fill_scene(Z.into(), 7, 5, [0, 0, 7, 5], [0, 0, 6, 4], word),
            ClearPolicy::Persist,
        );
        assert_depth(&device, &queue, &renderer, Z.into(), [7, 5], |_, _| {
            fixed_depth / 262144.0
        });
    }
}

#[test]
fn depth_height_growth_preserves_rows() {
    let (device, queue) = headless_device_forced_fallback();
    for policy in POLICIES {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
        renderer.render_into_store(
            &device,
            &queue,
            &depth_fill_scene(Z.into(), 7, 3, [0, 0, 7, 3], [0, 0, 6, 2], 0x4000_4000),
            policy,
        );
        assert_depth(&device, &queue, &renderer, Z.into(), [7, 3], |_, _| {
            196608.0 / 262144.0
        });
        renderer.render_into_store(
            &device,
            &queue,
            &depth_fill_scene(Z.into(), 7, 6, [0, 0, 7, 6], [0, 5, 6, 5], 0x8000_8000),
            policy,
        );
        assert_depth(
            &device,
            &queue,
            &renderer,
            Z.into(),
            [7, 6],
            |_, y| match y {
                0..3 => 196608.0 / 262144.0,
                5 => 245760.0 / 262144.0,
                _ => 1.0,
            },
        );
        renderer.render_into_store(
            &device,
            &queue,
            &depth_fill_scene(Z.into(), 7, 2, [0, 0, 7, 2], [0, 0, 0, 0], 0),
            policy,
        );
        assert_depth(&device, &queue, &renderer, Z.into(), [7, 6], |x, y| {
            if x == 0 && y == 0 {
                0.0
            } else {
                match y {
                    0..3 => 196608.0 / 262144.0,
                    5 => 245760.0 / 262144.0,
                    _ => 1.0,
                }
            }
        });
    }
}

#[test]
fn depth_stride_change_is_explicit() {
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
    renderer.render_into_store(
        &device,
        &queue,
        &depth_fill_scene(Z.into(), 7, 5, [0, 0, 7, 5], [0, 0, 6, 4], 0x4000_4000),
        ClearPolicy::Persist,
    );
    renderer.render_into_store(
        &device,
        &queue,
        &depth_fill_scene(Z.into(), 9, 5, [0, 0, 9, 5], [0, 0, 0, 0], 0x8000_8000),
        ClearPolicy::Persist,
    );
    assert!(renderer.diagnostics.iter().any(|diagnostic| matches!(diagnostic.kind,
        crate::diag::DiagKind::UnsupportedImageReinterpretation { address, depth: true } if address == u64::from(Z)
    )), "{:?}", renderer.diagnostics);
    assert_depth(&device, &queue, &renderer, Z.into(), [9, 5], |x, y| {
        if x == 0 && y == 0 {
            245760.0 / 262144.0
        } else {
            1.0
        }
    });
}

#[test]
fn depth_alias_nonfill_is_diagnostic() {
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, Z, Z);
    quad(&mut b, &mut dl, [32, 32, 288, 208], 0, RED);
    let built = finish(b, dl);
    let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
    assert!(
        result
            .diags
            .iter()
            .any(|diagnostic| matches!(diagnostic.kind,
                crate::diag::DiagKind::UnsupportedDepthAlias { address } if address == u64::from(Z)
            )),
        "{:?}",
        result.diags
    );
    assert!(result.scene.indices.is_empty());
}

#[test]
fn depth_clear_before_and_after_draws_is_ordered() {
    let (device, queue) = headless_device_forced_fallback();
    for policy in POLICIES {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
        renderer.render_into_store(&device, &queue, &draw_task(A, Z, 0, RED), policy);
        renderer.render_into_store(
            &device,
            &queue,
            &depth_fill_scene(
                Z.into(),
                320,
                240,
                [0, 0, 320, 240],
                [64, 64, 127, 127],
                0xfffc_fffc,
            ),
            policy,
        );
        assert_depth(&device, &queue, &renderer, Z.into(), [320, 240], |x, y| {
            if inside(x, y, [64, 64, 128, 128]) {
                262143.0 / 262144.0
            } else if !inside(x, y, [32, 32, 288, 208]) {
                1.0
            } else {
                511.0 / 1024.0
            }
        });
        renderer.render_into_store(&device, &queue, &draw_task(A, Z, 64, BLUE), policy);
        assert_pixels(&scanout(&device, &queue, &mut renderer, A), |x, y| {
            if inside(x, y, [64, 64, 128, 128]) {
                BLUE
            } else if inside(x, y, [32, 32, 288, 208]) {
                RED
            } else {
                BACKGROUND
            }
        });
    }
}

#[cfg(feature = "capture")]
fn shared_depth_fixture() -> crate::capture::Fixture {
    let built = shared_depth_scene();
    let mut fixture = super::capture_fixture::make_image(
        built.rdram, built.entry, crate::Microcode::F3dex2, 320, 240,
        crate::capture::Provenance {
            decomp_revision: "libultra gbi.h; authored library-contract PR 7".into(),
            source_symbols: "gsDPSetDepthImage, gsDPSetColorImage, gsDPFillRectangle, gsSP2Triangles".into(),
            command_vector: "Clear Z=0x200000 with packed fffc. Clear A=0x100000 black; draw red [32,144)x[32,208) z=0. Switch to B=0x300000 sharing Z; clear B black; draw green [176,288)x[32,208) z=0, then blue [32,144)x[32,208) z=64 (occluded). Switch back to A/Z; draw blue [32,288)x[32,208) z=64, visible only in [144,176)x[32,208). Projection Z=1/128, viewport Z scale/translation=511.".into(),
            synthetic_data: "IMAGE BE F3DEX2 commands, integer-edge flat primary geometry, RGBA16 color targets, dither disabled. Expected final A pixels and the PersistentDepthByAddress mask are independently enumerated rectangles. Shared-Z authored acceptance only; no game capture or live sm64 claim.".into(),
        },
    );
    fixture.frame.vi = None;
    fixture
}

#[cfg(feature = "capture")]
#[test]
#[ignore = "writes an RT64 oracle fixture and independent expectations to FAST3D_WRITE_FIXTURES"]
fn write_rt64_shared_depth_fixture() {
    let directory = std::env::var_os("FAST3D_WRITE_FIXTURES").expect("set FAST3D_WRITE_FIXTURES");
    let directory = std::path::Path::new(&directory);
    std::fs::create_dir_all(directory).unwrap();
    std::fs::write(
        directory.join("shared-depth.f3dcap"),
        shared_depth_fixture().to_bytes().unwrap(),
    )
    .unwrap();
    let mut expected = Vec::with_capacity(320 * 240 * 4);
    let mut mask = Vec::with_capacity(320 * 240 * 4);
    for y in 0..240 {
        for x in 0..320 {
            expected.extend(shared_depth_expected(x, y));
            let changed = inside(x, y, [32, 32, 144, 208]) || inside(x, y, [176, 32, 288, 208]);
            let value = if changed { 255 } else { 0 };
            mask.extend([value, value, value, 255]);
        }
    }
    std::fs::write(directory.join("shared-depth.expected.rgba8"), expected).unwrap();
    std::fs::write(
        directory.join("shared-depth.PersistentDepthByAddress.rgba8"),
        mask,
    )
    .unwrap();
}

#[cfg(feature = "capture")]
#[test]
fn shared_depth_fixture_pixels() {
    let fixture = shared_depth_fixture();
    assert_eq!(
        fixture,
        crate::capture::Fixture::from_bytes(include_bytes!(
            "../../tests/fixtures/shared-depth.f3dcap"
        ))
        .unwrap()
    );
    assert_eq!(
        fixture.tasks[0].source.memory,
        crate::capture::MemoryLayout::IMAGE
    );
    let (device, queue) = headless_device_forced_fallback();
    let output = pollster::block_on(fixture.replay(device, queue)).unwrap();
    assert!(
        output.diagnostics.iter().all(Vec::is_empty),
        "{:?}",
        output.diagnostics
    );
    assert_pixels(&output.rgba8, shared_depth_expected);
}

#[test]
fn depth_growth_within_scene_preserves_earlier_fill() {
    let (device, queue) = headless_device_forced_fallback();
    let b = DlBuilder::new();
    let mut dl = vec![
        gdp_set_depth_image(Z),
        gdp_set_color_image(0, 2, 7, Z),
        gdp_set_scissor(0, 0, 0, 28, 8),
    ];
    fill(&mut dl, 0x4000_4000, [0, 0, 6, 1]);
    dl.push(gdp_set_scissor(0, 0, 0, 28, 16));
    fill(&mut dl, 0x8000_8000, [0, 3, 6, 3]);
    let scene = interpret(&finish(b, dl));
    for policy in POLICIES {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
        renderer.render_into_store(&device, &queue, &scene, policy);
        assert_depth(
            &device,
            &queue,
            &renderer,
            Z.into(),
            [7, 4],
            |_, y| match y {
                0..2 => 196608.0 / 262144.0,
                2 => 1.0,
                _ => 245760.0 / 262144.0,
            },
        );
    }
}

#[test]
fn shallow_color_target_uses_taller_stored_depth() {
    let (device, queue) = headless_device_forced_fallback();
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, B, Z);
    dl.push(gdp_set_scissor(0, 0, 0, 1280, 480));
    quad(&mut b, &mut dl, [32, 32, 288, 208], -64, BLUE);
    let shallow = interpret(&finish(b, dl));
    for policy in POLICIES {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
        renderer.render_into_store(&device, &queue, &draw_task(A, Z, 0, RED), policy);
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        renderer.render_into_store(&device, &queue, &shallow, policy);
        assert!(pollster::block_on(scope.pop()).is_none());
        renderer.render_into_store(&device, &queue, &draw_task(B, Z, 64, GREEN), policy);
        assert_pixels(&scanout(&device, &queue, &mut renderer, B), |x, y| {
            if inside(x, y, [32, 32, 288, 120]) {
                BLUE
            } else {
                BACKGROUND
            }
        });
        assert_pixels(&scanout(&device, &queue, &mut renderer, A), |x, y| {
            if inside(x, y, [32, 32, 288, 208]) {
                RED
            } else {
                BACKGROUND
            }
        });
        assert_depth(&device, &queue, &renderer, Z.into(), [320, 240], |x, y| {
            if inside(x, y, [32, 32, 288, 120]) {
                255.5 / 1024.0
            } else if inside(x, y, [32, 120, 288, 208]) {
                511.0 / 1024.0
            } else {
                1.0
            }
        });
    }
}

struct ImageHardware(Vec<u8>);

impl crate::Hardware for ImageHardware {
    fn rdram(&self) -> impl crate::Rdram + '_ {
        crate::RdramImage::new(&self.0)
    }
}

#[test]
fn depth_generation_diagnostics_reach_process_dl() {
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer = crate::Renderer::with_device(
        device,
        queue,
        crate::PresentTarget::Headless {
            format: FORMAT,
            width: 320,
            height: 240,
        },
        crate::RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: Some(FORMAT),
            clear_policy: ClearPolicy::Persist,
            power_preference: wgpu::PowerPreference::None,
        },
    );
    for (width, color, expected_warns) in [(7, A, 0), (9, B, 1)] {
        let b = DlBuilder::new();
        let mut dl = vec![
            gdp_set_depth_image(Z),
            gdp_set_color_image(0, 2, width, Z),
            gdp_set_scissor(0, 0, 0, width * 4, 20),
        ];
        fill(&mut dl, 0x4000_4000, [0, 0, width - 1, 4]);
        dl.push(gdp_set_color_image(0, 2, width, color));
        fill(&mut dl, 0x0001_0001, [0, 0, width - 1, 4]);
        let built = finish(b, dl);
        let mut diagnostics = Vec::new();
        let summary = renderer.process_dl(
            &ImageHardware(built.rdram),
            built.entry.into(),
            crate::Microcode::F3dex2,
            &mut diagnostics,
        );
        assert_eq!(summary.warns, expected_warns, "{diagnostics:?}");
        assert_eq!(summary.errors, 0, "{diagnostics:?}");
        assert!(summary.renderable);
        if expected_warns != 0 {
            assert!(diagnostics.iter().any(|diagnostic| matches!(diagnostic.kind,
                crate::diag::DiagKind::UnsupportedImageReinterpretation { address, depth: true } if address == u64::from(Z)
            )));
        }
        assert_depth(
            renderer.device(),
            renderer.queue(),
            &renderer.inner,
            Z.into(),
            [width, 5],
            |_, _| 196608.0 / 262144.0,
        );
    }
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, Z, Z);
    quad(&mut b, &mut dl, [32, 32, 288, 208], 0, RED);
    let built = finish(b, dl);
    let mut diagnostics = Vec::new();
    let summary = renderer.process_dl(
        &ImageHardware(built.rdram),
        built.entry.into(),
        crate::Microcode::F3dex2,
        &mut diagnostics,
    );
    assert!(summary.errors > 0, "{diagnostics:?}");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic.kind,
            crate::diag::DiagKind::UnsupportedDepthAlias { address } if address == u64::from(Z)
        )));
    assert_depth(
        renderer.device(),
        renderer.queue(),
        &renderer.inner,
        Z.into(),
        [9, 5],
        |_, _| 196608.0 / 262144.0,
    );
}

#[test]
fn depth_legacy_draws_snapshot_selected_address() {
    let (device, queue) = headless_device_forced_fallback();
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, A, Z);
    dl.retain(|&(w0, _)| w0 >> 24 != 0xff);
    quad(&mut b, &mut dl, [32, 32, 144, 208], 0, RED);
    dl.push(gdp_set_depth_image(B));
    quad(&mut b, &mut dl, [176, 32, 288, 208], 0, GREEN);
    dl.extend([gdp_set_depth_image(Z), gdp_set_color_image(0, 2, 320, A)]);
    quad(&mut b, &mut dl, [32, 32, 288, 208], 64, BLUE);
    dl.extend([
        gdp_set_depth_image(B),
        gdp_set_color_image(0, 2, 320, 0x0040_0000),
    ]);
    quad(&mut b, &mut dl, [32, 32, 288, 208], 64, RED);
    let scene = interpret(&finish(b, dl));
    for policy in POLICIES {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
        renderer.render_into_store(&device, &queue, &scene, policy);
        assert_pixels(&scanout(&device, &queue, &mut renderer, A), |x, y| {
            if inside(x, y, [144, 32, 288, 208]) {
                BLUE
            } else {
                BACKGROUND
            }
        });
        assert_pixels(
            &scanout(&device, &queue, &mut renderer, 0x0040_0000),
            |x, y| {
                if inside(x, y, [32, 32, 176, 208]) {
                    RED
                } else {
                    BACKGROUND
                }
            },
        );
        assert_depth(&device, &queue, &renderer, Z.into(), [320, 240], |x, y| {
            if inside(x, y, [32, 32, 144, 208]) {
                511.0 / 1024.0
            } else if inside(x, y, [144, 32, 288, 208]) {
                766.5 / 1024.0
            } else {
                1.0
            }
        });
        assert_depth(&device, &queue, &renderer, B.into(), [320, 240], |x, y| {
            if inside(x, y, [176, 32, 288, 208]) {
                511.0 / 1024.0
            } else if inside(x, y, [32, 32, 176, 208]) {
                766.5 / 1024.0
            } else {
                1.0
            }
        });
    }
}

#[test]
fn depth_attachment_height_does_not_change_color_presentation() {
    let (device, queue) = headless_device_forced_fallback();
    for tall_depth in [false, true] {
        for policy in POLICIES {
            let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, false);
            if tall_depth {
                renderer.render_into_store(
                    &device,
                    &queue,
                    &depth_fill_scene(
                        Z.into(),
                        320,
                        240,
                        [0, 0, 320, 240],
                        [0, 0, 319, 239],
                        0xfffc_fffc,
                    ),
                    policy,
                );
            }
            let b = DlBuilder::new();
            let mut dl = vec![
                gdp_set_depth_image(Z),
                gdp_set_color_image(0, 2, 320, B),
                gdp_set_scissor(0, 0, 0, 1280, 480),
            ];
            fill(&mut dl, 0xf801_f801, [0, 0, 319, 119]);
            renderer.render_into_store(&device, &queue, &interpret(&finish(b, dl)), policy);
            assert_pixels(&scanout(&device, &queue, &mut renderer, B), |_, _| RED);
            let b = DlBuilder::new();
            let mut dl = vec![
                gdp_set_depth_image(Z),
                gdp_set_color_image(0, 2, 320, B),
                gdp_set_scissor(0, 0, 0, 1280, 960),
            ];
            fill(&mut dl, 0x003f_003f, [0, 200, 319, 239]);
            renderer.render_into_store(&device, &queue, &interpret(&finish(b, dl)), policy);
            assert_pixels(&scanout(&device, &queue, &mut renderer, B), |_, y| {
                if y < 120 {
                    RED
                } else if y >= 200 {
                    BLUE
                } else {
                    BACKGROUND
                }
            });
        }
    }
}

#[test]
fn depth_legacy_extent_mismatch_preserves_guest_storage() {
    let (device, queue) = headless_device_forced_fallback();
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, A, Z);
    dl.retain(|&(w0, _)| w0 >> 24 != 0xff);
    quad(&mut b, &mut dl, [32, 32, 64, 64], 0, RED);
    let scene = interpret(&finish(b, dl));
    let pc = scene.draw_origins[0].pc;
    for (width, height) in [(160, 120), (480, 360), (640, 480), (640, 240), (320, 480)] {
        for policy in POLICIES {
            let mut renderer = SceneRenderer::new(&device, FORMAT, width, height, false);
            let initial = depth_fill_scene(
                Z.into(),
                320,
                240,
                [0, 0, 320, 240],
                [0, 0, 319, 239],
                0x4000_8000,
            );
            renderer.render_into_store(&device, &queue, &initial, policy);
            renderer.begin_frame();
            assert_eq!(
                renderer.render_into_store(&device, &queue, &scene, policy),
                None
            );
            assert_eq!(
                renderer.diagnostics,
                [crate::Diagnostic {
                    at: pc,
                    kind: crate::DiagKind::UnsupportedLegacyDepthExtent {
                        address: Z.into(),
                        canvas: (width, height),
                        depth: (320, 240),
                    },
                }]
            );
            assert!(!renderer.has_fb(TargetId::Legacy));
            assert_depth(&device, &queue, &renderer, Z.into(), [320, 240], |x, _| {
                if x.is_multiple_of(2) {
                    196608.0 / 262144.0
                } else {
                    245760.0 / 262144.0
                }
            });
            renderer.render_into_store(&device, &queue, &draw_task(B, Z, 64, BLUE), policy);
            assert_pixels(&scanout(&device, &queue, &mut renderer, B), |x, y| {
                if inside(x, y, [32, 32, 288, 208]) {
                    BLUE
                } else {
                    BACKGROUND
                }
            });
        }
    }
}

#[test]
fn depth_legacy_extent_diagnostic_reaches_process_dl_and_allows_guest_draws() {
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer = crate::Renderer::with_device(
        device,
        queue,
        crate::PresentTarget::Headless {
            format: FORMAT,
            width: 640,
            height: 480,
        },
        crate::RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: Some(FORMAT),
            clear_policy: ClearPolicy::Persist,
            power_preference: wgpu::PowerPreference::None,
        },
    );
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b, A, 0);
    dl.retain(|&(w0, _)| w0 >> 24 != 0xff);
    quad(&mut b, &mut dl, [32, 32, 288, 208], 0, RED);
    dl.push(gdp_set_color_image(0, 2, 320, B));
    quad(&mut b, &mut dl, [32, 32, 288, 208], 64, BLUE);
    let built = finish(b, dl);
    let pc = interpret(&built).draw_origins[0].pc;
    let mut diagnostics = Vec::new();
    let summary = renderer.process_dl(
        &ImageHardware(built.rdram),
        built.entry.into(),
        crate::Microcode::F3dex2,
        &mut diagnostics,
    );
    assert_eq!(summary.errors, 1, "{diagnostics:?}");
    assert_eq!(
        diagnostics,
        [crate::Diagnostic {
            at: pc,
            kind: crate::DiagKind::UnsupportedLegacyDepthExtent {
                address: 0,
                canvas: (640, 480),
                depth: (320, 240),
            },
        }]
    );
    assert_eq!(renderer.last_scanout_addr, Some(TargetId::Guest(B.into())));
    assert!(!renderer.inner.has_fb(TargetId::Legacy));
    assert_pixels(
        &scanout(
            &renderer.device.clone(),
            &renderer.queue.clone(),
            &mut renderer.inner,
            B,
        ),
        |x, y| {
            if inside(x, y, [32, 32, 288, 208]) {
                BLUE
            } else {
                BACKGROUND
            }
        },
    );
}

#[test]
fn depth_attachment_height_does_not_change_framebuffer_sampling() {
    let (device, queue) = headless_device_forced_fallback();
    for tall_depth in [false, true] {
        for policy in POLICIES {
            let mut renderer = SceneRenderer::new(&device, FORMAT, 64, 32, false);
            if tall_depth {
                renderer.render_into_store(
                    &device,
                    &queue,
                    &depth_fill_scene(
                        Z.into(),
                        64,
                        64,
                        [0, 0, 64, 64],
                        [0, 0, 63, 63],
                        0xfffc_fffc,
                    ),
                    policy,
                );
            }
            let b = DlBuilder::new();
            let mut dl = vec![
                gdp_set_depth_image(Z),
                gdp_set_color_image(0, 2, 64, B),
                gdp_set_scissor(0, 0, 0, 256, 128),
            ];
            fill(&mut dl, 0xf801_f801, [0, 0, 31, 15]);
            fill(&mut dl, 0x07c1_07c1, [32, 0, 63, 15]);
            fill(&mut dl, 0x003f_003f, [0, 16, 31, 31]);
            fill(&mut dl, 0xffff_ffff, [32, 16, 63, 31]);
            dl.extend([
                gdp_pipe_sync(),
                gdp_set_color_image(0, 2, 64, A),
                gdp_set_cycle_type(2),
                gdp_set_texture_image(0, 2, 64, B),
                gdp_set_tile(0, 2, 16, 0, 0, 0, 2, 0, 0, 2, 0, 0),
                gdp_set_tile_size(0, 0, 0, 252, 124),
            ]);
            dl.extend(gsp_texture_rectangle(
                0, 0, 252, 124, 0, 0, 0, 4096, 1024, false,
            ));
            let scene = interpret(&finish(b, dl));
            renderer.render_into_store(&device, &queue, &scene, policy);
            let pixels = pixels_from_render(&device, &queue, 64, 32, FORMAT, |view| {
                let mut encoder = device.create_command_encoder(&Default::default());
                renderer.scanout(&device, &mut encoder, view, TargetId::Guest(A.into()), None);
                queue.submit(Some(encoder.finish()));
            });
            assert_eq!(pixels.len(), 64 * 32 * 4);
            for (i, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
                assert_eq!(
                    *pixel,
                    match (i % 64 < 32, i / 64 < 16) {
                        (true, true) => RED,
                        (false, true) => GREEN,
                        (true, false) => BLUE,
                        (false, false) => [255; 4],
                    },
                    "({}, {}), tall depth: {tall_depth}, {policy:?}",
                    i % 64,
                    i / 64
                );
            }
        }
    }
}

#[test]
fn color_format_change_preserves_independent_depth() {
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer = SceneRenderer::new(&device, FORMAT, 6, 4, false);
    renderer.render_into_store(
        &device,
        &queue,
        &depth_fill_scene(Z.into(), 6, 4, [0, 0, 6, 4], [0, 0, 5, 3], 0x4000_4000),
        ClearPolicy::Persist,
    );
    for (size, word, bounds) in [
        (2, 0xf801_f801, [0, 0, 5, 3]),
        (3, 0x0000_ffff, [0, 0, 0, 0]),
    ] {
        let mut dl = vec![
            gdp_set_depth_image(Z),
            gdp_set_color_image(0, size, 6, A),
            gdp_set_scissor(0, 0, 0, 24, 16),
        ];
        fill(&mut dl, word, bounds);
        renderer.render_into_store(
            &device,
            &queue,
            &interpret(&finish(DlBuilder::new(), dl)),
            ClearPolicy::Persist,
        );
    }
    assert_eq!(renderer.diagnostics.len(), 1);
    assert!(matches!(renderer.diagnostics[0].kind,
        crate::DiagKind::UnsupportedImageReinterpretation { address, depth: false } if address == u64::from(A)));
    let pixels = pixels_from_render(&device, &queue, 6, 4, FORMAT, |view| {
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.scanout(&device, &mut encoder, view, u64::from(A), None);
        queue.submit(Some(encoder.finish()));
    });
    for (i, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        assert_eq!(*pixel, if i == 0 { BLUE } else { BACKGROUND });
    }
    assert_depth(&device, &queue, &renderer, Z.into(), [6, 4], |_, _| {
        196608.0 / 262144.0
    });
}

#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
mod writeback {
    use super::*;
    use crate::{
        GuestWrite, Hardware, HostRam, Microcode, NopSink, PresentTarget, Rdram, RdramImage,
        Renderer, RendererConfig,
    };

    fn renderer(width: u32, height: u32) -> Renderer {
        let (device, queue) = headless_device_forced_fallback();
        Renderer::with_device(
            device,
            queue,
            PresentTarget::Headless {
                format: FORMAT,
                width,
                height,
            },
            RendererConfig {
                clear_policy: ClearPolicy::Persist,
                resolution_multiplier: 1,
                sample_count: 1,
                present_mode: wgpu::PresentMode::Fifo,
                format: Some(FORMAT),
                power_preference: wgpu::PowerPreference::LowPower,
            },
        )
    }

    fn depth_fill(address: u64, word: u32) -> Vec<[u64; 2]> {
        let mut dl = vec![
            gdp_set_depth_image(0),
            gdp_set_color_image(0, 2, 7, 0),
            gdp_set_scissor(0, 0, 0, 28, 20),
        ];
        fill(&mut dl, word, [0, 0, 6, 4]);
        dl.push(gsp_enddl());
        let mut dl: Vec<_> = dl
            .into_iter()
            .map(|(a, b)| [u64::from(a), u64::from(b)])
            .collect();
        dl[0][1] = address;
        dl[1][1] = address;
        dl
    }

    fn process(renderer: &mut Renderer, dl: &[[u64; 2]]) -> crate::DlSummary {
        unsafe {
            renderer.process_dl_host(
                HostRam::new(&[]),
                dl.as_ptr() as u64,
                Microcode::F3dex2,
                &mut NopSink,
            )
        }
    }

    fn words(patch: &GuestWrite) -> Vec<u16> {
        patch
            .bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_ne_bytes(*b))
            .collect()
    }

    #[test]
    fn guest_writeback_clear_roundtrips_at_native_extent() {
        let memory = [0u16; 35];
        let address = memory.as_ptr() as u64;
        for (width, height) in [(1280, 720), (13, 9)] {
            let mut r = renderer(width, height);
            r.set_guest_writeback(true);
            assert_eq!(
                process(&mut r, &depth_fill(address, 0xfffc_fffc)).termination,
                crate::inspect::WalkTermination::End
            );
            let patches = r.take_guest_writes();
            assert_eq!(patches.len(), 1);
            assert_eq!(patches[0].address, address);
            assert_eq!(words(&patches[0]), vec![0xfffc; 7 * 5]);
            assert!(r.take_guest_writes().is_empty());
        }
    }

    #[test]
    fn guest_writeback_fill_preserves_words_and_row_parity() {
        let mut r = renderer(64, 64);
        r.set_guest_writeback(true);
        let mut memory = [0u16; 35];
        let address = memory.as_mut_ptr() as u64;
        for exponent in 0..8u32 {
            let even = (exponent << 13) | 0x1234;
            let odd = (exponent << 13) | 0x1ffc;
            process(&mut r, &depth_fill(address, even << 16 | odd));
            let patches = r.take_guest_writes();
            assert_eq!(
                words(&patches[0]),
                (0..35)
                    .map(|i| if i % 2 == 0 { even as u16 } else { odd as u16 })
                    .collect::<Vec<_>>()
            );
            apply(patches);
        }
    }

    #[test]
    fn disabled_guest_writeback_does_not_read_back() {
        let mut r = renderer(64, 64);
        let recorder = crate::profiling::Recorder::new(crate::profiling::Mode::Counters);
        r.inner.profiling = recorder.clone();
        assert!(!r.guest_writeback);
        let memory = [0u16; 35];
        for enabled in [false, true, false] {
            r.set_guest_writeback(enabled);
            recorder.drain();
            process(&mut r, &depth_fill(memory.as_ptr() as u64, 0xfffc_fffc));
            assert_eq!(r.take_guest_writes().len(), usize::from(enabled));
            assert_eq!(r.inner.coherence.is_some(), enabled);
            assert!(recorder
                .snapshot()
                .counters
                .iter()
                .all(|(key, value)| !key.starts_with("readback.") || *value == 0));
        }
    }

    struct Image(Built);
    impl Hardware for Image {
        fn rdram(&self) -> impl Rdram + '_ {
            RdramImage::new(&self.0.rdram)
        }
    }

    #[test]
    fn guest_writeback_triangle_matches_viewport_packed_z() {
        let mut r = renderer(1280, 720);
        r.set_guest_writeback(true);
        for modified in [false, true] {
            r.reset();
            let mut b = DlBuilder::new();
            let mut dl = setup(&mut b, A, Z);
            quad(
                &mut b,
                &mut dl,
                [32, 32, 288, 208],
                if modified { 64 } else { 0 },
                RED,
            );
            if modified {
                let tri = dl.pop().unwrap();
                for slot in 0..4 {
                    dl.push(gsp_modifyvertex(slot, G_MWO_POINT_ZSCREEN, 511 << 16));
                }
                dl.push(tri);
            }
            let hw = Image(finish(b, dl));
            r.process_dl(&hw, u64::from(hw.0.entry), Microcode::F3dex2, &mut NopSink);
            let patches = r.take_guest_writes();
            for (address, expected) in [(A, 0xf801), (Z, 0x1ff0)] {
                let writes: Vec<_> = patches
                    .iter()
                    .filter(|p| {
                        (u64::from(address)..u64::from(address) + 320 * 240 * 2)
                            .contains(&p.address)
                    })
                    .collect();
                assert_eq!(writes.len(), 176);
                for (row, patch) in writes.iter().enumerate() {
                    assert_eq!(
                        patch.address,
                        u64::from(address) + (((row + 32) * 320 + 32) * 2) as u64
                    );
                    assert_eq!(words(patch), vec![expected; 256]);
                }
            }
        }
    }

    #[test]
    fn guest_writeback_only_accepted_tasks_produce_patches() {
        let mut r = renderer(64, 64);
        r.set_guest_writeback(true);
        let memory = [0u16; 35];
        let address = memory.as_ptr() as u64;
        process(&mut r, &depth_fill(address, 0xfffc_fffc));
        assert_eq!(r.take_guest_writes().len(), 1);
        let mut rejected = depth_fill(address, 0);
        rejected.insert(rejected.len() - 1, [0xd500_0000, 0]);
        assert_eq!(
            process(&mut r, &rejected).termination,
            crate::inspect::WalkTermination::Rejected
        );
        assert!(r.take_guest_writes().is_empty());
        let mut b = DlBuilder::new();
        b.list(
            "main",
            &[
                gdp_set_depth_image(Z),
                gdp_set_color_image(0, 2, 7, Z),
                gdp_set_scissor(0, 0, 0, 28, 20),
                gdp_set_cycle_type(3),
                gdp_set_fill_color(0),
                gdp_fill_rectangle(0, 0, 24, 16),
                gsp_vertex(0, 1, 0x700000),
            ],
        );
        let hw = Image(b.finish("main"));
        assert!(
            !r.process_dl(&hw, u64::from(hw.0.entry), Microcode::F3dex2, &mut NopSink)
                .renderable
        );
        assert!(r.take_guest_writes().is_empty());
        struct Stop;
        impl crate::inspect::WalkObserver for Stop {
            fn command(&mut self, step: crate::inspect::WalkStep<'_>) -> std::ops::ControlFlow<()> {
                if step.seq == 5 {
                    std::ops::ControlFlow::Break(())
                } else {
                    std::ops::ControlFlow::Continue(())
                }
            }
        }
        r.process_dl_observed(
            &hw,
            u64::from(hw.0.entry),
            Microcode::F3dex2,
            &mut NopSink,
            &mut Stop,
        );
        assert!(r.take_guest_writes().is_empty());
    }

    fn color_pixels(r: &mut Renderer, address: u64, width: u32, height: u32) -> Vec<u8> {
        let device = r.device().clone();
        let queue = r.queue().clone();
        pixels_from_render(&device, &queue, width, height, FORMAT, |view| {
            let mut encoder = device.create_command_encoder(&Default::default());
            r.inner.scanout(&device, &mut encoder, view, address, None);
            queue.submit([encoder.finish()]);
        })
    }

    fn apply(patches: Vec<GuestWrite>) {
        for patch in patches {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    patch.bytes.as_ptr(),
                    patch.address as *mut u8,
                    patch.bytes.len(),
                );
            }
        }
    }

    fn color_fill(
        address: u64,
        width: u32,
        height: u32,
        fmt: u32,
        siz: u32,
        word: u32,
        bounds: [u32; 4],
    ) -> Vec<[u64; 2]> {
        let mut dl = vec![
            gdp_set_color_image(fmt, siz, width, 0),
            gdp_set_scissor(0, 0, 0, width * 4, height * 4),
        ];
        fill(&mut dl, word, bounds);
        let mut dl: Vec<_> = dl
            .into_iter()
            .map(|(a, b)| [u64::from(a), u64::from(b)])
            .collect();
        dl[0][1] = address;
        dl
    }

    fn run(r: &mut Renderer, mut dl: Vec<[u64; 2]>) -> Vec<GuestWrite> {
        dl.push([0xdf00_0000, 0]);
        assert_eq!(
            process(r, &dl).termination,
            crate::inspect::WalkTermination::End
        );
        r.take_guest_writes()
    }

    #[test]
    fn color_supersedes_depth_and_literal_words_survive() {
        let mut r = renderer(1280, 720);
        r.set_guest_writeback(true);
        let mut memory = [0u16; 35];
        let address = memory.as_mut_ptr() as u64;
        let mut dl = depth_fill(address, 0xfffc_fffc);
        dl.pop();
        dl.push([u64::from(gdp_set_depth_image(0).0), address + 70]);
        dl.extend(color_fill(address, 7, 5, 0, 2, 0x1234_abcd, [0, 0, 6, 4]));
        apply(run(&mut r, dl));
        assert_eq!(
            memory,
            std::array::from_fn(|i| if i % 2 == 0 { 0x1234 } else { 0xabcd })
        );
        assert!(run(&mut r, vec![]).is_empty());
    }

    #[test]
    fn i8_alias_preserves_second_half_of_rgba16() {
        let mut r = renderer(640, 480);
        r.set_guest_writeback(true);
        let mut memory = vec![0u16; 320 * 240];
        let address = memory.as_mut_ptr() as u64;
        let mut dl = color_fill(address, 320, 240, 0, 2, 0x1234_1234, [0, 0, 319, 239]);
        dl.extend(color_fill(
            address,
            320,
            240,
            4,
            1,
            0xffff_ffff,
            [0, 0, 319, 239],
        ));
        apply(run(&mut r, dl));
        assert!(memory[..38400].iter().all(|&word| word == 0xffff));
        assert!(memory[38400..].iter().all(|&word| word == 0x1234));
    }

    #[test]
    fn fullscreen_color_overwrites_portrait_depth() {
        let mut r = renderer(640, 480);
        r.set_guest_writeback(true);
        let mut memory = vec![0u16; 320 * 240];
        let address = memory.as_mut_ptr() as u64;
        let portrait = address + 4096;
        let mut dl = color_fill(address, 320, 240, 0, 2, 0x1234_1234, [0, 0, 319, 239]);
        dl.push([u64::from(gdp_set_depth_image(0).0), portrait]);
        dl.extend(color_fill(
            portrait,
            64,
            112,
            0,
            2,
            0xfffc_fffc,
            [0, 0, 63, 111],
        ));
        let color = CcPass {
            a: ZERO_C,
            b: ZERO_C,
            c: ZERO_C,
            d: 3,
        };
        let alpha = CcPass {
            a: ZERO_A,
            b: ZERO_A,
            c: ZERO_A,
            d: 3,
        };
        let mut draw: Vec<_> = [
            gdp_set_color_image(0, 2, 320, 0),
            gdp_set_scissor(0, 0, 0, 1280, 960),
            gdp_set_cycle_type(0),
            gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
            gdp_set_combine_lerp(color, alpha, color, alpha),
            gdp_set_prim_color(0, 0, 0x00ff00ff),
            gdp_fill_rectangle(0, 0, 1280, 960),
        ]
        .into_iter()
        .map(|(a, b)| [u64::from(a), u64::from(b)])
        .collect();
        draw[0][1] = address;
        dl.extend(draw);
        apply(run(&mut r, dl));
        assert!(memory.iter().all(|&word| word == 0x07c1));
    }

    #[test]
    fn cpu_color_edit_is_imported_and_partial_writes_stay_partial() {
        let mut r = renderer(80, 60);
        r.set_guest_writeback(true);
        let recorder = crate::profiling::Recorder::new(crate::profiling::Mode::Counters);
        r.inner.profiling = recorder.clone();
        let mut memory = [0u16; 35];
        let address = memory.as_mut_ptr() as u64;
        apply(run(
            &mut r,
            color_fill(address, 7, 5, 0, 2, 0xf801_f801, [0, 0, 6, 4]),
        ));
        recorder.drain();
        apply(run(
            &mut r,
            color_fill(address, 7, 5, 0, 2, 0x003f_003f, [6, 4, 6, 4]),
        ));
        assert_eq!(
            recorder
                .snapshot()
                .counters
                .get("import.pixels")
                .copied()
                .unwrap_or(0),
            0
        );
        memory[0] = 0x07c1;
        recorder.drain();
        let patches = run(
            &mut r,
            color_fill(address, 7, 5, 0, 2, 0xffff_ffff, [3, 2, 3, 2]),
        );
        assert_eq!(
            patches,
            vec![GuestWrite {
                address: address + 34,
                bytes: 0xffffu16.to_ne_bytes().to_vec()
            }]
        );
        apply(patches);
        assert_eq!(memory[0], 0x07c1);
        assert_eq!(memory[34], 0x003f);
        assert_eq!(memory[1], 0xf801);
        assert_eq!(recorder.snapshot().counters.get("import.pixels"), Some(&1));
        let pixels = color_pixels(&mut r, address, 7, 5);
        assert_eq!(&pixels[..4], &GREEN);
        assert_eq!(&pixels[4..8], &RED);
    }

    #[test]
    fn cpu_depth_edit_is_imported_before_partial_fill() {
        let mut r = renderer(80, 60);
        r.set_guest_writeback(true);
        let mut memory = [0u16; 35];
        let address = memory.as_mut_ptr() as u64;
        process(&mut r, &depth_fill(address, 0xfffc_fffc));
        apply(r.take_guest_writes());
        memory[0] = 0x4000;
        apply(run(
            &mut r,
            color_fill(address, 7, 5, 0, 2, 0x8000_8000, [6, 4, 6, 4]),
        ));
        assert_depth(r.device(), r.queue(), &r.inner, address, [7, 5], |x, y| {
            if x == 0 && y == 0 {
                196608.0 / 262144.0
            } else if x == 6 && y == 4 {
                245760.0 / 262144.0
            } else {
                262143.0 / 262144.0
            }
        });
        assert_eq!(memory[0], 0x4000);
    }
    #[test]
    fn bind_only_task_imports_edits_before_the_next_task() {
        let mut r = renderer(7, 5);
        r.set_guest_writeback(true);
        let mut memory = [0u16; 35];
        let address = memory.as_mut_ptr() as u64;
        apply(run(
            &mut r,
            color_fill(address, 7, 5, 0, 2, 0xf801_f801, [0, 0, 6, 4]),
        ));
        memory[0] = 0x07c1;
        assert!(run(&mut r, vec![]).is_empty());
        apply(run(
            &mut r,
            color_fill(address, 7, 5, 0, 2, 0x003f_003f, [6, 4, 6, 4]),
        ));
        let pixels = color_pixels(&mut r, address, 7, 5);
        assert_eq!(&pixels[..4], &GREEN);
        assert_eq!(memory[0], 0x07c1);
    }

    #[test]
    fn edits_seen_by_a_rejected_task_are_imported_by_the_next_accepted_one() {
        let mut r = renderer(7, 5);
        r.set_guest_writeback(true);
        let recorder = crate::profiling::Recorder::new(crate::profiling::Mode::Counters);
        r.inner.profiling = recorder.clone();
        let mut memory = [0u16; 35];
        let address = memory.as_mut_ptr() as u64;
        apply(run(
            &mut r,
            color_fill(address, 7, 5, 0, 2, 0xf801_f801, [0, 0, 6, 4]),
        ));
        memory[0] = 0x07c1;
        assert_eq!(
            process(&mut r, &[[0xd500_0000, 0], [0xdf00_0000, 0]]).termination,
            crate::inspect::WalkTermination::Rejected
        );
        assert!(run(&mut r, vec![]).is_empty());
        assert_eq!(&color_pixels(&mut r, address, 7, 5)[..4], &GREEN);
        recorder.drain();
        assert!(run(&mut r, vec![]).is_empty());
        assert_eq!(
            recorder
                .snapshot()
                .counters
                .get("import.pixels")
                .copied()
                .unwrap_or(0),
            0
        );
        assert_eq!(memory[0], 0x07c1);
    }

    #[cfg(feature = "capture")]
    #[test]
    fn writeback_capture_records_import_reads_and_refuses_foreign_uploads() {
        use crate::capture::{CaptureFrame, Provenance};
        let mut r = renderer(7, 5);
        r.set_guest_writeback(true);
        let mut memory = [0u16; 35];
        let address = memory.as_mut_ptr() as u64;
        let mut dl = color_fill(address, 7, 5, 0, 2, 0xf801_f801, [0, 0, 6, 4]);
        dl.push([0xdf00_0000, 0]);
        let mut capture = CaptureFrame::begin(&mut r, 0, 0, Provenance::default());
        for _ in 0..2 {
            unsafe {
                capture.process_dl_host(
                    &mut r,
                    HostRam::new(&[]),
                    dl.as_ptr() as u64,
                    Microcode::F3dex2,
                    crate::DataFormat::Fixed,
                    &mut NopSink,
                )
            }
            .unwrap();
            apply(r.take_guest_writes());
        }
        let fixture = capture.present_last(&mut r).unwrap();
        assert!(fixture.tasks[1]
            .spans
            .iter()
            .any(|span| span.address <= address
                && span.address + span.bytes.len() as u64 >= address + 70));
        let replay = pollster::block_on(fixture.replay_headless()).unwrap();
        assert!(replay
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == RED));
        let mut capture = CaptureFrame::begin(&mut r, 0, 0, Provenance::default());
        memory[0] = 0x07c1;
        let result = unsafe {
            capture.process_dl_host(
                &mut r,
                HostRam::new(&[]),
                dl.as_ptr() as u64,
                Microcode::F3dex2,
                crate::DataFormat::Fixed,
                &mut NopSink,
            )
        };
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("imports framebuffer bytes"));
        assert_eq!(memory[0], 0x07c1);
    }

    #[test]
    fn unsupported_color_layout_diagnoses_once_and_emits_no_patches() {
        let mut r = renderer(7, 5);
        r.set_guest_writeback(true);
        let mut memory = [0u32; 35];
        let address = memory.as_mut_ptr() as u64;
        let mut dl = color_fill(address, 7, 5, 0, 3, 0xffff_ffff, [0, 0, 6, 4]);
        dl.extend(color_fill(address, 7, 5, 0, 3, 0, [0, 0, 6, 4]));
        dl.push([0xdf00_0000, 0]);
        let mut diagnostics = Vec::new();
        unsafe {
            r.process_dl_host(
                HostRam::new(&[]),
                dl.as_ptr() as u64,
                Microcode::F3dex2,
                &mut diagnostics,
            );
        }
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diag| matches!(
                    diag.kind,
                    crate::DiagKind::UnsupportedGuestWriteback { .. }
                ))
                .count(),
            1
        );
        assert!(r.take_guest_writes().is_empty());
    }
    #[test]
    fn recreated_color_view_restores_edits_outside_the_previous_layout() {
        let mut r = renderer(4, 2);
        r.set_guest_writeback(true);
        let mut memory = [0u16; 8];
        let address = memory.as_mut_ptr() as u64;
        apply(run(
            &mut r,
            color_fill(address, 4, 2, 0, 2, 0xf801_f801, [0, 0, 3, 1]),
        ));
        apply(run(
            &mut r,
            color_fill(address, 4, 2, 4, 1, 0xffff_ffff, [0, 0, 3, 1]),
        ));
        memory[7] = 0x07c1;
        let bind = color_fill(address, 4, 2, 0, 2, 0, [0, 0, 0, 0]);
        assert!(run(&mut r, bind[..2].to_vec()).is_empty());
        apply(run(
            &mut r,
            color_fill(address, 4, 2, 0, 2, 0x003f_003f, [0, 0, 0, 0]),
        ));
        let pixels = color_pixels(&mut r, address, 4, 2);
        assert_eq!(&pixels[28..32], &GREEN);
        assert_eq!(&pixels[16..20], &RED);
        assert_eq!(memory[7], 0x07c1);
    }
    #[test]
    fn target_growth_keeps_draws_over_imported_pixels() {
        let mut r = renderer(4, 2);
        r.set_guest_writeback(true);
        let mut memory = [0u16; 8];
        let address = memory.as_mut_ptr() as u64;
        apply(run(
            &mut r,
            color_fill(address, 4, 1, 0, 2, 0xf801_f801, [0, 0, 3, 0]),
        ));
        memory[0] = 0x07c1;
        let mut dl = color_fill(address, 4, 1, 0, 2, 0x003f_003f, [0, 0, 0, 0]);
        dl.extend(color_fill(address, 4, 2, 0, 2, 0xffff_ffff, [3, 1, 3, 1]));
        apply(run(&mut r, dl));
        let pixels = color_pixels(&mut r, address, 4, 2);
        assert_eq!(&pixels[..4], &BLUE);
        assert_eq!(memory[0], 0x003f);
    }

    #[test]
    fn framebuffer_texture_source_imports_cpu_edits() {
        let mut r = renderer(7, 5);
        r.set_guest_writeback(true);
        let mut source = [0u16; 35];
        let mut destination = [0u16; 35];
        let address = source.as_mut_ptr() as u64;
        let target = destination.as_mut_ptr() as u64;
        apply(run(
            &mut r,
            color_fill(address, 7, 5, 0, 2, 0xf801_f801, [0, 0, 6, 4]),
        ));
        apply(run(
            &mut r,
            color_fill(target, 7, 5, 0, 2, 0x0001_0001, [0, 0, 6, 4]),
        ));
        source[0] = 0x07c1;
        let mut commands = vec![
            gdp_set_cycle_type(2),
            gdp_set_texture_image(0, 2, 7, 0),
            gdp_set_tile(0, 2, 2, 0, 0, 0, 2, 0, 0, 2, 0, 0),
            gdp_set_tile_size(0, 0, 0, 24, 16),
        ];
        commands.extend(gsp_texture_rectangle(
            0, 0, 24, 16, 0, 0, 0, 4096, 1024, false,
        ));
        let mut commands: Vec<_> = commands
            .into_iter()
            .map(|(a, b)| [u64::from(a), u64::from(b)])
            .collect();
        commands[1][1] = address;
        apply(run(&mut r, commands));
        assert_eq!(destination[0], 0x07c1);
        assert!(destination[1..].iter().all(|&pixel| pixel == 0xf801));
        assert_eq!(source[0], 0x07c1);
    }
    #[test]
    fn same_task_layout_roundtrip_restores_latest_writers() {
        let mut r = renderer(4, 2);
        r.set_guest_writeback(true);
        let mut memory = [0u16; 8];
        let address = memory.as_mut_ptr() as u64;
        let mut dl = color_fill(address, 4, 2, 0, 2, 0xf801_f801, [0, 0, 3, 1]);
        dl.extend(color_fill(address, 4, 2, 4, 1, 0xffff_ffff, [0, 0, 3, 1]));
        dl.extend(color_fill(address, 4, 2, 0, 2, 0x003f_003f, [0, 0, 0, 0]));
        apply(run(&mut r, dl));
        let pixels = color_pixels(&mut r, address, 4, 2);
        assert_eq!(&pixels[..4], &BLUE);
        assert_eq!(&pixels[4..8], &[255; 4]);
        assert_eq!(&pixels[16..20], &RED);
        assert_eq!(
            memory,
            [0x003f, 0xffff, 0xffff, 0xffff, 0xf801, 0xf801, 0xf801, 0xf801]
        );
    }
    #[test]
    fn framebuffer_load_uses_latest_color_or_rejects_depth() {
        for later in [false, true] {
            for color_last in [false, true] {
                let mut r = renderer(4, 2);
                r.set_guest_writeback(true);
                let mut source = [0u16; 70];
                let mut destination = [0u16; 8];
                let address = source.as_mut_ptr() as u64;
                let target = destination.as_mut_ptr() as u64;
                let mut depth = depth_fill(address, 0xfffc_fffc);
                depth.pop();
                let mut color = vec![[u64::from(gdp_set_depth_image(0).0), address + 70]];
                color.extend(color_fill(address, 4, 2, 0, 2, 0xf801_f801, [0, 0, 3, 1]));
                let mut dl = if color_last {
                    depth.into_iter().chain(color).collect::<Vec<_>>()
                } else {
                    color.into_iter().chain(depth).collect()
                };
                if later {
                    apply(run(&mut r, dl));
                    dl = Vec::new();
                    if color_last {
                        source[0] = 0x07c1;
                    }
                }
                let commands = [
                    gdp_set_texture_image(0, 2, 4, 0),
                    gdp_set_tile(0, 2, 1, 0, 7, 0, 2, 0, 0, 2, 0, 0),
                    (0xf4000000, (7 << 24) | (12 << 12) | 4),
                    gdp_set_color_image(0, 2, 4, 0),
                    gdp_set_scissor(0, 0, 0, 16, 8),
                    gdp_set_tile(0, 2, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0),
                    gdp_set_tile_size(0, 0, 0, 12, 4),
                    gdp_set_cycle_type(2),
                    gdp_set_render_mode(0, 0),
                ];
                let mut commands: Vec<_> = commands
                    .into_iter()
                    .map(|(a, b)| [u64::from(a), u64::from(b)])
                    .collect();
                commands[0][1] = address;
                commands[3][1] = target;
                dl.extend(commands);
                dl.extend(
                    gsp_texture_rectangle(0, 0, 12, 4, 0, 0, 0, 4096, 1024, false)
                        .into_iter()
                        .map(|(a, b)| [u64::from(a), u64::from(b)]),
                );
                dl.push([0xdf000000, 0]);
                let mut diagnostics = Vec::new();
                let summary = unsafe {
                    r.process_dl_host(
                        HostRam::new(&[]),
                        dl.as_ptr() as u64,
                        Microcode::F3dex2,
                        &mut diagnostics,
                    )
                };
                assert_eq!(summary.termination, crate::inspect::WalkTermination::End);
                if later && color_last {
                    assert_eq!(source[0], 0x07c1);
                }
                if color_last {
                    assert!(diagnostics.is_empty(), "{diagnostics:?}");
                    let pixels = color_pixels(&mut r, target, 4, 2);
                    for (i, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
                        assert_eq!(
                            *pixel,
                            if later && i == 0 {
                                [0, 255, 0, 255]
                            } else {
                                RED
                            }
                        );
                    }
                } else {
                    assert!(
                        diagnostics.iter().any(|d| matches!(
                            d.kind,
                            crate::DiagKind::UnsupportedFramebufferAccess {
                                reason: crate::FramebufferAccess::DepthTexture,
                                ..
                            }
                        )),
                        "{diagnostics:?}"
                    );
                }
            }
        }
    }
    #[test]
    fn framebuffer_load_tmem_survives_reconfigure() {
        let mut r = renderer(4, 2);
        let source = 0x10000;
        let target = 0x20000;
        let mut dl = color_fill(source, 4, 2, 0, 2, 0xf801_f801, [0, 0, 3, 1]);
        dl.extend(
            [
                gdp_set_texture_image(0, 2, 4, source as u32),
                gdp_set_tile(0, 2, 1, 0, 7, 0, 2, 0, 0, 2, 0, 0),
                (0xf4000000, (7 << 24) | (12 << 12) | 4),
            ]
            .into_iter()
            .map(|(a, b)| [u64::from(a), u64::from(b)]),
        );
        run(&mut r, dl);
        r.reconfigure(r.config);
        let mut dl: Vec<_> = [
            gdp_set_color_image(0, 2, 4, target as u32),
            gdp_set_tile(0, 2, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0),
            gdp_set_tile_size(0, 0, 0, 12, 4),
            gdp_set_cycle_type(2),
            gdp_set_render_mode(0, 0),
        ]
        .into_iter()
        .map(|(a, b)| [u64::from(a), u64::from(b)])
        .collect();
        dl.extend(
            gsp_texture_rectangle(0, 0, 12, 4, 0, 0, 0, 4096, 1024, false)
                .into_iter()
                .map(|(a, b)| [u64::from(a), u64::from(b)]),
        );
        run(&mut r, dl);
        assert_eq!(color_pixels(&mut r, target, 4, 2), RED.repeat(8));
    }

    #[test]
    fn framebuffer_load_ia16_strips_keep_unwritten_rgba16_view() {
        let mut r = renderer(4, 4);
        r.set_guest_writeback(true);
        let mut memory = [0u16; 16];
        let address = memory.as_mut_ptr() as u64;
        let mut dl = color_fill(address, 4, 4, 0, 2, 0xf801_f801, [0, 0, 3, 1]);
        dl.extend(color_fill(address, 4, 4, 0, 2, 0x003f_003f, [0, 2, 3, 3]));
        let texel = n64_gbi::encode::CcPass {
            a: ZERO_C,
            b: ZERO_C,
            c: ZERO_C,
            d: 1,
        };
        let alpha = n64_gbi::encode::CcPass {
            a: ZERO_A,
            b: ZERO_A,
            c: ZERO_A,
            d: ZERO_A,
        };
        for y in [0, 2] {
            let mut commands = vec![
                gdp_set_color_image(4, 1, 4, 0),
                gdp_set_scissor(0, 0, 0, 16, 16),
                gdp_set_texture_image(3, 2, 4, 0),
                gdp_set_tile(3, 2, 1, 0, 7, 0, 2, 0, 0, 2, 0, 0),
                (0xf4000000 | (y * 4), (7 << 24) | (12 << 12) | ((y + 1) * 4)),
                gdp_set_tile(3, 2, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0),
                gdp_set_tile_size(0, 0, 0, 12, 4),
                gdp_set_cycle_type(0),
                gdp_set_other_mode_h(12, 2, 0),
                gdp_set_other_mode_h(4, 2, 3 << 4),
                gdp_set_other_mode_h(6, 2, 3 << 6),
                gdp_set_render_mode(
                    gbl_c1(CLR_IN, A_0, CLR_IN, B_1),
                    gbl_c2(CLR_IN, A_0, CLR_IN, B_1),
                ),
                gdp_set_combine_lerp(texel, alpha, texel, alpha),
            ];
            commands.extend(gsp_texture_rectangle(
                0,
                y * 4,
                16,
                (y + 2) * 4,
                0,
                0,
                0,
                1024,
                1024,
                false,
            ));
            let mut commands: Vec<_> = commands
                .into_iter()
                .map(|(a, b)| [u64::from(a), u64::from(b)])
                .collect();
            commands[0][1] = address;
            commands[2][1] = address;
            dl.extend(commands);
        }
        dl.push([0xdf000000, 0]);
        let mut diagnostics = Vec::new();
        unsafe {
            r.process_dl_host(
                HostRam::new(&[]),
                dl.as_ptr() as u64,
                Microcode::F3dex2,
                &mut diagnostics,
            );
        }
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(matches!(
            diagnostics[0].kind,
            crate::DiagKind::UnsupportedImageReinterpretation { depth: false, .. }
        ));
        apply(r.take_guest_writes());
        let bytes: Vec<_> = memory.into_iter().flat_map(u16::to_ne_bytes).collect();
        assert_eq!(&bytes[..8], &[255; 8]);
        assert_eq!(&bytes[8..16], &[0; 8]);
        assert_eq!(&bytes[16..], [0x003fu16.to_ne_bytes(); 8].concat());
    }
}
