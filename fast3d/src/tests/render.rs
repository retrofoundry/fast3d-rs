use crate::render::{
    decode_rgba16, headless_device, CombinerUniform, OutVertex, RspProcessParams,
    RspProcessPipeline, TexturedPipeline, CLEAR_COLOR, DEPTH_FORMAT,
};
use wgpu::util::DeviceExt;

/// Build split bind groups: group0 (tex + sampler) and group1 (combiner uniform).
/// Mirrors the A8a layout: `@group(0)` for material resources, `@group(1)` for the
/// per-run uniform (dynamic offset; stride=0 so all runs share the same slot).
#[allow(clippy::too_many_arguments)]
fn make_bind_groups(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &TexturedPipeline,
    tex_rgba8: &[u8],
    tex_w: u32,
    tex_h: u32,
    uniform: &CombinerUniform,
    nearest: bool,
) -> (wgpu::BindGroup, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test-tex"),
        size: wgpu::Extent3d {
            width: tex_w,
            height: tex_h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        tex_rgba8,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(tex_w * 4),
            rows_per_image: Some(tex_h),
        },
        wgpu::Extent3d {
            width: tex_w,
            height: tex_h,
            depth_or_array_layers: 1,
        },
    );
    let tex_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let filter = if nearest {
        wgpu::FilterMode::Nearest
    } else {
        wgpu::FilterMode::Linear
    };
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("test-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });

    let group0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("test-bg-g0"),
        layout: pipeline.bind_group_layout(),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&tex_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            // TEXEL1 slot: these single-texture tests never sample it — reuse tex0's view/sampler
            // to satisfy the group(0) layout (tex_enable1 = 0 in the uniform).
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&tex_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            // DETAIL slot (4/5): never sampled by these tests — reuse tex0's view/sampler.
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&tex_view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ]
        // LOD-level slots (6..=12): never sampled by these non-LOD tests — reuse tex0's view.
        .into_iter()
        .chain((6..13u32).map(|b| wgpu::BindGroupEntry {
            binding: b,
            resource: wgpu::BindingResource::TextureView(&tex_view),
        }))
        .collect::<Vec<_>>(),
    });

    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("combiner-uniform"),
        contents: bytemuck::bytes_of(uniform),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let group1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("test-bg-g1"),
        layout: pipeline.uniform_bind_group_layout(),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buf.as_entire_binding(),
        }],
    });

    (group0, group1)
}

// ---- Helper: render a full-screen quad with a solid shade and read top-quarter and bottom-quarter
//      center-column pixels. Uses a non-symmetric 2x2 texture: row 0 = tex_val (top), row 1 = 0
//      (bottom). Returns (top_r, bottom_r) so callers can assert both the combiner arithmetic and
//      the absence of a V-flip.
//
//      UV layout (matches the quad vertex assignment below):
//        clip y=+1 (screen top, pixel row 0)  → UV.y=0.0  → texture row 0 (top)
//        clip y=-1 (screen bottom, pixel row 63) → UV.y=1.0  → texture row 1 (bottom)
//
//      Pixel row 8  (UV.y ≈ 0.125) → Nearest → texture row 0  (top color)
//      Pixel row 56 (UV.y ≈ 0.875) → Nearest → texture row 1  (bottom = 0)
//      A V-flip would swap these, making top_r ≈ 0 and bottom_r ≈ tex_val. ----

fn render_and_read_center_channel(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    combine_l: u32,
    combine_h: u32,
    shade_val: f32,
    tex_val: f32, // top-row texel (replicated to RGB, a=1.0); bottom row is always 0
) -> (u8, u8) {
    const W: u32 = 64;
    const H: u32 = 64;
    let bytes_per_row = W * 4;

    let format = wgpu::TextureFormat::Rgba8Unorm;

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
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

    // A full-screen quad (covers all pixels) with shade = shade_val.
    // UV.y=0.0 at screen-top (clip y=+1), UV.y=1.0 at screen-bottom (clip y=-1).
    struct V {
        position: [f32; 4],
        color: [f32; 4],
        uv: [f32; 2],
    }
    let s = shade_val;
    let verts = [
        V {
            position: [-1.0, -1.0, 0.0, 1.0],
            color: [s, s, s, s],
            uv: [0.0, 1.0],
        },
        V {
            position: [1.0, -1.0, 0.0, 1.0],
            color: [s, s, s, s],
            uv: [1.0, 1.0],
        },
        V {
            position: [1.0, 1.0, 0.0, 1.0],
            color: [s, s, s, s],
            uv: [1.0, 0.0],
        },
        V {
            position: [-1.0, 1.0, 0.0, 1.0],
            color: [s, s, s, s],
            uv: [0.0, 0.0],
        },
    ];
    let indices: [u32; 6] = [0, 1, 2, 0, 2, 3];

    let pos: Vec<OutVertex> = verts
        .iter()
        .map(|v| OutVertex {
            position: v.position,
            color: v.color,
            uv: v.uv,
            _pad: [0.0, 0.0],
        })
        .collect();
    let pos_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("pos"),
        contents: bytemuck::cast_slice(&pos),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ibuf"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // Non-symmetric 2x2 texture:
    //   row 0 (top)    = [t, t, t, 255]  where t = tex_val * 255
    //   row 1 (bottom) = [0, 0, 0, 255]  (clearly different color)
    // A V-flip would put the black row at the top of the rendered quad, immediately visible.
    let t = (tex_val * 255.0) as u8;
    #[rustfmt::skip]
    let tex_rgba8: Vec<u8> = vec![
        t, t, t, 255,  // row 0, col 0
        t, t, t, 255,  // row 0, col 1
        0, 0, 0, 255,  // row 1, col 0
        0, 0, 0, 255,  // row 1, col 1
    ];

    let uniform = CombinerUniform {
        combine_l,
        combine_h,
        cycle_type: 0, // 1-cycle
        tex_enable: 1,
        blender_mux: 0,
        force_blend: 0,
        alpha_mode: 0,
        alpha_threshold: 0.0,
        prim: [1.0, 1.0, 1.0, 1.0],
        env: [0.0, 0.0, 0.0, 1.0],
        blend_color: [0.0; 4],
        fog_color: [0.0; 4],
        inv_tex_size: [1.0, 1.0, 0.0, 0.0],
        inv_tex1_size: [1.0, 1.0, 0.0, 0.0],
        lod_params: [0.0, 1.0, 0.0, 1.0],
        inv_detail_size: [1.0, 1.0, 0.0, 0.0],
    };

    let pipeline = TexturedPipeline::new(device, format, DEPTH_FORMAT);
    let (group0_bg, group1_bg) =
        make_bind_groups(device, queue, &pipeline, &tex_rgba8, 2, 2, &uniform, true);

    let clear = wgpu::Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let mock_scene = crate::hle::Scene {
        draw_runs: vec![crate::hle::DrawRun {
            fog_color: [0; 4],
            material_index: 0,
            render_mode_index: 0,
            cull: crate::hle::CullKind::None,
            index_count: indices.len() as u32,
            index_start: 0,
        }],
        render_modes: vec![Default::default()],
        ..Default::default()
    };
    pipeline.draw(
        &mut encoder,
        &view,
        &pos_buf,
        &ibuf,
        &mock_scene,
        clear,
        &[&group0_bg],
        &group1_bg,
        0,
        None,
    );

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
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| {
        tx.send(res).unwrap();
    });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();

    let data = slice.get_mapped_range();
    // Top-quarter center-column pixel (row 8, col 32): UV.y ≈ 0.125 → texture row 0 (top)
    let top_off = (8 * bytes_per_row + 32 * 4) as usize;
    // Bottom-quarter center-column pixel (row 56, col 32): UV.y ≈ 0.875 → texture row 1 (bottom=0)
    let bot_off = (56 * bytes_per_row + 32 * 4) as usize;
    let top_r = data[top_off];
    let bot_r = data[bot_off];
    drop(data);
    readback.unmap();
    (top_r, bot_r)
}

#[test]
fn decode_rgba16_bit_replication() {
    // v=0xF801 (big-endian bytes 0xF8,0x01): r5=31,g5=0,b5=0,a1=1 -> (c<<3)|(c>>2) replication
    let out = decode_rgba16(&[0xF8, 0x01]);
    assert_eq!(out, vec![255, 0, 0, 255]);
}

#[test]
fn combiner_uniform_packs_raw_words() {
    // Can't call gbi from renderer tests; build the selectors directly.
    use crate::hle::Material;

    let selectors = crate::hle::combiner::decode_combine(0xFC12_7E24, 0xFFFF_F9FC);

    let mat = Material {
        texture: vec![128u8; 4],
        tex_w: 1,
        tex_h: 1,
        selectors,
        cycle_type: 0, // 1-cycle
        prim: [255, 255, 255, 255],
        env: [0, 0, 0, 255],
        blend_color: [0, 0, 0, 255],
        tex_enable: true,
        wrap_s: 2,
        wrap_t: 2,
        fmt: 0,
        siz: 0,
        tile_count: 1,
        tex1: None,
        prim_lod_frac: 0.0,
        prim_min_level: 0.0,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    };
    let u = CombinerUniform::from_run(&mat, &crate::hle::RenderMode::default(), [0; 4]);
    assert_eq!(u.combine_l, 0xFC12_7E24);
    assert_eq!(u.combine_h, 0xFFFF_F9FC);
    assert_eq!(u.cycle_type, 0);
    assert_eq!(u.tex_enable, 1);
}

#[test]
fn modulate_is_texel_times_shade_and_decal_is_texel() {
    let (device, queue, _dual_source) = headless_device();

    // MODULATE: RGB=(TEXEL0-ZERO)*SHADE+ZERO, ALPHA=SHADE passthrough
    // combine words (0xFC127E24, 0xFFFFF9FC) from golden test
    let modulate_l: u32 = 0xFC12_7E24;
    let modulate_h: u32 = 0xFFFF_F9FC;

    // DECAL combine, both cycles: RGB = TEXEL0 (a=b=c=ZERO, d=TEXEL0) and ALPHA = TEXEL0
    // (a=b=c=ZERO, d=TEXEL0). The words below are the encoded combine; the exact per-slot
    // selector decode is asserted in hle's combiner round-trip tests.
    let decal_l: u32 = 0xFCFF_FFFF;
    let decal_h: u32 = 0xFFFC_F279;

    // Texture: top row = T ≈ 0.502 (128/255), bottom row = 0.0 (black).
    // shade = 0.5.
    // MODULATE: top_r = T * shade ≈ 0.502 * 0.502 ≈ 0.252 -> ~64/255 ≈ 0.251
    //           bot_r  = 0 * shade = 0 (bottom texture row is black)
    // DECAL:    top_r = T ≈ 0.502
    //           bot_r = 0
    // V-flip guard: if flipped, top_r would sample the bottom (black) row -> ~0,
    //               failing the arithmetic assertion directly.
    let (top_modulate, bot_modulate) = render_and_read_center_channel(
        &device,
        &queue,
        modulate_l,
        modulate_h,
        0.5,           // shade
        128.0 / 255.0, // top-row texel ≈ 0.502; bottom row = 0
    );
    // MODULATE result at top-quarter pixel: texel * shade ≈ 0.252
    let modulate_f = top_modulate as f32 / 255.0;
    assert!(
        (modulate_f - 0.25).abs() < 0.01,
        "MODULATE top-quarter should be ~0.25 (texel*shade), got {}",
        modulate_f
    );
    // Bottom-quarter pixel samples texture row 1 (black): result must be near 0 for both
    // MODULATE (0*shade=0) and for the non-symmetric texture to be meaningful.
    // This also confirms no V-flip: a V-flip would swap top and bottom, making top_modulate≈0.
    assert!(
        bot_modulate < 5,
        "MODULATE bottom-quarter should be ~0 (black texture row), got {}",
        bot_modulate
    );

    let (top_decal, bot_decal) = render_and_read_center_channel(
        &device,
        &queue,
        decal_l,
        decal_h,
        0.5,           // shade (should NOT affect DECAL output)
        128.0 / 255.0, // top-row texel ≈ 0.502; bottom row = 0
    );
    // DECAL result at top-quarter pixel: texel passthrough ≈ 0.502
    let decal_f = top_decal as f32 / 255.0;
    assert!(
        (decal_f - 0.5).abs() < 0.01,
        "DECAL top-quarter should be ~0.5 (texel passthrough), got {}",
        decal_f
    );
    // Non-symmetric texture confirms no V-flip: bottom row must sample the black texel.
    assert!(
        bot_decal < 5,
        "DECAL bottom-quarter should be ~0 (black texture row, no V-flip), got {}",
        bot_decal
    );
}

#[test]
fn renders_red_triangle_center_and_clear_corner() {
    const W: u32 = 64;
    const H: u32 = 64;
    let bytes_per_row = W * 4; // 256, already 256-aligned -> no padding
    assert_eq!(bytes_per_row % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, 0);

    let (device, queue, _dual_source) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm; // linear -> exact byte asserts

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
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

    // A point-up red triangle covering the center but not the corners.
    // tex_enable=0 so the combiner just uses shade color (passed as vertex color).
    // SHADE-passthrough combine, both cycles: RGB = SHADE (a=b=c=ZERO, d=SHADE) and ALPHA =
    // SHADE (a=b=c=ZERO, d=SHADE). With tex_enable=0 the fragment returns the shade (vertex
    // color) unchanged, since (ZERO-ZERO)*ZERO + SHADE = SHADE. The exact per-slot selector
    // decode is asserted in hle's combiner round-trip tests.
    let shade_through_l: u32 = 0xFCFF_FFFF;
    let shade_through_h: u32 = 0xFF9F_F93C;

    struct V {
        position: [f32; 4],
        color: [f32; 4],
        uv: [f32; 2],
    }
    let red = [1.0f32, 0.0, 0.0, 1.0];
    let verts = [
        V {
            position: [-0.75, -0.75, 0.0, 1.0],
            color: red,
            uv: [0.0, 0.0],
        },
        V {
            position: [0.75, -0.75, 0.0, 1.0],
            color: red,
            uv: [1.0, 0.0],
        },
        V {
            position: [0.0, 0.75, 0.0, 1.0],
            color: red,
            uv: [0.5, 1.0],
        },
    ];
    let indices: [u32; 3] = [0, 1, 2];

    let pos: Vec<OutVertex> = verts
        .iter()
        .map(|v| OutVertex {
            position: v.position,
            color: v.color,
            uv: v.uv,
            _pad: [0.0, 0.0],
        })
        .collect();
    let pos_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("pos"),
        contents: bytemuck::cast_slice(&pos),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ibuf"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let clear = wgpu::Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

    let uniform = CombinerUniform {
        combine_l: shade_through_l,
        combine_h: shade_through_h,
        cycle_type: 0,
        tex_enable: 0, // no texture
        blender_mux: 0,
        force_blend: 0,
        alpha_mode: 0,
        alpha_threshold: 0.0,
        prim: [1.0, 1.0, 1.0, 1.0],
        env: [0.0, 0.0, 0.0, 1.0],
        blend_color: [0.0; 4],
        fog_color: [0.0; 4],
        inv_tex_size: [1.0, 1.0, 0.0, 0.0],
        inv_tex1_size: [1.0, 1.0, 0.0, 0.0],
        lod_params: [0.0, 1.0, 0.0, 1.0],
        inv_detail_size: [1.0, 1.0, 0.0, 0.0],
    };

    // Use a 1x1 white texture as placeholder (tex_enable=0 so it doesn't matter)
    let tex_rgba8 = vec![255u8, 255, 255, 255];

    let pipeline = TexturedPipeline::new(&device, format, DEPTH_FORMAT);
    let (group0_bg, group1_bg) =
        make_bind_groups(&device, &queue, &pipeline, &tex_rgba8, 1, 1, &uniform, true);

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let mock_scene = crate::hle::Scene {
        draw_runs: vec![crate::hle::DrawRun {
            fog_color: [0; 4],
            material_index: 0,
            render_mode_index: 0,
            cull: crate::hle::CullKind::None,
            index_count: indices.len() as u32,
            index_start: 0,
        }],
        render_modes: vec![Default::default()],
        ..Default::default()
    };
    pipeline.draw(
        &mut encoder,
        &view,
        &pos_buf,
        &ibuf,
        &mock_scene,
        clear,
        &[&group0_bg],
        &group1_bg,
        0,
        None,
    );

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
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| {
        tx.send(res).unwrap();
    });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();

    let data = slice.get_mapped_range();
    // Center pixel (32,32): inside the triangle -> opaque red.
    let center = (32 * bytes_per_row + 32 * 4) as usize;
    assert_eq!(
        &data[center..center + 4],
        &[255, 0, 0, 255],
        "center should be opaque red"
    );
    // Top-left corner pixel (0,0): outside the point-up triangle -> the clear color (black).
    let corner = 0usize;
    assert_eq!(
        &data[corner..corner + 4],
        &[0, 0, 0, 255],
        "corner should be the clear color"
    );
    drop(data);
    readback.unmap();
}

