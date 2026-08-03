//! Headless golden-image harness for the renderer.
//!
//! Each test renders a test scene offscreen and compares the pixel output to a committed golden
//! (`.bin` raw-RGBA8 file).  Running with `UPDATE_GOLDENS=1` writes a new golden instead of
//! comparing.
//!
//! `render_scene_to_rgba8` is the shared render path: it ports the manual headless flow from
//! `tests/render.rs` (RSP-process compute pass + textured raster pass + `copy_texture_to_buffer`
//! readback).  It does NOT use `SceneRenderer::render` because that path provides no pixel
//! readback API.
//!
//! Golden storage: raw RGBA8 `.bin` files stored in `crates/renderer/goldens/`.  We use `.bin`
//! rather than PNG because the `image` crate is not in the offline lockfile; the comparison is
//! a byte-wise max-channel-diff ≤ `TOL`.

use crate::render::{headless_device, CLEAR_COLOR};
#[cfg(feature = "asm")]
use crate::render::{
    headless_device_forced_fallback, CombinerUniform, RspProcessParams, RspProcessPipeline,
    TexturedPipeline, DEPTH_FORMAT,
};
#[cfg(feature = "asm")]
use wgpu::util::DeviceExt;

use crate::tests::common;

/// Maps an N64 wrap mode (cms/cmt: 0=WRAP, 1=MIRROR, 2+=CLAMP) to a wgpu `AddressMode`.
/// Mirrors `crate::render::address_mode` (private) so golden tests can select the correct sampler.
#[cfg(feature = "asm")]
fn address_mode(wrap: u8) -> wgpu::AddressMode {
    match wrap {
        0 => wgpu::AddressMode::Repeat,
        1 => wgpu::AddressMode::MirrorRepeat,
        _ => wgpu::AddressMode::ClampToEdge,
    }
}

/// Maximum per-channel absolute difference allowed in golden comparisons.
const TOL: u8 = 2;

/// Seed source — mirrors `tests/scenes/textured-quad.n64` but uses a 4×4 texture so the
/// golden file is small (64×64 × 4 bytes = 16 KiB).  No `update {}` block: time-invariant.
#[cfg(feature = "asm")]
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

/// 4×4 RGBA8 seed texture: red/green top half (alpha=255, opaque), blue/yellow bottom half
/// (alpha=0, transparent — the cutout hole for Phase D alpha-test).
///
/// Each colour is strongly non-black and non-white, and survives the RGBA16 5-bit encode/decode
/// round-trip (red→248, green→248, blue→248, yellow→248/248/0).  The checkerboard pattern is
/// visible in the golden image, confirming it is not noise.
///
/// Rows 0-1 have alpha=255 (above the 0.125 CVG_X_ALPHA threshold → opaque texels keep).
/// Rows 2-3 have alpha=0 (below 0.125 → discarded when alpha_mode=CVG_X_ALPHA → background hole).
/// The `golden_rgba16_quad` test uses SHADE for alpha (not TEXEL0.a), so its golden is unchanged.
#[cfg(feature = "asm")]
#[rustfmt::skip]
const RGBA16_QUAD_TEX: &[u8] = &[
    255,   0,   0, 255,   0, 255,   0, 255,   255,   0,   0, 255,   0, 255,   0, 255, // row 0  α=255
    255,   0,   0, 255,   0, 255,   0, 255,   255,   0,   0, 255,   0, 255,   0, 255, // row 1  α=255
      0,   0, 255,   0, 255, 255,   0,   0,     0,   0, 255,   0, 255, 255,   0,   0, // row 2  α=0 (hole)
      0,   0, 255,   0, 255, 255,   0,   0,     0,   0, 255,   0, 255, 255,   0,   0, // row 3  α=0 (hole)
];

// ── helpers ──────────────────────────────────────────────────────────────────────────────────────

/// Inner render path: takes an already-created `(device, queue)` pair and runs the full
/// assemble → HLE → RSP-compute → textured-raster → readback pipeline.
///
/// Factored out so both the primary (`headless_device`) and forced-fallback
/// (`headless_device_forced_fallback`) paths can share the rendering code.
/// `TexturedPipeline::new` reads `DUAL_SOURCE_BLENDING` from the device features to select
/// between the dual-source primary blend and the B3 AlphaOver/Replace fallback pipelines.
#[cfg(feature = "asm")]
#[allow(clippy::too_many_arguments)] // test helper: all 8 params are logically distinct
fn render_scene_with_device(
    src: &str,
    tex_native: &[u8],
    w: u32,
    h: u32,
    addr_u: wgpu::AddressMode,
    addr_v: wgpu::AddressMode,
    device: wgpu::Device,
    queue: wgpu::Queue,
) -> Vec<u8> {
    // --- Step 1: infer square texture dimensions from the RGBA8 byte count. ---
    assert!(
        tex_native.len().is_multiple_of(4),
        "tex_native must be RGBA8 (length must be a multiple of 4)"
    );
    let pixel_count = tex_native.len() / 4;
    let tex_side = (pixel_count as f64).sqrt() as u32;
    assert_eq!(
        (tex_side * tex_side) as usize,
        pixel_count,
        "tex_native must be a square RGBA8 texture for this seed implementation"
    );

    // --- Step 2: assemble source → flat RDRAM image. ---
    let img = crate::asm::assemble_with_texture(src, tex_native, tex_side, tex_side)
        .unwrap_or_else(|d| panic!("assembly failed: {d:?}"));

    // --- Step 3: HLE interpret → Scene. ---
    let interp = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
    assert!(interp.diags.is_empty(), "HLE diags: {:?}", interp.diags);
    let scene = &interp.scene;
    let format = wgpu::TextureFormat::Rgba8Unorm;

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("golden-color"),
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
        label: Some("golden-depth"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        // TEXTURE_BINDING so the decal pass can sample the depth pass 1 wrote (E2).
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
    // A second view over the same depth texture, bound at `@group(2)` as `texture_depth_2d` in
    // the decal pass (used only when the scene has ZMODE_DEC runs).
    let depth_sample_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

    // bytes_per_row must be padded to wgpu::COPY_BYTES_PER_ROW_ALIGNMENT (256).
    // For w=64: raw=256, already aligned.  For other widths, ceil-divide to the next multiple.
    let bytes_per_row_raw = w * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let bytes_per_row = bytes_per_row_raw.div_ceil(align) * align;

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("golden-readback"),
        size: (bytes_per_row * h) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // --- Step 5: early-out for empty scenes (clear only). ---
    if scene.draw_runs.is_empty() || scene.raw_pos.is_empty() || scene.indices.is_empty() {
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("golden-clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR_COLOR),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
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
        return finish_readback(readback, &device, bytes_per_row, w, h);
    }

    // --- Step 6: per-material GPU textures + @group(0) bind groups (pooled path). ---
    // Each material gets its own texture upload and bind group.  The passed `addr_u`/`addr_v`
    // are used as the sampler address mode for all materials (the golden tests are all
    // single-material, so this is byte-identical to the old A8a single-material path).
    let pipeline = TexturedPipeline::new(&device, format, DEPTH_FORMAT);
    let mut material_bgs: Vec<wgpu::BindGroup> = Vec::with_capacity(scene.materials.len());
    for mat in &scene.materials {
        let tex_size = wgpu::Extent3d {
            width: mat.tex_w,
            height: mat.tex_h,
            depth_or_array_layers: 1,
        };
        let gpu_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("golden-tex"),
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
                texture: &gpu_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &mat.texture,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(mat.tex_w * 4),
                rows_per_image: Some(mat.tex_h),
            },
            tex_size,
        );
        let tex_view = gpu_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("golden-sampler"),
            address_mode_u: addr_u,
            address_mode_v: addr_v,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("golden-bg-g0"),
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
                // TEXEL1 slot: these single-texture goldens never sample it — reuse tex0's
                // view/sampler to satisfy the group(0) layout (tex_enable1 = 0). Output is unchanged.
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                // DETAIL slot (4/5): never sampled by these goldens — reuse tex0's view/sampler.
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ]
            // LOD-level slots (6..=12): never sampled by these non-LOD goldens — reuse tex0's view.
            .into_iter()
            .chain((6..13u32).map(|b| wgpu::BindGroupEntry {
                binding: b,
                resource: wgpu::BindingResource::TextureView(&tex_view),
            }))
            .collect::<Vec<_>>(),
        });
        material_bgs.push(bg);
    }
    let material_bg_refs: Vec<&wgpu::BindGroup> = material_bgs.iter().collect();

    // --- Step 7: pooled uniform buffer (N_runs × 256 bytes) + @group(1) bind group. ---
    // Each run's CombinerUniform sits at byte offset [i*256 .. i*256+48].  The BufferBinding
    // with explicit `size` (not as_entire_binding) keeps the binding within WebGL2's 16 KiB
    // max_uniform_buffer_binding_size for large run counts [MIN11].
    let n_runs = scene.draw_runs.len();
    let mut pool = vec![0u8; n_runs * 256];
    for (i, run) in scene.draw_runs.iter().enumerate() {
        let mat = &scene.materials[run.material_index as usize];
        let rm = &scene.render_modes[run.render_mode_index as usize];
        // C3: normalize scene.fog_color [u8;4] → [f32;4] for the combiner uniform.
        let fc = scene.fog_color;
        let fog_color = [
            fc[0] as f32 / 255.0,
            fc[1] as f32 / 255.0,
            fc[2] as f32 / 255.0,
            fc[3] as f32 / 255.0,
        ];
        let mut combiner = CombinerUniform::from_run(mat, rm, fog_color);
        // Texcoord table is TEXEL-space: normalize by draw-time tile dims in the fragment.
        combiner.inv_tex_size = crate::render::triangle_inv_tex_size(scene, mat, run);
        let slot = bytemuck::bytes_of(&combiner);
        pool[i * 256..i * 256 + slot.len()].copy_from_slice(slot);
    }
    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("golden-uniform-pool"),
        contents: &pool,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let group1_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("golden-bg-g1"),
        layout: pipeline.uniform_bind_group_layout(),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &uniform_buf,
                offset: 0,
                size: wgpu::BufferSize::new(std::mem::size_of::<CombinerUniform>() as u64),
            }),
        }],
    });

    // --- Step 8: RSP-process compute pass (transform vertices). ---
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

    // Output buffer: STORAGE for compute write, VERTEX for the raster draw.
    let dst = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("golden-dst-verts"),
        size: (n as u64) * 48,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
        mapped_at_creation: false,
    });

    let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("golden-rsp-params"),
        contents: bytemuck::bytes_of(&RspProcessParams {
            vertex_count: n,
            fog_enable: u32::from(scene.fog_enable),
            fog_mul: scene.fog_mul as f32,
            fog_offset: scene.fog_offset as f32,
        }),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let rsp_pipe = RspProcessPipeline::new(&device);
    let rsp_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("golden-rsp-bg"),
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
        ],
    });

    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("golden-ibuf"),
        contents: bytemuck::cast_slice(&scene.indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    // --- Step 9: encode compute → raster → copy. ---
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

    rsp_pipe.dispatch(&mut encoder, &rsp_bg, n);

    // E2: a scene with ZMODE_DEC runs takes the two-phase decal path (pass 1 writes depth, pass 2
    // samples it for the in-shader occlusion/coplanar test). Scenes without decals take the exact
    // single-pass `draw` path (byte-identical to before — guards the existing goldens).
    let has_decal = scene.draw_runs.iter().any(|run| {
        scene.render_modes[run.render_mode_index as usize].z_mode == crate::hle::ZMode::Decal
    });
    if has_decal {
        let depth_sample_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("golden-decal-depth-sample-bg"),
            layout: pipeline.depth_bind_group_layout(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&depth_sample_view),
            }],
        });
        pipeline.draw_with_decals(
            &mut encoder,
            &view,
            &dst,
            &ibuf,
            scene,
            CLEAR_COLOR,
            &material_bg_refs,
            &group1_bg,
            256,
            &depth_view,
            &depth_sample_bg,
        );
    } else {
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
            &material_bg_refs,
            &group1_bg,
            256,
            depth,
        );
    }

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

    finish_readback(readback, &device, bytes_per_row, w, h)
}

/// Render a `.n64` test scene source using the primary headless device (dual-source when available)
/// and explicit sampler address modes.
///
/// Thin wrapper around `render_scene_with_device` that creates the primary `(device, queue)`.
/// Called by `render_scene_to_rgba8` (ClampToEdge) and the wrap/mirror golden tests.
#[cfg(feature = "asm")]
fn render_scene_to_rgba8_addr(
    src: &str,
    tex_native: &[u8],
    w: u32,
    h: u32,
    addr_u: wgpu::AddressMode,
    addr_v: wgpu::AddressMode,
) -> Vec<u8> {
    let (device, queue, _dual_source) = headless_device();
    render_scene_with_device(src, tex_native, w, h, addr_u, addr_v, device, queue)
}

/// Render a `.n64` test scene source through the **forced-fallback device** (dual-source disabled),
/// exercising the B3 AlphaOver/Replace pipelines deterministically.
///
/// Uses `headless_device_forced_fallback()` which requests `Features::empty()` even when the
/// adapter supports `DUAL_SOURCE_BLENDING`.  `TexturedPipeline::new` then builds only the
/// AlphaOver/Replace fallback pipelines — the dual-source WGSL module is never compiled.
///
/// [IMP13] Called for every AlphaOver-expressible scene each CI run so the fallback module +
/// pipelines are compiled and rendered (a web-only fallback break cannot ship green).
#[cfg(feature = "asm")]
fn render_scene_to_rgba8_forced_fallback(src: &str, tex_native: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (device, queue) = headless_device_forced_fallback();
    render_scene_with_device(
        src,
        tex_native,
        w,
        h,
        wgpu::AddressMode::ClampToEdge,
        wgpu::AddressMode::ClampToEdge,
        device,
        queue,
    )
}

/// Render a `.n64` test scene source to an RGBA8 pixel buffer with a ClampToEdge sampler.
///
/// Thin wrapper around `render_scene_to_rgba8_addr` that fixes `addr_u = addr_v = ClampToEdge`
/// so all existing golden tests are byte-identical regardless of the material's wrap_s/wrap_t.
///
/// `tex_native` = RGBA8 source bytes.  For RGBA16 scenes the assembler encodes RGBA8→RGBA16
/// internally; other formats (I8/I4/IA/CI) are embedded by the assembler's own encoder.
/// `w`, `h` = render-target dimensions (pixels).  Returns `w × h × 4` raw RGBA8 bytes,
/// row-major, no row padding.
///
/// Ported from the manual headless flow in `tests/render.rs`.  Does NOT use
/// `SceneRenderer::render` (that path provides no pixel readback).
#[cfg(feature = "asm")]
fn render_scene_to_rgba8(src: &str, tex_native: &[u8], w: u32, h: u32) -> Vec<u8> {
    render_scene_to_rgba8_addr(
        src,
        tex_native,
        w,
        h,
        wgpu::AddressMode::ClampToEdge,
        wgpu::AddressMode::ClampToEdge,
    )
}

