//! P3.7 (D2) — the persistent, address-keyed framebuffer store + VI scanout, driven directly on the
//! crate-internal `SceneRenderer` (the public `Renderer`'s process_dl/present land in P3.8/P3.9).

use crate::tests::common;

use crate::render::{headless_device, SceneRenderer};
use crate::{render::workload::TargetId, ClearPolicy};
use common::{dl_2d_fill, dl_2d_fill_rect, pixel, scene_from_fixture}; // B1: no `solid_env_texture` (unused → -D warnings error)

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[test]
fn scanout_reuses_ntsc_extent_across_frames() {
    let (device, queue, dual) = headless_device();
    let mut renderer = SceneRenderer::new(&device, FORMAT, 64, 240, dual);
    let address = 0x10_0000_u64;
    let green =
        common::dl_2d_fill_rect_with_scissor_height(address, 0x07c1_07c1, 0, 0, 64, 240, 240);
    let bottom =
        common::dl_2d_fill_rect_with_scissor_height(address, 0xf801_f801, 0, 237, 64, 240, 240);
    renderer.render_into_store(&device, &queue, &green, ClearPolicy::Persist);
    let height = crate::ViRegisters {
        v_start: 0x0025_01ff,
        y_scale: 1024,
        ..Default::default()
    }
    .scanout_height();
    assert_eq!(height, Some(237));
    let profiling = crate::profiling::Recorder::new(crate::profiling::Mode::Counters);
    renderer.profiling = profiling.clone();
    for allocations in [1, 0, 0] {
        profiling.drain();
        renderer.begin_frame();
        renderer.render_into_store(&device, &queue, &green, ClearPolicy::Persist);
        renderer.render_into_store(&device, &queue, &bottom, ClearPolicy::Persist);
        let pixels = common::pixels_from_render(&device, &queue, 64, 237, FORMAT, |view| {
            let mut encoder = device.create_command_encoder(&Default::default());
            renderer.scanout(&device, &mut encoder, view, address, height);
            queue.submit([encoder.finish()]);
        });
        assert_eq!(
            profiling
                .snapshot()
                .counters
                .get("buffer.presentation.creations")
                .copied()
                .unwrap_or(0),
            allocations
        );
        for pixel in pixels.as_chunks::<4>().0 {
            assert_eq!(*pixel, [0, 255, 0, 255]);
        }
    }
}

#[test]
fn scanout_rows_leave_retained_pixels_available() {
    let (device, queue, dual) = headless_device();
    let mut renderer = SceneRenderer::new(&device, FORMAT, 64, 64, dual);
    let address = 0x10_0000_u64;
    renderer.render_into_store(
        &device,
        &queue,
        &dl_2d_fill(address, 0x07c1_07c1),
        ClearPolicy::Persist,
    );
    let shorter =
        common::dl_2d_fill_rect_with_scissor_height(address, 0xf801_f801, 0, 0, 64, 32, 32);
    assert_eq!(
        crate::render::workload::Workload::new(&shorter).targets[0].logical_extent,
        (64, 32)
    );
    let profiling = crate::profiling::Recorder::new(crate::profiling::Mode::Counters);
    renderer.profiling = profiling.clone();
    for (height, allocations) in [
        (None, 0),
        (Some(32), 1),
        (Some(32), 0),
        (Some(16), 1),
        (Some(16), 0),
        (Some(64), 1),
        (None, 0),
        (Some(32), 1),
        (None, 1),
    ] {
        profiling.drain();
        renderer.begin_frame();
        renderer.render_into_store(&device, &queue, &shorter, ClearPolicy::Persist);
        let output_height = height.unwrap_or(64);
        let pixels =
            common::pixels_from_render(&device, &queue, 64, output_height, FORMAT, |view| {
                let mut encoder = device.create_command_encoder(&Default::default());
                renderer.scanout(&device, &mut encoder, view, address, height);
                queue.submit([encoder.finish()]);
            });
        assert_eq!(
            profiling
                .snapshot()
                .counters
                .get("buffer.presentation.creations")
                .copied()
                .unwrap_or(0),
            allocations,
            "height {height:?}"
        );
        for (i, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
            assert_eq!(
                *pixel,
                if i / 64 < 32 {
                    [255, 0, 0, 255]
                } else {
                    [0, 255, 0, 255]
                },
                "height {height:?}, pixel {i}"
            );
        }
    }
}

/// Render `scene` into the store, scan the returned FB out to a fresh (w×h) target, read it back.
fn store_to_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sr: &mut SceneRenderer,
    scene: &crate::hle::Scene,
    clear_policy: ClearPolicy,
    w: u32,
    h: u32,
) -> (Option<TargetId>, Vec<u8>) {
    let addr = sr.render_into_store(device, queue, scene, clear_policy); // owns encoder + submits
    let bpr = w * 4;
    // COPY_BYTES_PER_ROW_ALIGNMENT (256) may exceed the tightly-packed row stride at small `w`
    // (e.g. 32×4=128) — pad the copy's bytes_per_row and de-pad below so `pixel()`'s tightly-packed
    // indexing is unaffected for callers that read the result.
    let padded_bpr =
        bpr.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("store-scanout-target"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    if let Some(a) = addr {
        sr.scanout(device, &mut encoder, &view, a, None); // records the blit into the caller's encoder
    }
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("store-readback"),
        size: (padded_bpr * h) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bpr),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let data = slice.get_mapped_range();
    let out = if padded_bpr == bpr {
        data.to_vec()
    } else {
        (0..h)
            .flat_map(|row| {
                let start = (row * padded_bpr) as usize;
                data[start..start + bpr as usize].to_vec()
            })
            .collect()
    };
    drop(data);
    readback.unmap();
    (addr, out)
}