#[test]
fn depth_test_hides_the_farther_triangle() {
    // Two full-screen triangles: a NEAR green one (z=0.1) drawn FIRST, then a FAR red one (z=0.9).
    // With depth_compare=Less + a cleared depth buffer, the near green survives (the later far red is
    // rejected) — which only holds if depth actually works (paint order alone would make red win).
    let (device, queue, _dual_source) = headless_device();
    const W: u32 = 8;
    const H: u32 = 8;
    let bytes_per_row = W * 4 * 8; // pad to 256-byte alignment (8*4=32 -> round up to 256)
    let format = wgpu::TextureFormat::Rgba8Unorm;

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
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

    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());

    #[derive(Clone)]
    struct V {
        position: [f32; 4],
        color: [f32; 4],
        uv: [f32; 2],
    }
    let far = [1.0f32, 0.0, 0.0, 1.0];
    let near = [0.0f32, 1.0, 0.0, 1.0];
    let mk = |z: f32, c: [f32; 4]| {
        [
            V {
                position: [-1.0, -1.0, z, 1.0],
                color: c,
                uv: [0.0, 0.0],
            },
            V {
                position: [3.0, -1.0, z, 1.0],
                color: c,
                uv: [0.0, 0.0],
            },
            V {
                position: [-1.0, 3.0, z, 1.0],
                color: c,
                uv: [0.0, 0.0],
            },
        ]
    };
    let mut verts = Vec::new();
    // NEAR drawn FIRST, FAR second: only a working Less depth test keeps the earlier near-green
    // fragment alive against the later far-red. (If depth were off, paint order would make red win.)
    verts.extend_from_slice(&mk(0.1, near));
    verts.extend_from_slice(&mk(0.9, far));
    let indices: [u32; 6] = [0, 1, 2, 3, 4, 5];

    let pos: Vec<OutVertex> = verts
        .iter()
        .map(|v| OutVertex {
            position: v.position,
            color: v.color,
            uv: v.uv,
            _pad: [0.0, 0.0],
        })
        .collect();
    let pos_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("pos"),
        contents: bytemuck::cast_slice(&pos),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ibuf"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let uniform = CombinerUniform {
        combine_l: 0xFCFF_FFFF,
        combine_h: 0xFF9F_F93C,
        cycle_type: 0,
        tex_enable: 0,
        blender_mux: 0,
        force_blend: 0,
        alpha_mode: 0,
        alpha_threshold: 0.0,
        prim: [1.0, 1.0, 1.0, 1.0],
        env: [0.0, 0.0, 0.0, 1.0],
        blend_color: [0.0; 4],
        fog_color: [0.0; 4],
        inv_tex_size: [1.0, 1.0, 0.0, 0.0],
        inv_tex1_size: [1.0, 1.0, 0.0, 0.0],
        lod_params: [0.0, 1.0, 0.0, 1.0],
        inv_detail_size: [1.0, 1.0, 0.0, 0.0],
    };
    let pipeline = TexturedPipeline::new(&device, format, DEPTH_FORMAT);
    let (group0_bg, group1_bg) = make_bind_groups(
        &device,
        &queue,
        &pipeline,
        &[255, 255, 255, 255],
        1,
        1,
        &uniform,
        true,
    );

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let mock_scene = crate::hle::Scene {
        draw_runs: vec![crate::hle::DrawRun {
            fog_color: [0; 4],
            material_index: 0,
            render_mode_index: 0,
            cull: crate::hle::CullKind::None,
            index_count: indices.len() as u32,
            index_start: 0,
        }],
        render_modes: vec![crate::hle::RenderMode {
            z_test: true,
            z_write: true,
            ..Default::default()
        }],
        ..Default::default()
    };
    pipeline.draw(
        &mut encoder,
        &view,
        &pos_buf,
        &ibuf,
        &mock_scene,
        wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
        &[&group0_bg],
        &group1_bg,
        0,
        Some(&depth_view),
    );
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
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
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
    let off = (4 * bytes_per_row + 4 * 4) as usize;
    assert_eq!(
        &data[off..off + 4],
        &[0, 255, 0, 255],
        "near (green) must win the depth test"
    );
    drop(data);
    readback.unmap();
}

pub(super) struct GpuOut {
    pos: [f32; 4],
    pub(super) color: [f32; 4],
    pub(super) uv: [f32; 2],
}

pub(super) fn run_compute_outputs(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &crate::hle::Scene,
) -> Vec<GpuOut> {
    use crate::render::rsp_buffers as rb;
    use wgpu::util::DeviceExt;
    let n = scene.raw_pos.len() as u32;
    if n == 0 {
        return Vec::new();
    }
    let mk = |data: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: data,
            usage,
        })
    };
    let s = wgpu::BufferUsages::STORAGE;
    let source = mk(bytemuck::cast_slice(&rb::src_vertices(scene)), s);
    let mvp_table = mk(bytemuck::cast_slice(&rb::mvp_table(scene)), s);
    let viewport_table = mk(bytemuck::cast_slice(&rb::viewport_table(scene)), s);
    let texcoord_table = mk(bytemuck::cast_slice(&rb::texcoord_table(scene)), s);
    let lights_table = mk(bytemuck::cast_slice(&rb::lights_table(scene)), s);
    let lookat_table = mk(bytemuck::cast_slice(&rb::lookat_table(scene)), s);
    let fog_table = mk(bytemuck::cast_slice(&rb::fog_table(scene)), s);
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (n as u64) * 48,
        mapped_at_creation: false,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let params = mk(
        bytemuck::bytes_of(&crate::render::RspProcessParams {
            vertex_count: n,
            _pad: [0; 3],
        }),
        wgpu::BufferUsages::UNIFORM,
    );
    let pipe = crate::render::RspProcessPipeline::new(device);
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: pipe.bind_group_layout(),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: source.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: mvp_table.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: viewport_table.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: texcoord_table.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: lights_table.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: lookat_table.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: out.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: fog_table.as_entire_binding(),
            },
        ],
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (n as u64) * 48,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    pipe.dispatch(&mut enc, &bg, n);
    enc.copy_buffer_to_buffer(&out, 0, &readback, 0, (n as u64) * 48);
    queue.submit(Some(enc.finish()));
    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let data = slice.get_mapped_range();
    // OutVertex = 12 f32 (48B): [0..4]=pos, [4..8]=color, [8..10]=uv, [10..12]=pad
    let res: Vec<GpuOut> = bytemuck::cast_slice::<u8, f32>(&data)
        .as_chunks::<12>()
        .0
        .iter()
        .map(|c| GpuOut {
            pos: [c[0], c[1], c[2], c[3]],
            color: [c[4], c[5], c[6], c[7]],
            uv: [c[8], c[9]],
        })
        .collect();
    drop(data);
    readback.unmap();
    res
}

// Independent oracle: re-derives expected pos/uv from raw inputs + tables only.
fn ref_pos(raw: [f32; 3], mvp: &crate::hle::math::Mat4, vp: &([f32; 3], [f32; 3])) -> [f32; 4] {
    let clip = crate::hle::math::mul_row_vec4([raw[0], raw[1], raw[2], 1.0], *mvp);
    let w = if clip[3] == 0.0 { 1e-6 } else { clip[3] };
    let (sc, tr) = (vp.0, vp.1);
    let (fw, fh) = (crate::hle::rsp::FB_WIDTH, crate::hle::rsp::FB_HEIGHT);
    [
        clip[0] * (2.0 * sc[0] / fw) + w * (2.0 * tr[0] / fw - 1.0),
        clip[1] * (2.0 * sc[1] / fh) + w * (1.0 - 2.0 * tr[1] / fh),
        clip[2] * sc[2] + w * tr[2],
        w,
    ]
}

// uv oracle: the table already holds the f64-prefolded scale; kernel == this f32 multiply.
fn ref_uv(st: [f32; 2], tc: [f32; 2]) -> [f32; 2] {
    [st[0] * tc[0], st[1] * tc[1]]
}

fn ref_texgen_uv(scene: &crate::hle::Scene, i: usize) -> Option<[f32; 2]> {
    let mode = scene.texgen_mode[i];
    if mode == 0 {
        return None;
    }
    let normal = [0, 8, 16].map(|shift| (scene.cn[i] >> shift) as u8 as i8 as f64 / 127.0);
    let (s, t) = scene.lookat_table[scene.lookat_index[i] as usize];
    let scale = scene.texgen_scale_table[scene.texcoord_index[i] as usize];
    let generated = |axis: [f32; 3]| {
        let dot = normal
            .iter()
            .zip(axis)
            .map(|(n, a)| n * f64::from(a))
            .sum::<f64>()
            .clamp(-1.0, 1.0);
        if mode == 2 {
            (-dot).acos() * (1024.0 / std::f64::consts::PI)
        } else {
            (dot + 1.0) * 512.0
        }
    };
    Some([
        (generated(s) * f64::from(scale[0])) as f32,
        (generated(t) * f64::from(scale[1])) as f32,
    ])
}

// color oracle: diffuse lighting if light_count > 0, else cn RGBA passthrough.
fn ref_color(scene: &crate::hle::Scene, i: usize) -> [f32; 4] {
    let cn = scene.cn[i];
    // Per-vertex fog (per-vertex fog indices): mirror rsp_process.wgsl — when this vertex was loaded with
    // G_FOG (scene.fog[i] != 0) the kernel OVERWRITES color.a with the fog factor from RAW clip-Z.
    let a = if scene.fog.get(i).copied().unwrap_or(0) != 0 {
        let mvp = &scene.mvp_table[scene.mtx_index[i] as usize];
        let clip = crate::hle::math::mul_row_vec4(
            [
                scene.raw_pos[i][0],
                scene.raw_pos[i][1],
                scene.raw_pos[i][2],
                1.0,
            ],
            *mvp,
        );
        let w = if clip[3] == 0.0 { 1e-6 } else { clip[3] };
        let fz = clip[2].max(0.0) / w;
        let [mul, offset] = scene.fog_table[scene.fog[i] as usize - 1];
        (fz * mul as f32 + offset as f32).clamp(0.0, 255.0) / 255.0
    } else {
        ((cn >> 24) & 0xff) as f32 / 255.0
    };
    let lc = scene.light_count[i] as usize;
    if lc > 0 {
        let li = scene.light_index[i] as usize;
        let n = [
            (cn & 0xff) as i8 as f32 / 127.0,
            ((cn >> 8) & 0xff) as i8 as f32 / 127.0,
            ((cn >> 16) & 0xff) as i8 as f32 / 127.0,
        ];
        let mut c = scene.lights_table[li + lc - 1].1; // ambient
        for k in 0..lc - 1 {
            let (d, col) = scene.lights_table[li + k];
            let nl = (n[0] * d[0] + n[1] * d[1] + n[2] * d[2]).max(0.0);
            c = [c[0] + nl * col[0], c[1] + nl * col[1], c[2] + nl * col[2]];
        }
        [c[0].min(1.0), c[1].min(1.0), c[2].min(1.0), a]
    } else {
        [
            (cn & 0xff) as f32 / 255.0,
            ((cn >> 8) & 0xff) as f32 / 255.0,
            ((cn >> 16) & 0xff) as f32 / 255.0,
            a,
        ]
    }
}

#[test]
fn compute_outputs_match_oracle_for_every_scene() {
    let (device, queue, _dual_source) = headless_device();
    let scenes_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scenes");
    let white = vec![255u8; 32 * 32 * 4];
    let mut checked = 0;
    for entry in std::fs::read_dir(&scenes_dir).expect("tests/scenes") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("n64") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(&path).unwrap();
        let img = crate::asm::assemble_with_texture(&src, &white, 32, 32).unwrap();
        let r = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
        assert!(r.diags.is_empty(), "{name}: {:?}", r.diags);
        // Regression guard: chrome-icosphere must actually exercise texgen (else this golden test
        // could "pass" on a scene whose texgen silently broke into the normal st path).
        if name == "chrome-icosphere.n64" {
            assert!(
                r.scene.texgen_mode.iter().any(|&m| m != 0),
                "chrome-icosphere must exercise texgen (no texgen vertex seen)"
            );
        }
        let gpu = run_compute_outputs(&device, &queue, &r.scene);
        assert_eq!(gpu.len(), r.scene.raw_pos.len(), "{name}: vertex count");
        for (i, gv) in gpu.iter().enumerate() {
            let mvp = &r.scene.mvp_table[r.scene.mtx_index[i] as usize];
            let vp = &r.scene.viewport_table[r.scene.viewport_index[i] as usize];
            let tc = r.scene.texcoord_table[r.scene.texcoord_index[i] as usize];
            let ep = ref_pos(r.scene.raw_pos[i], mvp, vp);
            // Exactly one uv oracle per vertex: the kernel OVERRIDES o.uv for texgen vertices,
            // so the st-based ref_uv would fail on them.
            let eu = match ref_texgen_uv(&r.scene, i) {
                Some(e) => e,                          // texgen: reflection-mapped uv
                None => ref_uv(r.scene.raw_st[i], tc), // normal st-based path (tc unchanged, [f32;2])
            };
            let ec = ref_color(&r.scene, i);
            for (c, (&a, &b)) in gv.pos.iter().zip(ep.iter()).enumerate() {
                let tol = 1e-4_f32.max(1e-4 * a.abs().max(b.abs()));
                assert!(
                    (a - b).abs() <= tol,
                    "{name}: vtx {i} pos {c}: gpu {a} vs ref {b}"
                );
            }
            for (c, (&a, &b)) in gv.uv.iter().zip(eu.iter()).enumerate() {
                let tol = 1.0 / 1024.0;
                assert!(
                    (a - b).abs() <= tol,
                    "{name}: vtx {i} uv {c}: gpu {a} vs ref {b}"
                );
            }
            for (c, (&a, &b)) in gv.color.iter().zip(ec.iter()).enumerate() {
                let tol = 1e-4_f32.max(1e-4 * a.abs().max(b.abs()));
                assert!(
                    (a - b).abs() <= tol,
                    "{name}: vtx {i} color {c}: gpu {a} vs ref {b}"
                );
            }
        }
        checked += 1;
    }
    assert!(
        checked >= 6,
        "expected the gallery scenes, checked {checked}"
    );
}

/// FIX 3 — focused kernel-math GPU test: diffuse lighting color output.
///
/// Scene setup (identity MVP/viewport so position is irrelevant):
///   Light dir = +Z (object-space, already prefolded), col = white [1,1,1].
///   Ambient = grey [0.2, 0.2, 0.2].
///   lights_table = [ ([0,0,1], [1,1,1]), ([0,0,0], [0.2,0.2,0.2]) ]  (dir then ambient last).
///
///   Vertex A: normal = (0,0,127) s8 → raw dot = 0/127*0 + 0/127*0 + 127/127*1 = 1.0
///             expected color = clamp(ambient + 1.0*white, 0,1) = [1.2,1.2,1.2] clamped = [1,1,1].
///   Vertex B: normal = (0,0,-127) s8 → raw dot = -1.0 → max(dot,0) = 0.0
///             expected color = ambient = [0.2,0.2,0.2].
///
/// These expected values are computed by hand (not via ref_color), so a shared kernel+oracle
/// bug can't hide.
#[test]
fn kernel_lit_color_front_facing_and_back_facing() {
    let (device, queue, _dual_source) = headless_device();

    // Build a minimal Scene by hand.
    use crate::hle::math::identity;

    let mut scene = crate::hle::Scene::default();

    // lights_table: one directional +Z / white, then ambient grey.
    scene
        .lights_table
        .push(([0.0_f32, 0.0, 1.0], [1.0, 1.0, 1.0])); // dir in obj space, col white
    scene
        .lights_table
        .push(([0.0_f32, 0.0, 0.0], [0.2, 0.2, 0.2])); // ambient (dir unused)

    // MVP table: identity.
    scene.mvp_table = vec![identity()];
    // Viewport table: default full-screen FB (unused for color, but kernel needs it).
    scene.viewport_table = vec![(
        [
            crate::hle::rsp::FB_WIDTH / 2.0,
            crate::hle::rsp::FB_HEIGHT / 2.0,
            511.0 / crate::hle::rsp::DEPTH_RANGE,
        ],
        [
            crate::hle::rsp::FB_WIDTH / 2.0,
            crate::hle::rsp::FB_HEIGHT / 2.0,
            511.0 / crate::hle::rsp::DEPTH_RANGE,
        ],
    )];
    scene.texcoord_table = vec![[0.0, 0.0]];

    // Helper to pack s8 normal bytes into cn u32 (LE: r=b12, g=b13, b=b14, a=b15).
    // cn = nx_u8 | (ny_u8 << 8) | (nz_u8 << 16) | (alpha_u8 << 24)
    let make_cn = |nx: i8, ny: i8, nz: i8| -> u32 {
        (nx as u8 as u32) | ((ny as u8 as u32) << 8) | ((nz as u8 as u32) << 16) | (255u32 << 24)
    };

    // Vertex A: normal (0,0,127) — faces +Z light directly.
    scene.raw_pos.push([0.0, 0.0, 0.0]);
    scene.mtx_index.push(0);
    scene.viewport_index.push(0);
    scene.raw_st.push([0.0, 0.0]);
    scene.texcoord_index.push(0);
    scene.cn.push(make_cn(0, 0, 127));
    scene.light_index.push(0); // start of lights_table
    scene.light_count.push(2); // 1 dir + 1 ambient

    // Vertex B: normal (0,0,-127) — faces away from +Z light.
    scene.raw_pos.push([0.0, 0.0, 0.0]);
    scene.mtx_index.push(0);
    scene.viewport_index.push(0);
    scene.raw_st.push([0.0, 0.0]);
    scene.texcoord_index.push(0);
    scene.cn.push(make_cn(0, 0, -127));
    scene.light_index.push(0);
    scene.light_count.push(2);

    let gpu = run_compute_outputs(&device, &queue, &scene);
    assert_eq!(gpu.len(), 2);

    // Vertex A: N·L = (127/127)*1.0 = 1.0 → color = clamp(ambient + 1.0*white) = [1,1,1,1].
    // Hand-computed (not using ref_color), tol = 1e-4.
    let tol = 1e-4_f32;
    let a = &gpu[0].color;
    assert!(
        (a[0] - 1.0).abs() <= tol,
        "vtx A color.r: expected 1.0 got {}",
        a[0]
    );
    assert!(
        (a[1] - 1.0).abs() <= tol,
        "vtx A color.g: expected 1.0 got {}",
        a[1]
    );
    assert!(
        (a[2] - 1.0).abs() <= tol,
        "vtx A color.b: expected 1.0 got {}",
        a[2]
    );

    // Vertex B: N·L = (-127/127)*1.0 = -1.0 → max(-1.0, 0.0) = 0 → color = ambient = [0.2,0.2,0.2,1].
    let b = &gpu[1].color;
    assert!(
        (b[0] - 0.2).abs() <= tol,
        "vtx B color.r: expected 0.2 (ambient) got {}",
        b[0]
    );
    assert!(
        (b[1] - 0.2).abs() <= tol,
        "vtx B color.g: expected 0.2 (ambient) got {}",
        b[1]
    );
    assert!(
        (b[2] - 0.2).abs() <= tol,
        "vtx B color.b: expected 0.2 (ambient) got {}",
        b[2]
    );
}