/// Map the readback buffer, strip row padding, return unpacked RGBA8.
#[cfg(feature = "asm")]
fn finish_readback(
    readback: wgpu::Buffer,
    device: &wgpu::Device,
    bytes_per_row: u32,
    w: u32,
    h: u32,
) -> Vec<u8> {
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

/// Compare `actual` RGBA8 pixels against the committed golden `.bin` file, or write the golden
/// when `UPDATE_GOLDENS=1` is set.
///
/// Goldens live in `crates/renderer/goldens/<name>.bin` (raw RGBA8, `w × h × 4` bytes).
/// The comparison tolerates a max per-channel absolute difference of `TOL` to absorb
/// platform-specific rounding in GPU rasterisation.
fn compare_or_write(name: &str, actual: &[u8], w: u32, h: u32) {
    let path = format!("{}/goldens/{name}.bin", env!("CARGO_MANIFEST_DIR"));
    if std::env::var("UPDATE_GOLDENS").is_ok() {
        std::fs::write(&path, actual)
            .unwrap_or_else(|e| panic!("failed to write golden {path}: {e}"));
        eprintln!("golden written: {path} ({} bytes, {w}×{h})", actual.len());
        return;
    }
    let golden = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "golden '{name}' missing ({path}): {e}\n\
             Run `UPDATE_GOLDENS=1 cargo test -p renderer golden_{name}` to generate it."
        )
    });
    assert_eq!(
        golden.len(),
        actual.len(),
        "{name}: golden size {} ≠ actual size {} (expected {w}×{h}×4={})",
        golden.len(),
        actual.len(),
        w * h * 4
    );
    let max = actual
        .iter()
        .zip(golden.iter())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    assert!(max <= TOL, "{name}: max per-channel diff {max} > {TOL}");
}

// ── Tier-1 texture format golden seeds ───────────────────────────────────────────────────────────

/// I8-format 4×4 vertical ramp: `.n64` source with `G_IM_FMT_I / G_IM_SIZ_8b`.
///
/// Same quad geometry as `RGBA16_QUAD_SRC` but declares `Texture tex = { 4, 4, I8 }` and
/// uses `G_IM_SIZ_8b`. The assembler calls `encode_i8_texel` (luminance = (R+G+B)/3);
/// the HLE dispatcher routes tile (fmt=4,siz=1) to `decode_i8`.
#[cfg(feature = "asm")]
const I8_RAMP_SRC: &str = r#"
Texture tex = { 4, 4, I8 }
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
gsDPLoadTextureBlock(tex, G_IM_FMT_I, G_IM_SIZ_8b, 4, 4)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

/// I4-format 4×4 vertical ramp: `.n64` source with `G_IM_FMT_I / G_IM_SIZ_4b`.
///
/// Identical geometry to `I8_RAMP_SRC` but declares `I4` and uses `G_IM_SIZ_4b`.
/// The assembler packs 2 texels per byte (high nibble = even column);
/// the HLE dispatcher routes tile (fmt=4,siz=0) to `decode_i4`.
#[cfg(feature = "asm")]
const I4_RAMP_SRC: &str = r#"
Texture tex = { 4, 4, I4 }
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
gsDPLoadTextureBlock(tex, G_IM_FMT_I, G_IM_SIZ_4b, 4, 4)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

/// 4×4 RGBA8 vertical intensity ramp: rows 0–3 have uniform gray 0, 85, 170, 255.
///
/// Row-distinct so an odd-row word swap (§8 linear-TMEM bet) would appear as visible band
/// reordering in the golden. Values 0, 85 (=0x55), 170 (=0xAA), 255 survive both the I8 and
/// I4 round-trips exactly (85>>4=5, (5<<4)|5=85; 170>>4=10, (10<<4)|10=170).
#[cfg(feature = "asm")]
#[rustfmt::skip]
const RAMP_TEX: &[u8] = &[
      0,   0,   0, 255,   0,   0,   0, 255,   0,   0,   0, 255,   0,   0,   0, 255, // row 0: black
     85,  85,  85, 255,  85,  85,  85, 255,  85,  85,  85, 255,  85,  85,  85, 255, // row 1: dark gray
    170, 170, 170, 255, 170, 170, 170, 255, 170, 170, 170, 255, 170, 170, 170, 255, // row 2: light gray
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // row 3: white
];

/// Generate CI8 ramp texture: 32×32 RGBA8, row-distinct grayscale with alternating alpha.
///
/// Row i has lum = i*8 (0..248) with alpha=255 for even rows (a1=1 in RGBA16 palette)
/// and alpha=0 for odd rows (a1=0).  Serves two canary roles:
/// 1. Swizzle canary: row-distinct flat regions make a TMEM row-swap immediately visible.
/// 2. Alpha canary (ci8-canary): alternating a1 means TEXEL0_ALPHA→color renders alternating
///    white/black bands — not solid white, so it catches the "palette all-opaque" false pass.
///
#[cfg(feature = "asm")]
const fn gen_ci8_tex() -> [u8; 32 * 32 * 4] {
    let mut data = [0u8; 32 * 32 * 4];
    let mut row = 0usize;
    while row < 32 {
        let lum = (row * 8) as u8;
        let alpha: u8 = if row.is_multiple_of(2) { 255 } else { 0 };
        let mut col = 0usize;
        while col < 32 {
            let base = (row * 32 + col) * 4;
            data[base] = lum;
            data[base + 1] = lum;
            data[base + 2] = lum;
            data[base + 3] = alpha;
            col += 1;
        }
        row += 1;
    }
    data
}

#[cfg(feature = "asm")]
const CI8_TEX_ARRAY: [u8; 32 * 32 * 4] = gen_ci8_tex();

/// CI8 ramp + canary source texture: 32×32 RGBA8, row-distinct grayscale with alternating alpha.
#[cfg(feature = "asm")]
const CI8_TEX: &[u8] = &CI8_TEX_ARRAY;

/// CI8 ramp: MODULATE combiner (TEXEL0 × SHADE), white shade, G_TT_RGBA16.
#[cfg(feature = "asm")]
const CI8_RAMP_SRC: &str = r#"
Texture tex = { 32, 32, CI8 }
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
gsDPSetOtherMode_H(14, 2, 0x8000)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_CI, G_IM_SIZ_8b, 32, 32)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

/// CI8 combine-route canary: routes TEXEL0_ALPHA (cc1=8) into RGB output.
/// c1=(ONE−0)×TEXEL0_ALPHA+0=texel.alpha; palette alternating a1 → alternating white/black.
#[cfg(feature = "asm")]
const CI8_CANARY_SRC: &str = r#"
Texture tex = { 32, 32, CI8 }
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
gsDPSetOtherMode_H(14, 2, 0x8000)
gsDPSetCombineLERP(0, 0, 0, 0, 0, 0, 0, 0, ONE, 0, 8, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_CI, G_IM_SIZ_8b, 32, 32)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

// ── tests ─────────────────────────────────────────────────────────────────────────────────────────

/// Golden test for the RGBA16 textured quad — the seed of the Tier-1 texture format harness.
///
/// Renders a 4×4 RGBA8 checkerboard (red/green/blue/yellow quadrants) through the full
/// assemble → HLE → RSP-compute → textured-raster pipeline at 64×64, and compares the output
/// to the committed golden.
///
/// The MODULATE combiner (TEXEL0 × SHADE, white shade) passes the texture through unchanged.
/// After the RGBA16 5-bit encode/decode round-trip the red channel rounds to 248 and the green
/// channel rounds to 248, so the rendered quad shows four clearly distinct colour regions —
/// not noise, not a blank clear colour.
#[cfg(feature = "asm")]
#[test]
fn golden_rgba16_quad() {
    let px = render_scene_to_rgba8(RGBA16_QUAD_SRC, RGBA16_QUAD_TEX, 64, 64);
    compare_or_write("rgba16-quad", &px, 64, 64);
}

/// Golden test for I8 intensity format — vertical ramp (black → white, top to bottom).
///
/// The MODULATE combiner with white shade passes the texture through; each of the 4 rows
/// should render as a clearly distinct horizontal band (black / dark-gray / light-gray / white).
/// A clean gradient confirms both the assembler's I8 encoder and the HLE I8 decoder are correct.
/// A row-scrambled output (bands out of order) would indicate the linear-TMEM bet failed.
#[cfg(feature = "asm")]
#[test]
fn golden_i8_ramp() {
    let px = render_scene_to_rgba8(I8_RAMP_SRC, RAMP_TEX, 64, 64);
    compare_or_write("i8-ramp", &px, 64, 64);
}

/// Golden test for I4 intensity format — same vertical ramp through 4-bit encode/decode.
///
/// I4 quantises to 4 bits (>> 4) then replicates ((v4 << 4) | v4). The seed values 0, 85,
/// 170, 255 are all exact I4 round-trips, so the I4 golden should be byte-identical to I8.
/// Row-scramble or banding differences indicate a nibble-order or TMEM layout bug.
#[cfg(feature = "asm")]
#[test]
fn golden_i4_ramp() {
    let px = render_scene_to_rgba8(I4_RAMP_SRC, RAMP_TEX, 64, 64);
    compare_or_write("i4-ramp", &px, 64, 64);
}

/// IA16-format 4×4 vertical ramp: same geometry, declares `IA16` / `G_IM_SIZ_16b`.
#[cfg(feature = "asm")]
const IA16_RAMP_SRC: &str = r#"
Texture tex = { 4, 4, IA16 }
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
gsDPLoadTextureBlock(tex, G_IM_FMT_IA, G_IM_SIZ_16b, 4, 4)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

/// IA8-format 4×4 vertical ramp.
#[cfg(feature = "asm")]
const IA8_RAMP_SRC: &str = r#"
Texture tex = { 4, 4, IA8 }
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
gsDPLoadTextureBlock(tex, G_IM_FMT_IA, G_IM_SIZ_8b, 4, 4)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

/// IA4-format 4×4 vertical ramp.
#[cfg(feature = "asm")]
const IA4_RAMP_SRC: &str = r#"
Texture tex = { 4, 4, IA4 }
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
gsDPLoadTextureBlock(tex, G_IM_FMT_IA, G_IM_SIZ_4b, 4, 4)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

/// Golden test for IA16 intensity+alpha format — vertical ramp (black→white).
///
/// Alpha validation: the combiner uses SHADE alpha (vertex alpha=255), so texel alpha does NOT
/// reach the framebuffer. The golden validates the INTENSITY/color channel only.
/// Alpha decode correctness is validated by unit tests (`ia16_splits_intensity_and_alpha`, etc.).
/// The swizzle bet: IA16 is a 2-byte format (siz=2); the linear-TMEM decoder iterates two bytes
/// per texel, so nibble-order is irrelevant — no swizzle risk.
#[cfg(feature = "asm")]
#[test]
fn golden_ia16_ramp() {
    let px = render_scene_to_rgba8(IA16_RAMP_SRC, RAMP_TEX, 64, 64);
    compare_or_write("ia16-ramp", &px, 64, 64);
}

/// Golden test for IA8 intensity+alpha format.
///
/// Alpha validation: same as IA16 — combiner outputs SHADE alpha, not texel alpha.
/// Unit tests cover alpha decode. Swizzle: IA8 is 1-byte/texel, no nibble order.
#[cfg(feature = "asm")]
#[test]
fn golden_ia8_ramp() {
    let px = render_scene_to_rgba8(IA8_RAMP_SRC, RAMP_TEX, 64, 64);
    compare_or_write("ia8-ramp", &px, 64, 64);
}

/// Golden test for IA4 intensity+alpha format — 4-bit format, 2 texels/byte.
///
/// Alpha validation: combiner outputs SHADE alpha; texel alpha validated by unit tests.
/// Swizzle: IA4 uses the same nibble order as I4 (even col = high nibble). The 2×2 multi-row
/// unit test (`ia4_multirow_swizzle_canary`) is the primary swizzle check; the golden confirms
/// the intensity bands are visible and row-distinct.
#[cfg(feature = "asm")]
#[test]
fn golden_ia4_ramp() {
    let px = render_scene_to_rgba8(IA4_RAMP_SRC, RAMP_TEX, 64, 64);
    compare_or_write("ia4-ramp", &px, 64, 64);
}

// ── Wrap / mirror sampler golden seeds ───────────────────────────────────────────────────────────

/// 4×4 RGBA8 texture with a 2×2 color-block layout: red/green top half, blue/yellow bottom half,
/// each color occupying a 2×2 block.  Used for the wrap / mirror golden tests.
///
/// The 2×2 block size means that with UVs spanning [0,2] and WRAP mode the texture tiles 2×2
/// — producing clearly distinct quadrants in the rendered image.  With MIRROR the right and
/// bottom halves are reflected.  With CLAMP the outer portion of the quad is filled by the
/// clamped edge colour (yellow), making the three modes visually distinguishable.
#[cfg(feature = "asm")]
#[rustfmt::skip]
const WRAP_TEX: &[u8] = &[
    255,   0,   0, 255,   255,   0,   0, 255,     0, 255,   0, 255,     0, 255,   0, 255, // row 0: R R G G
    255,   0,   0, 255,   255,   0,   0, 255,     0, 255,   0, 255,     0, 255,   0, 255, // row 1: R R G G
      0,   0, 255, 255,     0,   0, 255, 255,   255, 255,   0, 255,   255, 255,   0, 255, // row 2: B B Y Y
      0,   0, 255, 255,     0,   0, 255, 255,   255, 255,   0, 255,   255, 255,   0, 255, // row 3: B B Y Y
];

/// WRAP-mode quad: 4×4 texture with UVs spanning [0,2] (S/T = 0..256 on a 4×4 tile),
/// `cms=cmt=0` (G_TX_WRAP).  With a Repeat sampler the texture tiles 2×2.
///
/// Vertex S=256 maps to UV ≈ 2.0 via `scale_s = sc/(TC_DIVISOR*tile_w) = 65535/(65536·32·4)`.
/// `gsDPLoadTextureBlock(…, 4, 4, 0, 0, 0, 0)` sets tile 0 with cms=cmt=0 (WRAP, no mask).
#[cfg(feature = "asm")]
const WRAP_REPEAT_SRC: &str = r#"
Texture tex = { 4, 4, RGBA16 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,   0,   0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 256,   0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 256, 256, 255,255,255,255 }
Vtx { -48,  48, 0, 0,   0, 256, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_RGBA, G_IM_SIZ_16b, 4, 4, 0, 0, 0, 0)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