#[test]
fn pairless_walk_stores_and_scans_out() {
    let (device, queue, dual) = headless_device();
    let scene = scene_from_fixture("flat-color--white1");
    assert!(
        scene.framebuffer_pairs.is_empty(),
        "flat-color is pair-less"
    );
    let mut sr = SceneRenderer::new(&device, FORMAT, 64, 64, dual);

    let (addr, buf) = store_to_pixels(
        &device,
        &queue,
        &mut sr,
        &scene,
        ClearPolicy::PerFrame,
        64,
        64,
    );

    assert_eq!(
        addr,
        Some(TargetId::Legacy),
        "pairless scanout uses the legacy target"
    );
    assert!(
        sr.has_fb(TargetId::Legacy),
        "store must hold the legacy framebuffer"
    );
    let [r, g, b, _] = pixel(&buf, 64, 32, 32);
    assert!(
        (r as i16 - 64).abs() <= 2 && (g as i16 - 200).abs() <= 2 && (b as i16 - 255).abs() <= 2,
        "scanned-out center must be PRIM (64,200,255), got ({r},{g},{b})"
    );
}

/// A draw-nothing (empty) walk leaves the store + the scanout pointer untouched (spec §4 step 4).
#[test]
fn draw_nothing_walk_returns_none_and_touches_nothing() {
    let (device, queue, dual) = headless_device();
    let mut sr = SceneRenderer::new(&device, FORMAT, 64, 64, dual);
    let empty = crate::hle::Scene::default();
    let addr = sr.render_into_store(&device, &queue, &empty, ClearPolicy::PerFrame);
    assert_eq!(
        addr, None,
        "draw-nothing → None (present keeps the last good frame)"
    );
    assert!(
        !sr.has_fb(TargetId::Legacy),
        "draw-nothing must not create a store FB"
    );
}

#[test]
fn store_fb_recreates_on_resize() {
    let (device, queue, dual) = headless_device();
    let scene = scene_from_fixture("perspective-cube--white1");
    assert!(
        scene.framebuffer_pairs.is_empty(),
        "perspective-cube must be pair-less (no SetColorImage)"
    );
    assert!(
        scene.render_modes.iter().any(|r| r.z_test || r.z_write),
        "perspective-cube must use the z-buffer, else the depth-size mismatch has no teeth"
    );
    let mut sr = SceneRenderer::new(&device, FORMAT, 64, 64, dual);
    let (addr0, _) = store_to_pixels(
        &device,
        &queue,
        &mut sr,
        &scene,
        ClearPolicy::PerFrame,
        64,
        64,
    ); // 64²
    assert!(addr0.is_some(), "first render must store a framebuffer");

    sr.resize(&device, 32, 32);
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let (addr, _buf) = store_to_pixels(
        &device,
        &queue,
        &mut sr,
        &scene,
        ClearPolicy::PerFrame,
        32,
        32,
    );
    let err = pollster::block_on(scope.pop());
    assert_eq!(
        addr, addr0,
        "the same scene keeps its scanout addr across resize"
    );
    assert!(
        err.is_none(),
        "resized store FB (32²) must match the 32² depth view — a stale 64² color fb would raise a \
         color/depth attachment-size-mismatch validation error; got {err:?}"
    );
}

/// A paired 2D walk (SetColorImage + a full-FB FillRect at CIMG addr A) lands in the store keyed by
/// A, `has_fb(A)`, and `scanout(A)` shows the fill color.
#[test]
fn paired_walk_stores_by_cimg_addr_and_scans_out() {
    let (device, queue, dual) = headless_device();
    let scene = dl_2d_fill(0x0020_0000, /*rgba5551 red*/ 0xF801_F801);
    assert!(
        !scene.framebuffer_pairs.is_empty(),
        "must be a paired scene"
    );
    let mut sr = SceneRenderer::new(&device, FORMAT, 64, 64, dual);

    let (addr, buf) = store_to_pixels(
        &device,
        &queue,
        &mut sr,
        &scene,
        ClearPolicy::PerFrame,
        64,
        64,
    );
    assert_eq!(
        addr,
        Some(TargetId::Guest(0x0020_0000)),
        "paired scanout addr = last non-depth-clear CIMG"
    );
    assert!(sr.has_fb(0x0020_0000));
    let [r, g, b, _] = pixel(&buf, 64, 32, 32);
    assert!(
        r > 200 && g < 40 && b < 40,
        "fill must be ~red, got ({r},{g},{b})"
    );
}

/// ClearPolicy::Persist keeps the prior frame under a HUD-only repaint; PerFrame clears it.
#[test]
fn clear_policy_persist_keeps_prior_frame_perframe_clears() {
    let (device, queue, dual) = headless_device();
    let full_red = dl_2d_fill(0x0020_0000, 0xF801_F801);
    let corner_blue = dl_2d_fill_rect(0x0020_0000, 0x003F_003F, 0, 0, 8, 8); // 8×8 corner

    for (policy, expect_bg_red) in [(ClearPolicy::Persist, true), (ClearPolicy::PerFrame, false)] {
        let mut sr = SceneRenderer::new(&device, FORMAT, 64, 64, dual);
        sr.begin_frame();
        let _ = store_to_pixels(&device, &queue, &mut sr, &full_red, policy, 64, 64);
        sr.begin_frame();
        let (_a, buf) = store_to_pixels(&device, &queue, &mut sr, &corner_blue, policy, 64, 64);
        let [r, _g, _b, _] = pixel(&buf, 64, 40, 40); // OUTSIDE the blue corner
        assert_eq!(
            r > 200,
            expect_bg_red,
            "{policy:?}: background-red survives iff Persist"
        );
    }
}