/// Multi-light kernel test — locks the 2..7-light accumulation loop with an oracle-INDEPENDENT
/// hand-computed assertion (NOT ref_color). `kernel_lit_color_front_facing_and_back_facing` only
/// exercises ONE directional light; the accumulation `c += max(N·L_k,0)·col_k` over k>=2 lights is
/// otherwise unexercised except against the shared ref_color oracle (a shared kernel+oracle bug
/// could hide). The new `lights.n64` teapot uses gdSPDefLights2 (2 dirs + ambient), so this is the
/// matching focused test.
///
/// Scene setup (identity MVP/viewport — position irrelevant; only the color math matters):
///   lights_table = [ dir1, dir2, ambient ]  (ambient LAST, the kernel's convention), light_count=3.
///     dir1 = [0,0,1]      col1 = [0.4, 0.4, 0.0]   (a yellow directional)
///     dir2 = [0.6,0,0.8]  col2 = [0.5, 0.0, 0.5]   (unit: 0.36+0.64 = 1.0)
///     ambient            col  = [0.05, 0.05, 0.05]
///
///   Vertex A: normal (0,0,127) s8 → n = [0,0,1] (exact 127/127, no quantization).
///     N·L1 = 1.0  ; N·L2 = 0.8  (both positive → both accumulate).
///     expected = ambient + 1.0·col1 + 0.8·col2
///       R = 0.05 + 0.4 + 0.8·0.5 = 0.85
///       G = 0.05 + 0.4 + 0.8·0.0 = 0.45
///       B = 0.05 + 0.0 + 0.8·0.5 = 0.45     (all < 1.0 → no clamp; the SUM is what's tested)
///
///   Vertex B: normal (0,0,-127) s8 → n = [0,0,-1].
///     N·L1 = -1.0 → max(.,0)=0 ; N·L2 = -0.8 → max(.,0)=0 (both lights drop out).
///     expected = ambient only = [0.05, 0.05, 0.05]  (locks that back-facing lights add nothing).
///
/// These literals are computed by hand, so a shared kernel+oracle bug in the multi-light loop
/// cannot hide — mirrors how kernel_lit_color deliberately avoids ref_color.
#[test]
fn kernel_lit_color_two_directional_lights_plus_ambient() {
    let (device, queue, _dual_source) = headless_device();
    use crate::hle::math::identity;

    let mut scene = crate::hle::Scene::default();

    // lights_table: two directionals, then ambient LAST (the kernel reads ambient at li+lc-1).
    scene
        .lights_table
        .push(([0.0_f32, 0.0, 1.0], [0.4, 0.4, 0.0])); // dir1
    scene
        .lights_table
        .push(([0.6_f32, 0.0, 0.8], [0.5, 0.0, 0.5])); // dir2 (unit)
    scene
        .lights_table
        .push(([0.0_f32, 0.0, 0.0], [0.05, 0.05, 0.05])); // ambient (dir unused)

    scene.mvp_table = vec![identity()];
    scene.viewport_table = vec![(
        [
            crate::hle::rsp::FB_WIDTH / 2.0,
            crate::hle::rsp::FB_HEIGHT / 2.0,
            511.0 / crate::hle::rsp::DEPTH_RANGE,
        ],
        [
            crate::hle::rsp::FB_WIDTH / 2.0,
            crate::hle::rsp::FB_HEIGHT / 2.0,
            511.0 / crate::hle::rsp::DEPTH_RANGE,
        ],
    )];
    scene.texcoord_table = vec![[0.0, 0.0]];

    let make_cn = |nx: i8, ny: i8, nz: i8| -> u32 {
        (nx as u8 as u32) | ((ny as u8 as u32) << 8) | ((nz as u8 as u32) << 16) | (255u32 << 24)
    };

    // Vertex A: normal (0,0,127) — faces both lights' +Z component.
    scene.raw_pos.push([0.0, 0.0, 0.0]);
    scene.mtx_index.push(0);
    scene.viewport_index.push(0);
    scene.raw_st.push([0.0, 0.0]);
    scene.texcoord_index.push(0);
    scene.cn.push(make_cn(0, 0, 127));
    scene.light_index.push(0);
    scene.light_count.push(3); // 2 dir + 1 ambient

    // Vertex B: normal (0,0,-127) — faces away from both lights.
    scene.raw_pos.push([0.0, 0.0, 0.0]);
    scene.mtx_index.push(0);
    scene.viewport_index.push(0);
    scene.raw_st.push([0.0, 0.0]);
    scene.texcoord_index.push(0);
    scene.cn.push(make_cn(0, 0, -127));
    scene.light_index.push(0);
    scene.light_count.push(3);

    let gpu = run_compute_outputs(&device, &queue, &scene);
    assert_eq!(gpu.len(), 2);
    let tol = 1e-4_f32;

    // Vertex A: ambient + 1.0·col1 + 0.8·col2 = [0.85, 0.45, 0.45] (hand-computed, NOT ref_color).
    let a = &gpu[0].color;
    let expect_a = [0.85_f32, 0.45, 0.45];
    for (c, &e) in expect_a.iter().enumerate() {
        assert!(
            (a[c] - e).abs() <= tol,
            "two-light vtx A color[{c}]: expected {e} got {}",
            a[c]
        );
    }

    // Vertex B: both lights drop out (max(N·L,0)=0) → ambient only = [0.05, 0.05, 0.05].
    let b = &gpu[1].color;
    for (c, &got) in b.iter().take(3).enumerate() {
        assert!(
            (got - 0.05).abs() <= tol,
            "two-light vtx B color[{c}]: expected 0.05 (ambient) got {got}"
        );
    }
}