/// MIRROR-mode quad: identical geometry and texture to `WRAP_REPEAT_SRC`, but `cms=cmt=1`
/// (G_TX_MIRROR).  With a MirrorRepeat sampler the second tile is the reflection of the first.
#[cfg(feature = "asm")]
const MIRROR_REPEAT_SRC: &str = r#"
Texture tex = { 4, 4, RGBA16 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,   0,   0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 256,   0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 256, 256, 255,255,255,255 }
Vtx { -48,  48, 0, 0,   0, 256, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_RGBA, G_IM_SIZ_16b, 4, 4, 1, 0, 1, 0)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

/// Golden test for WRAP (Repeat) sampler — 4×4 two-colour-block texture tiled 2×2.
///
/// The quad UVs span [0,2] (S/T vertices at 256 on a 4-texel tile with sc=0xFFFF).
/// `cms=cmt=0` routes to `samplers[0][0]` (wgpu Repeat).  With Repeat the rendered image shows
/// four quadrants of the texture (R/G/B/Y repeated 2×2), visibly different from a ClampToEdge
/// render that would fill the outer ~50 % of the quad with the clamped edge colour.
#[cfg(feature = "asm")]
#[test]
fn golden_wrap_repeat() {
    let px = render_scene_to_rgba8_addr(
        WRAP_REPEAT_SRC,
        WRAP_TEX,
        64,
        64,
        address_mode(0),
        address_mode(0),
    );
    compare_or_write("wrap-repeat", &px, 64, 64);
}

/// Golden test for MIRROR (MirrorRepeat) sampler — same geometry, `cms=cmt=1`.
///
/// With MirrorRepeat the second tile (UV 1..2) is the horizontal/vertical reflection of the
/// first, so the rendered image shows R|G||G|R (top) and B|Y||Y|B (bottom) — distinct from
/// both WRAP (which repeats R|G||R|G) and CLAMP.
#[cfg(feature = "asm")]
#[test]
fn golden_mirror_repeat() {
    let px = render_scene_to_rgba8_addr(
        MIRROR_REPEAT_SRC,
        WRAP_TEX,
        64,
        64,
        address_mode(1),
        address_mode(1),
    );
    compare_or_write("mirror-repeat", &px, 64, 64);
}

/// Golden test for CI8 color-indexed format — vertical ramp via RGBA16 palette.
///
/// The MODULATE combiner with white shade passes through the palette colors; each of the 32
/// rows should render as a distinct horizontal band of grayscale. Clean gradient confirms both
/// the CI8 encoder, TLUT loading, and palette decode are correct.
/// Row-distinct flat regions also serve as the swizzle canary: a TMEM row-swap would produce
/// bands out of order, immediately visible.
#[cfg(feature = "asm")]
#[test]
fn golden_ci8_ramp() {
    let px = render_scene_to_rgba8(CI8_RAMP_SRC, CI8_TEX, 64, 64);
    compare_or_write("ci8-ramp", &px, 64, 64);
}

/// Golden test for CI8 combine-route canary — TEXEL0_ALPHA routed into RGB output.
///
/// Combiner: (ONE−0)×TEXEL0_ALPHA+0 = texel.alpha as grayscale.
/// CI8_TEX palette has alternating a1 (0/1): even-row entries → alpha=255 (white output),
/// odd-row entries → alpha=0 (black output). The golden MUST show alternating bands,
/// NOT solid white. Solid white means tex_enable is false (TEXEL0_ALPHA not wired) — a
/// false pass matching the broken IA state; do not bake if white.
#[cfg(feature = "asm")]
#[test]
fn golden_ci8_canary() {
    let px = render_scene_to_rgba8(CI8_CANARY_SRC, CI8_TEX, 64, 64);
    // Verify the canary output is NOT solid white (which would indicate tex_enable=false bug).
    let max_r = px.chunks(4).map(|p| p[0]).max().unwrap_or(0);
    let min_r = px.chunks(4).map(|p| p[0]).min().unwrap_or(255);
    assert!(
        max_r > 200 && min_r < 50,
        "ci8-canary rendered solid color (max_r={max_r}, min_r={min_r}); \
         expected alternating white/black bands (TEXEL0_ALPHA path broken?)"
    );
    compare_or_write("ci8-canary", &px, 64, 64);
}

// ── CI4 golden seeds ─────────────────────────────────────────────────────────────────────────────

/// Generate CI4 grid+canary texture: 32×32 RGBA8, 4×4 grid of 8×8 solid-color cells.
///
/// Cell (cell_row, cell_col) uses palette index (cell_row*4 + cell_col), each mapped to a
/// distinct rainbow color. Even-indexed cells are opaque (alpha=255, palette a1=1) and
/// odd-indexed cells are transparent (alpha=0, palette a1=0). This alternating alpha makes
/// the texture serve both roles:
/// 1. Grid scene (MODULATE combiner, SHADE alpha): all 16 cells render with their palette RGB;
///    flat regions make palette-index scrambles immediately visible.
/// 2. Canary scene (TEXEL0_ALPHA→color): even cells → white, odd cells → black; non-uniform
///    output validates the TEXEL0_ALPHA→color path with CI4+TLUT (guards the IA gap).
///
#[cfg(feature = "asm")]
const fn gen_ci4_tex() -> [u8; 32 * 32 * 4] {
    // 16 rainbow colors spread across the hue wheel; even indices opaque, odd transparent.
    #[rustfmt::skip]
    const COLORS: [(u8, u8, u8, u8); 16] = [
        (255,   0,   0, 255), // 0: red,          opaque
        (255, 128,   0,   0), // 1: orange,        transparent
        (255, 255,   0, 255), // 2: yellow,        opaque
        (128, 255,   0,   0), // 3: lime,          transparent
        (  0, 255,   0, 255), // 4: green,         opaque
        (  0, 255, 128,   0), // 5: spring green,  transparent
        (  0, 255, 255, 255), // 6: cyan,          opaque
        (  0, 128, 255,   0), // 7: dodger blue,   transparent
        (  0,   0, 255, 255), // 8: blue,          opaque
        (128,   0, 255,   0), // 9: violet,        transparent
        (255,   0, 255, 255), // 10: magenta,      opaque
        (255,   0, 128,   0), // 11: rose,         transparent
        (255, 255, 128, 255), // 12: pale yellow,  opaque
        (128, 255, 255,   0), // 13: pale cyan,    transparent
        (255, 128, 255, 255), // 14: pale magenta, opaque
        (128, 128, 255,   0), // 15: periwinkle,   transparent
    ];
    let mut data = [0u8; 32 * 32 * 4];
    let mut cell_row = 0usize;
    while cell_row < 4 {
        let mut cell_col = 0usize;
        while cell_col < 4 {
            let (r, g, b, a) = COLORS[cell_row * 4 + cell_col];
            let mut py = 0usize;
            while py < 8 {
                let mut px = 0usize;
                while px < 8 {
                    let base = ((cell_row * 8 + py) * 32 + (cell_col * 8 + px)) * 4;
                    data[base] = r;
                    data[base + 1] = g;
                    data[base + 2] = b;
                    data[base + 3] = a;
                    px += 1;
                }
                py += 1;
            }
            cell_col += 1;
        }
        cell_row += 1;
    }
    data
}

#[cfg(feature = "asm")]
const CI4_TEX_ARRAY: [u8; 32 * 32 * 4] = gen_ci4_tex();

/// CI4 grid + canary source texture: 32×32 RGBA8, 4×4 grid of 8×8 cells, alternating alpha.
#[cfg(feature = "asm")]
const CI4_TEX: &[u8] = &CI4_TEX_ARRAY;

/// CI4 grid: MODULATE combiner (TEXEL0 × SHADE), white shade, G_TT_RGBA16.
#[cfg(feature = "asm")]
const CI4_GRID_SRC: &str = r#"
Texture tex = { 32, 32, CI4 }
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
gsDPSetOtherMode_H(14, 2, 0x8000)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_CI, G_IM_SIZ_4b, 32, 32)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

/// CI4 combine-route canary: routes TEXEL0_ALPHA (cc1=8) into RGB output.
/// c1=(ONE−0)×TEXEL0_ALPHA+0=texel.alpha; palette alternating a1 → alternating white/black.
#[cfg(feature = "asm")]
const CI4_CANARY_SRC: &str = r#"
Texture tex = { 32, 32, CI4 }
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
gsDPSetOtherMode_H(14, 2, 0x8000)
gsDPSetCombineLERP(0, 0, 0, 0, 0, 0, 0, 0, ONE, 0, 8, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_CI, G_IM_SIZ_4b, 32, 32)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

/// Golden test for CI4 color-indexed format — 4×4 rainbow grid via RGBA16 palette.
///
/// The MODULATE combiner with white shade passes through the palette colors; the 4×4 grid
/// of 8×8 solid-color cells should each render as a distinct flat region. Flat-cell distinct
/// regions make palette-index scrambles (nibble-order or TMEM layout bugs) immediately visible.
/// SHADE alpha (vertex alpha=255) is used as the alpha output, so even transparent-palette cells
/// (odd indices, a1=0) render fully opaque with their palette RGB.
#[cfg(feature = "asm")]
#[test]
fn golden_ci4_grid() {
    let px = render_scene_to_rgba8(CI4_GRID_SRC, CI4_TEX, 64, 64);
    compare_or_write("ci4-grid", &px, 64, 64);
}

/// Golden test for CI4 combine-route canary — TEXEL0_ALPHA routed into RGB output.
///
/// Combiner: (ONE−0)×TEXEL0_ALPHA+0 = texel.alpha as grayscale.
/// CI4_TEX palette has alternating a1 (0/1): even-index cells → alpha=255 (white output),
/// odd-index cells → alpha=0 (black output). The golden MUST show a non-uniform alternating
/// chequerboard of 8×8 bright and dark cells — NOT solid white. Solid white means
/// TEXEL0_ALPHA is not wired for CI4+TLUT — a false pass; do not bake if white.
#[cfg(feature = "asm")]
#[test]
fn golden_ci4_canary() {
    let px = render_scene_to_rgba8(CI4_CANARY_SRC, CI4_TEX, 64, 64);
    // Verify the canary output is NOT solid white (which would indicate tex_enable=false bug).
    let max_r = px.chunks(4).map(|p| p[0]).max().unwrap_or(0);
    let min_r = px.chunks(4).map(|p| p[0]).min().unwrap_or(255);
    assert!(
        max_r > 200 && min_r < 50,
        "ci4-canary rendered solid color (max_r={max_r}, min_r={min_r}); \
         expected alternating white/black cells (TEXEL0_ALPHA path broken for CI4?)"
    );
    compare_or_write("ci4-canary", &px, 64, 64);
}

// ── B3: AlphaOver fallback-blend smoke test ───────────────────────────────────────────────────────

/// A full-screen XLU quad: PRIMITIVE combiner with prim=(R=255,G=0,B=0,A=128), G_RM_AA_ZB_XLU_SURF.
/// Alpha combiner `d = PRIMITIVE` routes prim_alpha=128/255≈0.502 into the fragment alpha,
/// which the AlphaOver pipeline blends over the clear color.
#[cfg(feature = "asm")]
const XLU_QUAD_SRC: &str = r#"
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -128, -128, 0, 0, 0, 0, 255, 255, 255, 255 }
Vtx {  128, -128, 0, 0, 0, 0, 255, 255, 255, 255 }
Vtx {  128,  128, 0, 0, 0, 0, 255, 255, 255, 255 }
Vtx { -128,  128, 0, 0, 0, 0, 255, 255, 255, 255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_AA_ZB_XLU_SURF, G_RM_AA_ZB_XLU_SURF2)
gsDPSetCombineLERP(0, 0, 0, PRIMITIVE, 0, 0, 0, PRIMITIVE, 0, 0, 0, PRIMITIVE, 0, 0, 0, PRIMITIVE)
gsDPSetPrimColor(0, 0, 255, 0, 0, 128)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

/// B3 smoke test: AlphaOver pipeline blends a translucent XLU quad over the background.
///
/// A full-screen red quad with prim_alpha=128/255≈0.502 and G_RM_AA_ZB_XLU_SURF must BLEND
/// over the clear color (not Replace it). With AlphaOver:
///   result.R ≈ 0.502 * 255 + 0.498 * clear(13) ≈ 134 → strictly between clear(13) and 255.
/// With the old Replace pipeline:
///   result.R = 255 → fails `px[c] < 220`.
///
/// Placed before golden_multi_material so a failing blend assertion stops the run early.
#[cfg(feature = "asm")]
#[test]
fn alphaover_pipeline_blends_translucent_over_background() {
    // 1×1 white placeholder; the XLU scene has no gsDPLoadTextureBlock so tex_enable=false.
    let px = render_scene_to_rgba8(XLU_QUAD_SRC, &[255, 255, 255, 255], 32, 32);
    // Center pixel of the 32×32 render: row 16, col 16.
    let c = ((16 * 32 + 16) * 4) as usize;
    // AlphaOver result ≈ 134; Replace result = 255.
    // CLEAR_COLOR.r=0.05 → ~13 in u8; prim.r=1.0 → 255 in u8.
    assert!(
        px[c] > 60 && px[c] < 220,
        "expected AlphaOver blend (R≈134), got R={}; Replace pipeline still active?",
        px[c]
    );
}

// ── Multi-material golden ─────────────────────────────────────────────────────────────────────────

#[cfg(feature = "asm")]
const MULTI_MATERIAL_SRC: &str = include_str!("../../tests/scenes/multi-material.n64");

