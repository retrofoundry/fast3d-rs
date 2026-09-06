use crate::hle::{ColorImage, FramebufferPair, Scene, SceneOp, Scissor};
use crate::render::{headless_device, SceneRenderer};
use crate::tests::common::{pixel, pixels_from_render, render_to_pixels, scene_from_fixture};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn quad_scene() -> Scene {
    scene_from_fixture("framebuffer-extent--white1")
}

fn pair(scene: &Scene, addr: u64, width: u32, height: u32) -> FramebufferPair {
    FramebufferPair {
        color_image: ColorImage {
            fmt: 0,
            siz: 2,
            width: width as u16,
            addr,
        },
        ops: scene.draw_runs.iter().copied().map(SceneOp::Tris).collect(),
        active_scissor: Scissor {
            lrx: width as i32,
            lry: height as i32,
            ..Default::default()
        },
        size_extent: (width, height),
        ..Default::default()
    }
}

fn scanout_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &SceneRenderer,
    addr: impl Into<crate::render::workload::TargetId>,
    width: u32,
    height: u32,
) -> Vec<u8> {
    pixels_from_render(device, queue, width, height, FORMAT, |view| {
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.scanout(&mut encoder, view, addr);
        queue.submit(Some(encoder.finish()));
    })
}

fn assert_coverage(pixels: &[u8], width: u32, height: u32, bounds: [u32; 4]) {
    let [left, top, right, bottom] = bounds;
    for y in 0..height {
        for x in 0..width {
            let expected = if (left..right).contains(&x) && (top..bottom).contains(&y) {
                [255; 4]
            } else {
                [13, 13, 20, 255]
            };
            assert_eq!(
                pixel(pixels, width, x, y),
                expected,
                "{width}x{height} ({x},{y})"
            );
        }
    }
}

#[test]
fn framebuffer_640x480_triangles_align_with_texrect() {
    let (device, queue, dual) = headless_device();
    let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, dual);
    for (width, height) in [(320, 240), (640, 480)] {
        let mut scene = quad_scene();
        scene.framebuffer_pairs = vec![pair(&scene, 0x0010_0000, width, height)];
        let mut rect_scene = scene.clone();
        rect_scene.materials[0].tex_enable = true;
        rect_scene.materials[0].texture = vec![255; 4];
        rect_scene.materials[0].tex_w = 1;
        rect_scene.materials[0].tex_h = 1;
        rect_scene.framebuffer_pairs[0].ops = vec![SceneOp::TexRect {
            rect: crate::hle::TexRectBounds {
                ulx: 80 * 4,
                uly: 60 * 4,
                lrx: 159 * 4,
                lry: 119 * 4,
            },
            tile: 0,
            uls: 0,
            ult: 0,
            dsdx: 1024,
            dtdy: 1024,
            flip: false,
            copy_mode: true,
            material_index: 0,
            render_mode_index: 0,
            fog_color: [0; 4],
            prim_depth: Default::default(),
            fb_source: None,
        }];
        for source in [&scene, &rect_scene] {
            let pixels = render_to_pixels(&device, &queue, &mut renderer, source, width, height);
            assert_coverage(&pixels, width, height, [80, 60, 160, 120]);
            renderer.begin_frame();
            let addr =
                renderer.render_into_store(&device, &queue, source, crate::ClearPolicy::PerFrame);
            assert_eq!(
                addr,
                Some(crate::render::workload::TargetId::Guest(0x0010_0000))
            );
            let pixels = scanout_pixels(&device, &queue, &renderer, addr.unwrap(), width, height);
            assert_coverage(&pixels, width, height, [80, 60, 160, 120]);
        }
    }
}