// Locks cull_mode:Back + front_face:Ccw == N64 back-face cull.
//
// N64-front face == CCW-in-NDC (proved by crates/hle/tests/culling.rs).
// With front_face:Ccw and cull_mode:Back, wgpu keeps CCW-in-NDC (N64-front) and culls
// CW-in-NDC (N64-back) — which is correct N64 CULL_BACK semantics.
//
// Two triangles under one Cull run:
//   LEFT  (CCW in NDC, signed area > 0 in Y-up): N64-front face → KEPT.
//   RIGHT (CW  in NDC, signed area < 0 in Y-up): N64-back  face → CULLED.
#[test]
fn cull_back_mode_keeps_n64_front_drops_n64_back() {
    let (device, queue, _dual_source) = headless_device();
    const W: u32 = 64;
    const H: u32 = 64;
    let bytes_per_row = W * 4;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("t"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
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

    let pv = |x: f32, y: f32| crate::render::OutVertex {
        position: [x, y, 0.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
        uv: [0.0, 0.0],
        _pad: [0.0, 0.0],
    };
    // LEFT (CCW in NDC, signed area > 0 in Y-up NDC) = N64-front -> KEPT by cull_mode:Back.
    // RIGHT (CW in NDC, signed area < 0) = N64-back -> CULLED. (N64-front == CCW-in-NDC is proved
    // by crates/hle/tests/culling.rs.) Centroids stay (-0.4,-0.2) and (0.4,-0.2).
    let pos: [crate::render::OutVertex; 6] = [
        pv(-0.6, -0.5),
        pv(-0.2, -0.5),
        pv(-0.4, 0.4),
        pv(0.2, -0.5),
        pv(0.4, 0.4),
        pv(0.6, -0.5),
    ];
    let indices: [u32; 6] = [0, 1, 2, 3, 4, 5];
    let runs = [crate::hle::DrawRun {
        fog_color: [0; 4],
        material_index: 0,
        render_mode_index: 0,
        cull: crate::hle::CullKind::Cull,
        index_count: 6,
        index_start: 0,
    }];

    use wgpu::util::DeviceExt;
    let pos_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&pos),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    // SHADE passthrough: (ZERO-ZERO)*ZERO + SHADE = SHADE
    let uniform = CombinerUniform {
        combine_l: 0xFCFF_FFFF,
        combine_h: 0xFF9F_F93C,
        cycle_type: 0,
        tex_enable: 0,
        blender_mux: 0,
        force_blend: 0,
        alpha_mode: 0,
        alpha_threshold: 0.0,
        prim: [1.0, 1.0, 1.0, 1.0],
        env: [0.0, 0.0, 0.0, 1.0],
        blend_color: [0.0; 4],
        fog_color: [0.0; 4],
        inv_tex_size: [1.0, 1.0, 0.0, 0.0],
        inv_tex1_size: [1.0, 1.0, 0.0, 0.0],
        lod_params: [0.0, 1.0, 0.0, 1.0],
        inv_detail_size: [1.0, 1.0, 0.0, 0.0],
    };
    let pipeline = TexturedPipeline::new(&device, format, DEPTH_FORMAT);
    let (group0_bg, group1_bg) = make_bind_groups(
        &device,
        &queue,
        &pipeline,
        &[255, 255, 255, 255],
        1,
        1,
        &uniform,
        true,
    );
    let mock_scene = crate::hle::Scene {
        draw_runs: runs.to_vec(),
        render_modes: vec![Default::default()],
        ..Default::default()
    };

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    pipeline.draw(
        &mut encoder,
        &view,
        &pos_buf,
        &ibuf,
        &mock_scene,
        wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
        &[&group0_bg],
        &group1_bg,
        0,
        None,
    );
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
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
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

    // ndc -> framebuffer pixel (Y-down). Sample each triangle's centroid.
    let px = |x: f32, y: f32| {
        (
            ((x + 1.0) * 0.5 * W as f32) as u32,
            ((1.0 - y) * 0.5 * H as f32) as u32,
        )
    };
    let (lx, ly) = px(-0.4, -0.2); // left centroid
    let (rx2, ry) = px(0.4, -0.2); // right centroid
    let lo = (ly * bytes_per_row + lx * 4) as usize;
    let ro = (ry * bytes_per_row + rx2 * 4) as usize;
    let left = data[lo];
    let right = data[ro];
    drop(data);
    readback.unmap();
    assert!(
        left > 200,
        "N64-front/CCW-in-NDC (left) must be KEPT by cull_mode:Back, got {left}"
    );
    assert!(
        right < 50,
        "N64-back/CW-in-NDC (right) must be CULLED by cull_mode:Back, got {right}"
    );
}

/// Cross-frame morph assertion: assembles `morphcube` at three times and verifies a GENUINE
/// cube↔sphere morph — not the old "scaled cube" fake (where the sphere VtxSet was just the 8 cube
/// corners shrunk to ±23, leaving the silhouette a cube at every weight).
///
/// morphcube is now a frequency-2 spherified cube (26 verts: 8 corners + 12 edge-mids + 6 face-
/// centers). weight = (1 - cos(time)) / 2. Across the three sampled frames:
///
/// - t=0 → weight 0 → CUBE: vertices on the cube surface, radii ranging from 40 (face-centers, at
///   distance S from origin) up to ~69 (corners at S√3 ≈ 40·1.732). This SPREAD of radii is what
///   makes it a cube.
/// - t=PI → weight 1 → SPHERE: every vertex normalized to radius ≈40 (corners pulled IN from 69,
///   edge-mids from 56, face-centers unchanged). A near-constant radius == a real sphere.
/// - t=PI/2 → weight 0.5 → midpoint (positions differ from both endpoints; the morph animates).
///
/// Radius derivation (S = 40, cube half-extent / sphere target radius):
///   cube corner      (±40,±40,±40) → |p| = 40·√3 ≈ 69.28
///   cube edge-mid    (±40,±40,  0) → |p| = 40·√2 ≈ 56.57
///   cube face-center (  0,  0,±40) → |p| = 40
///   sphere (any vert): p/|p|·40, rounded to int → |p| ≈ 40 (±~1 from integer rounding).
///
/// The full-morph radius assertion FAILS for the old scaled-cube target (its "sphere" corners sit at
/// 23·√3 ≈ 39.8 BUT its face-region verts would also be ~23·… — i.e. it was still a cube of mixed
/// radii) and passes only for a target where ALL verts share radius ≈40.
#[test]
fn morphcube_morphs_cube_to_sphere_across_frames() {
    // A 1×1 white texture is needed because gsDPSetOtherMode_H / gsDPSetCombineLERP require
    // a texture context even when the combiner uses SHADE only (no actual texture sampling).
    let white1x1 = vec![255u8; 4];
    let scenes_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scenes");
    let src = std::fs::read_to_string(scenes_dir.join("morphcube.n64"))
        .expect("morphcube.n64 must exist");
    let tex = Some((white1x1.as_slice(), 1u32, 1u32));

    let radius = |p: &[f32; 3]| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
    const S: f32 = 40.0; // cube half-extent / sphere target radius.

    // t=0: weight = (1-cos(0))/2 = 0.0 → pure cube.
    let asm0 = crate::asm::assemble_at(&src, 0.0, tex).expect("morphcube assembles at t=0");
    assert!(
        crate::asm::analyze(&src).references_time,
        "morphcube morph weight reads time — must be time-variant"
    );
    let r0 = crate::hle::interpret_rdram(&asm0.rdram, asm0.entry_addr);
    assert!(r0.diags.is_empty(), "t=0 interp diags: {:?}", r0.diags);
    assert!(!r0.scene.raw_pos.is_empty(), "t=0: no vertices");

    // The CUBE (t=0) must span a RANGE of radii: face-centers at S=40, corners at S√3≈69. A genuine
    // cube has min ≈ 40 (some vertex at distance S) and max ≈ 69 (a corner). If min≈max, it's a sphere
    // (or the degenerate scaled-cube) — not a cube.
    let cube_radii: Vec<f32> = r0.scene.raw_pos.iter().map(radius).collect();
    let cube_min = cube_radii.iter().cloned().fold(f32::INFINITY, f32::min);
    let cube_max = cube_radii.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    assert!(
        (cube_min - S).abs() < 1.0,
        "t=0 cube: minimum radius should be ~{S} (face-centers), got {cube_min}"
    );
    let corner = S * 3.0_f32.sqrt(); // ≈ 69.28
    assert!(
        (cube_max - corner).abs() < 1.5,
        "t=0 cube: maximum radius should be ~{corner} (corners at S√3), got {cube_max}"
    );

    // t=PI: weight = (1-cos(PI))/2 = 1.0 → FULL morph → sphere.
    let asm_full = crate::asm::assemble_at(&src, std::f32::consts::PI, tex)
        .expect("morphcube assembles at t=PI (full morph)");
    let r_full = crate::hle::interpret_rdram(&asm_full.rdram, asm_full.entry_addr);
    assert!(
        r_full.diags.is_empty(),
        "t=PI interp diags: {:?}",
        r_full.diags
    );
    assert_eq!(
        r0.scene.raw_pos.len(),
        r_full.scene.raw_pos.len(),
        "vertex count must be stable across frames"
    );

    // SPHERE assertion: at full morph EVERY vertex must sit at radius ≈40. Tolerance 2.0 absorbs the
    // integer rounding of the normalized sphere coords (edge-mids land at ~39.6, corners at ~39.8).
    // This is the assertion that fails the old scaled-cube "sphere" (whose verts kept the cube's
    // mixed radii) and passes only for a real spherified target.
    for (i, pos) in r_full.scene.raw_pos.iter().enumerate() {
        let r = radius(pos);
        assert!(
            (r - S).abs() < 2.0,
            "t=PI vtx {i} {pos:?}: full-morph vertex must be on a sphere of radius ~{S}, got radius {r}"
        );
    }

    // t=PI/2: weight ≈ 0.5 → midpoint; positions must differ from BOTH endpoints (the morph animates).
    let t_half = std::f32::consts::FRAC_PI_2;
    let asm_half =
        crate::asm::assemble_at(&src, t_half, tex).expect("morphcube assembles at t=PI/2");
    let r_half = crate::hle::interpret_rdram(&asm_half.rdram, asm_half.entry_addr);
    assert!(
        r_half.diags.is_empty(),
        "t=PI/2 interp diags: {:?}",
        r_half.diags
    );

    let differs = |a: &[[f32; 3]], b: &[[f32; 3]]| {
        a.iter()
            .zip(b.iter())
            .any(|(p, q)| p.iter().zip(q.iter()).any(|(&x, &y)| (x - y).abs() > 1.0))
    };
    assert!(
        differs(&r0.scene.raw_pos, &r_half.scene.raw_pos),
        "morph positions must differ between t=0 (cube) and t=PI/2 — the morph is not animating"
    );
    assert!(
        differs(&r_full.scene.raw_pos, &r_half.scene.raw_pos),
        "morph positions must differ between t=PI (sphere) and t=PI/2 — the morph is not animating"
    );
}

/// Cross-frame matrix-animation assertion: assembles `perspective-cube` at two different times and
/// verifies that the MVP table differs (the update block rotates the model matrix each frame).
/// This closes the gap where no test previously ticked an `update{}` block across two frames.
#[test]
fn perspective_cube_mvp_differs_between_frames() {
    // Dummy 1×1 texture needed for gsDPSetOtherMode_H / gsDPSetCombineLERP.
    let white1x1 = vec![255u8; 4];
    let scenes_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scenes");
    let src = std::fs::read_to_string(scenes_dir.join("perspective-cube.n64"))
        .expect("perspective-cube.n64 must exist");
    let tex = Some((white1x1.as_slice(), 1u32, 1u32));

    // t=0: model = identity (guRotate at time=0 is no-op rotation by 0°).
    let asm0 = crate::asm::assemble_at(&src, 0.0, tex).expect("perspective-cube assembles at t=0");
    assert!(
        crate::asm::analyze(&src).references_time,
        "perspective-cube update block reads time — must be time-variant"
    );
    let r0 = crate::hle::interpret_rdram(&asm0.rdram, asm0.entry_addr);
    assert!(r0.diags.is_empty(), "t=0 interp diags: {:?}", r0.diags);

    // t=2.0s: model has rotated 2*45=90° about Y — the MVP will differ from the t=0 case.
    let asm2 = crate::asm::assemble_at(&src, 2.0, tex).expect("perspective-cube assembles at t=2");
    let r2 = crate::hle::interpret_rdram(&asm2.rdram, asm2.entry_addr);
    assert!(r2.diags.is_empty(), "t=2 interp diags: {:?}", r2.diags);

    // The MVP tables from the two frames must differ (rotation changed the model matrix).
    // The hle interpreter starts with an identity() sentinel at index 0; the actual scene MVPs
    // follow. Compare the full tables (as flat f32 arrays) rather than a single index — the
    // second non-identity entry is the one that actually carries the model rotation.
    assert!(!r0.scene.mvp_table.is_empty(), "t=0: no MVP entries");
    assert!(!r2.scene.mvp_table.is_empty(), "t=2: no MVP entries");

    // The table length or content must differ: at t=0 the rotation is 0° (no new deduplicated
    // entry vs the identity sentinel), at t=2 the 90° rotation produces a new MVP entry.
    // Compare length first (different table size is conclusive); then compare element-wise.
    let same_len = r0.scene.mvp_table.len() == r2.scene.mvp_table.len();
    let same_content = same_len
        && r0
            .scene
            .mvp_table
            .iter()
            .zip(r2.scene.mvp_table.iter())
            .all(|(m0, m2)| {
                m0.iter().zip(m2.iter()).all(|(r0, r2)| {
                    r0.iter()
                        .zip(r2.iter())
                        .all(|(&a, &b)| (a - b).abs() <= 1e-4)
                })
            });
    assert!(
        !same_content,
        "perspective-cube MVP must differ between t=0 and t=2 (update block must tick the rotation)"
    );
}

/// Scene-driven DECAL pixel test — chrome-icosphere end-to-end through the combiner/rasterizer.
///
/// This is the first test that runs a REAL scene (chrome-icosphere.n64) end-to-end through:
///   assemble → crate::hle::interpret → CombinerUniform::from_run → RspProcessPipeline → TexturedPipeline
///   → readback → pixel assert.
///
/// The chrome-icosphere combiner is G_CC_DECALRGB: `(ZERO−ZERO)×ZERO + TEXEL0 = TEXEL0` — the
/// fragment output is exactly the sampled env texture, with shade/lighting IGNORED for color.
///
/// To avoid a PNG-decode dependency, a solid-color 32×32 env texture is assembled: every texel is
/// RGBA8 (200, 100, 50, 255). The RGBA16 N64 round-trip (5-bit per channel) maps these to
/// approximately (206, 99, 49, 255) — all strongly non-black.
///
/// Regression guard: a combiner bug that zeroes the DECAL output (the "chrome-black-ball" class)
/// makes the rendered pixel black (R ≈ 0). A bug that falls through to SHADE would produce the
/// lighting colour (dominated by yellow lights, not matching a solid red-orange env). Either way
/// the R > 100 + G < 150 + B < 100 assertions fail.
///
/// The center pixel of a 64×64 target is asserted:
///   R > 100  (non-black: DECAL must have sampled the env, not zeroed it)
///   G < 150  (not the lighting colour: a MODULATE-of-yellow-lit shade would be G ≈ R ≈ high,
///             not tracking the orange env)
///   B < 100  (the env's B channel is ~49 after RGBA16 round-trip)
#[test]
fn chrome_icosphere_decal_pixel_is_env_texel_not_black() {
    let (device, queue, _dual_source) = headless_device();

    // Build a synthetic 32x32 env texture (solid orange-ish color that survives RGBA16 round-trip).
    // RGBA16 encode/decode: r5 = 200>>3 = 25 -> (25<<3)|(25>>2) = 206
    //                       g5 = 100>>3 = 12 -> (12<<3)|(12>>2) =  96+3 = 99
    //                       b5 =  50>>3 =  6 -> ( 6<<3)|( 6>>2) =  48+1 = 49
    // All three channels are clearly non-black, and clearly not white (255) — distinguishable from
    // both a zero-output and an unsampled white placeholder texture.
    const TEX_W: u32 = 32;
    const TEX_H: u32 = 32;
    let env_rgba8: Vec<u8> = (0..TEX_W * TEX_H)
        .flat_map(|_| [200u8, 100, 50, 255])
        .collect();

    // Assemble the chrome-icosphere scene with the synthetic env texture.
    let scenes_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scenes");
    let src = std::fs::read_to_string(scenes_dir.join("chrome-icosphere.n64"))
        .expect("chrome-icosphere.n64 must exist");
    let img = crate::asm::assemble_with_texture(&src, &env_rgba8, TEX_W, TEX_H)
        .expect("chrome-icosphere.n64 must assemble");

    // HLE interpret: the scene must produce a material with tex_enable=true.
    let r = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
    assert!(
        r.diags.is_empty(),
        "chrome-icosphere: unexpected HLE diags: {:?}",
        r.diags
    );
    assert!(
        !r.scene.materials.is_empty(),
        "chrome-icosphere must produce a material"
    );

    let mat = &r.scene.materials[0];
    assert!(
        mat.tex_enable,
        "chrome-icosphere material must have tex_enable=true (DECAL uses TEXEL0)"
    );

    // Build CombinerUniform from the real scene material (not hand-constructed).
    let u = CombinerUniform::from_run(mat, &crate::hle::RenderMode::default(), [0; 4]);

    // Run the RSP-process compute pass to get GPU-transformed OutVertex positions, colors, and UVs.
    let gpu_verts = run_compute_outputs(&device, &queue, &r.scene);
    assert!(
        !gpu_verts.is_empty(),
        "chrome-icosphere: RSP compute produced no output vertices"
    );

    // Render setup: 64×64 target.
    const W: u32 = 64;
    const H: u32 = 64;
    let bytes_per_row = W * 4;
    let format = wgpu::TextureFormat::Rgba8Unorm;

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("chrome-target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
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

    // Build OutVertex buffer from RSP-compute output.
    let pos: Vec<OutVertex> = gpu_verts
        .iter()
        .map(|v| OutVertex {
            position: v.pos,
            color: v.color,
            uv: v.uv,
            _pad: [0.0, 0.0],
        })
        .collect();
    let pos_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("chrome-pos"),
        contents: bytemuck::cast_slice(&pos),
        usage: wgpu::BufferUsages::VERTEX,
    });

    // Index buffer from the HLE scene.
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("chrome-ibuf"),
        contents: bytemuck::cast_slice(&r.scene.indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("chrome-readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // Use the decoded texture from the material (RGBA8, already decoded by the HLE from RGBA16).
    let tex_rgba8 = &mat.texture;
    let tex_w = mat.tex_w;
    let tex_h = mat.tex_h;

    let pipeline = TexturedPipeline::new(&device, format, DEPTH_FORMAT);
    let (group0_bg, group1_bg) = make_bind_groups(
        &device, &queue, &pipeline, tex_rgba8, tex_w, tex_h, &u,
        false, // linear filter (sphere-mapped texgen uvs are continuous)
    );

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    pipeline.draw(
        &mut encoder,
        &view,
        &pos_buf,
        &ibuf,
        &r.scene,
        wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
        &[&group0_bg],
        &group1_bg,
        0,
        None,
    );

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
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
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

    // Sample the center pixel (32, 32).  The scene projects a sphere that covers the center of the
    // 64×64 target when rendered at t=0 (identity model rotation).
    //
    // DECAL combiner: out = TEXEL0 = the sampled env texel.
    // The solid env texture (after RGBA16 round-trip) is approximately (206, 99, 49, 255).
    //
    // Regression assertions:
    //   R > 100  — non-black: DECAL must have passed TEXEL0, not zeroed it (black-ball class)
    //   G < 150  — not dominated by the yellow lighting shade (which would make G ≈ R ≈ high)
    //   B < 100  — env texture's B channel is ~49 (not white or modulated-high)
    //
    // A combiner that zeroes the output → R ≈ 0 → R > 100 fails.
    // A combiner that passes SHADE (lighting color, yellow-dominant) → G ≈ R → G < 150 fails.
    let center_off = (32 * bytes_per_row + 32 * 4) as usize;
    let cr = data[center_off];
    let cg = data[center_off + 1];
    let cb = data[center_off + 2];

    drop(data);
    readback.unmap();

    assert!(
        cr > 100,
        "DECAL center pixel R must be > 100 (env texel ~206, not zeroed by black-ball combiner), got R={cr}"
    );
    assert!(
        cg < 150,
        "DECAL center pixel G must be < 150 (env texel ~99, not dominated by yellow lighting shade), got G={cg}"
    );
    assert!(
        cb < 100,
        "DECAL center pixel B must be < 100 (env texel ~49, not white or high-modulated), got B={cb}"
    );
}

/// Scene-driven flat-color PRIM pixel test — end-to-end through the combiner/rasterizer.
///
/// The scene `flat-color.n64` uses:
///   gsDPSetCombineLERP(0, 0, 0, PRIMITIVE, 0, 0, 0, SHADE, 0, 0, 0, PRIMITIVE, 0, 0, 0, SHADE)
///   gsDPSetPrimColor(0, 0, 64, 200, 255, 255)
///
/// Combiner formula: `(ZERO − ZERO) × ZERO + PRIMITIVE = PRIMITIVE`.
/// Expected pixel: R=64, G=200, B=255, A=255 (exact, no RGBA16 round-trip — prim bytes are stored
/// directly in the material from the DL command, not through texture encoding).
///
/// Regression guard: a combiner bug that substitutes SHADE (vertex color = white → 255) for PRIM
/// would make R=255 instead of 64 (fails R≈64). A bug that zeroes the output gives R=0 (fails too).
#[test]
fn flat_color_prim_pixel_equals_gsdpsetprimcolor() {
    let (device, queue, _dual_source) = headless_device();

    // No texture needed for flat-color (combiner uses PRIMITIVE, not TEXEL0); pass a 1×1 white
    // placeholder so gsDPLoadTextureBlock (if any) assembles without error.
    let white1x1 = vec![255u8; 4];
    let scenes_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scenes");
    let src = std::fs::read_to_string(scenes_dir.join("flat-color.n64"))
        .expect("flat-color.n64 must exist");
    let img = crate::asm::assemble_with_texture(&src, &white1x1, 1, 1)
        .expect("flat-color.n64 must assemble");

    // HLE interpret: the scene must produce a material.
    let r = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
    assert!(
        r.diags.is_empty(),
        "flat-color: unexpected HLE diags: {:?}",
        r.diags
    );
    assert!(
        !r.scene.materials.is_empty(),
        "flat-color must produce a material"
    );

    let mat = &r.scene.materials[0];

    // Build CombinerUniform from the real scene material.
    let u = CombinerUniform::from_run(mat, &crate::hle::RenderMode::default(), [0; 4]);

    // Run the RSP-process compute pass to get GPU-transformed OutVertex positions and colors.
    let gpu_verts = run_compute_outputs(&device, &queue, &r.scene);
    assert!(
        !gpu_verts.is_empty(),
        "flat-color: RSP compute produced no output vertices"
    );

    // Render setup: 64×64 target.
    const W: u32 = 64;
    const H: u32 = 64;
    let bytes_per_row = W * 4;
    let format = wgpu::TextureFormat::Rgba8Unorm;

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("flat-target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
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

    // Build OutVertex buffer from RSP-compute output.
    let pos: Vec<OutVertex> = gpu_verts
        .iter()
        .map(|v| OutVertex {
            position: v.pos,
            color: v.color,
            uv: v.uv,
            _pad: [0.0, 0.0],
        })
        .collect();
    let pos_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("flat-pos"),
        contents: bytemuck::cast_slice(&pos),
        usage: wgpu::BufferUsages::VERTEX,
    });

    // Index buffer from the HLE scene.
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("flat-ibuf"),
        contents: bytemuck::cast_slice(&r.scene.indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("flat-readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // Placeholder 1×1 white texture (tex_enable=false for flat-color, so texture is not sampled).
    let tex_rgba8 = vec![255u8, 255, 255, 255];

    let pipeline = TexturedPipeline::new(&device, format, DEPTH_FORMAT);
    let (group0_bg, group1_bg) =
        make_bind_groups(&device, &queue, &pipeline, &tex_rgba8, 1, 1, &u, true);

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    pipeline.draw(
        &mut encoder,
        &view,
        &pos_buf,
        &ibuf,
        &r.scene,
        wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
        &[&group0_bg],
        &group1_bg,
        0,
        None,
    );

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
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
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

    // Sample the center pixel (32, 32).  The scene projects a full-screen quad covering the center.
    //
    // Hand-computed expected value from gsDPSetPrimColor(0, 0, 64, 200, 255, 255):
    //   PRIM = (R=64, G=200, B=255, A=255)
    //   Combiner: (ZERO−ZERO)×ZERO + PRIMITIVE = PRIMITIVE → output = (64, 200, 255, 255)
    //
    // These are stored verbatim in the material `prim` bytes (no RGBA16 round-trip).
    // The f32 conversion prim_u8/255.0 rounds trip back to the same byte via Rgba8Unorm.
    let center_off = (32 * bytes_per_row + 32 * 4) as usize;
    let cr = data[center_off];
    let cg = data[center_off + 1];
    let cb = data[center_off + 2];

    drop(data);
    readback.unmap();

    // R=64 (not 255 from SHADE, not 0 from a zeroed combiner), tolerance ±2 for f32 rounding.
    assert!(
        (cr as i16 - 64).abs() <= 2,
        "flat-color center pixel R: expected 64 (PRIM.r from gsDPSetPrimColor), got {cr}"
    );
    // G=200 (distinguishes PRIM from SHADE=255 and from zero).
    assert!(
        (cg as i16 - 200).abs() <= 2,
        "flat-color center pixel G: expected 200 (PRIM.g from gsDPSetPrimColor), got {cg}"
    );
    // B=255 (the full blue channel of the prim color).
    assert!(
        (cb as i16 - 255).abs() <= 2,
        "flat-color center pixel B: expected 255 (PRIM.b from gsDPSetPrimColor), got {cb}"
    );
}

/// cycle_type:1 pixel test — scene-driven, exercises the 2-cycle COMBINED selector path.
///
/// The scene `two-cycle-combiner.n64` renders a static, textureless quad filling most of a 64×64
/// target.  The quad has vivid corner SHADE colors and a warm-orange PRIM tint:
///   TL=red(255,0,0), TR=green(0,255,0), BL=blue(0,0,255), BR=white(255,255,255)
///   PRIM=(255,160,64,255)
///
/// The scale(0.015625) projection maps ±48 vertices to NDC ±0.75, which lands at pixel 8..56
/// inside the 64×64 target (48×48 quad area).
///
/// Combine formula (both cycles):
///   cyc0: (ZERO−ZERO)×ZERO + SHADE  → COMBINED = per-vertex SHADE gradient
///   cyc1: (COMBINED−ZERO)×PRIMITIVE → output   = SHADE × PRIM/255 per channel
///
/// Three pixels are asserted (tolerance ±10/255 to cover GPU rounding and sub-pixel jitter):
///
/// The quad is made of two triangles: T1=(BL,BR,TR) and T2=(BL,TR,TL), sharing the BL–TR
/// diagonal.  Pixel (32,32) lies exactly on that diagonal, so its SHADE is the 50/50 mix of BL
/// and TR (blue+green = grey with R=0), NOT the full-quad average.
///
///   1. Near-TL corner pixel (10,10) — in triangle T2=(BL,TR,TL):
///      barycentric weights ~(w_TL≈0.92, w_TR≈0.04, w_BL≈0.04)
///      SHADE ≈ 0.92×(255,0,0) + 0.04×(0,255,0) + 0.04×(0,0,255) ≈ (235, 10, 10)
///      output = SHADE×PRIM/255 ≈ (235, 6, 2); expected with tol: (228, 8, 3) as-measured
///
///   2. Near-BR corner pixel (54,54) — in triangle T1=(BL,BR,TR):
///      barycentric weights ~(w_BR≈0.92, w_TR≈0.04, w_BL≈0.04)
///      SHADE ≈ 0.92×(255,255,255) + 0.04×(0,255,0) + 0.04×(0,0,255) ≈ (235, 245, 245)
///      output ≈ (235, 154, 62)
///
///   3. Diagonal pixel (32,32) — on the BL–TR shared edge:
///      SHADE = 0.5×(0,0,255) + 0.5×(0,255,0) = (0, 128, 128)
///      output = (0×255/255, 128×160/255, 128×64/255) = (0, 80, 32) ≈ (5, 82, 33) measured
///
/// A broken COMBINED-routing or wrong-cycle regression produces a uniform tint instead of the
/// vivid corner/tint variation, failing at least one of these three distinct-color assertions.
#[test]
fn cycle_type_1_two_cycle_combiner_pixel() {
    let (device, queue, _dual_source) = headless_device();

    // Assemble the scene (no actual texture needed; the combine is textureless).
    let white1x1 = vec![255u8; 4];
    let scenes_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scenes");
    let src = std::fs::read_to_string(scenes_dir.join("two-cycle-combiner.n64"))
        .expect("two-cycle-combiner.n64 must exist");
    let img = crate::asm::assemble_with_texture(&src, &white1x1, 1, 1)
        .expect("two-cycle-combiner.n64 must assemble");

    // HLE interpret: extract material (scene-driven — exercises the real cycle_type/combine-word path).
    let r = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
    assert!(
        r.diags.is_empty(),
        "two-cycle-combiner: unexpected HLE diags: {:?}",
        r.diags
    );
    let mat = &r.scene.materials[0];

    // Build CombinerUniform from the scene's material — NOT a hand-constructed uniform.
    let u = CombinerUniform::from_run(mat, &crate::hle::RenderMode::default(), [0; 4]);

    // Primary assertion: the scene must have set 2-cycle mode.
    assert_eq!(
        u.cycle_type, 1,
        "two-cycle-combiner must produce cycle_type==1"
    );

    // Run the RSP-process compute pass to get GPU-transformed OutVertex positions and colors.
    let gpu_verts = run_compute_outputs(&device, &queue, &r.scene);
    assert!(
        !gpu_verts.is_empty(),
        "two-cycle-combiner: RSP compute produced no output vertices"
    );

    // Render setup: 64×64 target.
    const W: u32 = 64;
    const H: u32 = 64;
    let bytes_per_row = W * 4;
    let format = wgpu::TextureFormat::Rgba8Unorm;

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("2cyc-target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
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

    // Build OutVertex buffer from RSP-compute output.
    let pos: Vec<OutVertex> = gpu_verts
        .iter()
        .map(|v| OutVertex {
            position: v.pos,
            color: v.color,
            uv: v.uv,
            _pad: [0.0, 0.0],
        })
        .collect();
    let pos_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("2cyc-pos"),
        contents: bytemuck::cast_slice(&pos),
        usage: wgpu::BufferUsages::VERTEX,
    });

    // Index buffer from the HLE scene.
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("2cyc-ibuf"),
        contents: bytemuck::cast_slice(&r.scene.indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("2cyc-readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // Placeholder 1×1 white texture (tex_enable=0 from the textureless scene).
    let tex_rgba8 = vec![255u8, 255, 255, 255];

    let pipeline = TexturedPipeline::new(&device, format, DEPTH_FORMAT);
    let (group0_bg, group1_bg) =
        make_bind_groups(&device, &queue, &pipeline, &tex_rgba8, 1, 1, &u, true);

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    pipeline.draw(
        &mut encoder,
        &view,
        &pos_buf,
        &ibuf,
        &r.scene,
        wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
        &[&group0_bg],
        &group1_bg,
        0,
        None,
    );

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
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
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

    // Helper: read RGBA bytes at a pixel coordinate.
    let pixel = |px: u32, py: u32| -> [u8; 4] {
        let off = (py * bytes_per_row + px * 4) as usize;
        [data[off], data[off + 1], data[off + 2], data[off + 3]]
    };

    // Sample 3 pixels — derivations in the doc-comment above.
    let tl = pixel(10, 10); // near-TL: expected ≈ (228, 8, 3)
    let br = pixel(54, 54); // near-BR: expected ≈ (239, 154, 62)
    let diag = pixel(32, 32); // on BL–TR diagonal: expected ≈ (0, 80, 32)

    drop(data);
    readback.unmap();

    // Tolerance ±10 covers GPU barycentric rounding and sub-pixel jitter.
    let tol: i32 = 10;
    let check = |label: &str, got: [u8; 4], exp_rgb: [u8; 3]| {
        for (ch, (&g, &e)) in got[..3].iter().zip(exp_rgb.iter()).enumerate() {
            let diff = (g as i32 - e as i32).abs();
            assert!(
                diff <= tol,
                "2-cycle SHADE×PRIM {label} ch{ch}: expected {e}±{tol}, got {g} (diff={diff})"
            );
        }
    };

    check("near-TL(10,10)", tl, [228, 8, 3]);
    check("near-BR(54,54)", br, [239, 154, 62]);
    check("diagonal(32,32)", diag, [0, 80, 32]);
}

// ── A8a: single-run path smoke test ──────────────────────────────────────────────────────────────

/// Same quad source and texture as in the goldens harness — duplicated here so render.rs can run
/// a fast smoke test for the split-bind-group path without pulling goldens.rs into scope.
const RGBA16_QUAD_SRC: &str = r#"
Texture tex = { 4, 4, RGBA16 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,    0,    0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 1024,    0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 1024, 1024, 255,255,255,255 }
Vtx { -48,  48, 0, 0,    0, 1024, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_RGBA, G_IM_SIZ_16b, 4, 4)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

#[rustfmt::skip]
const RGBA16_QUAD_TEX: &[u8] = &[
    255,   0,   0, 255,   0, 255,   0, 255,   255,   0,   0, 255,   0, 255,   0, 255, // row 0
    255,   0,   0, 255,   0, 255,   0, 255,   255,   0,   0, 255,   0, 255,   0, 255, // row 1
      0,   0, 255, 255, 255, 255,   0, 255,     0,   0, 255, 255, 255, 255,   0, 255, // row 2
      0,   0, 255, 255, 255, 255,   0, 255,     0,   0, 255, 255, 255, 255,   0, 255, // row 3
];

/// Full-pipeline render helper used by the A8a smoke test.
/// Assembles, HLE-interprets, RSP-computes, then rasterises with split bind groups.
fn render_source_to_rgba8(src: &str, tex_native: &[u8], w: u32, h: u32) -> Vec<u8> {
    let pixel_count = tex_native.len() / 4;
    let tex_side = (pixel_count as f64).sqrt() as u32;

    let img = crate::asm::assemble_with_texture(src, tex_native, tex_side, tex_side)
        .unwrap_or_else(|d| panic!("assembly failed: {d:?}"));
    let interp = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
    assert!(interp.diags.is_empty(), "HLE diags: {:?}", interp.diags);
    let scene = &interp.scene;

    let (device, queue, _dual_source) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("smoke-color"),
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

    let depth_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("smoke-depth"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

    let bytes_per_row_raw = w * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let bytes_per_row = bytes_per_row_raw.div_ceil(align) * align;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("smoke-readback"),
        size: (bytes_per_row * h) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let material = scene
        .materials
        .first()
        .expect("non-empty scene must have a material");

    let tex_size = wgpu::Extent3d {
        width: material.tex_w,
        height: material.tex_h,
        depth_or_array_layers: 1,
    };
    let gpu_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("smoke-tex"),
        size: tex_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &gpu_tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &material.texture,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(material.tex_w * 4),
            rows_per_image: Some(material.tex_h),
        },
        tex_size,
    );
    let tex_view2 = gpu_tex.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("smoke-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let combiner = CombinerUniform::from_run(material, &crate::hle::RenderMode::default(), [0; 4]);
    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("smoke-uniform"),
        contents: bytemuck::bytes_of(&combiner),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let pipeline = TexturedPipeline::new(&device, format, DEPTH_FORMAT);
    let group0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("smoke-g0"),
        layout: pipeline.bind_group_layout(),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&tex_view2),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            // TEXEL1 slot: unused by this single-texture smoke test — reuse tex0's view/sampler to
            // satisfy the group(0) layout.
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&tex_view2),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            // DETAIL slot (4/5): unused by this smoke test — reuse tex0's view/sampler.
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&tex_view2),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ]
        // LOD-level slots (6..=12): unused by this smoke test — reuse a bound view.
        .into_iter()
        .chain((6..13u32).map(|b| wgpu::BindGroupEntry {
            binding: b,
            resource: wgpu::BindingResource::TextureView(&tex_view2),
        }))
        .collect::<Vec<_>>(),
    });
    let group1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("smoke-g1"),
        layout: pipeline.uniform_bind_group_layout(),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buf.as_entire_binding(),
        }],
    });

    let n = scene.raw_pos.len() as u32;
    use crate::render::rsp_buffers as rb;
    let sb = |data: &[u8]| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: data,
            usage: wgpu::BufferUsages::STORAGE,
        })
    };
    let source_buf = sb(bytemuck::cast_slice(&rb::src_vertices(scene)));
    let mvp_buf = sb(bytemuck::cast_slice(&rb::mvp_table(scene)));
    let vp_buf = sb(bytemuck::cast_slice(&rb::viewport_table(scene)));
    let tc_buf = sb(bytemuck::cast_slice(&rb::texcoord_table(scene)));
    let lt_buf = sb(bytemuck::cast_slice(&rb::lights_table(scene)));
    let la_buf = sb(bytemuck::cast_slice(&rb::lookat_table(scene)));
    let fog_table = sb(bytemuck::cast_slice(&rb::fog_table(scene)));
    let dst = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("smoke-dst"),
        size: (n as u64) * 48,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
        mapped_at_creation: false,
    });
    let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("smoke-params"),
        contents: bytemuck::bytes_of(&RspProcessParams {
            vertex_count: n,
            _pad: [0; 3],
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let rsp_pipe = RspProcessPipeline::new(&device);
    let rsp_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("smoke-rsp-bg"),
        layout: rsp_pipe.bind_group_layout(),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: source_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: mvp_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: vp_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: tc_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: lt_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: la_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: dst.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: fog_table.as_entire_binding(),
            },
        ],
    });
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("smoke-ibuf"),
        contents: bytemuck::cast_slice(&scene.indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    rsp_pipe.dispatch(&mut encoder, &rsp_bg, n);
    let depth = if scene.render_modes.iter().any(|r| r.z_test || r.z_write) {
        Some(&depth_view)
    } else {
        None
    };
    pipeline.draw(
        &mut encoder,
        &view,
        &dst,
        &ibuf,
        scene,
        CLEAR_COLOR,
        &[&group0],
        &group1,
        0,
        depth,
    );

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
    let row_bytes = (w * 4) as usize;
    let mut result = Vec::with_capacity(row_bytes * h as usize);
    for row in 0..h {
        let start = (row * bytes_per_row) as usize;
        result.extend_from_slice(&data[start..start + row_bytes]);
    }
    drop(data);
    readback.unmap();
    result
}

