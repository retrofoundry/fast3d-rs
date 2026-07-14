//! P4: the render-hook seam fires after VI scan-out in `present_to` (LoadOp::Load compositing), the
//! hook's returned pre-frame command buffers submit STRICTLY BEFORE the frame encoder, `HookFrame`
//! carries the target dimensions, and a hook-free present is unchanged.

use crate::{
    ClearPolicy, Hardware, HookFrame, Microcode, NopSink, PresentTarget, Rdram, RdramImage,
    RenderHook, Renderer, RendererConfig,
};
use std::cell::Cell;
use std::rc::Rc;

struct ImgHw {
    rdram: Vec<u8>,
}
impl Hardware for ImgHw {
    fn rdram(&self) -> impl Rdram + '_ {
        RdramImage::new(&self.rdram)
    }
}

fn cfg() -> RendererConfig {
    RendererConfig {
        resolution_multiplier: 1,
        sample_count: 1,
        present_mode: wgpu::PresentMode::Fifo,
        format: Some(wgpu::TextureFormat::Rgba8Unorm),
        clear_policy: ClearPolicy::PerFrame,
        power_preference: wgpu::PowerPreference::LowPower,
    }
}

fn flat_color_hw() -> (ImgHw, u64) {
    let src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scenes/flat-color.n64"),
    )
    .unwrap();
    let img = crate::asm::assemble_with_texture(&src, &[255u8; 4], 1, 1).unwrap();
    (ImgHw { rdram: img.rdram }, img.entry_addr as u64)
}

fn rgba_target(device: &wgpu::Device, w: u32, h: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hook-target"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
}

fn readback_rgba(r: &Renderer, tex: &wgpu::Texture, w: u32, h: u32) -> Vec<u8> {
    let buf = r.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("rb"),
        size: (w * h * 4) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = r
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    r.queue().submit(Some(enc.finish()));
    let slice = buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| tx.send(res).unwrap());
    r.device()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    slice.get_mapped_range().to_vec()
}

fn px(data: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let o = ((y * w + x) * 4) as usize;
    [data[o], data[o + 1], data[o + 2], data[o + 3]]
}
fn near(a: u8, b: u8) -> bool {
    (a as i16 - b as i16).abs() <= 2
}
fn close(p: [u8; 4], q: [u8; 4]) -> bool {
    near(p[0], q[0]) && near(p[1], q[1]) && near(p[2], q[2]) && near(p[3], q[3])
}

#[test]
fn hook_overlay_composites_over_scanout() {
    let (hw, entry) = flat_color_hw();
    let (device, queue, _dual) = crate::render::headless_device();
    let mut r = Renderer::with_device(
        device,
        queue,
        PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 64,
            height: 64,
        },
        cfg(),
    );
    r.begin_frame();
    r.process_dl(&hw, entry, Microcode::F3dex2, &mut NopSink);
    let (target, view) = rgba_target(r.device(), 64, 64);

    // Overlay: upload a 16x16 magenta sentinel, then copy it over the TOP-LEFT of the scanned-out
    // view inside the frame encoder (so it runs AFTER scanout — proves LoadOp::Load, not Clear).
    r.set_draw_hook(move |frame: HookFrame<'_>| {
        let sten = frame.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sentinel"),
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let magenta = [255u8, 0, 255, 255].repeat(16 * 16);
        frame.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &sten,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &magenta,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16 * 4),
                rows_per_image: Some(16),
            },
            wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
        );
        frame.encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &sten,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: frame.view.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
        );
    });
    r.present_to(&hw, &view);

    let data = readback_rgba(&r, &target, 64, 64);
    assert!(
        close(px(&data, 64, 32, 32), [64, 200, 255, 255]),
        "center pixel keeps the scanned-out game PRIM (hook Load-composited, not Clear): {:?}",
        px(&data, 64, 32, 32)
    );
    assert!(
        close(px(&data, 64, 4, 4), [255, 0, 255, 255]),
        "top-left sub-rect shows the overlay sentinel: {:?}",
        px(&data, 64, 4, 4)
    );
}

struct OrderingHook {
    a: wgpu::Buffer,
    b: wgpu::Buffer,
    c: wgpu::Buffer,
}
impl RenderHook for OrderingHook {
    fn draw(&mut self, frame: HookFrame<'_>) -> Vec<wgpu::CommandBuffer> {
        // Frame encoder: A -> C. This must observe A AFTER the extra CB has overwritten it.
        frame
            .encoder
            .copy_buffer_to_buffer(&self.a, 0, &self.c, 0, 4);
        // Extra pre-frame CB: B(sentinel) -> A. Returned so it submits STRICTLY BEFORE the frame encoder.
        let mut e = frame
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("pre-frame"),
            });
        e.copy_buffer_to_buffer(&self.b, 0, &self.a, 0, 4);
        vec![e.finish()]
    }
}

fn u32_buf(device: &wgpu::Device, val: u32, extra: wgpu::BufferUsages) -> wgpu::Buffer {
    // NOTE: base usage is COPY_DST only (NOT unconditional COPY_SRC) — wgpu forbids MAP_READ
    // combined with COPY_SRC (only the opposite COPY direction is allowed per usage). Callers that
    // need the buffer as a copy SOURCE pass COPY_SRC explicitly via `extra`.
    let b = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 4,
        usage: wgpu::BufferUsages::COPY_DST | extra,
        mapped_at_creation: true,
    });
    b.slice(..)
        .get_mapped_range_mut()
        .copy_from_slice(&val.to_le_bytes());
    b.unmap();
    b
}