/// Golden test for multi-material per-run binding — Phase A gate.
///
/// One display list with THREE quads, each preceded by its own `gsDPSetCombineLERP` +
/// `gsDPSetRenderMode`:
/// - Left (pixels 0–32): opaque textured (TEXEL0 × white-SHADE, G_RM_OPA_SURF).
/// - Center (pixels 32–64): flat-primitive blue (PRIMITIVE combiner, G_RM_AA_ZB_XLU_SURF);
///   renders OPAQUE in Phase A (blender wired in Phase B).
/// - Right (pixels 64–96): textured × orange-SHADE (G_RM_AA_ZB_TEX_EDGE); renders OPAQUE
///   in Phase A (alpha-test wired in Phase D).
///
/// PASS: three visually distinct regions (not a single flat colour — the old collapse).
/// BLOCKED if the centre equals the left (dedup collapsed three materials into one).
#[cfg(feature = "asm")]
#[test]
fn golden_multi_material() {
    // 3 regions render 3 distinct materials (not the old flat collapse).
    let px = render_scene_to_rgba8(MULTI_MATERIAL_SRC, RGBA16_QUAD_TEX, 96, 96);
    // Canary: left-third center pixel vs. centre-third center pixel must differ.
    // For a 96×96 image, sample row 48:
    //   left-third   center x ≈ 16  → pixel byte offset (48*96 + 16)*4
    //   centre-third center x ≈ 48  → pixel byte offset (48*96 + 48)*4
    //   right-third  center x ≈ 80  → pixel byte offset (48*96 + 80)*4
    let row = 48usize;
    let stride = 96usize;
    let left_r = px[(row * stride + 16) * 4];
    let left_g = px[(row * stride + 16) * 4 + 1];
    let left_b = px[(row * stride + 16) * 4 + 2];
    let centre_r = px[(row * stride + 48) * 4];
    let centre_g = px[(row * stride + 48) * 4 + 1];
    let centre_b = px[(row * stride + 48) * 4 + 2];
    let right_r = px[(row * stride + 80) * 4];
    let right_g = px[(row * stride + 80) * 4 + 1];
    let right_b = px[(row * stride + 80) * 4 + 2];
    // Centre quad is flat blue (PRIMITIVE = 0,0,255) — assert blue channel dominant.
    assert!(
        centre_b > 200 && centre_b > centre_r + 100,
        "multi-material: centre quad not blue (r={centre_r},g={centre_g},b={centre_b}); \
         expected flat PRIMITIVE blue — per-material binding broken?"
    );
    // Left and right must differ from centre (proves distinct material binding, not collapse).
    let left_diff = (left_r as i32 - centre_r as i32).unsigned_abs()
        + (left_g as i32 - centre_g as i32).unsigned_abs()
        + (left_b as i32 - centre_b as i32).unsigned_abs();
    let right_diff = (right_r as i32 - centre_r as i32).unsigned_abs()
        + (right_g as i32 - centre_g as i32).unsigned_abs()
        + (right_b as i32 - centre_b as i32).unsigned_abs();
    assert!(
        left_diff > 60,
        "multi-material: left region too similar to centre \
         (L=[{left_r},{left_g},{left_b}] C=[{centre_r},{centre_g},{centre_b}] diff={left_diff}); \
         expected distinct textures — per-material binding collapsed?"
    );
    assert!(
        right_diff > 60,
        "multi-material: right region too similar to centre \
         (R=[{right_r},{right_g},{right_b}] C=[{centre_r},{centre_g},{centre_b}] diff={right_diff}); \
         expected distinct textures — per-material binding collapsed?"
    );
    compare_or_write("multi-material", &px, 96, 96);
}

/// Phase D cutout gate — the TEX_EDGE (CVG_X_ALPHA) quad must show a BACKGROUND HOLE.
///
/// The cutout quad (right third, G_RM_AA_ZB_TEX_EDGE) uses alpha = TEXEL0.a.  The shared
/// texture has rows 0-1 with alpha=255 (opaque, kept) and rows 2-3 with alpha=0 (sub-threshold,
/// discarded → background shows through).
///
/// UV 128 maps the 4×4 texture exactly once: V=0 at world_y=-128 (screen bottom) → row 0;
/// V=1 at world_y=+128 (screen top) → row 3.  The alpha=0 rows (2-3) occupy the UPPER half
/// of the screen (y=0..48) and the alpha=255 rows (0-1) the LOWER half (y=48..96).
///
/// Pixel (x=80, y=20) → V ≈ 0.79 → rows 2-3 (alpha=0) → discard → clear color.
/// Pixel (x=80, y=70) → V ≈ 0.27 → rows 0-1 (alpha=255) → kept → orange-tinted texture.
/// CLEAR_COLOR = (0.05, 0.05, 0.08) ≈ RGBA8 (13, 13, 20): R<20, G<20, B<40.
///
/// BLOCKED if: hole pixel is non-background (alpha-test not wired), threshold is wrong (not 0.125),
/// or alpha_mode leaks into non-cutout runs (texture-format/tron/fogworld goldens must be unchanged).
#[cfg(feature = "asm")]
#[test]
fn golden_multi_material_cutout_shows_hole() {
    // The cutout region must show BACKGROUND through a sub-threshold hole (not opaque texels).
    let px = render_scene_to_rgba8(MULTI_MATERIAL_SRC, RGBA16_QUAD_TEX, 96, 96);
    // Sample a pixel inside the cutout region's hole (right quad, upper area → texture rows 2-3, α=0).
    let hole = (20 * 96 + 80) * 4usize;
    assert!(
        px[hole] < 20 && px[hole + 1] < 20 && px[hole + 2] < 40,
        "cutout hole must show background (CLEAR_COLOR ≈ R<20,G<20,B<40); \
         got R={} G={} B={} — alpha-test discard not firing?",
        px[hole],
        px[hole + 1],
        px[hole + 2]
    );
    // ...but an opaque texel (right quad, lower area → texture rows 0-1, α=255) MUST survive the
    // discard. Without this, a fully-discarded region would still pass the hole assert above.
    let opaque = (70 * 96 + 80) * 4usize;
    assert!(
        px[opaque] > 50 || px[opaque + 1] > 50,
        "cutout opaque texel must survive; got R={} G={} B={}",
        px[opaque],
        px[opaque + 1],
        px[opaque + 2]
    );
    compare_or_write("multi-material", &px, 96, 96); // regenerate after wiring the cutout
}

// ── Tron scene + Phase B forced-fallback CI gate ─────────────────────────────────────────────────

#[cfg(feature = "asm")]
const TRON_SRC: &str = include_str!("../../tests/scenes/tron.n64");

/// Golden test for the `tron` scene — overlapping translucent neon panels.
///
/// Two semi-transparent quads (cyan + magenta, SHADE alpha=128/255≈0.5, G_RM_AA_ZB_XLU_SURF)
/// overlap in the center band.  The overlap must show a BLENDED MIX of both panel colors:
///
/// - Non-overlap cyan region  ≈ (6,  116, 134) — blue-green tinted clear
/// - Non-overlap magenta region ≈ (134, 6, 116) — reddish-purple tinted clear
/// - Overlap region           ≈ (131, 58, 177) — R from magenta, G from cyan, B from both
///
/// BLOCKED if overlap shows a single opaque color (Replace pipeline, not AlphaOver/DualSrc)
/// or shows the clear color (panels didn't render).
///
/// Rendered via the PRIMARY path (`headless_device` — dual-source when available).
#[cfg(feature = "asm")]
#[test]
fn golden_tron() {
    let px = render_scene_to_rgba8(TRON_SRC, &[], 96, 96);
    // Inspect the overlap region: pixel at (row=48, col=48) is inside the cyan+magenta overlap
    // band (x=-43..43 → pixels≈32..64).  Expected ≈ (131, 58, 177).
    let row = 48usize;
    let stride = 96usize;
    let c = (row * stride + 48) * 4;
    let r = px[c];
    let g = px[c + 1];
    let b = px[c + 2];
    // R>50 proves magenta component reached the framebuffer.
    // G>20 proves cyan component survived blending.
    // B>80 proves both panels contributed (both cyan and magenta have high blue).
    // If Replace pipeline: only last-drawn panel shows (pure magenta: R≈134, G≈6 → fails G>20
    // check but would pass R>50 — so G>20 is the key discriminator for Replace vs blend).
    assert!(
        r > 50 && g > 20 && b > 80,
        "tron overlap region must show blended mix of cyan+magenta \
         (R={r},G={g},B={b}); expected ≈(131,58,177); \
         XLU blending broken? (Replace pipeline would show R≈134,G≈6)"
    );
    compare_or_write("tron", &px, 96, 96);
}

/// Forced-fallback re-render of `tron` — proves B3 AlphaOver pipelines compile + render.
///
/// [IMP13] Re-renders `tron` through `headless_device_forced_fallback()` (dual-source disabled),
/// exercising the AlphaOver/Replace fallback blender pipelines.  `tron` uses the canonical XLU
/// lerp (B=1MA, A=A_IN) which is losslessly expressible as AlphaOver.
///
/// RGB channels match the primary golden within TOL=2; alpha differs by design (dual-source
/// preserves dst.a=255 from clear; AlphaOver writes src.a≈128).  The inline RGB cross-check
/// against the PRIMARY `tron.bin` is the IMP13 assertion (fallback RGB == primary RGB); the
/// `tron-fallback.bin` golden is an additional regression guard for the fallback alpha too.
#[cfg(feature = "asm")]
#[test]
fn golden_tron_forced_fallback() {
    let px = render_scene_to_rgba8_forced_fallback(TRON_SRC, &[], 96, 96);
    // Cross-check RGB against the PRIMARY golden (IMP13). Alpha intentionally differs:
    // dual-source preserves dst.a=255 from clear; AlphaOver writes src.a≈128.
    let primary_path = format!("{}/goldens/tron.bin", env!("CARGO_MANIFEST_DIR"));
    let primary =
        std::fs::read(&primary_path).unwrap_or_else(|e| panic!("primary tron.bin missing: {e}"));
    let max_rgb_diff = px
        .chunks(4)
        .zip(primary.chunks(4))
        .flat_map(|(a, p)| {
            [
                a[0].abs_diff(p[0]),
                a[1].abs_diff(p[1]),
                a[2].abs_diff(p[2]),
            ]
        })
        .max()
        .unwrap_or(0);
    assert!(
        max_rgb_diff <= TOL,
        "tron fallback RGB diverges from primary (max diff={max_rgb_diff} > TOL={TOL}); \
         XLU AlphaOver pipeline producing wrong RGB?"
    );
    compare_or_write("tron-fallback", &px, 96, 96);
}

/// Forced-fallback re-render of `multi-material` — [IMP13] second AlphaOver-expressible scene.
///
/// Re-renders `multi-material` through `headless_device_forced_fallback()` and asserts the
/// output is IDENTICAL (within TOL=2) to the primary `multi-material` golden.  The XLU center
/// quad (PRIMITIVE combiner, B=1MA, A=A_IN) is canonically expressible as AlphaOver so the
/// fallback is lossless.  If this test passes, the fallback module + pipelines compiled and
/// produced a correct result — a web-only regression cannot ship green.
#[cfg(feature = "asm")]
#[test]
fn golden_multi_material_forced_fallback() {
    let px = render_scene_to_rgba8_forced_fallback(MULTI_MATERIAL_SRC, RGBA16_QUAD_TEX, 96, 96);
    // Compare against the PRIMARY multi-material golden (not a separate fallback file).
    compare_or_write("multi-material", &px, 96, 96);
}

// ── Fog golden ───────────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "asm")]
const FOGWORLD_SRC: &str = include_str!("../../tests/scenes/fogworld.n64");

/// Golden test for the fogworld fog demo — proves the G_RM_FOG_SHADE_A + G_CYC_2CYCLE pipeline.
///
/// Two quads at different z depths: far (z=110) is heavily fogged → pixel ≈ fog_color [128,128,128];
/// near (z=0) has no fog → pixel = crisp surface [200,50,50].
///
/// MIN6: two pixel assertions verify the fog gradient is real:
///   far  (y=30, x=30): ≈ fog_color [0x80,0x80,0x80] within ±24 per channel
///   near (y=60, x=70): ≥ 1 channel differs from fog_color by > 40 (crisp surface)
#[cfg(feature = "asm")]
#[test]
fn golden_fogworld() {
    let px = render_scene_to_rgba8(FOGWORLD_SRC, &[], 96, 96);
    compare_or_write("fogworld", &px, 96, 96);
    // MIN6: assert the fog gradient at two sample points.
    let far = ((30 * 96 + 30) * 4) as usize; // distant quad — heavily fogged
    let near = ((60 * 96 + 70) * 4) as usize; // near quad — crisp surface
    let fog = [0x80u8, 0x80, 0x80]; // == gsDPSetFogColor in fogworld.n64
    for k in 0..3 {
        assert!(
            (px[far + k] as i32 - fog[k] as i32).abs() < 24,
            "far quad pixel channel {k}: {} is not within 24 of fog_color {} (far ≈ fog failed)",
            px[far + k],
            fog[k]
        );
    }
    // Near quad must be visibly LESS fogged — at least one channel must differ from fog by > 40.
    assert!(
        (0..3).any(|k| (px[near + k] as i32 - fog[k] as i32).abs() > 40),
        "near quad pixel [{},{},{}] is too close to fog_color — fog not cleared for near geometry",
        px[near],
        px[near + 1],
        px[near + 2]
    );
}

// ── Alpha-threshold scene (Phase D Task D2) ──────────────────────────────────────────────────────

#[cfg(feature = "asm")]
const ALPHA_THRESHOLD_SRC: &str = include_str!("../../tests/scenes/alpha-threshold.n64");

/// Golden test for the `alpha-threshold` scene — G_AC_THRESHOLD alpha-compare gate (Phase D, D2).
///
/// A full-screen textured quad with Gouraud vertex alpha varying left→right:
///   left (x=0..31)  vertex alpha ≈ 0..127 → combiner alpha < 0.502 → DISCARDED (background)
///   right (x=32..63) vertex alpha ≈ 128..255 → combiner alpha ≥ 0.502 → KEPT (textured surface)
///
/// Threshold = `gsDPSetBlendColor(0,0,0,128)` → blendColor.a = 128/255 ≈ 0.502.
/// This is DISTINCT from CVG_X_ALPHA (threshold fixed at 0.125): the THRESHOLD path reads
/// blendColor.a from the material, not a hardcoded constant.
///
/// MIN7: assert BOTH — sub-threshold sample shows BACKGROUND, supra-threshold shows SURFACE.
/// BLOCKED if whole quad shows (THRESHOLD path broken) or none shows (discard always fires).
#[cfg(feature = "asm")]
#[test]
fn golden_alpha_threshold() {
    let px = render_scene_to_rgba8(ALPHA_THRESHOLD_SRC, RGBA16_QUAD_TEX, 64, 64);
    compare_or_write("alpha-threshold", &px, 64, 64);
    // MIN7: assert the THRESHOLD gate (combiner-α < blendColor.a). A sub-threshold texel must show
    // BACKGROUND (discarded); a supra-threshold texel must show the texel — mirrors D1's cutout hole.
    let sub = ((/* y */50 * 64 + /* x */ 16) * 4) as usize; // alpha < 0.5 region → discarded
    let supra = ((/* y */50 * 64 + /* x */ 48) * 4) as usize; // alpha > 0.5 region → kept
    assert!(
        px[sub] < 20 && px[sub + 1] < 20 && px[sub + 2] < 40,
        "sub-threshold (x=16,y=50) must show background CLEAR_COLOR (R<20,G<20,B<40); \
         got R={} G={} B={} — G_AC_THRESHOLD discard not firing or threshold wrong?",
        px[sub],
        px[sub + 1],
        px[sub + 2]
    );
    assert!(
        px[supra] > 40 || px[supra + 1] > 40 || px[supra + 2] > 40,
        "supra-threshold (x=48,y=50) must show texel (at least one channel > 40); \
         got R={} G={} B={} — whole quad discarded? Check blendColor.a vs threshold.",
        px[supra],
        px[supra + 1],
        px[supra + 2]
    );
}

// ── Decal scene (Phase E Task E2) ─────────────────────────────────────────────────────────────