/// A8a smoke test: the split-bind-group path (group0=tex+sampler, group1=uniform, stride=0)
/// produces a textured center pixel — not the clear color. This is the TDD "failing test" that
/// gates the A8a bind-group split and new draw() signature.
#[test]
fn single_run_path_renders_textured_center() {
    let px = render_source_to_rgba8(RGBA16_QUAD_SRC, RGBA16_QUAD_TEX, 64, 64);
    let c = ((32 * 64 + 32) * 4) as usize;
    assert!(
        px[c] > 16 || px[c + 1] > 16 || px[c + 2] > 32,
        "center should be textured (not clear color), got R={} G={} B={}",
        px[c],
        px[c + 1],
        px[c + 2]
    );
}

// ── A8b: two-material / two-run test ─────────────────────────────────────────────────────────────

/// Render an `crate::hle::Scene` to raw RGBA8 pixels using `SceneRenderer` (the full facade path).
/// `w × h × 4` bytes returned, row-major, no row padding.
pub(super) fn render_scene_to_rgba8(scene: &crate::hle::Scene, w: u32, h: u32) -> Vec<u8> {
    use crate::render::SceneRenderer;

    let (device, queue, dual_source) = headless_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("2mat-color"),
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

    let mut sr = SceneRenderer::new(&device, format, w, h, dual_source);
    sr.render(&device, &queue, scene, &view);

    // Readback (separate encoder; the facade's own submit is already done).
    let bytes_per_row_raw = w * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let bytes_per_row = bytes_per_row_raw.div_ceil(align) * align;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("2mat-readback"),
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
    let row_bytes = (w * 4) as usize;
    let mut result = Vec::with_capacity(row_bytes * h as usize);
    for row in 0..h {
        let start = (row * bytes_per_row) as usize;
        result.extend_from_slice(&data[start..start + row_bytes]);
    }
    drop(data);
    readback.unmap();
    result
}