#[test]
fn hook_extra_command_buffers_run_before_frame_encoder() {
    // No content: with nothing scanned out, the hook STILL runs (RN) — so this needs no DL.
    let hw = ImgHw { rdram: Vec::new() };
    let (device, queue, _dual) = crate::render::headless_device();
    // A is initialised NON-ZERO (0x1111_1111); clear_buffer would zero-fill, so init it explicitly.
    // A and B are both used as copy SOURCEs, so they need COPY_SRC explicitly.
    let a = u32_buf(&device, 0x1111_1111, wgpu::BufferUsages::COPY_SRC);
    let b = u32_buf(&device, 0x2222_2222, wgpu::BufferUsages::COPY_SRC);
    let c = u32_buf(&device, 0x0000_0000, wgpu::BufferUsages::MAP_READ);
    let mut r = Renderer::with_device(
        device,
        queue,
        PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 4,
            height: 4,
        },
        cfg(),
    );
    let (_target, view) = rgba_target(r.device(), 4, 4);
    r.set_render_hook(Box::new(OrderingHook { a, b, c: c.clone() }));
    r.present_to(&hw, &view);

    let slice = c.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| tx.send(res).unwrap());
    r.device()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    let got = u32::from_le_bytes(slice.get_mapped_range()[..4].try_into().unwrap());
    assert_eq!(
        got, 0x2222_2222,
        "the extra CB (B->A) must run BEFORE the frame encoder (A->C); saw A's initial value if reversed"
    );
}

#[test]
fn present_to_reports_target_dimensions() {
    // The hook still fires with nothing scanned out (RN); HookFrame dims come from the target view's
    // texture (RM). No DL needed.
    let hw = ImgHw { rdram: Vec::new() };
    let (device, queue, _dual) = crate::render::headless_device();
    let mut r = Renderer::with_device(
        device,
        queue,
        PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 8,
            height: 8,
        },
        cfg(),
    );
    let (_target, view) = rgba_target(r.device(), 96, 48);
    let dims = Rc::new(Cell::new((0u32, 0u32)));
    let d = dims.clone();
    r.set_draw_hook(move |frame: HookFrame<'_>| {
        d.set((frame.width, frame.height));
    });
    r.present_to(&hw, &view);
    assert_eq!(
        dims.get(),
        (96, 48),
        "HookFrame width/height == the target view texture's dimensions"
    );
}

#[test]
fn present_without_hook_is_unchanged() {
    // Regression guard: with no hook installed, present_to still scans the FB out identically to P3.
    let (hw, entry) = flat_color_hw();
    let (device, queue, _dual) = crate::render::headless_device();
    let mut r = Renderer::with_device(
        device,
        queue,
        PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 64,
            height: 64,
        },
        cfg(),
    );
    r.begin_frame();
    r.process_dl(&hw, entry, Microcode::F3dex2, &mut NopSink);
    let (target, view) = rgba_target(r.device(), 64, 64);
    r.present_to(&hw, &view);
    let data = readback_rgba(&r, &target, 64, 64);
    assert!(
        close(px(&data, 64, 32, 32), [64, 200, 255, 255]),
        "no-hook present_to is byte-for-byte the P3 scan-out: {:?}",
        px(&data, 64, 32, 32)
    );
}

struct LifecycleHook {
    counts: Rc<Cell<(u32, u32, u32)>>, // (init, draw, deinit)
}
impl RenderHook for LifecycleHook {
    fn init(&mut self, _d: &wgpu::Device, _q: &wgpu::Queue, _f: wgpu::TextureFormat) {
        let (i, d, x) = self.counts.get();
        self.counts.set((i + 1, d, x));
    }
    fn draw(&mut self, frame: HookFrame<'_>) -> Vec<wgpu::CommandBuffer> {
        let (i, d, x) = self.counts.get();
        self.counts.set((i, d + 1, x));
        // Paint a 16x16 magenta sub-rect over the top-left, inside the frame encoder (after scanout).
        let sten = frame.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("life-sentinel"),
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let magenta = [255u8, 0, 255, 255].repeat(16 * 16);
        frame.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &sten,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &magenta,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16 * 4),
                rows_per_image: Some(16),
            },
            wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
        );
        frame.encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &sten,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: frame.view.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
        );
        Vec::new()
    }
    fn deinit(&mut self) {
        let (i, d, x) = self.counts.get();
        self.counts.set((i, d, x + 1));
    }
}

#[test]
fn hook_full_lifecycle_end_to_end() {
    let (hw, entry) = flat_color_hw();
    let (device, queue, _dual) = crate::render::headless_device();
    let mut r = Renderer::with_device(
        device,
        queue,
        PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 64,
            height: 64,
        },
        cfg(),
    );

    let counts = Rc::new(Cell::new((0u32, 0u32, 0u32)));
    r.set_render_hook(Box::new(LifecycleHook {
        counts: counts.clone(),
    }));
    assert_eq!(counts.get(), (1, 0, 0), "init fired once at registration");

    let (target, view) = rgba_target(r.device(), 64, 64);
    for _ in 0..2 {
        r.begin_frame();
        r.process_dl(&hw, entry, Microcode::F3dex2, &mut NopSink);
        r.present_to(&hw, &view);
    }
    assert_eq!(
        counts.get(),
        (1, 2, 0),
        "draw fired once per present, twice"
    );

    // Prove the last frame shows game + overlay before teardown.
    let data = readback_rgba(&r, &target, 64, 64);
    assert!(
        close(px(&data, 64, 32, 32), [64, 200, 255, 255]),
        "game visible at center"
    );
    assert!(
        close(px(&data, 64, 4, 4), [255, 0, 255, 255]),
        "overlay visible at top-left"
    );

    assert!(r.take_render_hook().is_some());
    assert_eq!(counts.get(), (1, 2, 1), "deinit fired once on removal");
}
