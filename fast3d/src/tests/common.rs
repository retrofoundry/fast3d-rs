use crate::hle::Scene;
use crate::render::{SceneRenderer, CLEAR_COLOR};

pub fn ref_pos(scene: &Scene, i: usize) -> [f32; 4] {
    let mvp = scene.mvp_table[scene.mtx_index[i] as usize];
    let (sc, tr) = scene.viewport_table[scene.viewport_index[i] as usize];
    let raw = scene.raw_pos[i];
    let clip = crate::hle::math::mul_row_vec4([raw[0], raw[1], raw[2], 1.0], mvp);
    let w = if clip[3] == 0.0 { 1e-6 } else { clip[3] };
    let (fw, fh) = (crate::hle::rsp::FB_WIDTH, crate::hle::rsp::FB_HEIGHT);
    [
        clip[0] * (2.0 * sc[0] / fw) + w * (2.0 * tr[0] / fw - 1.0),
        clip[1] * (2.0 * sc[1] / fh) + w * (1.0 - 2.0 * tr[1] / fh),
        clip[2] * sc[2] + w * tr[2],
        w,
    ]
}

pub fn ref_uv(scene: &Scene, i: usize) -> [f32; 2] {
    let s = scene.texcoord_table[scene.texcoord_index[i] as usize];
    let st = scene.raw_st[i];
    // The texcoord table is TEXEL-space (tile-size normalization is deferred to the fragment shader).
    // Recover the normalized UV the sampler sees by dividing by the textured material's tile dims,
    // matching the renderer's `inv_tex_size` = 1/(tex_w, tex_h).
    let (tw, th) = scene
        .materials
        .iter()
        .find(|m| m.tex_enable)
        .map(|m| (m.tex_w.max(1) as f32, m.tex_h.max(1) as f32))
        .unwrap_or((1.0, 1.0));
    [st[0] * s[0] / tw, st[1] * s[1] / th]
}

pub fn scene_from_fixture(name: &str) -> crate::hle::Scene {
    let (rdram, entry_addr) = crate::tests::fixtures::fixture(name);
    let r = crate::hle::interpret_rdram(rdram, entry_addr as u32);
    assert!(
        r.diags.is_empty(),
        "{name}: unexpected HLE diags: {:?}",
        r.diags
    );
    r.scene
}

/// A 32×32 solid-color env texture whose channels survive the RGBA16 round-trip (matches the
/// chrome-icosphere test's `[200,100,50]` synthetic env: decodes back to ≈(206,99,49)).
pub fn solid_env_texture(rgb: [u8; 3]) -> Vec<u8> {
    (0..32 * 32)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255u8])
        .collect()
}

/// Drive the facade END-TO-END: create an offscreen color target of size `(w, h)`, run
/// `renderer.render(scene → target)`, copy the target into a readback buffer, and return the
/// full RGBA8 pixel buffer (row-major, `bytes_per_row = w*4`). Native-only (uses `pollster`).
pub fn render_to_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut SceneRenderer,
    scene: &crate::hle::Scene,
    w: u32,
    h: u32,
) -> Vec<u8> {
    render_to_pixels_fmt(
        device,
        queue,
        renderer,
        scene,
        w,
        h,
        wgpu::TextureFormat::Rgba8Unorm,
    )
}

/// Like [`render_to_pixels`] but renders into a target of the given `format` and returns its raw
/// bytes (4 B/px). Used to headlessly cover the `Bgra8Unorm` present path: the bytes come back in
/// the format's native channel order (B,G,R,A for `Bgra8Unorm`). Both 2D scenes are 64 wide, so
/// `bytes_per_row = w*4 = 256` is already 256-byte-aligned — no readback row padding needed.
pub fn render_to_pixels_fmt(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut SceneRenderer,
    scene: &crate::hle::Scene,
    w: u32,
    h: u32,
    format: wgpu::TextureFormat,
) -> Vec<u8> {
    pixels_from_render(device, queue, w, h, format, |view| {
        renderer.render(device, queue, scene, view);
    })
}

pub fn pixels_from_render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    w: u32,
    h: u32,
    format: wgpu::TextureFormat,
    render: impl FnOnce(&wgpu::TextureView),
) -> Vec<u8> {
    let row_bytes = w * 4;
    let bytes_per_row = row_bytes.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("facade-target"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    // The facade owns its own encode→submit; this is the consumer's frame-present analog.
    render(&view);

    // Copy the rendered target into a mappable readback buffer (a separate encoder, as the real
    // consumer would do for a screenshot; the facade's own submit has already happened).
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("facade-readback"),
        size: (bytes_per_row * h) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
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
                bytes_per_row: Some(bytes_per_row),
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
    slice.map_async(wgpu::MapMode::Read, move |res| tx.send(res).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let data = slice.get_mapped_range();
    let mut out = Vec::with_capacity((row_bytes * h) as usize);
    for row in data.chunks(bytes_per_row as usize) {
        out.extend_from_slice(&row[..row_bytes as usize]);
    }
    drop(data);
    readback.unmap();
    out
}

/// Fetch the RGBA of pixel `(x, y)` from a row-major `w`-wide RGBA8 buffer.
pub fn pixel(buf: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let off = (y * w * 4 + x * 4) as usize;
    [buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]
}

/// `CLEAR_COLOR` (0.05, 0.05, 0.08) rendered into an Rgba8Unorm target rounds to ≈(13, 13, 20).
pub fn clear_color_rgb() -> [u8; 3] {
    [
        (CLEAR_COLOR.r * 255.0).round() as u8,
        (CLEAR_COLOR.g * 255.0).round() as u8,
        (CLEAR_COLOR.b * 255.0).round() as u8,
    ]
}

pub fn dl_2d_fill_rect(
    addr: u64,
    fill5551: u32,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
) -> crate::hle::Scene {
    use n64_gbi::encode::*; // gdp_set_color_image / gdp_set_scissor / gdp_set_fill_color /
                            // gdp_fill_rectangle / gsp_enddl  (encode.rs:387/399/405/409/242)
    let mut rdram: Vec<u8> = Vec::new();
    let mut push = |(w0, w1): (u32, u32)| {
        rdram.extend_from_slice(&w0.to_be_bytes());
        rdram.extend_from_slice(&w1.to_be_bytes());
    };
    push(gdp_set_color_image(
        0, /*RGBA*/
        2, /*16b*/
        64,
        addr as u32,
    ));
    push(gdp_set_scissor(0, 0, 0, 64 * 4, 64 * 4)); // → size_extent.1 = scissor.lry = 64 (M1)
    push(gdp_set_fill_color(fill5551));
    push(gdp_fill_rectangle(x0 * 4, y0 * 4, x1 * 4, y1 * 4));
    push(gsp_enddl());
    crate::hle::interpret_rdram(&rdram, 0).scene
}

/// Full-FB fill (0,0..64,64).
pub fn dl_2d_fill(addr: u64, fill5551: u32) -> crate::hle::Scene {
    dl_2d_fill_rect(addr, fill5551, 0, 0, 64, 64)
}