#[cfg(feature = "asm")]
const DECAL_SRC: &str = include_str!("../../tests/scenes/decal.n64");

/// Colors authored in `tests/scenes/decal.n64` (RGBA8 byte values).
#[cfg(feature = "asm")]
const DECAL_BASE_RGB: [u8; 3] = [40, 40, 200]; // blue base quad
#[cfg(feature = "asm")]
const DECAL_DECAL_RGB: [u8; 3] = [240, 220, 40]; // yellow coplanar decal
#[cfg(feature = "asm")]
const OCCLUDER_RGB: [u8; 3] = [220, 40, 40]; // red nearer quad

/// Golden test for the `decal` scene — in-shader ZMODE_DEC occlusion + coplanar discard (Phase E, E2).
///
/// A blue base quad fills the screen; a coplanar yellow decal covers the top half; a NEARER red
/// quad covers the upper-right quadrant. The decal fragment samples the depth the opaque pass
/// wrote and (a) shows coplanar on the base WITHOUT z-fighting, (b) is OCCLUDED (Z_CMP discard)
/// where the red quad is in front.
///
/// MIN8: assert BOTH — a pixel where the red quad covers the decal shows the OCCLUDER color (decal
/// discarded by Z_CMP), and a pixel where the decal sits coplanar on the base shows the DECAL color
/// (no z-fight). BLOCKED if the decal shows THROUGH the red quad (occlusion broken) or is missing on
/// the base (coplanar/z-fight broken).
#[cfg(feature = "asm")]
#[test]
fn golden_decal() {
    let px = render_scene_to_rgba8(DECAL_SRC, &[], 96, 96);
    compare_or_write("decal", &px, 96, 96);

    // (1) Occlusion: a pixel under the nearer red quad (upper-right) where the decal would be must
    // show the OCCLUDER color — the decal is discarded by the in-shader Z_CMP.
    let occluded = ((/* y */20 * 96 + /* x */ 70) * 4) as usize;
    for k in 0..3 {
        assert!(
            (px[occluded + k] as i32 - OCCLUDER_RGB[k] as i32).abs() < 24,
            "occluded pixel (x=70,y=20) must show the nearer quad (occluder) color {OCCLUDER_RGB:?}, \
             got [{},{},{}] — decal showing THROUGH the nearer quad (Z_CMP occlusion broken)?",
            px[occluded],
            px[occluded + 1],
            px[occluded + 2]
        );
    }

    // (2) Coplanar: a pixel where the decal sits ON the base (top-left, no occluder) must show the
    // DECAL color, NOT the base — proving the decal binds coplanar without z-fighting.
    let coplanar = ((/* y */20 * 96 + /* x */ 20) * 4) as usize;
    for k in 0..3 {
        assert!(
            (px[coplanar + k] as i32 - DECAL_DECAL_RGB[k] as i32).abs() < 24,
            "coplanar pixel (x=20,y=20) must show the DECAL color {DECAL_DECAL_RGB:?}, \
             got [{},{},{}] — decal missing on the base (z-fight / coplanar discard too strict)?",
            px[coplanar],
            px[coplanar + 1],
            px[coplanar + 2]
        );
    }

    // (3) Bottom half (no decal) shows the base color — sanity that the decal is bounded.
    let base = ((/* y */76 * 96 + /* x */ 20) * 4) as usize;
    for k in 0..3 {
        assert!(
            (px[base + k] as i32 - DECAL_BASE_RGB[k] as i32).abs() < 24,
            "base-only pixel (x=20,y=76) must show the base color {DECAL_BASE_RGB:?}, got [{},{},{}]",
            px[base],
            px[base + 1],
            px[base + 2]
        );
    }
}

// Boundary fixtures: a tilted base (z varies with y, so the per-pixel depth slope dz ≫ epsilon),
// with a coplanar decal offset a small constant in FRONT vs BEHIND. The asymmetry between the
// occlusion test (bare epsilon) and the coplanar test (max(dz, epsilon)) means a decal slightly in
// FRONT (delta < dz) shows, while one slightly BEHIND (delta > epsilon) is occluded.
#[cfg(feature = "asm")]
const DECAL_IN_FRONT_SRC: &str = r#"
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -128, -128, -96, 0, 0, 0,  40,  40, 200, 255 }
Vtx {  128, -128, -96, 0, 0, 0,  40,  40, 200, 255 }
Vtx {  128,  128,  96, 0, 0, 0,  40,  40, 200, 255 }
Vtx { -128,  128,  96, 0, 0, 0,  40,  40, 200, 255 }
Vtx { -128, -128, -98, 0, 0, 0, 240, 220,  40, 255 }
Vtx {  128, -128, -98, 0, 0, 0, 240, 220,  40, 255 }
Vtx {  128,  128,  94, 0, 0, 0, 240, 220,  40, 255 }
Vtx { -128,  128,  94, 0, 0, 0, 240, 220,  40, 255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetCombineLERP(0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE)
gsSPVertex(verts, 8, 0)
gsDPSetRenderMode(G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsDPSetRenderMode(G_RM_AA_ZB_OPA_DECAL, G_RM_AA_ZB_OPA_DECAL2)
gsSP1Triangle(4, 5, 6, 0)
gsSP1Triangle(4, 6, 7, 0)
gsSPEndDisplayList()
"#;

#[cfg(feature = "asm")]
const DECAL_BEHIND_SRC: &str = r#"
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -128, -128, -96, 0, 0, 0,  40,  40, 200, 255 }
Vtx {  128, -128, -96, 0, 0, 0,  40,  40, 200, 255 }
Vtx {  128,  128,  96, 0, 0, 0,  40,  40, 200, 255 }
Vtx { -128,  128,  96, 0, 0, 0,  40,  40, 200, 255 }
Vtx { -128, -128, -94, 0, 0, 0, 240, 220,  40, 255 }
Vtx {  128, -128, -94, 0, 0, 0, 240, 220,  40, 255 }
Vtx {  128,  128,  98, 0, 0, 0, 240, 220,  40, 255 }
Vtx { -128,  128,  98, 0, 0, 0, 240, 220,  40, 255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetCombineLERP(0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE)
gsSPVertex(verts, 8, 0)
gsDPSetRenderMode(G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsDPSetRenderMode(G_RM_AA_ZB_OPA_DECAL, G_RM_AA_ZB_OPA_DECAL2)
gsSP1Triangle(4, 5, 6, 0)
gsSP1Triangle(4, 6, 7, 0)
gsSPEndDisplayList()
"#;

/// Coplanar tolerance boundary: a decal slightly IN FRONT of the (tilted) surface shows; the same
/// decal slightly BEHIND is occluded (shows base). Asserts the two center pixels differ — the
/// in-shader tolerance must distinguish front (within dz) from behind (beyond epsilon).
#[cfg(feature = "asm")]
#[test]
fn decal_coplanar_tolerance_boundary() {
    let shown = render_scene_to_rgba8(DECAL_IN_FRONT_SRC, &[], 48, 48);
    let hidden = render_scene_to_rgba8(DECAL_BEHIND_SRC, &[], 48, 48);
    let c = ((24 * 48 + 24) * 4) as usize;
    assert_ne!(
        &shown[c..c + 3],
        &hidden[c..c + 3],
        "tolerance must distinguish front (decal shown) from behind (decal occluded): \
         front=[{},{},{}] behind=[{},{},{}]",
        shown[c],
        shown[c + 1],
        shown[c + 2],
        hidden[c],
        hidden[c + 1],
        hidden[c + 2]
    );
}

// ── High-poly scene (Phase F Task F1) ────────────────────────────────────────────────────────────

#[cfg(feature = "asm")]
const HIGH_POLY_SRC: &str = include_str!("../../tests/scenes/high-poly.n64");

/// Golden test for the `high-poly` scene — multi-batch vertex-loading guard (Phase F, F1).
///
/// 5 × `gsSPVertex(verts,28,0)` reloads accumulate 140 global entries (indices 0-139, > 127) of
/// the blue 4×7 grid mesh; the 6th batch (`gsSPVertex(verts,31,0)`) loads the red marker verts at
/// slots 28-30 — slots the mesh batches (count=28) NEVER touch — placing them at global indices
/// 168-170 (post-127). The marker triangle covers the top-left corner (pixel 10,10); the blue mesh
/// fills the right portion (x≥0 → screen x≥48), so pixel (10,10) is background unless the marker
/// renders there.
///
/// The marker is UNIQUELY tied to the post-127 batch: global indices 0-2 hold BLUE mesh corners,
/// not red. So a slot-reuse / wrong-global-index regression in the reload path (e.g. batch-6 slot
/// 28 resolving to a LOWER global) would point the marker at a blue mesh vertex (or off-screen),
/// drawing blue/background at (10,10) — NOT red — and FAILING the assertion. A marker authored
/// from low slots (also loaded by earlier batches) would be blind to this; this one is not.
///
/// MIN9: assert BOTH — the overall image matches the golden (whole-mesh regression), AND pixel
/// (10,10) is red (the post-127 batch resolved correctly). Red requires G<60; both blue
/// (0,50,200) and the dark background (≈13,13,20) fail `R>200`, so either regression is caught.
#[cfg(feature = "asm")]
#[test]
fn golden_high_poly() {
    let px = render_scene_to_rgba8(HIGH_POLY_SRC, &[], 96, 96);
    compare_or_write("high-poly", &px, 96, 96);
    // Marker triangle (red, from the post-127 batch — global 168-170) must appear at (10,10).
    // A slot-resolution regression would resolve the marker slots to a lower global (a BLUE mesh
    // vertex) or off-screen, drawing blue (0,50,200) / background (≈13,13,20) here — both fail R>200.
    let marker = ((/* y */10 * 96 + /* x */ 10) * 4) as usize;
    assert!(
        px[marker] > 200 && px[marker + 1] < 60,
        "marker triangle (red) must render at pixel (10,10); got R={} G={} B={} — post-127 batch \
         mis-resolved (drew the mesh's blue or background) or the marker slot-mapping regressed?",
        px[marker],
        px[marker + 1],
        px[marker + 2]
    );
}

// ── 2D / framebuffer-pipeline goldens (Slice 9) ──────────────────────────────────────────────────
//
// These route through `SceneRenderer::render` (the facade paired path) via `common::render_to_pixels`
// — the FB-pool + per-pair-pass + scanout-blit pipeline — unlike the older goldens, which use the
// manual `render_scene_with_device` raster path. The 2D scenes are 64×64 (`w*4 = 256`, readback-aligned).

/// Build a hand-crafted single-`FramebufferPair` scene with one `FillRect` op (no materials, no
/// triangles) over an `fb_w × fb_h` CIMG. The fill resolves through `CombinerUniform::fill_rect`.
fn fill_rect_scene(
    fb_w: u32,
    fb_h: u32,
    rect: crate::hle::Rect,
    color_raw: u32,
) -> crate::hle::Scene {
    crate::hle::Scene {
        framebuffer_pairs: vec![crate::hle::FramebufferPair {
            color_image: crate::hle::ColorImage {
                fmt: 0,
                siz: 2, // G_IM_SIZ_16b
                width: fb_w as u16,
                addr: 0x0010_0000,
            },
            depth_image: None,
            ops: vec![crate::hle::SceneOp::FillRect { rect, color_raw }],
            active_scissor: crate::hle::Scissor {
                ulx: 0,
                uly: 0,
                lrx: fb_w as i32,
                lry: fb_h as i32,
                mode: 0,
            },
            size_extent: (fb_w, fb_h),
            is_depth_clear: false,
        }],
        ..Default::default()
    }
}

/// A 1×1-texel material whose decoded RGBA8 is exactly `rgba` (so any sampled point returns it).
fn tex1x1_material(rgba: [u8; 4]) -> crate::hle::Material {
    crate::hle::Material {
        texture: rgba.to_vec(),
        tex_w: 1,
        tex_h: 1,
        selectors: crate::hle::combiner::decode_combine(0, 0),
        cycle_type: 2, // G_CYC_COPY
        prim: [0, 0, 0, 0],
        env: [0, 0, 0, 0],
        tex_enable: true,
        wrap_s: 2,
        wrap_t: 2,
        fmt: 0,
        siz: 2,
        blend_color: [0, 0, 0, 255],
        tile_count: 1,
        tex1: None,
        prim_lod_frac: 0.0,
        prim_min_level: 0.0,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    }
}

/// Hand-computed EXACT cross-check (no golden file): proves the rect clip-space mapping, the
/// exclusive lower-right `+1`, the scissor clamp, the FillRect flat-PRIM combine, and the COPY
/// TEXRECT TEXEL0-passthrough all land byte-exactly — BEFORE any `UPDATE_GOLDENS` blesses the scenes.
#[test]
fn golden_2d_rect_geometry_exact() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    // Readback requires bytes_per_row (= w*4) to be 256-aligned, so the FB is 64 wide.
    const W: u32 = 64;
    const H: u32 = 16;
    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, W, H, dual);

    // (A) Full-FB solid fill. RGBA16 0xF801 = R5=31,G5=0,B5=0,A1=1 → (255,0,0,255). The fill word
    // replicates the 16-bit pixel across both halves, so color_raw = 0xF801_F801.
    let scene = fill_rect_scene(
        W,
        H,
        crate::hle::Rect {
            ulx: 0,
            uly: 0,
            lrx: W as i32 - 1,
            lry: H as i32 - 1,
        },
        0xF801_F801,
    );
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, W, H);
    for y in 0..H {
        for x in 0..W {
            assert_eq!(
                common::pixel(&buf, W, x, y),
                [255, 0, 0, 255],
                "full-FB fill: pixel ({x},{y}) must be the resolved RGBA16 fill color"
            );
        }
    }

    // (B) Sub-region fill [2,2]..=[5,5] (inclusive) over a CLEAR_COLOR background. The quad spans
    // continuous pixel space [2,6)×[2,6) (exclusive +1), so pixel centers 2.5..5.5 are covered and
    // 1.5 / 6.5 are not — a precise test of the exclusive +1 and binary (no-MSAA) coverage.
    let scene = fill_rect_scene(
        W,
        H,
        crate::hle::Rect {
            ulx: 2,
            uly: 2,
            lrx: 5,
            lry: 5,
        },
        0xF801_F801,
    );
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, W, H);
    let clear = clear_color_rgb_local();
    for y in 0..H {
        for x in 0..W {
            let p = common::pixel(&buf, W, x, y);
            let inside = (2..=5).contains(&x) && (2..=5).contains(&y);
            if inside {
                assert_eq!(p, [255, 0, 0, 255], "inside fill: pixel ({x},{y})");
            } else {
                // CLEAR_COLOR ≈ (13,13,20); allow ±2 for unorm rounding.
                assert!(
                    p[0].abs_diff(clear[0]) <= 2
                        && p[1].abs_diff(clear[1]) <= 2
                        && p[2].abs_diff(clear[2]) <= 2,
                    "outside fill: pixel ({x},{y}) must be CLEAR_COLOR, got {p:?}"
                );
            }
        }
    }

    // (C) COPY TEXRECT over a 1×1 texel → every covered pixel is the texel exactly (no combine, no
    // filtering ambiguity on a 1×1 source). Verifies the TexRect quad + TEXEL0-passthrough combine.
    let texel = [10u8, 200, 30, 255];
    let scene = crate::hle::Scene {
        materials: vec![tex1x1_material(texel)],
        framebuffer_pairs: vec![crate::hle::FramebufferPair {
            color_image: crate::hle::ColorImage {
                fmt: 0,
                siz: 2,
                width: W as u16,
                addr: 0x0010_0000,
            },
            depth_image: None,
            ops: vec![crate::hle::SceneOp::TexRect {
                rect: crate::hle::Rect {
                    ulx: 0,
                    uly: 0,
                    lrx: W as i32 - 1,
                    lry: H as i32 - 1,
                },
                uls: 0,
                ult: 0,
                dsdx: 1024,
                dtdy: 1024,
                flip: false,
                copy_mode: true,
                material_index: 0,
                render_mode_index: 0,
                fb_source: None,
            }],
            active_scissor: crate::hle::Scissor {
                ulx: 0,
                uly: 0,
                lrx: W as i32,
                lry: H as i32,
                mode: 0,
            },
            size_extent: (W, H),
            is_depth_clear: false,
        }],
        ..Default::default()
    };
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, W, H);
    for y in 0..H {
        for x in 0..W {
            assert_eq!(
                common::pixel(&buf, W, x, y),
                texel,
                "copy texrect over 1×1: pixel ({x},{y}) must equal the source texel"
            );
        }
    }
}