/// Build a two-material / two-run Scene for the A8b real-world test.
///
/// Geometry: left-half quad (NDC x in [-1,0]) covered by material 0 (RED prim),
///           right-half quad (NDC x in [0,1]) covered by material 1 (BLUE prim).
///
/// Combiner: PRIM passthrough — `(COMBINED-COMBINED)*COMBINED + PRIM = PRIM`.
///   Encoded as: combine_l=0, combine_h=0x000000C3
///     cd1 = bits(h,6,3) = 3 → PRIM (color_d)
///     ad1 = bits(h,0,3) = 3 → PRIM (alpha_d)
///   All other selectors 0 → COMBINED (= 0 in 1-cycle zero4 starting point).
///
/// With passthrough viewport (sc=FB_W/2, tr=FB_W/2, identity MVP):
///   output_clip_x = raw_pos_x, output_clip_y = raw_pos_y.
/// Vertex color = white (cn=0xFFFFFFFF, light_count=0) — irrelevant for PRIM passthrough.
fn build_two_material_two_run_scene() -> crate::hle::Scene {
    let prim_l = 0x00000000u32;
    let prim_h = 0x000000C3u32;
    let selectors = crate::hle::combiner::decode_combine(prim_l, prim_h);
    let white_tex = vec![255u8, 255, 255, 255]; // 1×1 white placeholder (tex_enable=false)

    let mat0 = crate::hle::Material {
        texture: white_tex.clone(),
        tex_w: 1,
        tex_h: 1,
        selectors: selectors.clone(),
        cycle_type: 0,
        prim: [255, 0, 0, 255], // RED
        env: [0, 0, 0, 255],
        blend_color: [0, 0, 0, 255],
        tex_enable: false,
        wrap_s: 2,
        wrap_t: 2,
        fmt: 0,
        siz: 0,
        tile_count: 1,
        tex1: None,
        prim_lod_frac: 0.0,
        prim_min_level: 0.0,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    };
    let mat1 = crate::hle::Material {
        texture: white_tex,
        tex_w: 1,
        tex_h: 1,
        selectors,
        cycle_type: 0,
        prim: [0, 0, 255, 255], // BLUE
        env: [0, 0, 0, 255],
        blend_color: [0, 0, 0, 255],
        tex_enable: false,
        wrap_s: 2,
        wrap_t: 2,
        fmt: 0,
        siz: 0,
        tile_count: 1,
        tex1: None,
        prim_lod_frac: 0.0,
        prim_min_level: 0.0,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    };

    // Passthrough viewport: sc = (FB_W/2, FB_H/2, ds), tr = same.
    // With identity MVP: output_clip_x = raw_pos_x, output_clip_y = raw_pos_y.
    let half_w = crate::hle::rsp::FB_WIDTH / 2.0; // 160.0
    let half_h = crate::hle::rsp::FB_HEIGHT / 2.0; // 120.0
    let ds = 511.0 / crate::hle::rsp::DEPTH_RANGE;
    let vp = ([half_w, half_h, ds], [half_w, half_h, ds]);

    // Left-half quad vertices (NDC x in [-1,0]), CCW winding: BL BR TR TL.
    let lv: [[f32; 3]; 4] = [
        [-1.0, -1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
    ];
    // Right-half quad vertices (NDC x in [0,1]), CCW winding.
    let rv: [[f32; 3]; 4] = [
        [0.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];

    let cn_white = 0xFF_FF_FF_FFu32; // R=G=B=A=255, light_count=0 → vertex color passthrough

    let mut scene = crate::hle::Scene {
        materials: vec![mat0, mat1],
        mvp_table: vec![crate::hle::math::identity()],
        viewport_table: vec![vp],
        texcoord_table: vec![[0.0, 0.0]],
        render_modes: vec![Default::default()],
        ..Default::default()
    };

    for i in 0..8usize {
        let v = if i < 4 { lv[i] } else { rv[i - 4] };
        scene.raw_pos.push(v);
        scene.mtx_index.push(0);
        scene.viewport_index.push(0);
        scene.raw_st.push([0.0, 0.0]);
        scene.texcoord_index.push(0);
        scene.cn.push(cn_white);
        scene.light_index.push(0);
        scene.light_count.push(0);
    }

    // Left-half triangles: 0,1,2 and 0,2,3 (CCW winding in NDC Y-up).
    scene.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    // Right-half triangles: 4,5,6 and 4,6,7.
    scene.indices.extend_from_slice(&[4, 5, 6, 4, 6, 7]);

    scene.draw_runs = vec![
        crate::hle::DrawRun {
            fog_color: [0; 4],
            material_index: 0,
            render_mode_index: 0,
            cull: crate::hle::CullKind::None,
            index_count: 6,
            index_start: 0,
        },
        crate::hle::DrawRun {
            fog_color: [0; 4],
            material_index: 1,
            render_mode_index: 0,
            cull: crate::hle::CullKind::None,
            index_count: 6,
            index_start: 6,
        },
    ];

    scene
}

/// A8b real-world test: a Scene with two materials and two draw runs must render TWO DISTINCT
/// screen regions — left half red (material 0, prim=(1,0,0)), right half blue (material 1,
/// prim=(0,0,1)).  Proves that per-material @group(0) bind groups AND per-run dynamic-offset
/// uniform slots in the 256-byte-stride pooled buffer are wired correctly through SceneRenderer.
#[test]
fn two_material_scene_renders_two_distinct_regions() {
    let scene = build_two_material_two_run_scene();
    let px = render_scene_to_rgba8(&scene, 64, 64);
    let left = ((32 * 64 + 16) * 4) as usize; // center of left half (row 32, col 16)
    let right = ((32 * 64 + 48) * 4) as usize; // center of right half (row 32, col 48)
    assert_ne!(
        &px[left..left + 3],
        &px[right..right + 3],
        "two runs must render distinct colors"
    );
    assert!(
        px[left] > px[left + 2],
        "left run ≈ red (R > B), got R={} B={}",
        px[left],
        px[left + 2]
    );
    assert!(
        px[right + 2] > px[right],
        "right run ≈ blue (B > R), got R={} B={}",
        px[right],
        px[right + 2]
    );
}

/// Build a two-material / two-run Scene where the OUTPUT COLOR comes from each material's
/// TEXTURE (not prim) — used to prove per-material `@group(0)` (texture) bind-group routing.
///
/// Geometry is identical to `build_two_material_two_run_scene` (left half = run 0, right half
/// = run 1), but here:
///   - both materials set `tex_enable: true` with a distinct 1×1 texture
///     (material 0 = RED texel, material 1 = BLUE texel),
///   - the combiner is TEXEL0 passthrough — `combine_l=0, combine_h=0x41`
///     (cd1 = bits(h,6,3) = 1 → TEXEL0 color_d, ad1 = bits(h,0,3) = 1 → TEXEL0 alpha_d)
///     so `result = (0-0)*0 + TEXEL0 = TEXEL0` and the sampled texture IS the pixel,
///   - both `prim` colors are GREEN — deliberately neither red nor blue, so a regression that
///     read prim instead of the texture could never coincidentally produce the asserted colors.
///
/// The only way the left half reads RED and the right half reads BLUE is if the draw loop binds
/// `tex_caches[run.material_index]` per run; a hardcoded `tex_caches[0]` would sample the RED
/// texture for BOTH runs, turning the right half red and failing the assertion.
fn build_two_material_two_run_textured_scene() -> crate::hle::Scene {
    let texel_l = 0x00000000u32;
    let texel_h = 0x00000041u32; // cd1=1 (TEXEL0), ad1=1 (TEXEL0)
    let selectors = crate::hle::combiner::decode_combine(texel_l, texel_h);

    let red_tex = vec![255u8, 0, 0, 255]; // 1×1 RED
    let blue_tex = vec![0u8, 0, 255, 255]; // 1×1 BLUE

    let mat0 = crate::hle::Material {
        texture: red_tex,
        tex_w: 1,
        tex_h: 1,
        selectors: selectors.clone(),
        cycle_type: 0,
        prim: [0, 255, 0, 255], // GREEN — never the asserted output, only the texture is
        env: [0, 0, 0, 255],
        blend_color: [0, 0, 0, 255],
        tex_enable: true,
        wrap_s: 2,
        wrap_t: 2,
        fmt: 0,
        siz: 0,
        tile_count: 1,
        tex1: None,
        prim_lod_frac: 0.0,
        prim_min_level: 0.0,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    };
    let mat1 = crate::hle::Material {
        texture: blue_tex,
        tex_w: 1,
        tex_h: 1,
        selectors,
        cycle_type: 0,
        prim: [0, 255, 0, 255], // GREEN
        env: [0, 0, 0, 255],
        blend_color: [0, 0, 0, 255],
        tex_enable: true,
        wrap_s: 2,
        wrap_t: 2,
        fmt: 0,
        siz: 0,
        tile_count: 1,
        tex1: None,
        prim_lod_frac: 0.0,
        prim_min_level: 0.0,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    };

    let half_w = crate::hle::rsp::FB_WIDTH / 2.0;
    let half_h = crate::hle::rsp::FB_HEIGHT / 2.0;
    let ds = 511.0 / crate::hle::rsp::DEPTH_RANGE;
    let vp = ([half_w, half_h, ds], [half_w, half_h, ds]);

    let lv: [[f32; 3]; 4] = [
        [-1.0, -1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
    ];
    let rv: [[f32; 3]; 4] = [
        [0.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];

    let cn_white = 0xFF_FF_FF_FFu32;

    let mut scene = crate::hle::Scene {
        materials: vec![mat0, mat1],
        mvp_table: vec![crate::hle::math::identity()],
        viewport_table: vec![vp],
        texcoord_table: vec![[0.0, 0.0]],
        render_modes: vec![Default::default()],
        ..Default::default()
    };

    for i in 0..8usize {
        let v = if i < 4 { lv[i] } else { rv[i - 4] };
        scene.raw_pos.push(v);
        scene.mtx_index.push(0);
        scene.viewport_index.push(0);
        scene.raw_st.push([0.0, 0.0]);
        scene.texcoord_index.push(0);
        scene.cn.push(cn_white);
        scene.light_index.push(0);
        scene.light_count.push(0);
    }

    scene.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    scene.indices.extend_from_slice(&[4, 5, 6, 4, 6, 7]);

    scene.draw_runs = vec![
        crate::hle::DrawRun {
            fog_color: [0; 4],
            material_index: 0,
            render_mode_index: 0,
            cull: crate::hle::CullKind::None,
            index_count: 6,
            index_start: 0,
        },
        crate::hle::DrawRun {
            fog_color: [0; 4],
            material_index: 1,
            render_mode_index: 0,
            cull: crate::hle::CullKind::None,
            index_count: 6,
            index_start: 6,
        },
    ];

    scene
}

/// A8b per-material TEXTURE bind-group routing test.  Unlike
/// `two_material_scene_renders_two_distinct_regions` (which uses `tex_enable:false`, so
/// `@group(0)` has no effect and a `tex_caches[0]` hardcode would still pass), this test makes
/// the sampled texture the pixel output (TEXEL0 passthrough, `tex_enable:true`).  Left run uses
/// material 0's RED texture, right run uses material 1's BLUE texture.  Passing therefore REQUIRES
/// the draw loop to bind `tex_caches[run.material_index]` per run — a hardcoded `[0]` samples RED
/// for both halves and fails the right-half blue assertion.
#[test]
fn two_material_scene_routes_per_material_texture() {
    let scene = build_two_material_two_run_textured_scene();
    let px = render_scene_to_rgba8(&scene, 64, 64);
    let left = ((32 * 64 + 16) * 4) as usize; // center of left half
    let right = ((32 * 64 + 48) * 4) as usize; // center of right half
    assert_ne!(
        &px[left..left + 3],
        &px[right..right + 3],
        "two runs must sample distinct per-material textures"
    );
    assert!(
        px[left] > px[left + 2],
        "left run samples material 0 RED texture (R > B), got R={} B={}",
        px[left],
        px[left + 2]
    );
    assert!(
        px[right + 2] > px[right],
        "right run samples material 1 BLUE texture (B > R), got R={} B={}",
        px[right],
        px[right + 2]
    );
}

/// Build a two-run dual-source Scene: an opaque GREEN full-screen quad (run 0, establishing the
/// framebuffer "memory" = CLR_MEM) then a RED quad (run 1) drawn with a DualSrc render mode whose
/// blender mux is `mux_low` and prim alpha is `red_alpha`. Both runs are depth-less, so run 1
/// blends over run 0 within the single render pass. Used by the B4 dual-source eval tests; goes
/// through `SceneRenderer` (which carries `dual_source`) so the primary @blend_src path is taken.
fn build_dualsrc_over_green_scene(mux_low: u32, red_alpha: u8) -> crate::hle::Scene {
    // PRIM passthrough combiner: (COMBINED-COMBINED)*COMBINED + PRIM = PRIM for color AND alpha
    // (combine_l=0, combine_h=0xC3 → cd1=PRIM, ad1=PRIM).
    let selectors = crate::hle::combiner::decode_combine(0x0000_0000, 0x0000_00C3);
    let white = vec![255u8, 255, 255, 255];
    let mat_green = crate::hle::Material {
        texture: white.clone(),
        tex_w: 1,
        tex_h: 1,
        selectors: selectors.clone(),
        cycle_type: 0,
        prim: [0, 255, 0, 255], // GREEN backdrop = framebuffer memory (CLR_MEM)
        env: [0, 0, 0, 255],
        blend_color: [0, 0, 0, 255],
        tex_enable: false,
        wrap_s: 2,
        wrap_t: 2,
        fmt: 0,
        siz: 0,
        tile_count: 1,
        tex1: None,
        prim_lod_frac: 0.0,
        prim_min_level: 0.0,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    };
    let mat_red = crate::hle::Material {
        texture: white,
        tex_w: 1,
        tex_h: 1,
        selectors,
        cycle_type: 0,
        prim: [255, 0, 0, red_alpha], // RED prim; alpha drives the blender A coefficient
        env: [0, 0, 0, 255],
        blend_color: [0, 0, 0, 255],
        tex_enable: false,
        wrap_s: 2,
        wrap_t: 2,
        fmt: 0,
        siz: 0,
        tile_count: 1,
        tex1: None,
        prim_lod_frac: 0.0,
        prim_min_level: 0.0,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    };
    let rm_opaque = crate::hle::RenderMode::default(); // Replace backdrop
                                                       // DualSrc render mode for the red quad (FORCE_BL, no depth → no Z attachment).
    let rm_dual =
        crate::hle::decode_render_mode((mux_low << 16) | crate::hle::consts::rdp::FORCE_BL, 0, 0);
    assert_eq!(
        rm_dual.blend_class,
        crate::hle::BlendClass::DualSrc,
        "test mux must classify DualSrc"
    );

    let half_w = crate::hle::rsp::FB_WIDTH / 2.0;
    let half_h = crate::hle::rsp::FB_HEIGHT / 2.0;
    let ds = 511.0 / crate::hle::rsp::DEPTH_RANGE;
    let vp = ([half_w, half_h, ds], [half_w, half_h, ds]);
    // Full-screen quad (NDC [-1,1]), CCW winding: BL BR TR TL.
    let quad: [[f32; 3]; 4] = [
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
    ];

    let mut scene = crate::hle::Scene {
        materials: vec![mat_green, mat_red],
        mvp_table: vec![crate::hle::math::identity()],
        viewport_table: vec![vp],
        texcoord_table: vec![[0.0, 0.0]],
        render_modes: vec![rm_opaque, rm_dual],
        ..Default::default()
    };
    // Two copies of the quad (verts 0..4 green backdrop, 4..8 red dual-source).
    for _ in 0..2 {
        for v in &quad {
            scene.raw_pos.push(*v);
            scene.mtx_index.push(0);
            scene.viewport_index.push(0);
            scene.raw_st.push([0.0, 0.0]);
            scene.texcoord_index.push(0);
            scene.cn.push(0xFF_FF_FF_FF);
            scene.light_index.push(0);
            scene.light_count.push(0);
        }
    }
    scene.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    scene.indices.extend_from_slice(&[4, 5, 6, 4, 6, 7]);
    scene.draw_runs = vec![
        crate::hle::DrawRun {
            fog_color: [0; 4],
            material_index: 0,
            render_mode_index: 0,
            cull: crate::hle::CullKind::None,
            index_count: 6,
            index_start: 0,
        },
        crate::hle::DrawRun {
            fog_color: [0; 4],
            material_index: 1,
            render_mode_index: 1,
            cull: crate::hle::CullKind::None,
            index_count: 6,
            index_start: 6,
        },
    ];
    scene
}

// IMP10: this slice's CI runs on a DUAL_SOURCE_BLENDING-capable adapter; these two tests
// HARD-assert the feature so they fail loudly (not vacuously) if a runner lacks it. They are
// #[ignore]d so the local `cargo test` skips them on adapters without dual-source — CI runs them
// via `cargo test -- --ignored`. (On THIS dev machine the Metal adapter DOES advertise
// dual-source, so `cargo test -- --ignored` passes locally too — see the B4 report.)
#[test]
#[ignore = "requires a DUAL_SOURCE_BLENDING adapter; CI runs via cargo test -- --ignored"]
fn dual_source_eval_matches_convex_combination() {
    let (_d, _q, dual) = headless_device();
    assert!(
        dual,
        "B4 requires a DUAL_SOURCE_BLENDING adapter; configure CI to provide one"
    );
    // Canonical XLU lerp: P=CLR_IN(red), A=A_IN (=0.5 via prim alpha 128), M=CLR_MEM(green), B=1MA.
    // denom = a + (1-a) = 1 → out = 0.5*red + 0.5*green ≈ (128,128,0). (Convex; B3 also passes.)
    use crate::hle::consts::rdp::{gbl_c1, gbl_c2, A_IN, B_1MA, CLR_IN, CLR_MEM};
    let mux = (gbl_c1(CLR_IN, A_IN, CLR_MEM, B_1MA) | gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA)) >> 16;
    let scene = build_dualsrc_over_green_scene(mux, 128);
    let px = render_scene_to_rgba8(&scene, 32, 32);
    let c = ((16 * 32 + 16) * 4) as usize;
    assert!(
        px[c] > 90 && px[c] < 160 && px[c + 1] > 90,
        "expected ~half red/half green blend, got {:?}",
        &px[c..c + 3]
    );
}

#[test]
#[ignore = "requires a DUAL_SOURCE_BLENDING adapter; CI runs via cargo test -- --ignored"]
fn dual_source_additive_diverges_from_alphaover_fallback() {
    // IMP10 — the DISCRIMINATING case: additive mux A=1, B=1. With the normalized N64 blender
    // (P·A+M·B)/(A+B) the result is (P+M)/2 = (red+green)/2 ≈ (128,128,0). The B3 AlphaOver
    // fallback (SrcAlpha-over, srcAlpha=1) collapses this to PURE red (green≈0) and CANNOT
    // reproduce the green bleed-through — so `px[c+1] > 90` is impossible on the fallback path and
    // discriminates the dual-source primary path. (NB: the brief's narrative said "yellow
    // (255,255,0)", which assumes an UN-normalized additive; the verbatim §5 formula normalizes by
    // (A+B), so the true output is half-intensity — see the B4 report.)
    let (_d, _q, dual) = headless_device();
    assert!(dual, "B4 requires a DUAL_SOURCE_BLENDING adapter");
    use crate::hle::consts::rdp::{gbl_c1, gbl_c2, A_IN, B_1, CLR_IN, CLR_MEM};
    let mux = (gbl_c1(CLR_IN, A_IN, CLR_MEM, B_1) | gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1)) >> 16;
    let scene = build_dualsrc_over_green_scene(mux, 255);
    let px = render_scene_to_rgba8(&scene, 32, 32);
    let c = ((16 * 32 + 16) * 4) as usize;
    assert!(
        px[c] > 90 && px[c] < 170 && px[c + 1] > 90 && px[c + 1] < 170 && px[c + 2] < 60,
        "additive (P+M)/2 over green → ~(128,128,0), diverging from the pure-red AlphaOver fallback; got {:?}",
        &px[c..c + 3]
    );
}

/// MVP construction (vertex pos=(0,0,1), row-vector convention):
///   mul_row_vec4([0,0,1,1], mvp) must yield clip [*,*,clip_z,clip_w].
///   Identity except mvp[2][2]=0, mvp[3][2]=clip_z, mvp[2][3]=0, mvp[3][3]=clip_w.
///   z contribution: 0*mvp[0][2] + 0*mvp[1][2] + 1*0 + 1*clip_z = clip_z ✓
///   w contribution: 0*mvp[0][3] + 0*mvp[1][3] + 1*0 + 1*clip_w = clip_w ✓
///
/// The kernel's fog path: fz = max(clip.z, 0.0) / w, then
///   fog_alpha = clamp(fz*fm + fo, 0, 255) / 255.
/// Returns o.color.a of the single output vertex.
fn run_fog_kernel_alpha(clip_z: f32, clip_w: f32, fm: f32, fo: f32) -> f32 {
    let (device, queue, _) = headless_device();
    use crate::render::rsp_buffers as rb;
    use wgpu::util::DeviceExt;

    // Custom MVP: identity with row2/row3 cols 2 and 3 adjusted so that
    // mul_row_vec4([0,0,1,1], mvp) = [0, 0, clip_z, clip_w].
    let mut mvp = crate::hle::math::identity();
    mvp[2][2] = 0.0; // zero out identity's contribution at (row2, col2)
    mvp[3][2] = clip_z; // row3's contribution to clip.z
    mvp[2][3] = 0.0; // already 0 in identity, but explicit for clarity
    mvp[3][3] = clip_w; // row3's contribution to clip.w (overrides identity's 1.0)

    let scene = crate::hle::Scene {
        raw_pos: vec![[0.0, 0.0, 1.0]],
        raw_st: vec![[0.0, 0.0]],
        mtx_index: vec![0],
        viewport_index: vec![0],
        texcoord_index: vec![0],
        // cn alpha=255 (unlit, will be overwritten by fog), rgb=0
        cn: vec![0xFF00_0000u32],
        light_index: vec![0],
        light_count: vec![0],
        fog: vec![1],
        fog_table: vec![[fm as i16, fo as i16]],
        mvp_table: vec![mvp],
        viewport_table: vec![(
            [
                crate::hle::rsp::FB_WIDTH / 2.0,
                crate::hle::rsp::FB_HEIGHT / 2.0,
                511.0 / crate::hle::rsp::DEPTH_RANGE,
            ],
            [
                crate::hle::rsp::FB_WIDTH / 2.0,
                crate::hle::rsp::FB_HEIGHT / 2.0,
                511.0 / crate::hle::rsp::DEPTH_RANGE,
            ],
        )],
        texcoord_table: vec![[0.0, 0.0]],
        ..Default::default()
    };

    let n = 1u32;
    let mk = |data: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: data,
            usage,
        })
    };
    let s = wgpu::BufferUsages::STORAGE;
    let source = mk(bytemuck::cast_slice(&rb::src_vertices(&scene)), s);
    let mvp_buf = mk(bytemuck::cast_slice(&rb::mvp_table(&scene)), s);
    let vp_buf = mk(bytemuck::cast_slice(&rb::viewport_table(&scene)), s);
    let tc_buf = mk(bytemuck::cast_slice(&rb::texcoord_table(&scene)), s);
    let lights_buf = mk(bytemuck::cast_slice(&rb::lights_table(&scene)), s);
    let lookat_buf = mk(bytemuck::cast_slice(&rb::lookat_table(&scene)), s);
    let fog_table = mk(bytemuck::cast_slice(&rb::fog_table(&scene)), s);
    let out_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 48,
        mapped_at_creation: false,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let params = mk(
        bytemuck::bytes_of(&crate::render::RspProcessParams {
            vertex_count: n,
            _pad: [0; 3],
        }),
        wgpu::BufferUsages::UNIFORM,
    );
    let pipe = crate::render::RspProcessPipeline::new(&device);
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: pipe.bind_group_layout(),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: source.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: mvp_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: vp_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: tc_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: lights_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: lookat_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: out_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: fog_table.as_entire_binding(),
            },
        ],
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 48,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    pipe.dispatch(&mut enc, &bg, n);
    enc.copy_buffer_to_buffer(&out_buf, 0, &readback, 0, 48);
    queue.submit(Some(enc.finish()));
    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let data = slice.get_mapped_range();
    // OutVertex layout (48B): [pos f32x4 @0..16], [color f32x4 @16..32], [uv f32x2 @32..40], [pad @40..48]
    // color.a is float index 7 (byte offset 28).
    let color_a = bytemuck::cast_slice::<u8, f32>(&data)[7];
    drop(data);
    readback.unmap();
    color_a
}