#[test]
fn framebuffer_mixed_extents_reuses_vertices() {
    let (device, queue, dual) = headless_device();
    let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, dual);
    let mut scene = quad_scene();
    scene.framebuffer_pairs = vec![
        pair(&scene, 0x0010_0000, 320, 240),
        pair(&scene, 0x0020_0000, 640, 480),
        pair(&scene, 0x0030_0000, 320, 240),
    ];
    assert_eq!(scene.raw_pos.len(), 4);
    assert_eq!(scene.indices, [0, 1, 2, 0, 2, 3]);
    for _ in 0..2 {
        renderer.begin_frame();
        renderer.render_into_store(&device, &queue, &scene, crate::ClearPolicy::PerFrame);
        for pair in &scene.framebuffer_pairs {
            let width = u32::from(pair.color_image.width);
            let height = pair.size_extent.1;
            let pixels = scanout_pixels(
                &device,
                &queue,
                &renderer,
                pair.color_image.addr,
                width,
                height,
            );
            assert_coverage(&pixels, width, height, [80, 60, 160, 120]);
        }
        let last = scene.framebuffer_pairs.last().unwrap();
        let (width, height) = (u32::from(last.color_image.width), last.size_extent.1);
        let pixels = render_to_pixels(&device, &queue, &mut renderer, &scene, width, height);
        assert_coverage(&pixels, width, height, [80, 60, 160, 120]);
        scene.framebuffer_pairs.swap(1, 2);
    }
}

#[test]
fn framebuffer_modify_xy_uses_pair_extent() {
    use crate::hle::{mem::RdramImage, rdp::Rdp, rsp::Rsp};
    use n64_gbi::encode::VtxColored;

    let mut scene = Scene {
        materials: quad_scene().materials,
        render_modes: quad_scene().render_modes,
        ..Default::default()
    };
    let vertex = VtxColored {
        x: 0,
        y: 0,
        z: 0,
        flag: 0,
        s: 0,
        t: 0,
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    }
    .to_bytes();
    let bytes = vertex.repeat(4);
    let mut rsp = Rsp::default();
    rsp.set_vertex(
        &RdramImage::new(&bytes),
        0,
        4,
        0,
        &Rdp::default(),
        &mut scene,
    )
    .unwrap();
    for (slot, packed) in [0x0640_04B0, 0x0640_05A0, 0x0780_05A0, 0x0780_04B0]
        .into_iter()
        .enumerate()
    {
        rsp.modify_vertex(slot as u32, 0x18, packed, &mut scene)
            .unwrap();
    }
    rsp.draw_tri(0, 1, 2, 0, 0, [0; 4], Default::default(), &mut scene, None);
    rsp.draw_tri(0, 2, 3, 0, 0, [0; 4], Default::default(), &mut scene, None);
    rsp.finish(&mut scene);
    assert_eq!(scene.modify_screen[0], [400.0, 300.0, 0.0, 0.0]);
    assert_eq!(scene.modify_flags, [1; 4]);
    scene.framebuffer_pairs = vec![pair(&scene, 0x0010_0000, 640, 480)];
    let (device, queue, dual) = headless_device();
    let mut renderer = SceneRenderer::new(&device, FORMAT, 320, 240, dual);
    let pixels = render_to_pixels(&device, &queue, &mut renderer, &scene, 640, 480);
    assert_coverage(&pixels, 640, 480, [400, 300, 480, 360]);
    renderer.render_into_store(&device, &queue, &scene, crate::ClearPolicy::PerFrame);
    let pixels = scanout_pixels(&device, &queue, &renderer, 0x0010_0000, 640, 480);
    assert_coverage(&pixels, 640, 480, [400, 300, 480, 360]);
}

#[test]
fn framebuffer_pairless_logical_extent_is_unchanged() {
    let mut scene = quad_scene();
    scene.viewport_table[0].0[0..2].copy_from_slice(&[80.0, 60.0]);
    scene.viewport_table[0].1[0..2].copy_from_slice(&[120.0, 90.0]);
    assert!(scene.framebuffer_pairs.is_empty());
    let (device, queue, dual) = headless_device();
    let mut renderer = SceneRenderer::new(&device, FORMAT, 640, 480, dual);
    for (width, height, bounds) in [
        (640, 480, [160, 120, 240, 180]),
        (320, 240, [80, 60, 120, 90]),
    ] {
        renderer.resize(&device, width, height);
        let pixels = render_to_pixels(&device, &queue, &mut renderer, &scene, width, height);
        assert_coverage(&pixels, width, height, bounds);
        renderer.render_into_store(&device, &queue, &scene, crate::ClearPolicy::PerFrame);
        let pixels = scanout_pixels(
            &device,
            &queue,
            &renderer,
            crate::render::workload::TargetId::Legacy,
            width,
            height,
        );
        assert_coverage(&pixels, width, height, bounds);
    }
}