/// CLEAR_COLOR (0.05,0.05,0.08) rendered into an Rgba8Unorm target → ≈(13,13,20). Local copy
/// (goldens.rs cannot see `common::clear_color_rgb` is `pub` without it being used elsewhere here).
fn clear_color_rgb_local() -> [u8; 3] {
    [
        (CLEAR_COLOR.r * 255.0).round() as u8,
        (CLEAR_COLOR.g * 255.0).round() as u8,
        (CLEAR_COLOR.b * 255.0).round() as u8,
    ]
}

/// Scene 1 — `fill-texrect`: FILL clears the 64×64 CIMG to blue, then a COPY TEXRECT blits the
/// 4×4 `quad_tex` checker over the whole surface. The checker maps row-0-at-top (verified against
/// the texel alpha pattern: rows 0–1 α=255, rows 2–3 α=0 → a 4-px vertical period).
#[cfg(feature = "asm")]
#[test]
fn golden_2d_fill_texrect() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    let scene = common::scene_from_source("fill-texrect.n64", RGBA16_QUAD_TEX, 4, 4);
    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, 64, 64);
    // Hand cross-check the vertical orientation via the texel alpha period (dtdy=1024 → 1 texel/px):
    // screen rows 0–1 sample texrows 0–1 (α=255); row 2 samples texrow 2 (α=0). A vertical flip or a
    // wrong exclusive-+1 would invert this. (Linear filtering keeps the row centers near-pure.)
    assert!(
        common::pixel(&buf, 64, 2, 0)[3] > 200 && common::pixel(&buf, 64, 2, 1)[3] > 200,
        "fill-texrect: top rows (texrows 0–1) must be opaque (α≈255) — orientation/flip check"
    );
    assert!(
        common::pixel(&buf, 64, 2, 2)[3] < 60,
        "fill-texrect: row 2 (texrow 2) must be transparent (α≈0) — orientation/flip check"
    );
    // The blue FILL must be fully overwritten by the checker (no pure-blue, full-α pixel remains).
    assert!(
        common::pixel(&buf, 64, 2, 0)[3] != 0 || common::pixel(&buf, 64, 2, 0)[2] < 255,
        "fill-texrect: the TEXRECT must have drawn over the FILL"
    );
    compare_or_write("2d-fill-texrect", &buf, 64, 64);
}

/// Scene 2 — `hud-over-3d`: a Gouraud 3D quad in the center, then a COPY TEXRECT HUD in the
/// top-left 16×16 corner. Verifies tris + rects coexist in one pair and the HUD lands top-left.
#[cfg(feature = "asm")]
#[test]
fn golden_2d_hud_over_3d() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    let scene = common::scene_from_source("hud-over-3d.n64", RGBA16_QUAD_TEX, 4, 4);
    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, 64, 64);
    // Top-left corner = HUD checker (came from a copy texrect; texels there are α=0, B/Y/R/G).
    assert!(
        common::pixel(&buf, 64, 2, 2)[3] < 60,
        "hud: top-left corner must be the HUD checker overlay (α≈0)"
    );
    // Center = the 3D quad (opaque, not the background clear).
    let c = common::pixel(&buf, 64, 32, 32);
    assert!(
        c[3] == 255 && c != [13, 13, 20, 255],
        "hud: center must be the opaque 3D quad, got {c:?}"
    );
    // Bottom-right corner = background (outside both the HUD corner and the centered quad).
    let br = common::pixel(&buf, 64, 60, 60);
    assert!(
        br[0].abs_diff(13) <= 2 && br[2].abs_diff(20) <= 2,
        "hud: bottom-right must be the background clear, got {br:?}"
    );
    compare_or_write("2d-hud-over-3d", &buf, 64, 64);
}

/// Scene 4 — `texrectflip`: COPY TEXRECT with S/T axes swapped (`gsSPTextureRectangleFlip`). The
/// flipped UVs transpose the checker vs `fill-texrect`'s un-flipped layout.
#[cfg(feature = "asm")]
#[test]
fn golden_2d_texrectflip() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    let flip = common::scene_from_source("texrectflip.n64", RGBA16_QUAD_TEX, 4, 4);
    let plain = common::scene_from_source("fill-texrect.n64", RGBA16_QUAD_TEX, 4, 4);
    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &flip, 64, 64);
    let buf_plain = common::render_to_pixels(&device, &queue, &mut sr, &plain, 64, 64);
    // Flip transposes the pattern: under FLIP the texel ALPHA period runs along X (left columns
    // sample texrows 0–1 → α≈255) instead of along Y. So col 0 is opaque where the un-flipped col 0
    // (row 2) was transparent — a direct flip-vs-plain orientation discriminator at pixel (0,2).
    assert!(
        common::pixel(&buf, 64, 0, 2)[3] > 200,
        "texrectflip: left column must be opaque (texrows 0–1 run along X under flip)"
    );
    assert_ne!(
        common::pixel(&buf, 64, 0, 2),
        common::pixel(&buf_plain, 64, 0, 2),
        "texrectflip must differ from the un-flipped fill-texrect"
    );
    compare_or_write("2d-texrectflip", &buf, 64, 64);
}

/// Bgra8 headless cover: render `fill-texrect` once at `Rgba8Unorm` and once at `Bgra8Unorm` and
/// assert the readback bytes are R↔B swapped (G/A identical). Exercises the present pipeline's
/// surface-format path (`SceneRenderer::new(.., Bgra8Unorm, ..)`) without a real surface.
#[cfg(feature = "asm")]
#[test]
fn golden_2d_bgra8_present_cover() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    let scene = common::scene_from_source("fill-texrect.n64", RGBA16_QUAD_TEX, 4, 4);

    let mut sr_rgba = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let rgba = common::render_to_pixels(&device, &queue, &mut sr_rgba, &scene, 64, 64);

    let mut sr_bgra = SceneRenderer::new(&device, wgpu::TextureFormat::Bgra8Unorm, 64, 64, dual);
    let bgra = common::render_to_pixels_fmt(
        &device,
        &queue,
        &mut sr_bgra,
        &scene,
        64,
        64,
        wgpu::TextureFormat::Bgra8Unorm,
    );
    assert_eq!(rgba.len(), bgra.len());
    for i in (0..rgba.len()).step_by(4) {
        assert!(
            bgra[i].abs_diff(rgba[i + 2]) <= 2,
            "B channel mismatch at {i}"
        );
        assert!(bgra[i + 1].abs_diff(rgba[i + 1]) <= 2, "G channel at {i}");
        assert!(bgra[i + 2].abs_diff(rgba[i]) <= 2, "R channel at {i}");
        assert!(bgra[i + 3].abs_diff(rgba[i + 3]) <= 2, "A channel at {i}");
    }
}

/// Scene 3 — `offscreen-then-sample`: two `FramebufferPair`s. Pair 0 (scratch 0x00200000) is filled
/// orange via FILLRECT (RGBA16 0xFB81 → R=255, G=115, B=0, A=255). Pair 1 (scanout 0x00100000)
/// uses a COPY TEXRECT with `fb_source = Some(0x00200000)` to sample the scratch buffer into the
/// scanout via the FB-as-texture alias (spec §2.4, Task 10 Step 1).
///
/// **Hand cross-check (BEFORE UPDATE_GOLDENS):** The scratch FILLRECT resolves RGBA16 0xFB81 to
/// R8=(31<<3|31>>2)=255, G8=(14<<3|14>>2)=115, B8=0, A=255 — a saturated orange. With the
/// FB-as-texture alias active, the scanout TEXRECT samples that orange directly; without it the
/// scanout would show CLEAR_COLOR (≈13,13,20). The per-pixel assertion below verifies ORANGE
/// (R>200, G>80, B<60) before the golden is committed.
#[cfg(feature = "asm")]
#[test]
fn golden_2d_offscreen_then_sample() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    // No RDRAM texture: the scratch fill color is decoded from the FILLRECT color register, not
    // from RDRAM bytes. A 1×1 placeholder satisfies assemble_with_texture's texture argument.
    let scene = common::scene_from_source("offscreen-then-sample.n64", &[255u8; 4], 1, 1);
    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, 64, 64);

    // Hand cross-check: RGBA16 0xFB81 fills the scratch buffer orange.
    // R5=31 → R8=255, G5=14 → G8=115, B5=0 → B8=0, A1=1 → A=255.
    // With FB-as-texture the scanout should be orange; without it CLEAR_COLOR (≈13,13,20).
    // Verify a representative pixel at the center and corners — all should be orange.
    for &(x, y) in &[(0u32, 0u32), (32, 32), (63, 63), (0, 63), (63, 0)] {
        let p = common::pixel(&buf, 64, x, y);
        assert!(
            p[0] > 200 && p[1] > 80 && p[2] < 60,
            "offscreen-then-sample pixel ({x},{y}): expected orange (R>200,G>80,B<60) \
             from FB-as-texture alias, got {p:?}. \
             If this shows CLEAR_COLOR the FB alias is not firing."
        );
    }
    compare_or_write("2d-offscreen-then-sample", &buf, 64, 64);
}

// ── Alpha-blended TexRect regression golden (alpha HUD blend fix) ─────────────────────────────────

/// 2×2 RGBA8 source for the alpha-texrect golden: top row opaque red, bottom row transparent.
///
/// After the RGBA16 5-bit round-trip (assembler encodes RGBA8→RGBA16):
///   Row 0: R5=31→R8=255, A1=1→A8=255 (fully opaque red).
///   Row 1: R5=31→R8=255, A1=0→A8=0   (fully transparent).
///
/// With dtdy=1024 (1 texel/pixel) the 2-row texture tiles every 2 screen rows, so:
///   Even screen rows (0,2,4,...): texrow 0 → opaque red  → rendered color = (255,0,0)
///   Odd  screen rows (1,3,5,...): texrow 1 → transparent → AlphaOver shows background
///
/// Used exclusively by `golden_2d_alpha_texrect_over_bg` (no separate scene fixture needed).
#[rustfmt::skip]
#[cfg(feature = "asm")]
const ALPHA_TEXRECT_TEX: &[u8] = &[
    255, 0, 0, 255,  255, 0, 0, 255, // row 0: red opaque  (α=255)
    255, 0, 0,   0,  255, 0, 0,   0, // row 1: red transparent (α=0)
];

/// Alpha-texrect source: solid green FILLRECT, then 1-cycle XLU TEXRECT over it.
///
/// CIMG: 64×64 RGBA16 at 0x00100000.
/// Pass 1 — FILL: 0x07C1_07C1 → RGBA16 0x07C1 → R5=0, G5=31, B5=0, A1=1 → (0, 255, 0, 255).
/// Pass 2 — 1-CYCLE XLU: G_RM_AA_ZB_XLU_SURF has FORCE_BL → fallback_class = AlphaOver.
///   Combiner: (0-0)*0 + TEXEL0 = pure TEXEL0 passthrough (RGB and alpha).
///
/// This is a renderer-test-only fixture (not a shared scene file).
#[cfg(feature = "asm")]
const ALPHA_TEXRECT_OVER_BG_SRC: &str = r#"
Texture tex = { 2, 2, RGBA16 }
gsDPSetColorImage(G_IM_FMT_RGBA, G_IM_SIZ_16b, 64, 0x00100000)
gsDPSetScissor(0, 0, 0, 256, 256)
// Pass 1 — FILL mode: flood the CIMG with solid green (RGBA16 0x07C1 = G5=31 opaque).
gsDPSetOtherMode_H(G_CYC_FILL)
gsDPSetFillColor(0x07C107C1)
gsDPFillRectangle(0, 0, 256, 256)
// Pass 2 — 1-cycle XLU TEXRECT: TEXEL0 passthrough + AlphaOver blend over green bg.
// G_RM_AA_ZB_XLU_SURF has FORCE_BL → classified DualSrc primary / AlphaOver fallback.
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_AA_ZB_XLU_SURF, G_RM_AA_ZB_XLU_SURF2)
gsDPSetCombineLERP(0, 0, 0, TEXEL0, 0, 0, 0, TEXEL0, 0, 0, 0, TEXEL0, 0, 0, 0, TEXEL0)
gsDPLoadTextureBlock(tex, G_IM_FMT_RGBA, G_IM_SIZ_16b, 2, 2)
gsSPTextureRectangle(0, 0, 256, 256, 0, 0, 0, 1024, 1024)
gsSPEndDisplayList()
"#;

/// Assemble a scene from an inline source string for renderer-test-only fixtures.
/// Mirrors `common::scene_from_source` but takes a source string instead of a file path.
#[cfg(feature = "asm")]
fn scene_from_src_str(src: &str, tex_rgba8: &[u8], tex_w: u32, tex_h: u32) -> crate::hle::Scene {
    let img = crate::asm::assemble_with_texture(src, tex_rgba8, tex_w, tex_h)
        .unwrap_or_else(|d| panic!("assembly failed: {d:?}"));
    let r = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
    assert!(r.diags.is_empty(), "unexpected HLE diags: {:?}", r.diags);
    r.scene
}