/// C2 fog-factor test: proves the compute kernel writes fog alpha from RAW clip-Z (not
/// viewport-folded o.pos.z).
///
/// Two samples:
///
/// 1. Near-plane saturating case (clip_z=1, clip_w=1, fm=1280, fo=-1024):
///    fz = max(1.0,0)/1.0 = 1.0; fog_alpha = clamp(1.0*1280 - 1024, 0,255)/255 = 256/255 → 1.0.
///    This case is necessary but insufficient alone (a viewport-folded z would also saturate).
///
/// 2. Non-saturating case (clip_z=0.85, clip_w=1.0, fm=1280, fo=-1024) — the discriminating sample:
///    Raw clip: fz = 0.85/1.0 = 0.85; fog_alpha = (0.85*1280 - 1024)/255 = 64/255 ≈ 0.2510.
///    Viewport-folded o.pos.z (with default vp.scale.z=511/1024, vp.trans.z=511/1024):
///    o.pos.z = clip_z*(511/1024) + clip_w*(511/1024) = 1.85*(511/1024) ≈ 0.9232
///    fz_wrong = 0.9232/1.0; fog_alpha_wrong = (0.9232*1280-1024)/255 ≈ 182/255 ≈ 0.714
///    The raw-clip result (64/255≈0.251) differs decisively from the folded result (~0.714),
///    so the assertion `(mid - 64.0/255.0).abs() < 0.01` pins that we used raw clip-Z.
#[test]
fn fog_factor_uses_raw_clip_z() {
    // 1. Near-plane saturating case: fz=1.0 → clamp(1280-1024,0,255)/255 = 1.0.
    let a = run_fog_kernel_alpha(1.0, 1.0, 1280.0, -1024.0);
    assert!(
        (a - 1.0).abs() < 0.01,
        "near-plane fog factor should saturate to 1.0, got {a}"
    );

    // 2. Non-saturating case: clip_z/clip_w=0.85 → 0.85*1280 - 1024 = 64 → 64/255 ≈ 0.2510.
    //    Viewport-folded z would give ~0.714, not 0.251 — so this assertion is the discriminator.
    let mid = run_fog_kernel_alpha(0.85, 1.0, 1280.0, -1024.0);
    assert!(
        (mid - 64.0 / 255.0).abs() < 0.01,
        "raw-clip fog factor must be ~0.251 (64/255), got {mid} \
        (viewport-folded path would give ~0.714 — if this fails, the kernel used o.pos.z not clip.z)"
    );
}

/// Build a two-run DECAL Scene for the E1 pass-structure smoke test:
///   run 0 = base full-screen quad, `G_RM_AA_ZB_OPA_SURF` (z_mode=Opa, z_test+z_write), BLACK prim.
///   run 1 = coplanar full-screen quad (same Z), `G_RM_AA_ZB_OPA_DECAL` (z_mode=Decal,
///           z_test, no z_write), BRIGHT prim.
///
/// Coplanar at the same NDC Z, the decal would FAIL a `Less` depth test against the base in a
/// single depth-writing pass (`z < z` is false), so a single-pass renderer leaves the center the
/// BLACK base. E1's two-phase split renders the decal in a SECOND pass with NO depth attachment
/// (binding the prior pass's depth as a sampled texture at `@group(2)`), so the decal paints over
/// the base — proving the pass/layout machinery works without a wgpu validation error.
fn build_decal_smoke_scene() -> crate::hle::Scene {
    // PRIM passthrough combiner (combine_l=0, combine_h=0xC3 → cd1=PRIM, ad1=PRIM).
    let selectors = crate::hle::combiner::decode_combine(0x0000_0000, 0x0000_00C3);
    let white = vec![255u8, 255, 255, 255];
    let mat_base = crate::hle::Material {
        texture: white.clone(),
        tex_w: 1,
        tex_h: 1,
        selectors: selectors.clone(),
        cycle_type: 0,
        prim: [0, 0, 0, 255], // BLACK base — pre-impl, the hidden decal leaves this black
        env: [0, 0, 0, 255],
        blend_color: [0, 0, 0, 255],
        tex_enable: false,
        wrap_s: 2,
        wrap_t: 2,
        fmt: 0,
        siz: 0,
        tile_count: 1,
        tex1: None,
        prim_lod_frac: 0.0,
        prim_min_level: 0.0,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    };
    let mat_decal = crate::hle::Material {
        texture: white,
        tex_w: 1,
        tex_h: 1,
        selectors,
        cycle_type: 0,
        prim: [220, 40, 255, 255], // BRIGHT decal — only visible if the decal pass runs
        env: [0, 0, 0, 255],
        blend_color: [0, 0, 0, 255],
        tex_enable: false,
        wrap_s: 2,
        wrap_t: 2,
        fmt: 0,
        siz: 0,
        tile_count: 1,
        tex1: None,
        prim_lod_frac: 0.0,
        prim_min_level: 0.0,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    };
    let rm_base =
        crate::hle::decode_render_mode(crate::hle::consts::rdp::G_RM_AA_ZB_OPA_SURF, 0, 0);
    let rm_decal =
        crate::hle::decode_render_mode(crate::hle::consts::rdp::G_RM_AA_ZB_OPA_DECAL, 0, 0);
    assert_eq!(
        rm_decal.z_mode,
        crate::hle::ZMode::Decal,
        "run 1 must be a decal run"
    );
    assert_eq!(
        rm_base.z_mode,
        crate::hle::ZMode::Opa,
        "run 0 must be an opaque run"
    );

    let half_w = crate::hle::rsp::FB_WIDTH / 2.0;
    let half_h = crate::hle::rsp::FB_HEIGHT / 2.0;
    let ds = 511.0 / crate::hle::rsp::DEPTH_RANGE;
    let vp = ([half_w, half_h, ds], [half_w, half_h, ds]);
    // Full-screen quad (NDC [-1,1]) at Z=0, CCW winding: BL BR TR TL.
    let quad: [[f32; 3]; 4] = [
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
    ];

    let mut scene = crate::hle::Scene {
        materials: vec![mat_base, mat_decal],
        mvp_table: vec![crate::hle::math::identity()],
        viewport_table: vec![vp],
        texcoord_table: vec![[0.0, 0.0]],
        render_modes: vec![rm_base, rm_decal],
        ..Default::default()
    };
    // Two coplanar copies of the quad (verts 0..4 base, 4..8 decal).
    for _ in 0..2 {
        for v in &quad {
            scene.raw_pos.push(*v);
            scene.mtx_index.push(0);
            scene.viewport_index.push(0);
            scene.raw_st.push([0.0, 0.0]);
            scene.texcoord_index.push(0);
            scene.cn.push(0xFF_FF_FF_FF);
            scene.light_index.push(0);
            scene.light_count.push(0);
        }
    }
    scene.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    scene.indices.extend_from_slice(&[4, 5, 6, 4, 6, 7]);
    scene.draw_runs = vec![
        crate::hle::DrawRun {
            fog_color: [0; 4],
            material_index: 0,
            render_mode_index: 0,
            cull: crate::hle::CullKind::None,
            index_count: 6,
            index_start: 0,
        },
        crate::hle::DrawRun {
            fog_color: [0; 4],
            material_index: 1,
            render_mode_index: 1,
            cull: crate::hle::CullKind::None,
            index_count: 6,
            index_start: 6,
        },
    ];
    scene
}

/// E1 smoke: a base opaque quad + a coplanar decal quad render through `SceneRenderer` WITHOUT a
/// wgpu validation error, and the decal draws OVER the base. Pre-implementation the decal goes
/// through the single depth-writing pass and is rejected by the `Less` depth test (coplanar), so
/// the center stays the BLACK base and this fails; the two-phase split makes the bright decal win.
#[test]
fn decal_pass_structure_does_not_panic_and_depth_is_sampleable() {
    let scene = build_decal_smoke_scene();
    let px = render_scene_to_rgba8(&scene, 48, 48);
    let c = ((24 * 48 + 24) * 4) as usize;
    assert!(
        px[c] > 16 || px[c + 1] > 16 || px[c + 2] > 32,
        "decal should draw over the base, got R={} G={} B={}",
        px[c],
        px[c + 1],
        px[c + 2]
    );
}

// ---- two-texture (TEXEL1) + 2-cycle role-swap GPU tests --------------------------------------

/// Render a full-screen quad through the base ubershader with TWO distinct 1×1 textures bound at
/// group(0) slots 0/1 (tex0) and 2/3 (tex1), a 2-cycle combiner, and `tex_enable1 = 1` so the
/// shader's TEXEL1 wiring + role swap are live. Returns the center pixel RGBA. Because the textures
/// are 1×1 the sampled color is uniform across the quad, isolating the combiner arithmetic.
#[allow(clippy::too_many_arguments)]
fn render_two_texture_center(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    combine_l: u32,
    combine_h: u32,
    tex0_rgba: [u8; 4],
    tex1_rgba: [u8; 4],
    prim: [f32; 4],
) -> [u8; 4] {
    const W: u32 = 32;
    const H: u32 = 32;
    let bytes_per_row = W * 4; // 128 -> pad to 256 for copy alignment
    let padded_bpr = 256u32;
    let format = wgpu::TextureFormat::Rgba8Unorm;

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("2tex-target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
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

    // Full-screen quad; shade is irrelevant to these combiners (they never select SHADE).
    let verts = [
        OutVertex {
            position: [-1.0, -1.0, 0.0, 1.0],
            color: [1.0; 4],
            uv: [0.0, 1.0],
            _pad: [0.0; 2],
        },
        OutVertex {
            position: [1.0, -1.0, 0.0, 1.0],
            color: [1.0; 4],
            uv: [1.0, 1.0],
            _pad: [0.0; 2],
        },
        OutVertex {
            position: [1.0, 1.0, 0.0, 1.0],
            color: [1.0; 4],
            uv: [1.0, 0.0],
            _pad: [0.0; 2],
        },
        OutVertex {
            position: [-1.0, 1.0, 0.0, 1.0],
            color: [1.0; 4],
            uv: [0.0, 0.0],
            _pad: [0.0; 2],
        },
    ];
    let indices: [u32; 6] = [0, 1, 2, 0, 2, 3];
    let pos_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("pos"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ibuf"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    // Two distinct 1×1 textures.
    let mk_tex = |rgba: [u8; 4], label: &str| {
        let t = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &t,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        t.create_view(&wgpu::TextureViewDescriptor::default())
    };
    let tex0_view = mk_tex(tex0_rgba, "tex0-red");
    let tex1_view = mk_tex(tex1_rgba, "tex1-green");
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("nearest"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    let uniform = CombinerUniform {
        combine_l,
        combine_h,
        cycle_type: 1, // 2-cycle
        tex_enable: 1,
        blender_mux: 0,
        force_blend: 0,
        alpha_mode: 0,
        alpha_threshold: 0.0,
        prim,
        env: [0.0, 0.0, 0.0, 1.0],
        blend_color: [0.0; 4],
        fog_color: [0.0; 4],
        inv_tex_size: [1.0, 1.0, 0.0, 0.0],
        // .z = tex_enable1 = 1.0 -> TEXEL1 wiring + 2-cycle role swap are LIVE. 1×1 textures -> .xy=1.
        inv_tex1_size: [1.0, 1.0, 1.0, 0.0],
        lod_params: [0.0, 1.0, 0.0, 1.0],
        inv_detail_size: [1.0, 1.0, 0.0, 0.0],
    };

    let pipeline = TexturedPipeline::new(device, format, DEPTH_FORMAT);
    let group0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("2tex-g0"),
        layout: pipeline.bind_group_layout(),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&tex0_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&tex1_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            // DETAIL slot (4/5): unused by this test — reuse a bound view/sampler.
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&tex1_view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ]
        // LOD-level slots (6..=12): unused by this test — reuse a bound view.
        .into_iter()
        .chain((6..13u32).map(|b| wgpu::BindGroupEntry {
            binding: b,
            resource: wgpu::BindingResource::TextureView(&tex1_view),
        }))
        .collect::<Vec<_>>(),
    });
    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("2tex-uniform"),
        contents: bytemuck::bytes_of(&uniform),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let group1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("2tex-g1"),
        layout: pipeline.uniform_bind_group_layout(),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buf.as_entire_binding(),
        }],
    });

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded_bpr * H) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let _ = bytes_per_row;

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let mock_scene = crate::hle::Scene {
        draw_runs: vec![crate::hle::DrawRun {
            fog_color: [0; 4],
            material_index: 0,
            render_mode_index: 0,
            cull: crate::hle::CullKind::None,
            index_count: indices.len() as u32,
            index_start: 0,
        }],
        render_modes: vec![Default::default()],
        ..Default::default()
    };
    pipeline.draw(
        &mut encoder,
        &view,
        &pos_buf,
        &ibuf,
        &mock_scene,
        wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
        &[&group0],
        &group1,
        0,
        None,
    );
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
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
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
    let off = (16 * padded_bpr + 16 * 4) as usize; // center pixel
    let out = [data[off], data[off + 1], data[off + 2], data[off + 3]];
    drop(data);
    readback.unmap();
    out
}

/// DECISIVE role-swap test. tex0 = pure RED, tex1 = pure GREEN. The 2-cycle combiner's CYCLE 1 output
/// is a TEXEL0 passthrough (color d = TEXEL0). With the cycle-1 role swap a TEXEL0 selector reads
/// the tex1 sample -> the pixel is GREEN. WITHOUT the swap it would read tex0 -> RED. This single
/// assertion distinguishes correct-swap from no-swap unambiguously.
#[test]
fn two_cycle_role_swap_cycle1_texel0_reads_tex1() {
    let (device, queue, _dual_source) = headless_device();
    // cyc0 = (0-0)*0 + ONE (unused COMBINED); cyc1 color = (0-0)*0 + TEXEL0, alpha = ONE.
    // L=0x00887F10, H=0x88FFFE7E (hand-packed; cyc1 color d = TEXEL0(1), all else ZERO/ONE).
    let px = render_two_texture_center(
        &device,
        &queue,
        0x0088_7F10,
        0x88FF_FE7E,
        [255, 0, 0, 255], // tex0 = RED
        [0, 255, 0, 255], // tex1 = GREEN
        [1.0, 1.0, 1.0, 1.0],
    );
    // Cycle-1 TEXEL0 must resolve to the tex1 (GREEN) sample via the role swap.
    assert!(
        px[1] > 200 && px[0] < 60 && px[2] < 60,
        "cycle-1 TEXEL0 must read tex1 (GREEN) via the role swap; got R={} G={} B={} \
         (RED here would mean the swap is missing)",
        px[0],
        px[1],
        px[2],
    );
}

/// Hand-computed 2-cycle two-texture BLEND, accounting for the role swap.
///
/// cyc0 = TEXEL0 (unused). cyc1 color = (TEXEL1 - TEXEL0) * PRIM + TEXEL0. Under the cycle-1 swap
/// TEXEL0 -> tex1 sample, TEXEL1 -> tex0 sample, so cyc1 = (tex0 - tex1)*0.5 + tex1 = 0.5*(tex0+tex1).
///   tex0 = (204,51,102), tex1 = (102,153,204), PRIM.rgb = 0.5
///   => expected = ((204+102)/2, (51+153)/2, (102+204)/2) = (153, 102, 153).
#[test]
fn two_cycle_two_texture_blend_matches_hand_computed_pixel() {
    let (device, queue, _dual_source) = headless_device();
    // L=0x00887E43, H=0x81FCFC7E: cyc1 a=TEXEL1(2) b=TEXEL0(1) c=PRIMITIVE(3) d=TEXEL0(1);
    // cyc0 d=TEXEL0; alpha=ONE in both cycles.
    let px = render_two_texture_center(
        &device,
        &queue,
        0x0088_7E43,
        0x81FC_FC7E,
        [204, 51, 102, 255],  // tex0
        [102, 153, 204, 255], // tex1
        [0.5, 0.5, 0.5, 1.0], // PRIM = blend factor 0.5
    );
    let expect = [153u8, 102, 153, 255];
    for c in 0..3 {
        let d = (px[c] as i32 - expect[c] as i32).abs();
        assert!(
            d <= 2,
            "blend channel {c}: expected {} got {} (full pixel R={} G={} B={})",
            expect[c],
            px[c],
            px[0],
            px[1],
            px[2],
        );
    }
}

// ============================================================================================
// N64-faithful LOD decisive tests. No golden references LOD (no existing DL exercises
// G_TL_LOD), so these hand-build a `CombinerUniform` + a real mip-chain `tex0` (mirroring
// `render_two_texture_center`'s direct-pipeline style used above) and hand-compute the expected
// pixel from the `compute_lod` algorithm in `combiner_prelude.wgsl`.
// ============================================================================================

/// Pack the 1-cycle (cyc1-slot) combine words for `color = (a - b) * c + d`, `alpha = ONE`. Mirrors
/// `CombinerUniform::fill_rect`'s bit-packing style (documented cyc1 field positions in
/// `combiner_prelude.wgsl`/`eval_combiner`: color a=L[5,4] b=H[24,4] c=L[0,5] d=H[6,3]; alpha
/// a=H[21,3] b=H[3,3] c=H[18,3] d=H[0,3]).
fn pack_cyc1_combine(ca: u32, cb: u32, cc: u32, cd: u32) -> (u32, u32) {
    const AC_ZERO: u32 = 7; // G_ACMUX_0 (alpha_abd / alpha_c both resolve idx 7 -> ZERO)
    const AD_ONE: u32 = 6; // G_ACMUX_1 (alpha_abd idx 6 -> ONE)
    let combine_l = ((ca & 0xF) << 5) | (cc & 0x1F);
    let combine_h = ((cb & 0xF) << 24)
        | ((cd & 0x7) << 6)
        | ((AC_ZERO) << 21)
        | ((AC_ZERO) << 3)
        | ((AC_ZERO) << 18)
        | AD_ONE;
    (combine_l, combine_h)
}