/// Alpha-blend regression golden: a non-COPY XLU TEXRECT must blend over the background.
///
/// Scene: solid green FILLRECT (RGBA16 G5=31 → G8=255) followed by an alpha-blended TEXRECT
/// using a 2×2 texture (top row opaque red / bottom row transparent) and G_RM_AA_ZB_XLU_SURF.
///
/// **Hand-verify BEFORE UPDATE_GOLDENS:**
///   Even rows (texrow 0, α=255): AlphaOver writes red (255,0,0) — R>200, G<30.
///   Odd  rows (texrow 1, α=0):   AlphaOver passes background through — G>200, R<30.
///
/// **With the OLD Replace pipeline (pre-fix):**
///   Transparent pixels get REPLACED with the combiner output (red) ignoring α → odd rows
///   are red, NOT green. The `odd_pix[1] > 200` assertion fails → regression detected.
///
/// **Byte-identity:** existing COPY/FILL scenes are unaffected (Replace paths unchanged).
#[cfg(feature = "asm")]
#[test]
fn golden_2d_alpha_texrect_over_bg() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    let scene = scene_from_src_str(ALPHA_TEXRECT_OVER_BG_SRC, ALPHA_TEXRECT_TEX, 2, 2);
    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, 64, 64);

    // Even row (row 0): texrow 0 is fully opaque red (α=255). AlphaOver: out = red.
    let even_pix = common::pixel(&buf, 64, 32, 0);
    assert!(
        even_pix[0] > 200 && even_pix[1] < 30,
        "alpha-texrect even row (texrow 0, opaque): expected red (R>200, G<30), \
         got R={} G={} B={} — blend or combiner broken?",
        even_pix[0],
        even_pix[1],
        even_pix[2]
    );

    // Odd row (row 1): texrow 1 is transparent (α=0). AlphaOver: out = green background.
    // With the old Replace pipeline this would be red (combiner output ignoring α).
    let odd_pix = common::pixel(&buf, 64, 32, 1);
    assert!(
        odd_pix[1] > 200 && odd_pix[0] < 30,
        "alpha-texrect odd row (texrow 1, transparent): expected green background (G>200, R<30), \
         got R={} G={} B={} — Replace pipeline still active (ignoring α of transparent texel)?",
        odd_pix[0],
        odd_pix[1],
        odd_pix[2]
    );

    compare_or_write("2d-alpha-texrect-over-bg", &buf, 64, 64);
}

// ── COPY-mode alpha-keyed TexRect regression (sm64 HUD/text glyph fix) ────────────────────────────

/// COPY-mode alpha-key regression: mirrors sm64's HUD/text glyph setup exactly.
///
/// sm64 `bin/segment2.c` (`dl_hud_*`) issues HUD/number/logo glyphs under:
///   gsDPSetCycleType(G_CYC_COPY); gsDPSetAlphaCompare(G_AC_THRESHOLD);
///   gsDPSetBlendColor(255,255,255,255); gsDPSetTextureFilter(G_TF_POINT);
/// The glyphs are RGBA5551 (1-bit alpha): background texels have a1=0 (α=0), foreground a1=1
/// (α=255). The N64 RDP alpha-keys those α=0 texels away (background shows through). The bug:
/// `tex_copy()` hardcoded `alpha_mode=0` (no discard), so α=0 texels wrote as OPAQUE BLACK boxes.
///
/// This scene: green FILLRECT background, then a COPY TEXRECT whose decoded render mode has
/// `alpha_compare == Threshold` over a 2×2 texture (top row α=255 red, bottom row α=0). With
/// dtdy=1024 the 2-row texture tiles every 2 screen rows.
///
/// **Hand-verify (BEFORE UPDATE_GOLDENS):**
///   Even screen rows (texrow 0, α=255): the opaque red texel is copied → R>200, G<30.
///   Odd  screen rows (texrow 1, α=0):   alpha-keyed away → green background shows → G>200, R<30.
///
/// **With the OLD buggy tex_copy() (alpha_mode=0):** the α=0 odd rows write OPAQUE BLACK
/// (TEXEL0 RGB=0, no discard) → `odd_pix[1] > 200` fails. That is the bug this asserts against.
fn copy_alpha_keyed_scene() -> crate::hle::Scene {
    use crate::hle::{
        AlphaCompare, ColorImage, FramebufferPair, Material, Rect, RenderMode, Scene, SceneOp,
        Scissor,
    };
    const W: u32 = 64;
    const H: u32 = 64;
    // 2×2 RGBA8: top row opaque red, bottom row transparent (α=0). RGB stays red on the
    // transparent row to prove the discard (not a black RGB) is what reveals the background.
    let texture = vec![
        255, 0, 0, 255, 255, 0, 0, 255, // row 0 — α=255 (opaque)
        255, 0, 0, 0, 255, 0, 0, 0, // row 1 — α=0   (alpha-keyed hole)
    ];
    let material = Material {
        texture,
        tex_w: 2,
        tex_h: 2,
        selectors: crate::hle::combiner::decode_combine(0, 0),
        cycle_type: 2, // G_CYC_COPY
        prim: [0, 0, 0, 0],
        env: [0, 0, 0, 0],
        tex_enable: true,
        wrap_s: 0,
        wrap_t: 0,
        fmt: 0,                            // RGBA
        siz: 2,                            // 16b (RGBA5551, 1-bit alpha)
        blend_color: [255, 255, 255, 255], // sm64 sets blend_color.a = 255 → threshold must NOT be blend_a
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
    // Decoded render mode for sm64's HUD copy setup: G_AC_THRESHOLD → alpha_compare = Threshold.
    let rm = RenderMode {
        alpha_compare: AlphaCompare::Threshold,
        ..Default::default()
    };
    // Green FILLRECT background (RGBA16 0x07C1 → R=0,G=255,B=0,A=1) then the alpha-keyed COPY rect.
    Scene {
        materials: vec![material],
        render_modes: vec![rm],
        framebuffer_pairs: vec![FramebufferPair {
            color_image: ColorImage {
                fmt: 0,
                siz: 2,
                width: W as u16,
                addr: 0x0010_0000,
            },
            depth_image: None,
            ops: vec![
                SceneOp::FillRect {
                    rect: Rect {
                        ulx: 0,
                        uly: 0,
                        lrx: W as i32 - 1,
                        lry: H as i32 - 1,
                    },
                    color_raw: 0x07C1_07C1,
                },
                SceneOp::TexRect {
                    rect: Rect {
                        ulx: 0,
                        uly: 0,
                        lrx: W as i32 - 1,
                        lry: H as i32 - 1,
                    },
                    uls: 0,
                    ult: 0,
                    dsdx: 1024,
                    dtdy: 1024,
                    flip: false,
                    copy_mode: true,
                    material_index: 0,
                    render_mode_index: 0,
                    fb_source: None,
                },
            ],
            active_scissor: Scissor {
                ulx: 0,
                uly: 0,
                lrx: W as i32,
                lry: H as i32,
                mode: 0,
            },
            size_extent: (W, H),
            is_depth_clear: false,
        }],
        ..Default::default()
    }
}

/// COPY-mode alpha-keyed TexRect must discard its α=0 texels (background shows through), not
/// write them as opaque black. Regression guard for the sm64 HUD/text black-box bug.
#[test]
fn golden_2d_copy_alpha_keyed_over_bg() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    let scene = copy_alpha_keyed_scene();
    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, 64, 64);

    // Even row (texrow 0, α=255): opaque red copied verbatim.
    let even_pix = common::pixel(&buf, 64, 32, 0);
    assert!(
        even_pix[0] > 200 && even_pix[1] < 30,
        "copy-alpha-keyed even row (texrow 0, opaque): expected red (R>200, G<30), got {even_pix:?}"
    );

    // Odd row (texrow 1, α=0): alpha-keyed away → green background shows through.
    // With the buggy tex_copy() (alpha_mode=0) this is OPAQUE BLACK (R=0,G=0,B=0) — bug fails here.
    let odd_pix = common::pixel(&buf, 64, 32, 1);
    assert!(
        odd_pix[1] > 200 && odd_pix[0] < 30,
        "copy-alpha-keyed odd row (texrow 1, α=0): expected green background (G>200, R<30), \
         got {odd_pix:?} — copy-mode rect wrote the α=0 texel as opaque black (the bug)?"
    );

    compare_or_write("2d-copy-alpha-keyed-over-bg", &buf, 64, 64);
}

// ── Paired coplanar-decal regression (decal two-pass in per-pair rendering) ───────────────────────

/// Build a coplanar-decal scene: a BLACK opaque base quad (run 0, `G_RM_AA_ZB_OPA_SURF`) and a
/// coplanar BRIGHT MAGENTA decal quad (run 1, `G_RM_AA_ZB_OPA_DECAL`), both full-screen at Z=0.
/// This is the pair-LESS form (flat `draw_runs`). Mirrors `render.rs::build_decal_smoke_scene`.
///
/// The viewport uses the canonical FB_WIDTH/2, FB_HEIGHT/2 (160, 120) so the RSP-process fold
/// (`rsp_process.wgsl`, which hardcodes FB_WIDTH=320 / FB_HEIGHT=240) maps the NDC `[-1,1]` quad to
/// the FULL render target at ANY target size — so both the pair-less render and a paired render
/// into any-size FB fill the whole target.
fn build_decal_scene() -> crate::hle::Scene {
    // PRIM-passthrough combiner (combine_l=0, combine_h=0xC3 → cd1=PRIM, ad1=PRIM).
    let selectors = crate::hle::combiner::decode_combine(0x0000_0000, 0x0000_00C3);
    let mat = |prim: [u8; 4]| crate::hle::Material {
        texture: vec![255u8, 255, 255, 255],
        tex_w: 1,
        tex_h: 1,
        selectors: selectors.clone(),
        cycle_type: 0,
        prim,
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
        "run 0 must be opaque"
    );

    let half_w = crate::hle::rsp::FB_WIDTH / 2.0;
    let half_h = crate::hle::rsp::FB_HEIGHT / 2.0;
    let ds = 511.0 / crate::hle::rsp::DEPTH_RANGE;
    let vp = ([half_w, half_h, ds], [half_w, half_h, ds]);
    let quad: [[f32; 3]; 4] = [
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
    ];
    let mut scene = crate::hle::Scene {
        materials: vec![mat([0, 0, 0, 255]), mat([220, 40, 255, 255])],
        mvp_table: vec![crate::hle::math::identity()],
        viewport_table: vec![vp],
        texcoord_table: vec![[0.0, 0.0]],
        render_modes: vec![rm_base, rm_decal],
        ..Default::default()
    };
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
            material_index: 0,
            render_mode_index: 0,
            cull: crate::hle::CullKind::None,
            index_count: 6,
            index_start: 0,
        },
        crate::hle::DrawRun {
            material_index: 1,
            render_mode_index: 1,
            cull: crate::hle::CullKind::None,
            index_count: 6,
            index_start: 6,
        },
    ];
    scene
}

/// PAIRED coplanar-decal regression (the decal two-pass `render_decal_pair` fix).
///
/// Before this fix `render_pairs` drew every op in ONE depth-tested pass, so a coplanar DECAL run
/// in a PAIRED scene (sm64's carpet / door overlays once CIMG made sm64 paired) z-fought the
/// opaque surface and effectively vanished. The pair-LESS path always had the faithful
/// depth-as-sampled-texture two-pass (`draw_with_decals`); the fix mirrors it per-pair.
///
/// **Hand-reasoning (BEFORE comparing):** the scene is a BLACK opaque base quad + a coplanar
/// BRIGHT MAGENTA decal quad, both full-screen at Z=0. The decal MUST win the coplanar test and
/// paint magenta over the base. Rendered pair-LESS it goes through `draw_with_decals`; rendered
/// PAIRED (the SAME two runs moved into one `FramebufferPair` WITH a depth image) it goes through
/// `render_decal_pair`. The two MUST be pixel-equal (within `TOL`): a decal looks the same paired
/// or not. The center is asserted MAGENTA (R>180, B>180, G<90) — NOT the black base — so a broken
/// two-pass (decal z-fights/vanishes → black center) fails the test outright.
#[test]
fn golden_paired_decal_matches_pair_less() {
    use crate::render::SceneRenderer;
    const DIM: u32 = 64;
    let (device, queue, dual) = headless_device();

    // Pair-less decal scene (flat draw_runs → `draw_with_decals`).
    let pair_less = build_decal_scene();
    // Paired form: move the two runs into one FramebufferPair WITH a depth image (so depth exists →
    // `render_decal_pair` fires). CIMG width = DIM, scissor lry = DIM → a DIM×DIM FB, 1:1 blit.
    let mut paired = pair_less.clone();
    let ops: Vec<crate::hle::SceneOp> = paired
        .draw_runs
        .drain(..)
        .map(crate::hle::SceneOp::Tris)
        .collect();
    paired.framebuffer_pairs = vec![crate::hle::FramebufferPair {
        color_image: crate::hle::ColorImage {
            fmt: 0,
            siz: 2, // G_IM_SIZ_16b
            width: DIM as u16,
            addr: 0x0010_0000,
        },
        depth_image: Some(0x0020_0000), // distinct from CIMG → a real depth FB (not a depth-clear)
        ops,
        active_scissor: crate::hle::Scissor {
            ulx: 0,
            uly: 0,
            lrx: DIM as i32,
            lry: DIM as i32,
            mode: 0,
        },
        size_extent: (DIM, DIM),
        is_depth_clear: false,
    }];

    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, DIM, DIM, dual);
    let buf_pl = common::render_to_pixels(&device, &queue, &mut sr, &pair_less, DIM, DIM);
    let buf_pr = common::render_to_pixels(&device, &queue, &mut sr, &paired, DIM, DIM);

    // (1) The paired center must show the BRIGHT MAGENTA decal — not the black base. If the decal
    // z-fought (the bug), the center would be the black opaque base.
    let c = common::pixel(&buf_pr, DIM, DIM / 2, DIM / 2);
    assert!(
        c[0] > 180 && c[2] > 180 && c[1] < 90,
        "paired decal center must be the bright magenta decal (R>180,B>180,G<90), got {c:?} — \
         decal z-fighting/vanishing in the per-pair path?"
    );

    // (2) The paired render must match the pair-less render within TOL: a coplanar decal looks the
    // same whether the scene is paired or not.
    assert_eq!(buf_pl.len(), buf_pr.len(), "buffers must match in length");
    let max = buf_pl
        .iter()
        .zip(buf_pr.iter())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    assert!(
        max <= TOL,
        "paired vs pair-less decal max per-channel diff {max} > {TOL} — \
         the per-pair decal two-pass diverged from the pair-less `draw_with_decals`"
    );
}