/// Upload a solid-color mip chain (`levels[k]` is the flat RGBA8 color of level `k`, dims
/// `(max(1,base_w>>k), max(1,base_h>>k))`) plus an optional DETAIL tile, build the LOD
/// `CombinerUniform`, and render a full-screen quad with the given per-vertex UVs (already in
/// TEXEL space — the same convention `rsp.rs` emits for a real triangle draw) through the actual
/// `TexturedPipeline`/`eval_combiner` shader. Returns the center pixel.
#[allow(clippy::too_many_arguments)]
fn render_lod_center(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    combine_l: u32,
    combine_h: u32,
    levels: &[[u8; 4]],
    base_w: u32,
    base_h: u32,
    detail: Option<([u8; 4], u32, u32)>,
    prim_lod_frac: f32,
    prim_lod_min: f32,
    detail_mode: f32,
    uvs: [[f32; 2]; 4],
) -> [u8; 4] {
    const W: u32 = 64;
    const H: u32 = 64;
    let format = wgpu::TextureFormat::Rgba8Unorm;

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lod-target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
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

    let verts = [
        OutVertex {
            position: [-1.0, -1.0, 0.0, 1.0],
            color: [1.0; 4],
            uv: uvs[0],
            _pad: [0.0; 2],
        },
        OutVertex {
            position: [1.0, -1.0, 0.0, 1.0],
            color: [1.0; 4],
            uv: uvs[1],
            _pad: [0.0; 2],
        },
        OutVertex {
            position: [1.0, 1.0, 0.0, 1.0],
            color: [1.0; 4],
            uv: uvs[2],
            _pad: [0.0; 2],
        },
        OutVertex {
            position: [-1.0, 1.0, 0.0, 1.0],
            color: [1.0; 4],
            uv: uvs[3],
            _pad: [0.0; 2],
        },
    ];
    let indices: [u32; 6] = [0, 1, 2, 0, 2, 3];
    let pos_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("lod-pos"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("lod-ibuf"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    // N64-faithful per-level INDEPENDENT textures (NOT a mip chain) — mirrors the reworked
    // `build_tex_entry`. Level 0 is `tex0` (@binding 0) at (base_w, base_h); each further level k is
    // its OWN single-mip texture bound at `tex_lod{k}` (bindings 6..). Each level texture is sized at
    // (base_w, base_h) here: the levels are SOLID colors, so their sampled value is size-independent,
    // which lets this harness exercise NON-HALVING level sets (same-size levels) as well as halving
    // ones — the shader picks the level via the `sample_level` switch, and `compute_lod` depends only
    // on `inv_tex_size` (base dims) + `num_levels` + UVs, never on the per-level texture dims.
    let num_levels = levels.len() as u32;
    let make_level = |label: &str, w: u32, h: u32, color: [u8; 4]| -> wgpu::TextureView {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let pixels: Vec<u8> = color.repeat((w * h) as usize);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        tex.create_view(&wgpu::TextureViewDescriptor::default())
    };
    let tex0_view = make_level("lod-tex0", base_w, base_h, levels[0]);
    // Shared 1×1 dummy for unused LOD-level slots (never sampled — `compute_lod` clamps the selected
    // level to `num_levels`).
    let dummy_view = make_level("lod-dummy", 1, 1, [0, 0, 0, 0]);
    // Independent LOD levels 1..MAX_LOD bound at bindings 6..=12; slots past the uploaded count bind
    // the dummy.
    let lod_level_views: Vec<wgpu::TextureView> = (1..8u32)
        .map(|k| {
            if (k as usize) < levels.len() {
                make_level("lod-tex-level", base_w, base_h, levels[k as usize])
            } else {
                dummy_view.clone()
            }
        })
        .collect();

    let (detail_view, detail_w, detail_h) = match detail {
        Some((color, dw, dh)) => {
            let dt = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("lod-detail"),
                size: wgpu::Extent3d {
                    width: dw,
                    height: dh,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let pixels: Vec<u8> = color.repeat((dw * dh) as usize);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &dt,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(dw * 4),
                    rows_per_image: Some(dh),
                },
                wgpu::Extent3d {
                    width: dw,
                    height: dh,
                    depth_or_array_layers: 1,
                },
            );
            (
                dt.create_view(&wgpu::TextureViewDescriptor::default()),
                dw,
                dh,
            )
        }
        None => (tex0_view.clone(), 1, 1),
    };

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("lod-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });

    let uniform = CombinerUniform {
        combine_l,
        combine_h,
        cycle_type: 0, // 1-cycle: no swap needed, TEXEL0/TEXEL1 read the (level0,level1) taps directly.
        tex_enable: 1,
        blender_mux: 0,
        force_blend: 0,
        alpha_mode: 0,
        alpha_threshold: 0.0,
        prim: [0.0, 0.0, 0.0, 1.0],
        env: [0.0, 0.0, 0.0, 1.0],
        blend_color: [0.0; 4],
        fog_color: [0.0; 4],
        inv_tex_size: [1.0 / base_w as f32, 1.0 / base_h as f32, 0.0, 0.0],
        inv_tex1_size: [1.0, 1.0, 0.0, 0.0], // unused: LOD forces the swap on independent of this.
        lod_params: [1.0, num_levels as f32, prim_lod_frac, 1.0],
        inv_detail_size: [
            1.0 / detail_w as f32,
            1.0 / detail_h as f32,
            prim_lod_min,
            detail_mode,
        ],
    };

    let pipeline = TexturedPipeline::new(device, format, DEPTH_FORMAT);
    let group0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lod-g0"),
        layout: pipeline.bind_group_layout(),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&tex0_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            // TEXEL1 slot: unused under LOD (the shader ignores tex1 when lod_params.x != 0) —
            // reuse tex0's view/sampler to satisfy the group(0) layout.
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&tex0_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&detail_view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ]
        .into_iter()
        .chain(
            lod_level_views
                .iter()
                .enumerate()
                .map(|(i, v)| wgpu::BindGroupEntry {
                    binding: 6 + i as u32,
                    resource: wgpu::BindingResource::TextureView(v),
                }),
        )
        .collect::<Vec<_>>(),
    });
    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("lod-uniform"),
        contents: bytemuck::bytes_of(&uniform),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let group1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lod-g1"),
        layout: pipeline.uniform_bind_group_layout(),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buf.as_entire_binding(),
        }],
    });

    let bytes_per_row_raw = W * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bpr = bytes_per_row_raw.div_ceil(align) * align;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lod-readback"),
        size: (padded_bpr * H) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let mock_scene = crate::hle::Scene {
        draw_runs: vec![crate::hle::DrawRun {
            fog_color: [0; 4],
            material_index: 0,
            render_mode_index: 0,
            cull: crate::hle::CullKind::None,
            index_count: indices.len() as u32,
            index_start: 0,
        }],
        render_modes: vec![Default::default()],
        ..Default::default()
    };
    pipeline.draw(
        &mut encoder,
        &view,
        &pos_buf,
        &ibuf,
        &mock_scene,
        wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
        &[&group0],
        &group1,
        0,
        None,
    );
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
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
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
    let off = (32 * padded_bpr + 32 * 4) as usize; // center pixel (32,32) of a 64x64 target
    let out = [data[off], data[off + 1], data[off + 2], data[off + 3]];
    drop(data);
    readback.unmap();
    out
}

/// DECISIVE TRILINEAR test. A 2-level mip chain (level0 = RED, level1 = GREEN, 8x8/4x4), combiner
/// `color = (TEXEL1 - TEXEL0) * LOD_FRACTION + TEXEL0` (cyc1: a=TEXEL1 b=TEXEL0 c=LOD_FRACTION
/// d=TEXEL0), rendered at three UV setups landing on distinct points of the `compute_lod`
/// curve (`combiner_prelude.wgsl::compute_lod`): magnification (zero derivatives -> the
/// zero-derivative guard -> level0 only, RED), heavy minification (maxDst saturates tileBase past
/// tileMax -> level1 only, GREEN), and mid-scale (maxDst=1.5 -> lod_fraction=0.5 -> exact
/// RED/GREEN midpoint). See the per-case comments below for the hand-computed maxDst/lod_fraction.
#[test]
fn trilinear_lod_magnify_minify_and_blend_match_hand_computed_pixel() {
    let (device, queue, _dual_source) = headless_device();
    let (combine_l, combine_h) = pack_cyc1_combine(2, 1, 13, 1); // a=TEXEL1 b=TEXEL0 c=LOD_FRAC d=TEXEL0
    let levels: [[u8; 4]; 2] = [[255, 0, 0, 255], [0, 255, 0, 255]]; // level0=RED, level1=GREEN

    // MAGNIFICATION: constant UV -> zero derivatives -> level0 only.
    let magnify = render_lod_center(
        &device,
        &queue,
        combine_l,
        combine_h,
        &levels,
        8,
        8,
        None,
        0.0,
        0.0,
        0.0,
        [[2.0, 2.0]; 4],
    );
    assert!(
        magnify[0] > 240 && magnify[1] < 15 && magnify[2] < 15,
        "magnification must sample level0 (RED) only; got {magnify:?}"
    );

    // HEAVY MINIFICATION: maxDst = 6400/64 = 100 -> saturates to the coarsest level (GREEN).
    let minify = render_lod_center(
        &device,
        &queue,
        combine_l,
        combine_h,
        &levels,
        8,
        8,
        None,
        0.0,
        0.0,
        0.0,
        [[0.0, 0.0], [6400.0, 0.0], [6400.0, 0.0], [0.0, 0.0]],
    );
    assert!(
        minify[0] < 15 && minify[1] > 240 && minify[2] < 15,
        "heavy minification must saturate to the coarsest level (GREEN); got {minify:?}"
    );

    // MID-SCALE: maxDst = 96/64 = 1.5 -> lod_fraction = 0.5 -> exact RED/GREEN midpoint.
    let mid = render_lod_center(
        &device,
        &queue,
        combine_l,
        combine_h,
        &levels,
        8,
        8,
        None,
        0.0,
        0.0,
        0.0,
        [[0.0, 0.0], [96.0, 0.0], [96.0, 0.0], [0.0, 0.0]],
    );
    let expect = [128u8, 128, 0, 255];
    for c in 0..3 {
        let d = (mid[c] as i32 - expect[c] as i32).abs();
        assert!(
            d <= 3,
            "mid-scale blend channel {c}: expected ~{} got {} (full pixel {mid:?})",
            expect[c],
            mid[c],
        );
    }
}

/// DECISIVE NON-HALVING test (the per-level rework). Two SAME-SIZE levels (both 32×32),
/// level0 = RED, level1 = GREEN — sm64 Castle Inside's non-halving TRILERP case. This is the EXACT
/// input the OLD single mip-CHAIN gate rejected (a wgpu mip level k must be base>>k, so a same-size
/// level 1 forced `lod = false` → the material rendered non-LOD RED). The per-level rework binds each
/// level as its OWN texture and the shader switch-selects between them, so LOD now ENGAGES: rendered
/// at mid-LOD (maxDst = 96/64 = 1.5 → lod_fraction = 0.5), the `(TEXEL1 - TEXEL0)*LOD_FRACTION +
/// TEXEL0` combine must produce the exact RED/GREEN midpoint (~128,128,0), NOT the RED-only fallback.
#[test]
fn nonhalving_lod_blends_two_same_size_levels_at_mid_lod() {
    let (device, queue, _dual_source) = headless_device();
    let (combine_l, combine_h) = pack_cyc1_combine(2, 1, 13, 1); // a=TEXEL1 b=TEXEL0 c=LOD_FRAC d=TEXEL0
    let levels: [[u8; 4]; 2] = [[255, 0, 0, 255], [0, 255, 0, 255]]; // level0=RED, level1=GREEN

    // base 32×32 with TWO 32×32 levels (non-halving). maxDst = 96/64 = 1.5 → lod_fraction = 0.5.
    let mid = render_lod_center(
        &device,
        &queue,
        combine_l,
        combine_h,
        &levels,
        32,
        32,
        None,
        0.0,
        0.0,
        0.0,
        [[0.0, 0.0], [96.0, 0.0], [96.0, 0.0], [0.0, 0.0]],
    );
    let expect = [128u8, 128, 0, 255];
    for c in 0..3 {
        let d = (mid[c] as i32 - expect[c] as i32).abs();
        assert!(
            d <= 3,
            "non-halving mid-LOD blend channel {c}: expected ~{} got {} (full pixel {mid:?}) — a \
             RED-only result would mean LOD did not engage",
            expect[c],
            mid[c],
        );
    }
    // Guard the failure mode explicitly: a non-engaged (fallback) render would be pure RED.
    assert!(
        mid[1] > 100,
        "level 1 (GREEN) must contribute — a pure-RED pixel means the non-halving set fell back to \
         non-LOD; got {mid:?}"
    );
}

/// MAGNIFY/MINIFY level selection with NON-HALVING (same-size 32×32) levels: proves the per-level
/// switch selects level0 at magnification and the coarsest level at heavy minification even when the
/// levels do not halve (the case the mip-chain path could not represent at all).
#[test]
fn nonhalving_lod_magnify_minify_select_level0_and_coarsest() {
    let (device, queue, _dual_source) = headless_device();
    let (combine_l, combine_h) = pack_cyc1_combine(2, 1, 13, 1);
    let levels: [[u8; 4]; 2] = [[255, 0, 0, 255], [0, 255, 0, 255]]; // level0=RED, level1=GREEN

    // Magnification (constant UV → zero derivatives) → level0 only (RED).
    let magnify = render_lod_center(
        &device,
        &queue,
        combine_l,
        combine_h,
        &levels,
        32,
        32,
        None,
        0.0,
        0.0,
        0.0,
        [[2.0, 2.0]; 4],
    );
    assert!(
        magnify[0] > 240 && magnify[1] < 15 && magnify[2] < 15,
        "non-halving magnification must select level0 (RED); got {magnify:?}"
    );

    // Heavy minification → saturates to the coarsest level (GREEN).
    let minify = render_lod_center(
        &device,
        &queue,
        combine_l,
        combine_h,
        &levels,
        32,
        32,
        None,
        0.0,
        0.0,
        0.0,
        [[0.0, 0.0], [6400.0, 0.0], [6400.0, 0.0], [0.0, 0.0]],
    );
    assert!(
        minify[0] < 15 && minify[1] > 240 && minify[2] < 15,
        "non-halving heavy minification must saturate to the coarsest level (GREEN); got {minify:?}"
    );
}

/// DECISIVE DETAIL test. A 1-level base texture (BLUE) plus a distinct DETAIL tile (YELLOW),
/// DETAIL mode (`detail_mode` bit1), rendered at magnification (zero derivatives). Per
/// `compute_lod`'s DETAIL branch, the zero-derivative sentinel drives lod_fraction negative, then
/// DETAIL's `lod_fraction < 0.0 -> lod_fraction = max_dst` resets it to 0 -> level0 = 0, the
/// (shifted-index) DETAIL tap -> pixel = pure DETAIL tap (YELLOW), NOT the base level (BLUE) a
/// plain (non-DETAIL) magnified sample would give.
#[test]
fn detail_mode_samples_detail_tap_at_magnification() {
    let (device, queue, _dual_source) = headless_device();
    let (combine_l, combine_h) = pack_cyc1_combine(2, 1, 13, 1); // (TEXEL1-TEXEL0)*LOD_FRACTION+TEXEL0
    let levels: [[u8; 4]; 1] = [[0, 0, 255, 255]]; // base level0 = BLUE
    let px = render_lod_center(
        &device,
        &queue,
        combine_l,
        combine_h,
        &levels,
        4,
        4,
        Some(([255, 255, 0, 255], 4, 4)), // DETAIL tile = YELLOW
        0.0,
        0.0,
        2.0, // detail_mode bit1 (DETAIL)
        [[2.0, 2.0]; 4],
    );
    assert!(
        px[0] > 240 && px[1] > 240 && px[2] < 15,
        "DETAIL magnification must sample the DETAIL tap (YELLOW), not the base level (BLUE); got {px:?}"
    );
}

/// DECISIVE SHARPEN test. A 2-level mip chain with MUTED colors (level0 = (180,60,60), level1 =
/// (60,180,60) — not at the 0/255 channel extremes, so a negative-lodFraction extrapolation stays
/// visible after the combiner's final `clamp(.,0,1)`), SHARPEN mode (`detail_mode` bit0), rendered
/// at magnification (zero derivatives). Per `compute_lod`'s SHARPEN branch at max_dst == 0,
/// `lod_fraction = max_dst - 1.0 = -1.0` is NOT clamped to >= 0 (unlike plain trilinear), so
/// `(TEXEL1-TEXEL0)*(-1)+TEXEL0 = 2*level0 - level1` extrapolates below level0's own G=60 —
/// something a plain (non-SHARPEN) magnified sample could never produce.
#[test]
fn sharpen_mode_extrapolates_below_plain_level0_at_magnification() {
    let (device, queue, _dual_source) = headless_device();
    let (combine_l, combine_h) = pack_cyc1_combine(2, 1, 13, 1); // (TEXEL1-TEXEL0)*LOD_FRACTION+TEXEL0
    let levels: [[u8; 4]; 2] = [[180, 60, 60, 255], [60, 180, 60, 255]];
    let px = render_lod_center(
        &device,
        &queue,
        combine_l,
        combine_h,
        &levels,
        8,
        8,
        None,
        0.0,
        0.0,
        1.0, // detail_mode bit0 (SHARPEN)
        [[2.0, 2.0]; 4],
    );
    assert!(
        px[0] > 240,
        "SHARPEN R channel must extrapolate toward saturation (2*180-60=300 clamped to 255); got {px:?}"
    );
    assert!(
        px[1] < 15,
        "SHARPEN G channel must extrapolate BELOW plain level0's G=60 (2*60-180=-60 clamped to 0), \
         proving the negative-lodFraction path engaged, not just plain level0; got {px:?}"
    );
}