/// PAIRED op-ORDER regression (the `render_decal_pair` in-order replay fix).
///
/// The interior-castle black-out: sm64's main 3D pair opens with a background FILLRECT, THEN draws
/// opaque geometry, THEN decals (op stream `F T… D…`). The old `render_decal_pair` bucketed ops by
/// KIND (opaque → decal → **rects last**), hoisting that leading FILLRECT into a trailing pass so it
/// repainted OVER the finished scene → black. The fix replays ops in submission order (mirroring
/// the N64 RDP's depth read/write mode switching), so a leading fill stays a background.
///
/// **Hand-reasoning (BEFORE rendering):** ops = [green full-FB FILLRECT, black opaque base quad,
/// magenta coplanar decal quad], all full-screen. In N64 order: green (bg) → black (covers green) →
/// magenta decal (covers black) → the whole FB is MAGENTA. If the FILLRECT is reordered LAST (the
/// bug), it repaints green over everything → the whole FB is GREEN. The center is asserted MAGENTA,
/// so the reorder bug (a green center) fails the test outright.
#[test]
fn golden_paired_decal_respects_op_order() {
    use crate::render::SceneRenderer;
    const DIM: u32 = 64;
    let (device, queue, dual) = headless_device();

    let mut paired = build_decal_scene();
    // Op stream: a leading green full-FB FILLRECT, then the opaque base run, then the decal run.
    let mut ops: Vec<crate::hle::SceneOp> = vec![crate::hle::SceneOp::FillRect {
        rect: crate::hle::Rect {
            ulx: 0,
            uly: 0,
            lrx: DIM as i32 - 1,
            lry: DIM as i32 - 1,
        },
        color_raw: 0x07C1_07C1, // RGBA16 green (0,255,0,255), replicated across both halves
    }];
    ops.extend(paired.draw_runs.drain(..).map(crate::hle::SceneOp::Tris));
    paired.framebuffer_pairs = vec![crate::hle::FramebufferPair {
        color_image: crate::hle::ColorImage {
            fmt: 0,
            siz: 2, // G_IM_SIZ_16b
            width: DIM as u16,
            addr: 0x0010_0000,
        },
        depth_image: Some(0x0020_0000), // distinct from CIMG → real depth FB → `render_decal_pair`
        ops,
        active_scissor: crate::hle::Scissor {
            ulx: 0,
            uly: 0,
            lrx: DIM as i32,
            lry: DIM as i32,
            mode: 0,
        },
        size_extent: (DIM, DIM),
        is_depth_clear: false,
    }];

    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, DIM, DIM, dual);
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &paired, DIM, DIM);
    let c = common::pixel(&buf, DIM, DIM / 2, DIM / 2);
    // Correct in-order render → magenta decal on top. The reorder bug → green fill covers everything.
    assert!(
        c[0] > 180 && c[2] > 180 && c[1] < 90,
        "paired op-order center must be the magenta decal (R>180,B>180,G<90), got {c:?} — a GREEN \
         center means the leading FILLRECT was reordered AFTER the geometry (the interior black-out)"
    );
}

// ── Pair-less facade characterization goldens (Phase 1) ─────────────────────────────────────────
// These route through `SceneRenderer::render`'s PAIR-LESS branch (empty `framebuffer_pairs`) via
// `common::render_to_pixels`, unlike the 21 tier-1/2D goldens (manual `render_scene_with_device`).
// Captured against the current straight-to-target output; the Phase-1 internal-FB rework must keep
// them byte-identical (the present blit at 1:1 is an identity resample).

#[cfg(feature = "asm")]
#[test]
fn golden_pairless_flat_color() {
    let (device, queue, dual) = headless_device();
    let scene = common::scene_from_source("flat-color.n64", &[255u8; 4], 1, 1);
    assert!(
        scene.framebuffer_pairs.is_empty(),
        "flat-color must be a pair-less scene"
    );
    let mut sr =
        crate::render::SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let px = common::render_to_pixels(&device, &queue, &mut sr, &scene, 64, 64);
    compare_or_write("pairless-flat-color", &px, 64, 64);
}

#[cfg(feature = "asm")]
#[test]
fn golden_pairless_chrome_icosphere() {
    let (device, queue, dual) = headless_device();
    let env = common::solid_env_texture([200, 100, 50]);
    let scene = common::scene_from_source("chrome-icosphere.n64", &env, 32, 32);
    assert!(
        scene.framebuffer_pairs.is_empty()
            && scene.render_modes.iter().any(|r| r.z_test || r.z_write),
        "chrome-icosphere must be a pair-less DEPTH scene (exercises the owned depth buffer)"
    );
    let mut sr =
        crate::render::SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let px = common::render_to_pixels(&device, &queue, &mut sr, &scene, 64, 64);
    compare_or_write("pairless-chrome-icosphere", &px, 64, 64);
}

// --- LOD byte-identity regression guard -----------------------------------------------------------
//
// LOD_FRACTION / PRIM_LOD_FRAC (color-C idx 13/14, alpha-C idx 0/6) are wired into `wired()` and
// the WGSL combiner, feeding LOD_FRACTION = 1.0 / PRIM_LOD_FRAC. That changes a golden ONLY if some
// scene actually selects them.
//
// This is the byte-identity proof: assemble + interpret EVERY `tests/scenes/*.n64`, then for every
// drawn material (the exact combine words that reach the shader) INDEPENDENTLY re-decode the four
// color-C and alpha-C slot fields per cycle from the raw combine words and assert none select a LOD
// index in a non-LOD (G_TL_LOD off) draw. It also fails loudly if a FUTURE scene starts referencing
// them non-LOD — a byte-identity tradeoff a human must approve (do NOT regenerate goldens; STOP and
// report).

// Independent raw-word slot decoders (do not trust the CycleSel enums — decode from bits here).
fn lod_guard_bits(v: u32, pos: u32, n: u32) -> u32 {
    (v >> pos) & ((1 << n) - 1)
}

// The combiner computes (a - b) * c + d per channel. A LOD selector in the C (multiply) slot
// changes the OUTPUT only when (a - b) is not provably zero. COLOR A/B annulment is delegated to
// `crate::hle::combiner::color_ab_provably_equal` (single source of truth, unit-tested there
// against a synthetic a_idx==b_idx==6 case) — the two color mux tables are asymmetric, so a naive
// "same index" test is unsound (see that function's doc comment for the full case analysis).
//
// color-C field: cyc0 = L[15,5], cyc1 = L[0,5]; LOD indices = 13 (LOD_FRACTION) / 14
// (PRIM_LOD_FRAC). color A/B: cyc0 a=L[20,4] b=H[28,4]; cyc1 a=L[5,4] b=H[24,4].
fn color_c_lod_affects_output(l: u32, h: u32, second: bool) -> bool {
    let (c_idx, a_idx, b_idx) = if second {
        (
            lod_guard_bits(l, 0, 5),
            lod_guard_bits(l, 5, 4),
            lod_guard_bits(h, 24, 4),
        )
    } else {
        (
            lod_guard_bits(l, 15, 5),
            lod_guard_bits(l, 20, 4),
            lod_guard_bits(h, 28, 4),
        )
    };
    (c_idx == 13 || c_idx == 14) && !crate::hle::combiner::color_ab_provably_equal(a_idx, b_idx)
}
// alpha-C field: cyc0 = L[9,3], cyc1 = H[18,3]; LOD indices = 0 (LOD_FRACTION) / 6
// (PRIM_LOD_FRAC). alpha A/B: cyc0 a=L[12,3] b=H[12,3]; cyc1 a=H[21,3] b=H[3,3]. Unlike color,
// alpha A and B BOTH decode through the single shared `alpha_abd` mux table (ground truth:
// `alpha_abd` in render/combiner_prelude.wgsl and hle/combiner.rs's `alpha_abd`) — so index
// equality alone guarantees value equality on both sides, and `a_idx != b_idx` remains a sound (no
// fix needed) annulment test here.
fn alpha_c_lod_affects_output(l: u32, h: u32, second: bool) -> bool {
    let (c_idx, a_idx, b_idx) = if second {
        (
            lod_guard_bits(h, 18, 3),
            lod_guard_bits(h, 21, 3),
            lod_guard_bits(h, 3, 3),
        )
    } else {
        (
            lod_guard_bits(l, 9, 3),
            lod_guard_bits(l, 12, 3),
            lod_guard_bits(h, 12, 3),
        )
    };
    (c_idx == 0 || c_idx == 6) && a_idx != b_idx
}

/// Per-material half of the LOD byte-identity guard, extracted so it can be exercised directly by
/// a focused unit test (below) as well as by the full 32-scene sweep. `raw_l`/`raw_h` are the raw
/// combine words (`mat.selectors.raw_l/raw_h`), `cycle_type` is `mat.cycle_type`, `is_lod` is
/// `mat.lod`. Returns human-readable violation strings (empty = no output-affecting LOD reference).
///
/// cyc1 is evaluated in BOTH 1-cycle and 2-cycle mode (F3DEX2 1-cycle convention uses the
/// cyc1/index-1 slots — see `build_material`'s own `selectors.cyc1.unwired()` gate, which is
/// likewise unconditional on cycle_type). cyc0 is ONLY live when cycle_type == 1 (2-cycle): in
/// 1-cycle mode the shader never evaluates cyc0, so whatever those bits happen to decode to
/// (including a LOD selector with differing A/B) is dead and must NOT be flagged. Mirrors the
/// codebase's own `cycle_uses_texel0`/`cycle_uses_texel1` pattern (`build_material`), which gates
/// cyc0 checks on `cycle_type == 1` the same way.
fn lod_violations_for_material(
    name: &str,
    raw_l: u32,
    raw_h: u32,
    cycle_type: u32,
    is_lod: bool,
) -> Vec<String> {
    let mut violations = Vec::new();
    // No scene in tests/scenes exercises G_TL_LOD: every material must be non-LOD.
    if is_lod {
        violations.push(format!("{name}: material has G_TL_LOD set (unexpected)"));
    }
    let seconds: &[bool] = if cycle_type == 1 {
        &[false, true]
    } else {
        &[true]
    };
    for &second in seconds {
        if color_c_lod_affects_output(raw_l, raw_h, second) {
            violations.push(format!(
                "{name}: color-C cycle{} selects a LOD index (13/14) with a non-annulled A/B \
                 pair (output-affecting) in a non-LOD draw (combine_l={raw_l:#010x} \
                 combine_h={raw_h:#010x}, cycle_type={cycle_type})",
                second as u8
            ));
        }
        if alpha_c_lod_affects_output(raw_l, raw_h, second) {
            violations.push(format!(
                "{name}: alpha-C cycle{} selects a LOD index (0/6) with A≠B (output-affecting) \
                 in a non-LOD draw (combine_l={raw_l:#010x} combine_h={raw_h:#010x}, \
                 cycle_type={cycle_type})",
                second as u8
            ));
        }
    }
    violations
}

#[cfg(feature = "asm")]
#[test]
fn lod_selectors_unreferenced_in_every_non_lod_scene() {
    let scenes_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scenes");
    // A generic texture is enough: combine words come from G_SETCOMBINE and are texture-independent,
    // so the decoded mux slots do not depend on the pixels we embed.
    let tex = vec![255u8; 64 * 64 * 4];

    let mut scene_count = 0usize;
    let mut material_count = 0usize;
    let mut violations: Vec<String> = Vec::new();

    for entry in std::fs::read_dir(&scenes_dir).expect("scenes dir must exist") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("n64") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(&path).expect("scene source must read");
        let img = match crate::asm::assemble_with_texture(&src, &tex, 64, 64) {
            Ok(img) => img,
            Err(e) => panic!("{name} must assemble for the LOD byte-identity guard: {e:?}"),
        };
        let r = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
        scene_count += 1;

        for mat in &r.scene.materials {
            material_count += 1;
            violations.extend(lod_violations_for_material(
                &name,
                mat.selectors.raw_l,
                mat.selectors.raw_h,
                mat.cycle_type,
                mat.lod,
            ));
        }
    }

    assert!(scene_count > 0, "no scenes were decoded — dir glob failed");
    assert!(
        material_count > 0,
        "no materials decoded across {scene_count} scenes — interpret path broken"
    );
    assert!(
        violations.is_empty(),
        "LOD selectors ARE referenced by a non-LOD scene — wiring LOD_FRACTION=1.0/PRIM_LOD_FRAC \
         would NOT be byte-identical. STOP: do not regenerate goldens; a human must approve.\n{}",
        violations.join("\n")
    );
}

#[test]
fn one_cycle_material_ignores_a_lod_selector_living_in_the_dead_cyc0_slots() {
    // Regression for the byte-identity-guard hardening fix: in 1-cycle mode (cycle_type == 0) the
    // shader only ever evaluates cyc1 slots — cyc0 bits are dead regardless of what they decode to.
    // Construct a combine word whose cyc0 fields decode to a color-C LOD selector (idx 13) with a
    // GENUINELY differing, non-annulled A/B pair (a_idx=1 TEXEL0, b_idx=0 COMBINED) — a pattern
    // that, if checked, the guard would correctly flag as output-affecting. cyc1 is left fully
    // clean (all-zero: A=B=C=COMBINED, no LOD reference) so the ONLY possible violation source is
    // cyc0.
    //
    // color: cyc0 a=L[20,4] b=H[28,4] c=L[15,5]; cyc1 a=L[5,4] b=H[24,4] c=L[0,5].
    let l = (1u32 << 20) | (13u32 << 15); // cyc0: a_idx=1 (TEXEL0), c_idx=13 (LOD_FRACTION)
    let h = 0u32; // cyc0: b_idx=0 (COMBINED) -> a_idx != b_idx, NOT annulled

    // Precondition: if this were checked (2-cycle), it WOULD be flagged.
    assert!(
        color_c_lod_affects_output(l, h, /* second (cyc1) = */ false),
        "precondition: cyc0 must be a genuine, non-annulled color-C LOD reference"
    );
    // Precondition: cyc1 is clean regardless of cycle_type.
    assert!(!color_c_lod_affects_output(
        l, h, /* second (cyc1) = */ true
    ));
    assert!(!alpha_c_lod_affects_output(l, h, false));
    assert!(!alpha_c_lod_affects_output(l, h, true));

    // cycle_type = 0 (1-cycle): the guard must NOT flag the dead cyc0 LOD reference.
    let violations = lod_violations_for_material("synthetic-1-cycle", l, h, 0, false);
    assert!(
        violations.is_empty(),
        "1-cycle material must ignore its dead cyc0 slots, but got: {violations:?}"
    );

    // Sanity check the fix actually gates on cycle_type: the SAME raw words with cycle_type = 1
    // (2-cycle, cyc0 live) MUST be flagged — otherwise this test would pass vacuously.
    let violations_2cycle = lod_violations_for_material("synthetic-2-cycle", l, h, 1, false);
    assert!(
        !violations_2cycle.is_empty(),
        "2-cycle material must flag its live cyc0 LOD reference (sanity check for the gate itself)"
    );
}
