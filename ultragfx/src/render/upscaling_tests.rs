use super::*;
use crate::scene::{ColorImage, FramebufferPair, Rect, SceneOp, Scissor};
use crate::tests::{
    common,
    dl_builder::{Built, Command, DlBuilder},
};
use crate::{ClearPolicy, Hardware, Rdram, RdramImage, Renderer};
use n64_gbi::{consts::*, encode::*};

const A: u32 = 0x100000;
const B: u32 = 0x300000;
const Z: u32 = 0x200000;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const BUDGET: u64 = 256 * 1024 * 1024;

fn fill_scene(address: u32, width: u32, height: u32, word: u32) -> crate::Scene {
    crate::Scene {
        framebuffer_pairs: vec![FramebufferPair {
            color_image: ColorImage {
                addr: address.into(),
                width: width as u16,
                fmt: 0,
                siz: 2,
            },
            active_scissor: Scissor {
                lrx: width as i32,
                lry: height as i32,
                ..Default::default()
            },
            size_extent: (width, height),
            ops: vec![SceneOp::FillRect {
                rect: Rect {
                    ulx: 0,
                    uly: 0,
                    lrx: width as i32 - 1,
                    lry: height as i32 - 1,
                },
                color_raw: word,
                convert: [0; 6],
                key: Default::default(),
            }],
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn read_texture(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let stride = (texture.width() * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("upscaling-readback"),
        size: u64::from(stride) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: if texture.format() == DEPTH_FORMAT {
                wgpu::TextureAspect::DepthOnly
            } else {
                wgpu::TextureAspect::All
            },
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap()
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receive.recv().unwrap().unwrap();
    let data = buffer.slice(..).get_mapped_range();
    data.chunks(stride as usize)
        .flat_map(|row| &row[..texture.width() as usize * 4])
        .copied()
        .collect()
}

fn finish(mut builder: DlBuilder, mut commands: Vec<Command>) -> Built {
    commands.push(gsp_enddl());
    builder.list("main", &commands);
    builder.finish("main")
}

fn triangle(color: u32, depth: u32, z: u16, rgba: u32) -> Built {
    let mut builder = DlBuilder::new();
    let identity = builder.matrix(n64_gbi::gu::gu_scale(1.0, 1.0, 1.0));
    let viewport = builder.viewport(Vp {
        vscale: [64, 48, 511, 0],
        vtrans: [64, 48, 511, 0],
    });
    let vertices = builder.vertices(&[[-1, 1], [1, 1], [-1, -1]].map(|[x, y]| VtxColored {
        x,
        y,
        z: 0,
        flag: 0,
        s: 0,
        t: 0,
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    }));
    let rgb = CcPass {
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
    finish(
        builder,
        vec![
            gsp_matrix(identity, true, true, false),
            gsp_matrix(identity, false, true, false),
            gsp_viewport(viewport),
            gsp_clear_geometrymode(u32::MAX),
            gsp_set_geometrymode(G_SHADE | G_SHADING_SMOOTH),
            gdp_set_color_image(0, 2, 32, color),
            gdp_set_depth_image(depth),
            gdp_set_scissor(0, 0, 0, 128, 96),
            gdp_set_cycle_type(0),
            gdp_set_other_mode_h(4, 2, 3 << 4),
            gdp_set_other_mode_h(6, 2, 3 << 6),
            gdp_set_render_mode(G_ZS_PRIM | if depth != 0 { Z_CMP | Z_UPD } else { 0 }, 0),
            gdp_set_prim_depth(u32::from(z), 0),
            gdp_set_combine_lerp(rgb, alpha, rgb, alpha),
            gdp_set_prim_color(0, 0, rgba),
            gsp_vertex(0, 3, vertices),
            gsp_1triangle(0, 1, 2),
        ],
    )
}

fn scene(built: &Built) -> crate::Scene {
    let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    result.scene
}

#[test]
fn extents_growth_shorter_viewport_and_interior_vi_rows() {
    let (device, queue, dual) = headless_device();
    for scale in [1, 2, 3, 4] {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 33, 35, dual);
        renderer.set_test_scale(scale, BUDGET);
        let mut depth_scene = fill_scene(Z, 33, 35, 0xfffcfffc);
        depth_scene.framebuffer_pairs[0].is_depth_clear = true;
        depth_scene.framebuffer_pairs[0].depth_image = Some(Z.into());
        renderer.render_into_store(&device, &queue, &depth_scene, ClearPolicy::Persist);
        for height in [35, 43, 31] {
            let mut draw = fill_scene(
                A,
                33,
                height,
                if height == 31 { 0xf801f801 } else { 0x07c107c1 },
            );
            depth_scene.framebuffer_pairs[0].size_extent.1 = height;
            depth_scene.framebuffer_pairs[0].active_scissor.lry = height as i32;
            depth_scene.framebuffer_pairs[0].ops.clear();
            renderer.render_into_store(&device, &queue, &depth_scene, ClearPolicy::Persist);
            draw.framebuffer_pairs[0].depth_image = Some(Z.into());
            renderer.render_into_store(&device, &queue, &draw, ClearPolicy::Persist);
            let retained = height.max(if height == 31 { 43 } else { height });
            let fb = &renderer.framebuffers[&workload::TargetId::Guest(A.into())];
            assert_eq!(
                (fb.color.width(), fb.color.height(), fb.height),
                (33, retained, retained)
            );
            let descriptor = renderer.target_descriptors.get(A.into(), false).unwrap();
            assert_eq!(
                descriptor.range().unwrap(),
                u64::from(A)..u64::from(A) + 33 * u64::from(retained) * 2
            );
            let depth = &renderer.depthbuffers[&workload::TargetId::Guest(Z.into())];
            assert_eq!(
                (depth.texture.width(), depth.texture.height()),
                (33, retained)
            );
            for presentation in [
                fb.presentation.as_ref().map(|p| &p.texture),
                depth.presentation.as_ref().map(|p| &p.texture),
            ] {
                assert_eq!(
                    presentation.map(|p| (p.width(), p.height())),
                    (scale > 1).then_some((33 * scale, retained * scale))
                );
            }
        }
        let vi = crate::ViRegisters {
            origin: A + 33 * 7 * 2,
            width: 33,
            status: 2,
            v_start: 34,
            y_scale: 1024,
            ..Default::default()
        };
        let source =
            crate::select_scanout_source(Some(vi), true, None, renderer.scanout_images()).unwrap();
        assert_eq!(source.rows, Some(7..24));
        let pixels =
            common::pixels_from_render(&device, &queue, 33 * scale, 17 * scale, FORMAT, |view| {
                let mut encoder = device.create_command_encoder(&Default::default());
                renderer.scanout(&device, &mut encoder, view, source.target, source.rows);
                queue.submit([encoder.finish()]);
            });
        assert!(pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [255, 0, 0, 255]));
    }
}

#[test]
fn native_stream_is_identical_and_presentation_has_new_samples_without_writeback() {
    let (device, queue, _) = headless_device();
    let built = triangle(A, 0, 0, 0xffffffff);
    let mut native = None;
    for scale in [1, 2, 3, 4] {
        let profiling = crate::profiling::Recorder::new(crate::profiling::Mode::Counters);
        let mut facade = Renderer::with_device(
            device.clone(),
            queue.clone(),
            crate::PresentTarget::Headless {
                format: FORMAT,
                width: 32,
                height: 24,
            },
            crate::RendererConfig {
                clear_policy: ClearPolicy::Persist,
                resolution_multiplier: 1,
                sample_count: 1,
                present_mode: wgpu::PresentMode::Fifo,
                format: Some(FORMAT),
                power_preference: wgpu::PowerPreference::LowPower,
            },
        );
        facade.inner.profiling = profiling.clone();
        facade.inner.set_test_scale(scale, BUDGET);
        let (summary, diagnostics) = submit(&mut facade, &mut Memory(vec![0; 4096]), &built);
        assert!(summary.renderable && diagnostics.is_empty());
        assert!(facade.take_guest_writes().is_empty());
        let renderer = &facade.inner;
        let counters = profiling.snapshot().counters;
        assert_eq!(
            counters
                .get("workload.presentation_segments")
                .copied()
                .unwrap_or(0),
            u64::from(scale > 1)
        );
        assert_eq!(
            counters.get("workload.segments"),
            Some(&(if scale > 1 { 2 } else { 1 }))
        );
        assert_eq!(counters.get("buffer.compute_outputs.creations"), Some(&1));
        assert!(!counters
            .keys()
            .any(|key| key.starts_with("readback.") || key.starts_with("import.")));
        assert!(renderer.coherence.is_none() && renderer.coverage.is_none());
        let fb = &renderer.framebuffers[&workload::TargetId::Guest(A.into())];
        let bytes = read_texture(&device, &queue, &fb.color);
        assert_eq!(native.get_or_insert_with(|| bytes.clone()), &bytes);
        let Some(presentation) = &fb.presentation else {
            assert!(renderer.presentation_transfers.get().is_none());
            assert!(!counters
                .keys()
                .any(|key| key.starts_with("texture.presentation_")));
            continue;
        };
        let enhanced = read_texture(&device, &queue, &presentation.texture);
        let pixel = |x, y| &enhanced[((y * 32 * scale + x) * 4) as usize..][..4];
        assert_eq!(pixel(2 * scale, 2 * scale), [255; 4]);
        assert_eq!(pixel(31 * scale, 23 * scale), [13, 13, 20, 255]);
        let block: Vec<_> = (0..scale)
            .flat_map(|y| (0..scale).map(move |x| (9 * scale + x, 17 * scale + y)))
            .map(|(x, y)| pixel(x, y).to_vec())
            .collect();
        assert!(block.iter().any(|p| p == &[255; 4]));
        assert!(block.iter().any(|p| p == &[13, 13, 20, 255]));
        let mut differs_nearest = false;
        let mut differs_linear = false;
        for y in 0..24 * scale {
            for x in 0..32 * scale {
                let nearest = &bytes[(((y / scale) * 32 + x / scale) * 4) as usize..][..4];
                differs_nearest |= pixel(x, y) != nearest;
                let sx = ((x as f32 + 0.5) / scale as f32 - 0.5).clamp(0.0, 31.0);
                let sy = ((y as f32 + 0.5) / scale as f32 - 0.5).clamp(0.0, 23.0);
                let get = |px: u32, py: u32, c: usize| {
                    f32::from(bytes[((py.min(23) * 32 + px.min(31)) * 4) as usize + c])
                };
                for c in 0..4 {
                    let top = get(sx as u32, sy as u32, c) * (1.0 - sx.fract())
                        + get(sx as u32 + 1, sy as u32, c) * sx.fract();
                    let bottom = get(sx as u32, sy as u32 + 1, c) * (1.0 - sx.fract())
                        + get(sx as u32 + 1, sy as u32 + 1, c) * sx.fract();
                    differs_linear |= pixel(x, y)[c]
                        .abs_diff((top * (1.0 - sy.fract()) + bottom * sy.fract()).round() as u8)
                        > 1;
                }
            }
        }
        assert!(differs_nearest && differs_linear);
    }
}

#[test]
fn shared_presentation_depth_survives_color_switches_and_tasks() {
    let (device, queue, dual) = headless_device();
    for policy in [ClearPolicy::Persist, ClearPolicy::PerFrame] {
        for scale in [2, 3, 4] {
            let mut renderer = SceneRenderer::new(&device, FORMAT, 32, 24, dual);
            renderer.set_test_scale(scale, BUDGET);
            for (color, depth, z, rgba, expected) in [
                (A, Z, 0x4000, 0xff0000ff, [255, 0, 0, 255]),
                (B, Z, 0xc000, 0x00ff00ff, [13, 13, 20, 255]),
                (A, Z, 0xc000, 0x0000ffff, [255, 0, 0, 255]),
                (B, Z + 0x10000, 0xc000, 0x00ff00ff, [0, 255, 0, 255]),
            ] {
                let draw = scene(&triangle(color, depth, z, rgba));
                renderer.render_into_store(&device, &queue, &draw, policy);
                let fb = &renderer.framebuffers[&workload::TargetId::Guest(color.into())];
                for (texture, factor) in [
                    (&fb.color, 1),
                    (&fb.presentation.as_ref().unwrap().texture, scale),
                ] {
                    let bytes = read_texture(&device, &queue, texture);
                    assert_eq!(
                        &bytes[((2 * factor * texture.width() + 2 * factor) * 4) as usize..][..4],
                        expected
                    );
                }
                assert!(
                    renderer.diagnostics.is_empty(),
                    "{:?}",
                    renderer.diagnostics
                );
            }
            assert_eq!(renderer.depthbuffers.len(), 2);
            let depth = &renderer.depthbuffers[&workload::TargetId::Guest(Z.into())];
            assert!(depth.presentation.is_some());
        }
    }
}

struct Memory(Vec<u8>);
impl Hardware for Memory {
    fn rdram(&self) -> impl Rdram + '_ {
        RdramImage::new(&self.0)
    }
}

fn facade(scale: u32, fallback: bool) -> Renderer {
    let (device, queue) = if fallback {
        headless_device_forced_fallback()
    } else {
        let (device, queue, _) = headless_device();
        (device, queue)
    };
    Renderer::with_device(
        device,
        queue,
        crate::PresentTarget::Headless {
            format: FORMAT,
            width: 32,
            height: 24,
        },
        crate::RendererConfig {
            clear_policy: ClearPolicy::Persist,
            resolution_multiplier: scale,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: Some(FORMAT),
            power_preference: wgpu::PowerPreference::LowPower,
        },
    )
}

fn submit(
    renderer: &mut Renderer,
    memory: &mut Memory,
    built: &Built,
) -> (crate::DlSummary, Vec<crate::Diagnostic>) {
    memory.0[..built.rdram.len()].copy_from_slice(&built.rdram);
    let mut diagnostics = Vec::new();
    let summary = renderer.process_dl(
        memory,
        built.entry.into(),
        crate::Microcode::F3dex2,
        &mut diagnostics,
    );
    (summary, diagnostics)
}

fn apply(renderer: &mut Renderer, memory: &mut Memory) -> Vec<crate::GuestWrite> {
    let patches = renderer.take_guest_writes();
    for patch in &patches {
        memory.0[patch.address as usize..patch.address as usize + patch.bytes.len()]
            .copy_from_slice(&patch.bytes);
    }
    patches
}

fn fill_commands(address: u32, width: u32, height: u32, word: u32) -> Vec<Command> {
    vec![
        gdp_set_color_image(0, 2, width, address),
        gdp_set_scissor(0, 0, 0, width * 4, height * 4),
        gdp_set_cycle_type(3),
        gdp_set_render_mode(0, 0),
        gdp_set_fill_color(word),
        gdp_fill_rectangle(0, 0, (width - 1) * 4, (height - 1) * 4),
    ]
}

fn fill_task(address: u32, width: u32, height: u32, word: u32) -> Built {
    finish(
        DlBuilder::new(),
        fill_commands(address, width, height, word),
    )
}

#[test]
fn writeback_bytes_coverage_patches_and_guards_are_scale_independent() {
    for fallback in [false, true] {
        for encoding in [
            crate::FramebufferEncoding::Native,
            crate::FramebufferEncoding::BigEndian,
        ] {
            let mut baseline = None;
            for scale in [1, 3] {
                let mut renderer = facade(scale, fallback);
                renderer.set_guest_writeback_with_encoding(true, encoding);
                let mut memory = Memory(vec![0xa5; B as usize + 4096]);
                let tasks = [
                    triangle(A, Z, 0x4000, 0x4080c0ff),
                    fill_task(A, 32, 37, 0xf801f801),
                    triangle(B, Z, 0xc000, 0x00ff00ff),
                    triangle(A, Z, 0x6000, 0x0000ffff),
                ];
                let mut patches = Vec::new();
                let mut coverage = Vec::new();
                for (index, task) in tasks.iter().enumerate() {
                    if index == 1 {
                        let word = match encoding {
                            crate::FramebufferEncoding::Native => 0x07c1u16.to_ne_bytes(),
                            crate::FramebufferEncoding::BigEndian => 0x07c1u16.to_be_bytes(),
                        };
                        memory.0[A as usize + (32 * 12 + 10) * 2..][..2].copy_from_slice(&word);
                    }
                    let (summary, diagnostics) = submit(&mut renderer, &mut memory, task);
                    assert!(diagnostics.is_empty(), "{diagnostics:?}");
                    assert!(summary.renderable);
                    patches.push(apply(&mut renderer, &mut memory));
                    coverage.push(
                        (0..32 * 24)
                            .map(|pixel| {
                                renderer
                                    .inner
                                    .coherence
                                    .as_ref()
                                    .unwrap()
                                    .stored_coverage_code(u64::from(A) + pixel * 2)
                            })
                            .collect::<Vec<_>>(),
                    );
                }
                let color = read_texture(
                    renderer.device(),
                    renderer.queue(),
                    &renderer.inner.framebuffers[&workload::TargetId::Guest(A.into())].color,
                );
                let depth = read_texture(
                    renderer.device(),
                    renderer.queue(),
                    &renderer.inner.depthbuffers[&workload::TargetId::Guest(Z.into())].texture,
                );
                let result = (
                    memory.0[A as usize - 8..A as usize + 32 * 37 * 2 + 8].to_vec(),
                    memory.0[Z as usize - 8..Z as usize + 32 * 37 * 2 + 8].to_vec(),
                    patches,
                    coverage,
                    color,
                    depth,
                );
                assert!(result.0[..8]
                    .iter()
                    .chain(result.0[result.0.len() - 8..].iter())
                    .all(|&byte| byte == 0xa5));
                assert!(result.1[..8]
                    .iter()
                    .chain(result.1[result.1.len() - 8..].iter())
                    .all(|&byte| byte == 0xa5));
                assert_eq!(
                    baseline.get_or_insert_with(|| result.clone()),
                    &result,
                    "scale {scale}, fallback {fallback}, encoding {encoding:?}"
                );
            }
        }
    }
}

#[test]
fn sparse_cpu_imports_preserve_untouched_presentation_samples() {
    for scale in [2, 3, 4] {
        let mut renderer = facade(scale, false);
        renderer.set_guest_writeback_with_encoding(true, crate::FramebufferEncoding::BigEndian);
        let mut memory = Memory(vec![0; B as usize + 4096]);
        let (summary, diagnostics) = submit(
            &mut renderer,
            &mut memory,
            &triangle(A, Z, 0x4000, 0xffffffff),
        );
        assert!(summary.renderable && diagnostics.is_empty());
        apply(&mut renderer, &mut memory);
        let id = workload::TargetId::Guest(A.into());
        let depth_id = workload::TargetId::Guest(Z.into());
        let color_before = read_texture(
            renderer.device(),
            renderer.queue(),
            &renderer.inner.framebuffers[&id]
                .presentation
                .as_ref()
                .unwrap()
                .texture,
        );
        let depth_before = read_texture(
            renderer.device(),
            renderer.queue(),
            &renderer.inner.depthbuffers[&depth_id]
                .presentation
                .as_ref()
                .unwrap()
                .texture,
        );
        let pixel = 2 * 32 + 2;
        memory.0[A as usize + pixel * 2..][..2].copy_from_slice(&0x07c1u16.to_be_bytes());
        memory.0[Z as usize + pixel * 2..][..2].copy_from_slice(&0xc000u16.to_be_bytes());
        let no_draw = finish(DlBuilder::new(), vec![]);
        let (summary, diagnostics) = submit(&mut renderer, &mut memory, &no_draw);
        assert!(!summary.renderable && diagnostics.is_empty());
        assert!(renderer.take_guest_writes().is_empty());
        for (before, texture, expected) in [
            (
                color_before,
                &renderer.inner.framebuffers[&id]
                    .presentation
                    .as_ref()
                    .unwrap()
                    .texture,
                [0, 255, 0, 255],
            ),
            (
                depth_before,
                &renderer.inner.depthbuffers[&depth_id]
                    .presentation
                    .as_ref()
                    .unwrap()
                    .texture,
                (0x3f000 as f32 / 262144.0).to_le_bytes(),
            ),
        ] {
            let after = read_texture(renderer.device(), renderer.queue(), texture);
            for y in 0..24 * scale {
                for x in 0..32 * scale {
                    let offset = ((y * 32 * scale + x) * 4) as usize;
                    if x / scale == 2 && y / scale == 2 {
                        assert_eq!(&after[offset..offset + 4], expected);
                    } else {
                        assert_eq!(
                            &after[offset..offset + 4],
                            &before[offset..offset + 4],
                            "({x},{y}), scale {scale}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn preflight_rejects_whole_task_and_preserves_cpu_imports() {
    use crate::diag::ResourceAdmission;
    for reason in [
        ResourceAdmission::Dimensions,
        ResourceAdmission::ArithmeticOverflow,
        ResourceAdmission::AttachmentBudget,
    ] {
        let scale = if reason == ResourceAdmission::Dimensions {
            32
        } else {
            2
        };
        let mut renderer = facade(2, false);
        renderer.inner.set_test_scale(scale, BUDGET);
        renderer.set_guest_writeback_with_encoding(true, crate::FramebufferEncoding::BigEndian);
        let mut memory = Memory(vec![0; B as usize + 4096]);
        let (_, diagnostics) = submit(
            &mut renderer,
            &mut memory,
            &triangle(A, Z, 0x4000, 0xffffffff),
        );
        assert!(diagnostics.is_empty());
        apply(&mut renderer, &mut memory);
        if reason == ResourceAdmission::ArithmeticOverflow {
            renderer.reset_rdp_state();
            renderer.inner.fb_w = u32::MAX;
        }
        let saved_rdp = renderer.rdp.clone();
        let saved_epoch = renderer.color_image_epoch;
        let descriptors = renderer.inner.target_descriptors.clone();
        let scanout = renderer.last_scanout_addr;
        let first_touch = renderer.inner.first_touch.clone();
        let depth_first_touch = renderer.inner.depth_first_touch.clone();
        let id = workload::TargetId::Guest(A.into());
        let native = renderer.inner.framebuffers[&id].color.clone();
        let presentation = renderer.inner.framebuffers[&id]
            .presentation
            .as_ref()
            .unwrap()
            .texture
            .clone();
        let depth = &renderer.inner.depthbuffers[&workload::TargetId::Guest(Z.into())];
        let saved_depth: Vec<_> = [
            &depth.texture,
            &depth.presentation.as_ref().unwrap().texture,
        ]
        .into_iter()
        .map(|texture| {
            (
                texture.clone(),
                read_texture(renderer.device(), renderer.queue(), texture),
            )
        })
        .collect();
        let mut expected_native = read_texture(renderer.device(), renderer.queue(), &native);
        let mut expected_presentation =
            read_texture(renderer.device(), renderer.queue(), &presentation);
        let pixel = 2 * 32 + 2;
        memory.0[A as usize + pixel * 2..][..2].copy_from_slice(&0x07c1u16.to_be_bytes());
        expected_native[pixel * 4..][..4].copy_from_slice(&[0, 255, 0, 255]);
        for y in 2 * scale..3 * scale {
            for x in 2 * scale..3 * scale {
                expected_presentation[((y * 32 * scale + x) * 4) as usize..][..4]
                    .copy_from_slice(&[0, 255, 0, 255]);
            }
        }
        let mut commands = fill_commands(A, 32, 24, 0xf801f801);
        commands.extend(fill_commands(B, 1024, 24, 0x003f003f));
        commands.extend([
            gdp_set_prim_color(0, 0, 0x12345678),
            gdp_set_texture_image(0, 2, 32, A),
            gdp_set_tile(0, 2, 8, 0, 7, 0, 0, 0, 0, 0, 0, 0),
            gdp_load_block(7, 0, 0, 31, 0),
        ]);
        let mut task = finish(DlBuilder::new(), commands);
        match reason {
            ResourceAdmission::Dimensions => {}
            ResourceAdmission::ArithmeticOverflow => {
                task = triangle(A, 0, 0, 0xffffffff);
                for command in task.rdram[task.entry as usize..].as_chunks_mut::<8>().0 {
                    if command[0] == 0xff {
                        command.copy_from_slice(&[0xe7, 0, 0, 0, 0, 0, 0, 0]);
                    }
                }
            }
            ResourceAdmission::AttachmentBudget => {
                let residency = u64::from(native.width() * native.height() * 4) * 2 * 5;
                let growth = 1024 * 24 * 4 * 5;
                renderer.inner.attachment_budget = residency + growth / 2;
            }
            _ => unreachable!(),
        }
        let (summary, diagnostics) = submit(&mut renderer, &mut memory, &task);
        assert_eq!(
            summary.termination,
            crate::inspect::WalkTermination::Rejected,
            "{diagnostics:?}"
        );
        assert!(diagnostics.iter().any(|d| matches!(d.kind, crate::DiagKind::ResourceAdmission { reason: actual, .. } if actual == reason)), "{diagnostics:?}");
        assert_eq!(renderer.rdp, saved_rdp);
        assert_eq!(renderer.color_image_epoch, saved_epoch);
        assert_eq!(renderer.inner.target_descriptors, descriptors);
        assert_eq!(renderer.last_scanout_addr, scanout);
        assert_eq!(renderer.inner.first_touch, first_touch);
        assert_eq!(renderer.inner.depth_first_touch, depth_first_touch);
        assert_eq!(renderer.inner.framebuffers.len(), 1);
        assert_eq!(renderer.inner.framebuffers[&id].color, native);
        assert_eq!(
            renderer.inner.framebuffers[&id]
                .presentation
                .as_ref()
                .unwrap()
                .texture,
            presentation
        );
        assert_eq!(
            read_texture(renderer.device(), renderer.queue(), &native),
            expected_native
        );
        assert_eq!(
            read_texture(renderer.device(), renderer.queue(), &presentation),
            expected_presentation
        );
        for (texture, bytes) in saved_depth {
            assert_eq!(
                read_texture(renderer.device(), renderer.queue(), &texture),
                bytes
            );
        }
        assert!(renderer.take_guest_writes().is_empty());
    }
}

#[test]
fn preflight_checks_storage_bindings() {
    let (device, _, dual) = headless_device();
    let renderer = SceneRenderer::new(&device, FORMAT, 193, 131, dual);
    let triangle = scene(&triangle(A, 0, 0, 0xffffffff));
    let mut inputs = inputs::RenderInputs::new(&triangle, (32, 24), [0; 2]).unwrap();
    let mut limits = device.limits();
    limits.max_storage_buffer_binding_size = 16;
    assert!(matches!(
        renderer
            .preflight_inputs(&mut inputs, &limits)
            .unwrap_err()
            .kind,
        crate::DiagKind::ResourceAdmission {
            reason: crate::diag::ResourceAdmission::StorageBinding,
            ..
        }
    ));
    assert!(renderer.framebuffers.is_empty() && renderer.depthbuffers.is_empty());
}

#[test]
fn per_frame_clears_reset_both_representations() {
    let (device, queue, dual) = headless_device();
    for scale in [2, 3, 4] {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 32, 24, dual);
        renderer.set_test_scale(scale, BUDGET);
        renderer.render_into_store(
            &device,
            &queue,
            &fill_scene(A, 32, 24, 0x07c107c1),
            ClearPolicy::PerFrame,
        );
        renderer.begin_frame();
        let mut partial = fill_scene(A, 32, 24, 0xf801f801);
        if let SceneOp::FillRect { rect, .. } = &mut partial.framebuffer_pairs[0].ops[0] {
            *rect = Rect {
                ulx: 4,
                uly: 4,
                lrx: 7,
                lry: 7,
            };
        }
        renderer.render_into_store(&device, &queue, &partial, ClearPolicy::PerFrame);
        let fb = &renderer.framebuffers[&workload::TargetId::Guest(A.into())];
        for (texture, factor) in [
            (&fb.color, 1),
            (&fb.presentation.as_ref().unwrap().texture, scale),
        ] {
            let bytes = read_texture(&device, &queue, texture);
            for y in 0..24 * factor {
                for x in 0..32 * factor {
                    let expected =
                        if (4..8).contains(&(x / factor)) && (4..8).contains(&(y / factor)) {
                            [255, 0, 0, 255]
                        } else {
                            [13, 13, 20, 255]
                        };
                    assert_eq!(
                        &bytes[((y * 32 * factor + x) * 4) as usize..][..4],
                        expected
                    );
                }
            }
        }
    }
}

#[cfg(feature = "capture")]
#[test]
fn capture_readback_selects_native_while_scanout_selects_presentation() {
    let mut renderer = facade(3, false);
    let mut memory = Memory(vec![0; 4096]);
    let (summary, diagnostics) = submit(&mut renderer, &mut memory, &triangle(A, 0, 0, 0xffffffff));
    assert!(summary.renderable && diagnostics.is_empty());
    let fb = &renderer.inner.framebuffers[&workload::TargetId::Guest(A.into())];
    let native = read_texture(renderer.device(), renderer.queue(), &fb.color);
    let presentation = read_texture(
        renderer.device(),
        renderer.queue(),
        &fb.presentation.as_ref().unwrap().texture,
    );
    let device = renderer.device.clone();
    let queue = renderer.queue.clone();
    assert_eq!(
        common::pixels_from_render(&device, &queue, 32, 24, FORMAT, |view| renderer
            .present_native_to(&memory, view)),
        native
    );
    assert_eq!(
        common::pixels_from_render(&device, &queue, 96, 72, FORMAT, |view| renderer
            .present_to(&memory, view)),
        presentation
    );
}

#[test]
fn legacy_scissor_uses_existing_arithmetic_at_the_presentation_viewport() {
    let (device, _, dual) = headless_device();
    let mut renderer = SceneRenderer::new(&device, FORMAT, 193, 131, dual);
    renderer.set_test_scale(3, BUDGET);
    let scene = scene(&triangle(A, 0, 0, 0xffffffff));
    let mut inputs = inputs::RenderInputs::new(&scene, (193, 131), [0; 2]).unwrap();
    let target = &mut inputs.targets[0];
    target.id = workload::TargetId::Legacy;
    target.extents.logical = (320, 240);
    target.logical_scissors[0] = Scissor {
        ulx: 160,
        uly: 120,
        lrx: 320,
        lry: 240,
        ..Default::default()
    };
    renderer
        .preflight_inputs(&mut inputs, &device.limits())
        .unwrap();
    assert_eq!(
        inputs.targets[0].presentation_scissor(0),
        (289, 196, 290, 197)
    );
}

#[cfg(feature = "capture")]
#[test]
fn task_boundary_discards_depth_before_an_early_return() {
    let mut renderer = facade(1, false);
    renderer.set_guest_writeback_with_encoding(true, crate::FramebufferEncoding::BigEndian);
    renderer.set_depth_reset_policy(crate::DepthResetPolicy::TaskBoundary);
    let mut memory = Memory(vec![0; B as usize + 4096]);
    let (summary, diagnostics) = submit(
        &mut renderer,
        &mut memory,
        &triangle(A, Z, 0x4000, 0xffffffff),
    );
    assert!(summary.renderable && diagnostics.is_empty());
    apply(&mut renderer, &mut memory);
    assert!(!renderer.inner.depthbuffers.is_empty());
    assert!(!renderer
        .inner
        .coherence
        .as_ref()
        .unwrap()
        .owners
        .0
        .is_empty());
    let mut diagnostics = Vec::new();
    let summary = renderer.process_dl(
        &memory,
        u64::from(u32::MAX),
        crate::Microcode::F3dex2,
        &mut diagnostics,
    );
    assert!(!summary.renderable);
    assert!(diagnostics
        .iter()
        .any(|d| matches!(d.kind, crate::DiagKind::MemoryRead { .. })));
    assert!(renderer.inner.depthbuffers.is_empty());
    assert!(renderer
        .inner
        .coherence
        .as_ref()
        .unwrap()
        .owners
        .0
        .is_empty());
}

fn assert_rgba(texture: &wgpu::Texture, bytes: &[u8], expected: impl Fn(u32, u32) -> [u8; 4]) {
    assert_rgba_samples(texture, bytes, |x, y| Some(expected(x, y)));
}

fn assert_rgba_samples(
    texture: &wgpu::Texture,
    bytes: &[u8],
    expected: impl Fn(u32, u32) -> Option<[u8; 4]>,
) {
    for (i, pixel) in bytes.as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (i as u32 % texture.width(), i as u32 / texture.width());
        if let Some(expected) = expected(x, y) {
            assert_eq!(*pixel, expected, "({x},{y})");
        }
    }
}

fn texel_boundary_distance(coordinate: f64, rate: f64, slope: f64, scale: u32) -> f64 {
    let texel = coordinate * rate;
    (texel - texel.round()).abs() * f64::from(scale) / (rate * slope.abs())
}

#[test]
fn packed_fill_lanes_follow_native_cells_at_odd_width() {
    let (device, queue, dual) = headless_device();
    for scale in [2, 3, 4] {
        for siz in [1, 2] {
            let mut renderer = SceneRenderer::new(&device, FORMAT, 9, 5, dual);
            renderer.set_test_scale(scale, BUDGET);
            let mut draw = fill_scene(A, 9, 5, if siz == 2 { 0xf80107c1 } else { 0x204080ff });
            draw.framebuffer_pairs[0].color_image.siz = siz;
            renderer.render_into_store(&device, &queue, &draw, ClearPolicy::Persist);
            let texture = &renderer.framebuffers[&workload::TargetId::Guest(A.into())]
                .presentation
                .as_ref()
                .unwrap()
                .texture;
            let bytes = read_texture(&device, &queue, texture);
            assert_rgba(texture, &bytes, |x, y| {
                let pixel = y / scale * 9 + x / scale;
                if siz == 2 {
                    if pixel & 1 == 0 {
                        [255, 0, 0, 255]
                    } else {
                        [0, 255, 0, 255]
                    }
                } else {
                    let value = [32, 64, 128, 255][(pixel & 3) as usize];
                    [value; 4]
                }
            });
        }
        let mut renderer = SceneRenderer::new(&device, FORMAT, 9, 5, dual);
        renderer.set_test_scale(scale, BUDGET);
        let mut draw = fill_scene(Z, 9, 5, 0x0000c000);
        draw.framebuffer_pairs[0].is_depth_clear = true;
        draw.framebuffer_pairs[0].depth_image = Some(Z.into());
        renderer.render_into_store(&device, &queue, &draw, ClearPolicy::Persist);
        let texture = &renderer.depthbuffers[&workload::TargetId::Guest(Z.into())]
            .presentation
            .as_ref()
            .unwrap()
            .texture;
        let bytes = read_texture(&device, &queue, texture);
        assert_rgba(texture, &bytes, |x, y| {
            let depth: f32 = if (y / scale * 9 + x / scale) & 1 == 0 {
                0.0
            } else {
                0.984375
            };
            depth.to_le_bytes()
        });
    }
}

#[test]
fn fractional_scissors_ceil_each_boundary_before_scaling() {
    let (device, queue, dual) = headless_device();
    for scale in [2, 3, 4] {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 9, 7, dual);
        renderer.set_test_scale(scale, BUDGET);
        for depth in [false, true] {
            for boundary in 0..4 {
                for residue in 0..4 {
                    let mut raw = [4, 4, 28, 20];
                    raw[boundary] += residue;
                    let address = if depth { Z } else { A };
                    let mut commands =
                        fill_commands(address, 9, 7, if depth { 0xc000c000 } else { 0xf801f801 });
                    commands.insert(1, gdp_set_depth_image(if depth { Z } else { 0 }));
                    commands[2] = gdp_set_scissor(0, raw[0], raw[1], raw[2], raw[3]);
                    let mut draw = scene(&finish(DlBuilder::new(), commands));
                    draw.framebuffer_pairs[0].size_extent = (9, 7);
                    let mut inputs = inputs::RenderInputs::new(&draw, (9, 7), [0; 2]).unwrap();
                    renderer
                        .preflight_inputs(&mut inputs, &device.limits())
                        .unwrap();
                    let l = raw.map(|v| v.div_ceil(4));
                    assert_eq!(
                        inputs.targets[0].presentation_scissor(0),
                        (
                            l[0] * scale,
                            l[1] * scale,
                            (l[2] - l[0]) * scale,
                            (l[3] - l[1]) * scale
                        ),
                        "scale {scale}, boundary {boundary}, residue {residue}, depth {depth}"
                    );
                    renderer.begin_frame();
                    renderer.render_into_store(&device, &queue, &draw, ClearPolicy::PerFrame);
                    let texture = if depth {
                        &renderer.depthbuffers[&workload::TargetId::Guest(Z.into())]
                            .presentation
                            .as_ref()
                            .unwrap()
                            .texture
                    } else {
                        &renderer.framebuffers[&workload::TargetId::Guest(A.into())]
                            .presentation
                            .as_ref()
                            .unwrap()
                            .texture
                    };
                    let bytes = read_texture(&device, &queue, texture);
                    assert_rgba(texture, &bytes, |x, y| {
                        let inside = (l[0]..l[2]).contains(&(x / scale))
                            && (l[1]..l[3]).contains(&(y / scale));
                        if depth {
                            let value: f32 = if inside { 0.984375 } else { 1.0 };
                            value.to_le_bytes()
                        } else if inside {
                            [255, 0, 0, 255]
                        } else {
                            [13, 13, 20, 255]
                        }
                    });
                }
            }
        }
    }
}

fn texrect_scene(
    base: [i16; 2],
    step: [i16; 2],
    flip: bool,
    copy: bool,
    fractional_y: bool,
) -> crate::Scene {
    let mut draw = fill_scene(A, 16, 12, 0);
    let mut material = scene(&triangle(A, 0, 0, 0xffffffff)).materials.remove(0);
    let rgb = CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: 1,
    };
    let alpha = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 1,
    };
    let (l, h) = gdp_set_combine_lerp(rgb, alpha, rgb, alpha);
    material.selectors = crate::hle::combiner::decode_combine(l, h);
    material.tex_enable = true;
    material.tex_w = 16;
    material.tex_h = 12;
    material.tile_count = 1;
    material.sampling = crate::hle::tile_sampling::TileSampling::from_tile(
        &crate::hle::rdp::TileDescriptor {
            width: 16,
            height: 12,
            cms: 2,
            cmt: 2,
            ..Default::default()
        },
        0,
    );
    material.texture = crate::hle::texture_request::TextureSource::Rgba(
        (0..12)
            .flat_map(|y| (0..16).flat_map(move |x| [x * 12, y * 16, 0, 255]))
            .collect::<Vec<u8>>()
            .into(),
    );
    draw.materials = vec![material];
    draw.render_modes = vec![crate::hle::blender::decode_render_mode(0, 0, 0)];
    draw.framebuffer_pairs[0].ops = vec![SceneOp::TexRect {
        rect: crate::scene::TexRectBounds {
            ulx: 8,
            uly: 8 + i32::from(fractional_y),
            lrx: if copy { 20 } else { 24 },
            lry: if copy { 20 } else { 24 },
        },
        tile: 0,
        uls: base[0],
        ult: base[1],
        dsdx: step[0],
        dtdy: step[1],
        flip,
        copy_mode: copy,
        material_index: 0,
        render_mode_index: 0,
        fog_color: [0; 4],
        prim_depth: Default::default(),
        fb_source: None,
    }];
    draw
}

#[test]
fn identity_texrect_phase_uses_both_signed_derivatives() {
    let (device, queue, dual) = headless_device();
    for scale in [2, 3, 4] {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 16, 12, dual);
        renderer.set_test_scale(scale, BUDGET);
        for flip in [false, true] {
            for copy in [false, true] {
                for negative_s in [false, true] {
                    for negative_t in [false, true] {
                        for fractional_y in [false, true] {
                            if copy && fractional_y {
                                continue;
                            }
                            let ds = if negative_s { -1024 } else { 1024 };
                            let dt = if negative_t { -1024 } else { 1024 };
                            let draw = texrect_scene(
                                [168, 168],
                                [if copy { ds * 4 } else { ds }, dt],
                                flip,
                                copy,
                                fractional_y,
                            );
                            renderer.begin_frame();
                            renderer.render_into_store(
                                &device,
                                &queue,
                                &draw,
                                ClearPolicy::PerFrame,
                            );
                            let texture = &renderer.framebuffers
                                [&workload::TargetId::Guest(A.into())]
                                .presentation
                                .as_ref()
                                .unwrap()
                                .texture;
                            let bytes = read_texture(&device, &queue, texture);
                            let top = if fractional_y { 3 } else { 2 };
                            assert_rgba(texture, &bytes, |x, y| {
                                let (x, y) = (x / scale, y / scale);
                                if !(2..6).contains(&x) || !(top..6).contains(&y) {
                                    return [13, 13, 20, 255];
                                }
                                let (s, t) = if flip {
                                    (y - top, x - 2)
                                } else {
                                    (x - 2, y - top)
                                };
                                let s = if negative_s { 4 - s } else { 5 + s };
                                let t = if negative_t {
                                    4 - t - u32::from(fractional_y)
                                } else {
                                    5 + t + u32::from(fractional_y)
                                };
                                [(s * 12) as u8, (t * 16) as u8, 0, 255]
                            });
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn raw_only_scissor_change_keeps_triangle_operations_and_retained_rdp_state() {
    let mut built = triangle(A, 0, 0, 0xffffffff);
    built.rdram.truncate(built.rdram.len() - 8);
    for command in [
        gdp_set_scissor(0, 1, 0, 128, 96),
        gsp_1triangle(0, 1, 2),
        gsp_enddl(),
    ] {
        built.rdram.extend(command.0.to_be_bytes());
        built.rdram.extend(command.1.to_be_bytes());
    }
    let draw = scene(&built);
    let inputs = inputs::RenderInputs::new(&draw, (32, 24), [0; 2]).unwrap();
    assert_eq!(inputs.targets[0].operations.len(), 2);
    let (device, _, dual) = headless_device();
    let mut renderer = SceneRenderer::new(&device, FORMAT, 32, 24, dual);
    renderer.set_test_scale(3, BUDGET);
    let mut inputs = inputs;
    renderer
        .preflight_inputs(&mut inputs, &device.limits())
        .unwrap();
    assert_eq!(inputs.targets[0].presentation_scissor(0), (0, 0, 96, 72));
    assert_eq!(inputs.targets[0].presentation_scissor(1), (3, 0, 93, 72));
    assert_eq!(
        inputs.targets[0].operations[0].scissor,
        inputs.targets[0].operations[1].scissor
    );
}

#[test]
fn filtered_fractional_phase_and_intentionally_scaled_rects() {
    let (device, queue, dual) = headless_device();
    for scale in [2, 3, 4] {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 16, 12, dual);
        renderer.set_test_scale(scale, BUDGET);
        for step in [512i16, 1024, -1024] {
            for flip in [false, true] {
                let mut draw = texrect_scene([168, 168], [step, step], flip, false, false);
                draw.materials[0].filter_mode = 2;
                renderer.begin_frame();
                renderer.render_into_store(&device, &queue, &draw, ClearPolicy::PerFrame);
                let texture = &renderer.framebuffers[&workload::TargetId::Guest(A.into())]
                    .presentation
                    .as_ref()
                    .unwrap()
                    .texture;
                let bytes = read_texture(&device, &queue, texture);
                for y in 2 * scale..6 * scale {
                    for x in 2 * scale..6 * scale {
                        let position = |p: u32| {
                            if step.unsigned_abs() == 1024 {
                                (p / scale - 2) as f32 + 0.5
                            } else {
                                (p as f32 + 0.5) / scale as f32 - 2.0
                            }
                        };
                        let (s, t) = if flip {
                            (position(y), position(x))
                        } else {
                            (position(x), position(y))
                        };
                        let coordinate = |p: f32| {
                            ((5.25 + p * f32::from(step) / 1024.0) * 128.0).round() / 128.0
                        };
                        let expected = [
                            (coordinate(s) * 12.0).round() as u8,
                            (coordinate(t) * 16.0).round() as u8,
                            0,
                            255,
                        ];
                        let pixel = &bytes[((y * texture.width() + x) * 4) as usize..][..4];
                        for channel in 0..4 {
                            assert!(pixel[channel].abs_diff(expected[channel]) <= 1, "({x},{y}) scale {scale}, step {step}, flip {flip}: {pixel:?} vs {expected:?}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn clipped_empty_and_partial_depth_scissors_use_completed_native_bounds() {
    let (device, queue, dual) = headless_device();
    for scale in [2, 3, 4] {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 9, 7, dual);
        renderer.set_test_scale(scale, BUDGET);
        for (raw, expected) in [
            ([1, 1, 60, 60], [1, 1, 9, 7]),
            ([36, 0, 60, 28], [9, 0, 9, 7]),
            ([4, 4, 4, 20], [1, 1, 1, 5]),
            ([0, 20, 36, 4], [0, 5, 9, 1]),
        ] {
            let mut commands = fill_commands(Z, 9, 7, 0xc000c000);
            commands.insert(1, gdp_set_depth_image(Z));
            commands[2] = gdp_set_scissor(0, raw[0], raw[1], raw[2], raw[3]);
            *commands.last_mut().unwrap() = gdp_fill_rectangle(8, 4, 24, 12);
            let mut draw = scene(&finish(DlBuilder::new(), commands));
            draw.framebuffer_pairs[0].size_extent = (9, 7);
            renderer.begin_frame();
            renderer.render_into_store(&device, &queue, &draw, ClearPolicy::PerFrame);
            let texture = &renderer.depthbuffers[&workload::TargetId::Guest(Z.into())]
                .presentation
                .as_ref()
                .unwrap()
                .texture;
            let bytes = read_texture(&device, &queue, texture);
            assert_rgba(texture, &bytes, |x, y| {
                let (x, y) = (x / scale, y / scale);
                let inside = (expected[0]..expected[2]).contains(&x)
                    && (expected[1]..expected[3]).contains(&y)
                    && (2..7).contains(&x)
                    && (1..4).contains(&y);
                (if inside { 0.984375f32 } else { 1.0f32 }).to_le_bytes()
            });
        }
    }
}

fn alpha_threshold(x: u32, y: u32, width: u32, serial: u32, seed: u32) -> u8 {
    let (mut a, mut b, mut sum) = (serial ^ seed, x + width * y, 0u32);
    for _ in 0..16 {
        sum = sum.wrapping_add(0x9e3779b9);
        a = a.wrapping_add(
            ((b << 4).wrapping_add(0xa341316c))
                ^ b.wrapping_add(sum)
                ^ ((b >> 5).wrapping_add(0xc8013ea4)),
        );
        b = b.wrapping_add(
            ((a << 4).wrapping_add(0xad90777d))
                ^ a.wrapping_add(sum)
                ^ ((a >> 5).wrapping_add(0x7e95761e)),
        );
    }
    (a.wrapping_mul(1664525).wrapping_add(1013904223) >> 24) as u8
}

#[test]
fn alpha_dither_is_deterministic_and_relative_to_each_raster_role() {
    let (device, queue, dual) = headless_device();
    for scale in [1, 2, 3, 4] {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 16, 12, dual);
        renderer.set_test_scale(scale, BUDGET);
        let mut draw = texrect_scene([0, 0], [1024, 1024], false, false, false);
        let (l, h) = gdp_set_combine_lerp(
            CcPass {
                a: ZERO_C,
                b: ZERO_C,
                c: ZERO_C,
                d: 3,
            },
            CcPass {
                a: ZERO_A,
                b: ZERO_A,
                c: ZERO_A,
                d: 3,
            },
            CcPass {
                a: ZERO_C,
                b: ZERO_C,
                c: ZERO_C,
                d: 3,
            },
            CcPass {
                a: ZERO_A,
                b: ZERO_A,
                c: ZERO_A,
                d: 3,
            },
        );
        draw.materials[0].selectors = crate::hle::combiner::decode_combine(l, h);
        draw.materials[0].prim = [255, 255, 255, 128];
        draw.render_modes[0].alpha_compare = crate::hle::AlphaCompare::Dither;
        for _ in 0..2 {
            renderer.begin_frame();
            renderer.frame_serial = 7;
            renderer.dither_seed = 0x1234;
            renderer.render_into_store(&device, &queue, &draw, ClearPolicy::PerFrame);
            let fb = &renderer.framebuffers[&workload::TargetId::Guest(A.into())];
            for (texture, factor) in std::iter::once((&fb.color, 1))
                .chain(fb.presentation.as_ref().map(|p| (&p.texture, scale)))
            {
                let bytes = read_texture(&device, &queue, texture);
                assert_rgba(texture, &bytes, |x, y| {
                    if (2 * factor..6 * factor).contains(&x)
                        && (2 * factor..6 * factor).contains(&y)
                        && alpha_threshold(x, y, 16 * factor, 7, 0x1234) <= 128
                    {
                        [255, 255, 255, 128]
                    } else {
                        [13, 13, 20, 255]
                    }
                });
            }
        }
    }
}

fn replacement_source(
    tag: u8,
    scale: u32,
) -> (
    crate::hle::texture_request::TextureSource,
    crate::replacement::ReplacementRegistration,
) {
    use crate::hle::texture_request::{
        DecodeRecipe, EncodedInput, EncodedTextureRequest, Representation, TextureSource,
    };
    use crate::replacement::*;
    let recipe = DecodeRecipe {
        representation: Representation::Tile,
        output: [16, 12],
        logical: [16, 12],
        fmt: 0,
        siz: 2,
        base: 0,
        line: 4,
        palette: 0,
        tlut: 0,
        palette_lanes: false,
    };
    let input = EncodedInput::Tmem(std::sync::Arc::new([tag; 4096]));
    let target =
        ReplacementTarget::new(recipe.clone(), input.clone(), TextureKeyScheme::Fast3dV1).unwrap();
    let request = EncodedTextureRequest::from_test_parts(input, recipe);
    let pixels = (0..12 * scale)
        .flat_map(|y| (0..16 * scale).flat_map(move |x| [x as u8, y as u8, tag, 255]))
        .collect::<Vec<_>>();
    (
        TextureSource::Encoded(std::sync::Arc::new(request)),
        ReplacementRegistration {
            target,
            status: ReplacementStatus::Ready(ReplacementImage {
                extent: [16 * scale, 12 * scale],
                pixels: pixels.into(),
                format: FORMAT,
            }),
        },
    )
}

#[test]
fn non_square_replacement_copy_uses_target_rate_and_unrounded_coordinates() {
    let (device, queue, dual) = headless_device();
    for scale in [1, 2, 3, 4] {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 16, 12, dual);
        renderer.set_test_scale(scale, BUDGET);
        for dsdx in [4096, -4096, 4095, -4097, 2047] {
            let base_s = 170;
            let mut draw = texrect_scene([base_s, 169], [dsdx, 1024], false, true, false);
            let (source, registration) = replacement_source(20, 8);
            draw.materials[0].texture = source;
            renderer.set_replacement_provider(Some(std::sync::Arc::new(
                crate::replacement::ReplacementProvider::new(1, [registration]),
            )));
            renderer.begin_frame();
            renderer.render_into_store(&device, &queue, &draw, ClearPolicy::PerFrame);
            let fb = &renderer.framebuffers[&workload::TargetId::Guest(A.into())];
            for (texture, factor) in std::iter::once((&fb.color, 1))
                .chain(fb.presentation.as_ref().map(|p| (&p.texture, scale)))
            {
                let bytes = read_texture(&device, &queue, texture);
                assert_rgba_samples(texture, &bytes, |x, y| {
                    if !(2 * factor..6 * factor).contains(&x)
                        || !(2 * factor..6 * factor).contains(&y)
                    {
                        return Some([13, 13, 20, 255]);
                    }
                    let slope = match dsdx {
                        4096 => 1.0,
                        -4096 => -1.0,
                        4095 => 127.0 / 128.0,
                        -4097 => -129.0 / 128.0,
                        2047 => 63.0 / 128.0,
                        _ => unreachable!(),
                    };
                    let s = f64::from(base_s) / 32.0
                        + ((x as f64 + 0.5) / f64::from(factor) - 2.0) * slope;
                    let t = 169.0 / 32.0 + (y as f64 + 0.5) / f64::from(factor) - 2.0;
                    let rounding = if factor == 1 { 1.0 / 256.0 } else { 0.0 };
                    if texel_boundary_distance(s + rounding, 8.0, slope, factor) < 1.0 / 64.0
                        || texel_boundary_distance(t + rounding, 8.0, 1.0, factor) < 1.0 / 64.0
                    {
                        return None;
                    }
                    let coordinate = |v: f64| {
                        if factor == 1 {
                            (v * 128.0).round_ties_even() / 128.0
                        } else {
                            v
                        }
                    };
                    Some([
                        (coordinate(s) * 8.0).floor() as u8,
                        (coordinate(t) * 8.0).floor() as u8,
                        20,
                        255,
                    ])
                });
            }
        }
    }
}

#[test]
fn precision_and_snapping_follow_resolved_texel_mip_and_detail_roles() {
    use crate::hle::combiner::{MipLevel, Tex1};
    let (device, queue, dual) = headless_device();
    for scale in [2, 3, 4] {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 16, 12, dual);
        renderer.set_test_scale(scale, BUDGET);
        for role in [0, 1, 3, 2] {
            for (enhanced, precision) in
                [(false, false), (true, false), (false, true), (true, true)]
            {
                let step = if precision {
                    255
                } else if role == 2 {
                    512
                } else {
                    1024
                };
                let base = if precision {
                    189
                } else if role == 2 {
                    169
                } else {
                    170
                };
                let mut draw = texrect_scene([base, base], [step, step], false, false, false);
                let mat = &mut draw.materials[0];
                let ordinary = mat.texture.clone();
                let mut registrations = Vec::new();
                let sources: Vec<_> = (0..4)
                    .map(|index| {
                        if (index == role) == enhanced {
                            let (source, registration) = replacement_source(20 + index as u8, 8);
                            registrations.push(registration);
                            source
                        } else {
                            ordinary.clone()
                        }
                    })
                    .collect();
                mat.texture = sources[0].clone();
                mat.tex1 = Some(Tex1 {
                    texture: sources[1].clone(),
                    sampling: mat.sampling,
                    tex_w: 16,
                    tex_h: 12,
                    wrap_s: 2,
                    wrap_t: 2,
                    fmt: 0,
                    siz: 2,
                });
                mat.tile_count = 2;
                mat.mip_levels = [0, 3]
                    .map(|index| MipLevel {
                        texture: sources[index].clone(),
                        sampling: mat.sampling,
                        w: 16,
                        h: 12,
                    })
                    .to_vec();
                mat.num_levels = 2;
                mat.detail_tex = (role == 2).then(|| MipLevel {
                    texture: sources[2].clone(),
                    sampling: mat.sampling,
                    w: 16,
                    h: 12,
                });
                mat.lod = role >= 2;
                if role == 3 {
                    mat.text_detail = 1;
                    mat.prim_min_level = 1.25;
                }
                mat.cycle_type = u32::from(role == 1 || role == 3);
                renderer.set_replacement_provider(Some(std::sync::Arc::new(
                    crate::replacement::ReplacementProvider::new(1, registrations),
                )));
                renderer.begin_frame();
                renderer.render_into_store(&device, &queue, &draw, ClearPolicy::PerFrame);
                let texture = &renderer.framebuffers[&workload::TargetId::Guest(A.into())]
                    .presentation
                    .as_ref()
                    .unwrap()
                    .texture;
                let bytes = read_texture(&device, &queue, texture);
                assert_rgba_samples(texture, &bytes, |x, y| {
                    if !(2 * scale..6 * scale).contains(&x) || !(2 * scale..6 * scale).contains(&y)
                    {
                        return Some([13, 13, 20, 255]);
                    }
                    let p = |v: u32| {
                        if !enhanced && step == 1024 {
                            (v / scale - 2) as f64 + 0.5
                        } else {
                            (v as f64 + 0.5) / f64::from(scale) - 2.0
                        }
                    };
                    let slope = if precision {
                        31.0 / 128.0
                    } else {
                        f64::from(step) / 1024.0
                    };
                    let coordinate = |v: u32| f64::from(base) / 32.0 + p(v) * slope;
                    let rate = if enhanced { 8.0 } else { 1.0 };
                    let rounding = if enhanced { 0.0 } else { 1.0 / 256.0 };
                    if [x, y].into_iter().any(|v| {
                        texel_boundary_distance(coordinate(v) + rounding, rate, slope, scale)
                            < 1.0 / 64.0
                    }) {
                        return None;
                    }
                    if enhanced {
                        Some([
                            (coordinate(x) * 8.0).floor() as u8,
                            (coordinate(y) * 8.0).floor() as u8,
                            20 + role as u8,
                            255,
                        ])
                    } else {
                        let texel =
                            |v| ((coordinate(v) * 128.0).round_ties_even() / 128.0).floor() as u8;
                        Some([texel(x) * 12, texel(y) * 16, 0, 255])
                    }
                });
            }
        }
    }
}

#[test]
fn mip_selection_uses_unsnapped_native_derivatives() {
    use crate::hle::combiner::MipLevel;
    use crate::hle::texture_request::TextureSource;
    let (device, queue, dual) = headless_device();
    for scale in [1, 2, 3, 4] {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 16, 12, dual);
        renderer.set_test_scale(scale, BUDGET);
        for step in [1024, 2560, 5120] {
            let mut draw = texrect_scene([0, 0], [step, step], false, false, false);
            let mat = &mut draw.materials[0];
            mat.lod = true;
            mat.num_levels = 3;
            let colors = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
            mat.mip_levels = colors
                .into_iter()
                .map(|rgba| MipLevel {
                    sampling: mat.sampling,
                    texture: TextureSource::Rgba(rgba.repeat(16 * 12).into()),
                    w: 16,
                    h: 12,
                })
                .collect();
            mat.texture = mat.mip_levels[0].texture.clone();
            renderer.begin_frame();
            renderer.render_into_store(&device, &queue, &draw, ClearPolicy::PerFrame);
            let fb = &renderer.framebuffers[&workload::TargetId::Guest(A.into())];
            for (texture, factor) in std::iter::once((&fb.color, 1))
                .chain(fb.presentation.as_ref().map(|p| (&p.texture, scale)))
            {
                let bytes = read_texture(&device, &queue, texture);
                assert_rgba(texture, &bytes, |x, y| {
                    if (2 * factor..6 * factor).contains(&x)
                        && (2 * factor..6 * factor).contains(&y)
                    {
                        colors[match step {
                            1024 => 0,
                            2560 => 1,
                            _ => 2,
                        }]
                    } else {
                        [13, 13, 20, 255]
                    }
                });
            }
        }
    }
}

fn decal_scene(primitive: bool, dz: u16) -> crate::Scene {
    let mut draw = scene(&triangle(A, Z, 0x4000, 0xff0000ff));
    draw.draw_origins.clear();
    let SceneOp::Tris(base) = draw.framebuffer_pairs[0].ops[0].clone() else {
        unreachable!()
    };
    draw.materials.push(draw.materials[0].clone());
    draw.materials[1].prim = [0, 255, 0, 255];
    draw.render_modes = vec![
        crate::hle::decode_render_mode(crate::hle::consts::rdp::G_RM_AA_ZB_OPA_SURF, 0, 0),
        crate::hle::decode_render_mode(crate::hle::consts::rdp::G_RM_AA_ZB_XLU_DECAL, 0, 0),
    ];
    draw.render_modes[1].z_write = false;
    if primitive {
        draw.render_modes[0].z_source_prim = true;
        draw.render_modes[1].z_source_prim = true;
    }
    draw.raw_pos = vec![[-1.0, 1.0, -0.4], [1.0, 1.0, 0.4], [-1.0, -1.0, -0.4]];
    let mut decal_positions = draw.raw_pos.clone();
    for position in &mut decal_positions {
        position[2] -= 0.018;
    }
    draw.raw_pos.extend(decal_positions);
    draw.raw_st.extend_from_within(..);
    draw.mtx_index.extend_from_within(..);
    draw.viewport_index.extend_from_within(..);
    draw.texcoord_index.extend_from_within(..);
    draw.cn.extend_from_within(..);
    draw.light_index.extend_from_within(..);
    draw.light_count.extend_from_within(..);
    draw.indices.extend([3, 4, 5]);
    draw.framebuffer_pairs[0].ops = vec![
        SceneOp::Tris(base),
        SceneOp::Tris(crate::scene::DrawRun {
            material_index: 1,
            render_mode_index: 1,
            index_start: 3,
            prim_depth: crate::scene::PrimitiveDepth { z: 0x3ff0, dz },
            ..base
        }),
    ];
    draw
}

#[test]
fn sloped_decal_tolerance_uses_native_derivatives_without_scaling_z() {
    let (fallback_device, fallback_queue) = headless_device_forced_fallback();
    for (device, queue, primary) in [(fallback_device, fallback_queue, false), headless_device()] {
        for scale in [1, 2, 3, 4] {
            let mut renderer = SceneRenderer::new(&device, FORMAT, 32, 24, primary);
            renderer.set_test_scale(scale, BUDGET);
            renderer.render_into_store(
                &device,
                &queue,
                &decal_scene(false, 0),
                ClearPolicy::Persist,
            );
            let fb = &renderer.framebuffers[&workload::TargetId::Guest(A.into())];
            let depth = &renderer.depthbuffers[&workload::TargetId::Guest(Z.into())];
            for (color, depth, factor) in std::iter::once((&fb.color, &depth.texture, 1)).chain(
                fb.presentation.as_ref().map(|p| {
                    (
                        &p.texture,
                        &depth.presentation.as_ref().unwrap().texture,
                        scale,
                    )
                }),
            ) {
                let bytes = read_texture(&device, &queue, color);
                let z = read_texture(&device, &queue, depth);
                for y in 3 * factor..8 * factor {
                    for x in 3 * factor..12 * factor {
                        let offset = ((y * 32 * factor + x) * 4) as usize;
                        assert_eq!(
                            &bytes[offset..offset + 4],
                            [0, 255, 0, 255],
                            "scale {scale}, primary {primary}, ({x},{y})"
                        );
                        let expected =
                            (0.6 + 0.8 * x as f32 / (32 * factor) as f32) * 511.0 / 1024.0;
                        let actual = f32::from_le_bytes(z[offset..offset + 4].try_into().unwrap());
                        assert!((actual - expected).abs() < 1.0e-6, "{actual} vs {expected}");
                    }
                }
            }
        }
    }
}

#[test]
fn primitive_z_decal_retains_the_baseline_primitive_dz_gap() {
    let (fallback_device, fallback_queue) = headless_device_forced_fallback();
    for (device, queue, primary) in [(fallback_device, fallback_queue, false), headless_device()] {
        for scale in [1, 3] {
            for dz in [0, 0x7fff] {
                let mut renderer = SceneRenderer::new(&device, FORMAT, 32, 24, primary);
                renderer.set_test_scale(scale, BUDGET);
                renderer.render_into_store(
                    &device,
                    &queue,
                    &decal_scene(true, dz),
                    ClearPolicy::Persist,
                );
                let fb = &renderer.framebuffers[&workload::TargetId::Guest(A.into())];
                for (texture, factor) in std::iter::once((&fb.color, 1))
                    .chain(fb.presentation.as_ref().map(|p| (&p.texture, scale)))
                {
                    let bytes = read_texture(&device, &queue, texture);
                    for y in 3 * factor..8 * factor {
                        for x in 3 * factor..12 * factor {
                            assert_eq!(
                                &bytes[((y * 32 * factor + x) * 4) as usize..][..4],
                                [255, 0, 0, 255]
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn adjacent_normal_and_copy_strips_scale_completed_bounds_on_tall_attachments() {
    let (device, queue, dual) = headless_device();
    for scale in [2, 3, 4] {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 16, 12, dual);
        renderer.set_test_scale(scale, BUDGET);
        for copy in [false, true] {
            for residue in 0..4 {
                renderer.render_into_store(
                    &device,
                    &queue,
                    &fill_scene(A, 16, 20, 0x003f003f),
                    ClearPolicy::Persist,
                );
                let mut draw = texrect_scene(
                    [0, 0],
                    [if copy { 4096 } else { 1024 }, 1024],
                    false,
                    copy,
                    false,
                );
                draw.materials.push(draw.materials[0].clone());
                for (mat, rgba) in draw
                    .materials
                    .iter_mut()
                    .zip([[255, 0, 0, 255], [0, 255, 0, 255]])
                {
                    mat.texture = crate::hle::texture_request::TextureSource::Rgba(
                        rgba.repeat(16 * 12).into(),
                    );
                }
                let mut second = draw.framebuffer_pairs[0].ops[0].clone();
                for (index, op) in [&mut draw.framebuffer_pairs[0].ops[0], &mut second]
                    .into_iter()
                    .enumerate()
                {
                    let SceneOp::TexRect {
                        rect,
                        material_index,
                        ..
                    } = op
                    else {
                        unreachable!()
                    };
                    *material_index = index as u32;
                    *rect = crate::scene::TexRectBounds {
                        ulx: (2 + index as i32 * 2) * 4 + residue,
                        uly: 8 + residue,
                        lrx: (4 + index as i32 * 2) * 4 + residue - if copy { 4 } else { 0 },
                        lry: 24 + residue - if copy { 4 } else { 0 },
                    };
                }
                draw.framebuffer_pairs[0].ops.push(second);
                renderer.render_into_store(&device, &queue, &draw, ClearPolicy::Persist);
                let fb = &renderer.framebuffers[&workload::TargetId::Guest(A.into())];
                for (texture, factor) in [
                    (&fb.color, 1),
                    (&fb.presentation.as_ref().unwrap().texture, scale),
                ] {
                    assert_eq!(texture.height(), 20 * factor);
                    let bytes = read_texture(&device, &queue, texture);
                    let offset = u32::from(!copy && residue != 0);
                    assert_rgba(texture, &bytes, |x, y| {
                        let (x, y) = (x / factor, y / factor);
                        if (2 + offset..6 + offset).contains(&y)
                            && (2 + offset..6 + offset).contains(&x)
                        {
                            if x < 4 + offset {
                                [255, 0, 0, 255]
                            } else {
                                [0, 255, 0, 255]
                            }
                        } else {
                            [0, 0, 255, 255]
                        }
                    });
                }
            }
        }
    }
}

#[test]
fn triangle_half_target_pixel_offset_uses_draw_height_after_retained_growth() {
    let (device, queue, dual) = headless_device();
    for scale in [2, 3, 4] {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 32, 24, dual);
        renderer.set_test_scale(scale, BUDGET);
        renderer.render_into_store(
            &device,
            &queue,
            &fill_scene(A, 32, 48, 0x003f003f),
            ClearPolicy::Persist,
        );
        let mut draw = scene(&triangle(A, 0, 0, 0xffffffff));
        draw.raw_pos = vec![
            [-1.25, 1.25 + 1.0 / 256.0, 0.0],
            [1.25, 1.25 + 1.0 / 256.0, 0.0],
            [-1.25, -1.25 + 1.0 / 256.0, 0.0],
        ];
        renderer.render_into_store(&device, &queue, &draw, ClearPolicy::Persist);
        let texture = &renderer.framebuffers[&workload::TargetId::Guest(A.into())]
            .presentation
            .as_ref()
            .unwrap()
            .texture;
        let bytes = read_texture(&device, &queue, texture);
        assert_rgba(texture, &bytes, |x, y| {
            // Authored screen vertices: (-4,-3-3/64), (36,-3-3/64), (-4,27-3/64).
            // The half-target-pixel shift cancels the sample centers in this edge equation.
            let edge = 16 * (3 * x + 4 * y);
            let boundary = 1533 * scale;
            assert!(edge.abs_diff(boundary) >= 4);
            let inside = y < 24 * scale && edge < boundary;
            if inside {
                [255; 4]
            } else {
                [0, 0, 255, 255]
            }
        });
    }
}

#[test]
fn raw_scissor_survives_task_boundaries_and_raw_only_rectangle_changes() {
    let mut renderer = facade(3, false);
    let mut memory = Memory(vec![0; 4096]);
    let mut commands = fill_commands(A, 9, 7, 0xf801f801);
    commands[1] = gdp_set_scissor(0, 0, 0, 32, 24);
    commands.extend([
        gdp_set_scissor(0, 1, 3, 32, 24),
        gdp_set_fill_color(0x07c107c1),
        gdp_fill_rectangle(0, 0, 32, 24),
    ]);
    let (_, diagnostics) = submit(
        &mut renderer,
        &mut memory,
        &finish(DlBuilder::new(), commands),
    );
    assert!(diagnostics.is_empty());
    let mut commands = fill_commands(A, 9, 7, 0x003f003f);
    commands.remove(1);
    let (_, diagnostics) = submit(
        &mut renderer,
        &mut memory,
        &finish(DlBuilder::new(), commands),
    );
    assert!(diagnostics.is_empty());
    let texture = &renderer.inner.framebuffers[&workload::TargetId::Guest(A.into())]
        .presentation
        .as_ref()
        .unwrap()
        .texture;
    let bytes = read_texture(renderer.device(), renderer.queue(), texture);
    assert_rgba(texture, &bytes, |x, y| {
        if (3..24).contains(&x) && (3..18).contains(&y) {
            [0, 0, 255, 255]
        } else if x < 24 && y < 18 {
            [255, 0, 0, 255]
        } else {
            [13, 13, 20, 255]
        }
    });
}

fn loaded_feedback(
    fmt: u8,
) -> (
    crate::Scene,
    std::sync::Arc<crate::hle::framebuffer_load::FramebufferCopy>,
) {
    use crate::hle::{
        framebuffer_load::FramebufferCopy,
        rdp::{Rdp, TileDescriptor},
        texture_request::prepare,
    };
    let source = framebuffers::targets::ImageDescriptor {
        address: A.into(),
        layout: ImageLayout {
            width: 32,
            fmt: 0,
            siz: 2,
        },
        height: 24,
        generation: 1,
        depth: false,
    };
    let copy = std::sync::Arc::new(FramebufferCopy::new(
        1,
        source,
        u64::from(A) + (9 * 32 + 9) * 2,
        16,
        64,
        8,
    ));
    let mut rdp = Rdp::default();
    rdp.tmem_bank
        .copy_load(copy.clone(), |bank| bank.write_tile(&[], 0, 2, 8, 2, 64, 2));
    let tile = TileDescriptor {
        fmt,
        siz: 2,
        width: 8,
        height: 8,
        line: 2,
        cms: 2,
        cmt: 2,
        ..Default::default()
    };
    let binding = prepare(&rdp, &tile, 0, &crate::profiling::Recorder::default()).unwrap();
    let mut draw = texrect_scene([0, 0], [1024, 1024], false, false, false);
    draw.framebuffer_pairs[0].color_image.addr = B.into();
    draw.materials[0].texture = binding.source;
    draw.materials[0].sampling = binding.sampling;
    draw.materials[0].fmt = fmt;
    draw.materials[0].siz = 2;
    draw.materials[0].tex_w = 8;
    draw.materials[0].tex_h = 8;
    if let SceneOp::TexRect { rect, .. } = &mut draw.framebuffer_pairs[0].ops[0] {
        *rect = crate::scene::TexRectBounds {
            ulx: 0,
            uly: 0,
            lrx: 32,
            lry: 32,
        };
    }
    (draw, copy)
}

#[test]
fn loaded_subrectangle_retains_scaled_samples_after_overwrite_and_growth() {
    let (device, queue, dual) = headless_device();
    for scale in [2, 3, 4] {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 32, 24, dual);
        renderer.set_test_scale(scale, BUDGET);
        let producer = scene(&triangle(A, 0, 0, 0xffffffff));
        renderer.render_into_store(&device, &queue, &producer, ClearPolicy::Persist);
        let before = read_texture(
            &device,
            &queue,
            &renderer.framebuffers[&workload::TargetId::Guest(A.into())]
                .presentation
                .as_ref()
                .unwrap()
                .texture,
        );
        for fmt in [0, 3] {
            let (mut draw, copy) = loaded_feedback(fmt);
            let mut alias_material = draw.materials[0].clone();
            alias_material.fmt = 0;
            draw.materials.push(alias_material);
            let mut alias = draw.framebuffer_pairs[0].ops[0].clone();
            if let SceneOp::TexRect {
                rect,
                fb_source,
                uls,
                ult,
                material_index,
                ..
            } = &mut alias
            {
                *material_index = 1;
                rect.ulx = 32;
                rect.lrx = 64;
                *fb_source = Some(A.into());
                *uls = 9 * 32;
                *ult = 9 * 32;
            }
            draw.framebuffer_pairs[0].ops.push(alias);
            let mut load = fill_scene(A, 32, 24, 0);
            load.framebuffer_pairs[0].ops = vec![SceneOp::FramebufferCopy(copy.clone())];
            renderer.render_into_store(&device, &queue, &load, ClearPolicy::Persist);
            renderer.render_into_store(
                &device,
                &queue,
                &fill_scene(A, 32, 37, 0x07c107c1),
                ClearPolicy::Persist,
            );
            renderer.render_into_store(&device, &queue, &draw, ClearPolicy::Persist);
            assert!(
                renderer.diagnostics.is_empty(),
                "{:?}",
                renderer.diagnostics
            );
            assert_eq!(renderer.framebuffer_copies.source_scale(&copy, 1), scale);
            assert_eq!(renderer.material_bindings[0].high_resolution_mask, 0);
            assert_eq!(renderer.presentation_bindings[0].high_resolution_mask, 1);
            let fb = &renderer.framebuffers[&workload::TargetId::Guest(B.into())];
            assert_eq!((fb.color.width(), fb.color.height()), (16, 12));
            let output = read_texture(&device, &queue, &fb.presentation.as_ref().unwrap().texture);
            let mut varied = false;
            for y in 0..8 * scale {
                for x in 0..8 * scale {
                    let expected = &before
                        [(((9 * scale + y) * 32 * scale + 9 * scale + x) * 4) as usize..][..4];
                    let actual = &output[((y * 16 * scale + x) * 4) as usize..][..4];
                    let expected = if fmt == 0 {
                        expected.try_into().unwrap()
                    } else {
                        image_ia(expected)
                    };
                    assert_eq!(actual, expected, "fmt {fmt}, scale {scale}, ({x},{y})");
                    let live = &output[((y * 16 * scale + x + 8 * scale) * 4) as usize..][..4];
                    assert_eq!(live, [0, 255, 0, 255]);
                    varied |= before
                        [(((9 * scale + y) * 32 * scale + 9 * scale + x) * 4) as usize..][..4]
                        != before[(((9 * scale + (y / scale) * scale) * 32 * scale
                            + 9 * scale
                            + (x / scale) * scale)
                            * 4) as usize..][..4];
                }
            }
            assert!(varied);
            renderer.begin_frame();
            renderer.render_into_store(&device, &queue, &producer, ClearPolicy::PerFrame);
        }
    }
}

fn image_ia(color: &[u8]) -> [u8; 4] {
    let rgba: Vec<_> = color
        .iter()
        .map(|&c| (f32::from(c) * 1023.0 / 255.0).round() as u32)
        .collect();
    let packed =
        (rgba[0] << 22) | (rgba[1] << 12) | (rgba[2] << 2) | if rgba[3] > 0 { 3 } else { 0 };
    let i = ((packed >> 16) as f32 * 255.0 / 65535.0).round() as u8;
    [
        i,
        i,
        i,
        ((packed & 65535) as f32 * 255.0 / 65535.0).round() as u8,
    ]
}

#[test]
fn byte_routes_keep_native_samples_and_clear_role_bits() {
    use crate::hle::{
        rdp::{Rdp, TileDescriptor},
        texture_request::{prepare, TextureSource},
    };
    let (device, queue, dual) = headless_device();
    for route in ["ci8", "palette", "partial", "mixed", "split", "lookup"] {
        let mut baseline = None;
        for scale in [1, 3] {
            let mut renderer = SceneRenderer::new(&device, FORMAT, 32, 24, dual);
            renderer.set_test_scale(scale, BUDGET);
            let mut producer = scene(&triangle(A, 0, 0, 0xffffffff));
            let (mut draw, copy) = loaded_feedback(0);
            let mut rdp = Rdp::default();
            rdp.tmem_bank
                .copy_load(copy.clone(), |bank| bank.write_tile(&[], 0, 2, 8, 2, 64, 2));
            let mut tile = TileDescriptor {
                fmt: 0,
                siz: 2,
                width: 8,
                height: 8,
                line: 2,
                cms: 2,
                cmt: 2,
                ..Default::default()
            };
            let tlut = if route == "ci8" || route == "palette" {
                tile.fmt = 2;
                tile.siz = 1;
                tile.width = 16;
                let palette: Vec<_> = (0..256u16)
                    .flat_map(|index| ((index << 8) | 1).to_be_bytes())
                    .collect();
                rdp.tmem_bank.write_tlut(&palette, 256, 256);
                if route == "palette" {
                    let mut tags = rdp.tmem_bank.framebuffer_tags.clone().unwrap();
                    tags.bytes[2048] = tags.bytes[0];
                    tags.bytes[2049] = tags.bytes[1];
                    rdp.tmem_bank.framebuffer_tags = Some(tags);
                }
                2
            } else {
                if route == "partial" {
                    rdp.tmem_bank.framebuffer_tags.as_mut().unwrap().bytes[0] = Default::default();
                }
                if route == "mixed" {
                    rdp.tmem_bank.write_tile(&[0x12, 0x34], 0, 1, 1, 1, 2, 1);
                }
                if route == "split" {
                    rdp.tmem_bank.framebuffer_tags.as_mut().unwrap().bytes[1].pointer = 3;
                }
                if route == "lookup" {
                    tile.cms = 0;
                    tile.masks = 5;
                }
                0
            };
            let binding = prepare(&rdp, &tile, tlut, &Default::default()).unwrap();
            let TextureSource::Framebuffer(request) = &binding.source else {
                panic!("framebuffer route");
            };
            assert!(!request.image_route(), "{route}");
            if route == "ci8" {
                assert!(!request.materialize);
            }
            draw.materials[0].texture = binding.source;
            draw.materials[0].sampling = binding.sampling;
            draw.materials[0].tex_w = u32::from(tile.width);
            draw.materials[0].fmt = tile.fmt;
            draw.materials[0].siz = tile.siz;
            renderer.render_into_store(&device, &queue, &producer, ClearPolicy::Persist);
            let source = &renderer.framebuffers[&workload::TargetId::Guest(A.into())];
            let source_native = read_texture(&device, &queue, &source.color);
            if route == "ci8" && scale > 1 {
                let source_image = read_texture(
                    &device,
                    &queue,
                    &source.presentation.as_ref().unwrap().texture,
                );
                assert!((0..8 * scale).any(|y| (0..8 * scale).any(|x| {
                    packed_rgba16(
                        &source_native[(((9 + y / scale) * 32 + 9 + x / scale) * 4) as usize..]
                            [..4],
                    ) != packed_rgba16(
                        &source_image
                            [(((9 * scale + y) * 32 * scale + 9 * scale + x) * 4) as usize..][..4],
                    )
                })));
            }
            producer.framebuffer_pairs[0]
                .ops
                .push(SceneOp::FramebufferCopy(copy));
            let overwrite = fill_scene(A, 32, 24, 0x07c107c1)
                .framebuffer_pairs
                .remove(0)
                .ops
                .remove(0);
            producer.framebuffer_pairs[0].ops.push(overwrite);
            renderer.render_into_store(&device, &queue, &producer, ClearPolicy::Persist);
            renderer.render_into_store(&device, &queue, &draw, ClearPolicy::Persist);
            assert!(
                renderer.diagnostics.is_empty(),
                "{route}: {:?}",
                renderer.diagnostics
            );
            assert_eq!(renderer.material_bindings[0].high_resolution_mask, 0);
            let fb = &renderer.framebuffers[&workload::TargetId::Guest(B.into())];
            let native = read_texture(&device, &queue, &fb.color);
            if route == "ci8" {
                for y in 0..8 {
                    for x in 0..8 {
                        let word = packed_rgba16(
                            &source_native[(((9 + y) * 32 + 9 + x / 2) * 4) as usize..][..4],
                        );
                        let index = word.to_be_bytes()[(x & 1) as usize];
                        let value = (u16::from(index) << 8) | 1;
                        let channel = |shift: u32| {
                            let c = ((value >> shift) & 31u16) as u8;
                            (c << 3) | (c >> 2)
                        };
                        assert_eq!(
                            &native[((y * 16 + x) * 4) as usize..][..4],
                            [channel(11), channel(6), channel(1), 255]
                        );
                    }
                }
            }
            assert_eq!(
                baseline.get_or_insert_with(|| native.clone()),
                &native,
                "{route} {scale}"
            );
            if scale > 1 {
                assert_eq!(renderer.presentation_bindings[0].high_resolution_mask, 0);
                let output =
                    read_texture(&device, &queue, &fb.presentation.as_ref().unwrap().texture);
                for y in 0..8 * scale {
                    for x in 0..8 * scale {
                        assert_eq!(
                            &output[((y * 16 * scale + x) * 4) as usize..][..4],
                            &native[(((y / scale) * 16 + x / scale) * 4) as usize..][..4],
                            "{route} {scale} {x},{y}"
                        );
                    }
                }
            }
        }
    }
}

fn feedback_task(fmt: u32) -> Built {
    let mut built = triangle(A, 0, 0, 0xffffffff);
    built.rdram.truncate(built.rdram.len() - 8);
    let mut commands = vec![
        gdp_set_texture_image(0, 2, 32, A),
        gdp_set_tile(0, 2, 2, 0, 7, 0, 2, 0, 0, 2, 0, 0),
        (0xf400_0000 | (36 << 12) | 36, (7 << 24) | (64 << 12) | 64),
    ];
    commands.extend(fill_commands(A, 32, 37, 0x07c107c1));
    commands.extend(fill_commands(B, 32, 24, 0x003f003f));
    let pass = CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: 1,
    };
    let alpha = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 1,
    };
    commands.extend([
        gdp_set_cycle_type(0),
        gdp_set_render_mode(0, 0),
        gdp_set_combine_lerp(pass, alpha, pass, alpha),
        gdp_set_tile(fmt, 2, 2, 0, 0, 0, 2, 0, 0, 2, 0, 0),
        gdp_set_tile_size(0, 0, 0, 28, 28),
    ]);
    commands.extend(gsp_texture_rectangle(
        0, 0, 32, 32, 0, 0, 0, 1024, 1024, false,
    ));
    commands.extend([
        gdp_set_tile(0, 2, 8, 128, 1, 0, 2, 0, 0, 2, 0, 0),
        gdp_set_tile_size(1, 0, 0, 124, 92),
    ]);
    commands.extend(gsp_texture_rectangle(
        32,
        0,
        64,
        32,
        1,
        9 * 32,
        9 * 32,
        1024,
        1024,
        false,
    ));
    commands.push(gsp_enddl());
    built.rdram.extend(
        commands
            .into_iter()
            .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes())),
    );
    built
}

#[test]
fn loaded_and_live_feedback_keep_writeback_bytes_coverage_and_patch_ranges_native() {
    for fallback in [false, true] {
        for fmt in [0, 3] {
            let mut baseline = None;
            for scale in [1, 3] {
                let mut renderer = facade(scale, fallback);
                renderer
                    .set_guest_writeback_with_encoding(true, crate::FramebufferEncoding::BigEndian);
                let mut memory = Memory(vec![0xa5; B as usize + 4096]);
                let (_, diagnostics) = submit(&mut renderer, &mut memory, &feedback_task(fmt));
                assert!(diagnostics.is_empty(), "{diagnostics:?}");
                let patches = apply(&mut renderer, &mut memory);
                let bytes = read_texture(
                    renderer.device(),
                    renderer.queue(),
                    &renderer.inner.framebuffers[&workload::TargetId::Guest(B.into())].color,
                );
                let coverage: Vec<_> = (0..32 * 24)
                    .map(|pixel| {
                        renderer
                            .inner
                            .coherence
                            .as_ref()
                            .unwrap()
                            .stored_coverage_code(u64::from(B) + pixel * 2)
                    })
                    .collect();
                let result = (memory.0, patches, bytes, coverage);
                assert_eq!(
                    baseline.get_or_insert_with(|| result.clone()),
                    &result,
                    "fmt {fmt}, scale {scale}, fallback {fallback}"
                );
                assert_eq!(&result.0[B as usize - 16..B as usize], &[0xa5; 16]);
                assert_eq!(&result.0[B as usize + 32 * 24 * 2..][..16], &[0xa5; 16]);
            }
        }
    }
}

#[test]
fn retained_snapshot_uses_its_source_scale_after_reconfigure() {
    let mut renderer = facade(3, false);
    let producer = scene(&triangle(A, 0, 0, 0xffffffff));
    let (draw, copy) = loaded_feedback(0);
    let mut prefix = producer;
    prefix.framebuffer_pairs[0]
        .ops
        .push(SceneOp::FramebufferCopy(copy.clone()));
    let device = renderer.device().clone();
    let queue = renderer.queue().clone();
    renderer
        .inner
        .render_into_store(&device, &queue, &prefix, ClearPolicy::Persist);
    let before = read_texture(
        renderer.device(),
        renderer.queue(),
        &renderer.inner.framebuffers[&workload::TargetId::Guest(A.into())]
            .presentation
            .as_ref()
            .unwrap()
            .texture,
    );
    let config = crate::RendererConfig {
        resolution_multiplier: 2,
        ..renderer.config
    };
    renderer.reconfigure(config);
    assert_eq!(renderer.inner.framebuffer_copies.source_scale(&copy, 2), 3);
    let device = renderer.device().clone();
    let queue = renderer.queue().clone();
    renderer
        .inner
        .render_into_store(&device, &queue, &draw, ClearPolicy::Persist);
    let texture = &renderer.inner.framebuffers[&workload::TargetId::Guest(B.into())]
        .presentation
        .as_ref()
        .unwrap()
        .texture;
    let output = read_texture(&device, &queue, texture);
    for y in 0..16 {
        for x in 0..16 {
            let sx = ((x as f32 + 0.5) * 1.5).floor() as u32;
            let sy = ((y as f32 + 0.5) * 1.5).floor() as u32;
            assert_eq!(
                &output[((y * 32 + x) * 4) as usize..][..4],
                &before[(((27 + sy) * 96 + 27 + sx) * 4) as usize..][..4]
            );
        }
    }
}

fn packed_rgba16(color: &[u8]) -> u16 {
    ((u16::from(color[0]) >> 3) << 11)
        | ((u16::from(color[1]) >> 3) << 6)
        | ((u16::from(color[2]) >> 3) << 1)
        | ((u16::from(color[3]) >> 2) & 1)
}

#[test]
fn overlapping_load_rows_keep_scaled_span_phase() {
    use crate::hle::{
        framebuffer_load::FramebufferCopy,
        rdp::{Rdp, TileDescriptor},
        texture_request::prepare,
    };
    let (device, queue, dual) = headless_device();
    for scale in 2..=4 {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 32, 24, dual);
        renderer.set_test_scale(scale, BUDGET);
        let (mut draw, original) = loaded_feedback(0);
        let copy = std::sync::Arc::new(FramebufferCopy::new(
            1,
            original.source,
            u64::from(A) + (9 * 32 + 9) * 2,
            64,
            16,
            2,
        ));
        let mut rdp = Rdp::default();
        rdp.tmem_bank
            .copy_load(copy.clone(), |bank| bank.write_tile(&[], 0, 8, 2, 8, 16, 2));
        let tile = TileDescriptor {
            fmt: 0,
            siz: 2,
            width: 32,
            height: 2,
            line: 8,
            cms: 2,
            cmt: 2,
            ..Default::default()
        };
        let binding = prepare(&rdp, &tile, 0, &Default::default()).unwrap();
        draw.materials[0].texture = binding.source;
        draw.materials[0].sampling = binding.sampling;
        draw.materials[0].tex_w = 32;
        draw.materials[0].tex_h = 2;
        draw.framebuffer_pairs[0].color_image.width = 32;
        draw.framebuffer_pairs[0].size_extent.0 = 32;
        draw.framebuffer_pairs[0].active_scissor.lrx = 32;
        if let SceneOp::TexRect { rect, .. } = &mut draw.framebuffer_pairs[0].ops[0] {
            rect.lrx = 128;
            rect.lry = 8;
        }
        let mut producer = scene(&triangle(A, 0, 0, 0xffffffff));
        producer.framebuffer_pairs[0]
            .ops
            .push(SceneOp::FramebufferCopy(copy));
        renderer.render_into_store(&device, &queue, &producer, ClearPolicy::Persist);
        let source = read_texture(
            &device,
            &queue,
            &renderer.framebuffers[&workload::TargetId::Guest(A.into())]
                .presentation
                .as_ref()
                .unwrap()
                .texture,
        );
        renderer.render_into_store(&device, &queue, &draw, ClearPolicy::Persist);
        let output = read_texture(
            &device,
            &queue,
            &renderer.framebuffers[&workload::TargetId::Guest(B.into())]
                .presentation
                .as_ref()
                .unwrap()
                .texture,
        );
        for y in 0..2 * scale {
            for x in 0..32 * scale {
                let pixel = 9 * 32 + 9 + y / scale * 8 + x / scale;
                let sx = (pixel % 32) * scale + x % scale;
                let sy = (pixel / 32) * scale + y % scale;
                assert_eq!(
                    &output[((y * 32 * scale + x) * 4) as usize..][..4],
                    &source[((sy * 32 * scale + sx) * 4) as usize..][..4]
                );
            }
        }
    }
}

#[test]
fn scaled_feedback_without_writeback_stays_gpu_only() {
    for scale in 2..=4 {
        let mut renderer = facade(scale, false);
        let profiling = crate::profiling::Recorder::new(crate::profiling::Mode::Counters);
        renderer.inner.profiling = profiling.clone();
        let (_, diagnostics) = submit(&mut renderer, &mut Memory(vec![0; 4096]), &feedback_task(0));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(renderer.inner.coherence.is_none() && renderer.inner.coverage.is_none());
        assert!(renderer.take_guest_writes().is_empty());
        assert!(profiling
            .snapshot()
            .counters
            .keys()
            .all(|name| !name.starts_with("readback.") && !name.starts_with("import.")));
        assert!(profiling.snapshot().counters["workload.presentation_segments"] > 0);
    }
}

#[test]
fn scaled_snapshot_storage_rejection_leaves_the_store_unpublished() {
    let (device, _, dual) = headless_device();
    let mut renderer = SceneRenderer::new(&device, FORMAT, 32, 24, dual);
    renderer.set_test_scale(4, BUDGET);
    let (mut draw, copy) = loaded_feedback(0);
    let mut producer = scene(&triangle(A, 0, 0, 0xffffffff));
    producer.framebuffer_pairs[0]
        .ops
        .push(SceneOp::FramebufferCopy(copy));
    if let SceneOp::TexRect { material_index, .. } = &mut draw.framebuffer_pairs[0].ops[0] {
        *material_index = 1;
    }
    producer.materials.extend(draw.materials);
    producer.render_modes.extend(draw.render_modes);
    producer.framebuffer_pairs.extend(draw.framebuffer_pairs);
    let mut inputs = inputs::RenderInputs::with_targets(
        &producer,
        (32, 24),
        [0; 2],
        &renderer.target_descriptors,
    )
    .unwrap();
    let mut limits = device.limits();
    limits.max_storage_buffer_binding_size = 4096 + 4096 * 4 + 8 * 256;
    let descriptors = renderer.target_descriptors.clone();
    let error = renderer.preflight_inputs(&mut inputs, &limits).unwrap_err();
    assert!(matches!(
        error.kind,
        crate::DiagKind::ResourceAdmission {
            reason: crate::diag::ResourceAdmission::StorageBinding,
            ..
        }
    ));
    assert_eq!(renderer.target_descriptors, descriptors);
    assert!(renderer.framebuffers.is_empty());
}

#[path = "scale_tests.rs"]
mod scale_tests;

fn rejected_native_projection() -> Built {
    let mut builder = DlBuilder::new();
    let viewport = builder.viewport(Vp {
        vscale: [4, 4, 0, 0],
        vtrans: [0; 4],
    });
    let vertices = builder.vertices(&[[2000, 0], [2001, 0], [2000, 1]].map(|[x, y]| VtxColored {
        x,
        y,
        z: 0,
        flag: 0,
        s: 0,
        t: 0,
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    }));
    let mut commands = fill_commands(A, 32, 32, 0xffffffff);
    *commands.last_mut().unwrap() = gdp_fill_rectangle(0, 0, 0, 0);
    let rgb = CcPass {
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
    commands.extend([
        (0xef00_0030 | G_TC_FILT, CVG_DST_FULL),
        gdp_set_combine_lerp(rgb, alpha, rgb, alpha),
        gdp_set_prim_color(0, 0, 0xffffffff),
        gsp_viewport(viewport),
        gsp_vertex(0, 3, vertices),
        gsp_1triangle(0, 1, 2),
    ]);
    finish(builder, commands)
}

#[test]
fn strict_failure_restores_scaled_attachments_before_same_interpretation_fallback() {
    for scale in [2, 4] {
        let mut renderer = facade(scale, false);
        renderer.set_guest_writeback(true);
        let mut memory = Memory(vec![0; B as usize + 4096]);
        for task in [
            triangle(A, Z, 0x4000, 0x4080c0ff),
            fill_task(A, 32, 32, 0x003f003f),
        ] {
            let (summary, diagnostics) = submit(&mut renderer, &mut memory, &task);
            assert!(
                summary.renderable && diagnostics.is_empty(),
                "{diagnostics:?}"
            );
            apply(&mut renderer, &mut memory);
        }
        let color_id = workload::TargetId::Guest(A.into());
        let depth_id = workload::TargetId::Guest(Z.into());
        let presentation = |r: &Renderer| {
            read_texture(
                &r.device,
                &r.queue,
                &r.inner.framebuffers[&color_id]
                    .presentation
                    .as_ref()
                    .unwrap()
                    .texture,
            )
        };
        let enhanced_depth = |r: &Renderer| {
            read_texture(
                &r.device,
                &r.queue,
                &r.inner.depthbuffers[&depth_id]
                    .presentation
                    .as_ref()
                    .unwrap()
                    .texture,
            )
        };
        let scanout = |r: &mut Renderer| {
            common::pixels_from_render(
                &r.device,
                &r.queue,
                32 * scale,
                32 * scale,
                FORMAT,
                |view| {
                    let mut encoder = r.device.create_command_encoder(&Default::default());
                    r.inner
                        .scanout(&r.device, &mut encoder, view, color_id, None);
                    r.queue.submit([encoder.finish()]);
                },
            )
        };
        let before = (
            presentation(&renderer),
            enhanced_depth(&renderer),
            scanout(&mut renderer),
        );
        let native_color = read_texture(
            &renderer.device,
            &renderer.queue,
            &renderer.inner.framebuffers[&color_id].color,
        );
        let native_depth = read_texture(
            &renderer.device,
            &renderer.queue,
            &renderer.inner.depthbuffers[&depth_id].texture,
        );
        let native = renderer
            .inner
            .coherence
            .as_ref()
            .unwrap()
            .native_fill_pixels(&renderer.device, &renderer.queue);
        let registers = renderer.rdp.clone();
        let descriptors = renderer.inner.target_descriptors.clone();
        let last_scanout = renderer.last_scanout_addr;
        let guest = memory.0.clone();
        let mut interpreted = scene(&rejected_native_projection());
        let workload = workload::Workload::new(&interpreted);
        assert!(workload
            .check_strict_guest(&interpreted, renderer.inner.coherence.as_ref().unwrap())
            .is_ok());
        let prepared = renderer
            .inner
            .prepare_inputs(&renderer.device, &interpreted)
            .unwrap()
            .unwrap();
        let staged = renderer
            .inner
            .stage_strict_task(&renderer.device, &renderer.queue, color_id);
        assert_eq!(
            presentation(&renderer),
            before.0,
            "stage must retain untouched scaled pixels"
        );
        assert_eq!(enhanced_depth(&renderer), before.1);
        renderer.inner.coherence.as_mut().unwrap().strict_fills = true;
        assert!(renderer
            .inner
            .render_prepared(
                &renderer.device,
                &renderer.queue,
                &prepared,
                ClearPolicy::Persist
            )
            .is_some());
        assert_ne!(
            presentation(&renderer),
            before.0,
            "presentation pass must run before validation"
        );
        assert_eq!(
            renderer
                .inner
                .coherence
                .as_mut()
                .unwrap()
                .validate_strict(&renderer.device),
            Err(workload::UnsupportedGuestTask)
        );
        renderer.inner.discard_strict_task(staged);
        assert_eq!(
            (
                presentation(&renderer),
                enhanced_depth(&renderer),
                scanout(&mut renderer)
            ),
            before
        );
        assert_eq!(
            read_texture(
                &renderer.device,
                &renderer.queue,
                &renderer.inner.framebuffers[&color_id].color
            ),
            native_color
        );
        assert_eq!(
            read_texture(
                &renderer.device,
                &renderer.queue,
                &renderer.inner.depthbuffers[&depth_id].texture
            ),
            native_depth
        );
        assert_eq!(
            renderer
                .inner
                .coherence
                .as_ref()
                .unwrap()
                .native_fill_pixels(&renderer.device, &renderer.queue),
            native
        );
        assert_eq!(renderer.rdp, registers);
        assert_eq!(renderer.inner.target_descriptors, descriptors);
        assert_eq!(renderer.last_scanout_addr, last_scanout);
        assert_eq!(memory.0, guest);
        assert!(renderer.take_guest_writes().is_empty());
        interpreted.use_compatibility_policy();
        renderer.inner.coherence.as_mut().unwrap().begin_task();
        assert!(renderer
            .inner
            .render_into_store(
                &renderer.device,
                &renderer.queue,
                &interpreted,
                ClearPolicy::Persist
            )
            .is_some());
        let pixels = presentation(&renderer);
        for y in 0..32 * scale {
            for x in 0..32 * scale {
                let offset = ((y * 32 * scale + x) * 4) as usize;
                let expected = if x < scale && y < scale {
                    [255; 4]
                } else {
                    [0, 0, 255, 255]
                };
                assert_eq!(
                    &pixels[offset..offset + 4],
                    &expected,
                    "({x},{y}) at {scale}x"
                );
            }
        }
        assert_eq!(enhanced_depth(&renderer), before.1);
    }
}

#[test]
fn process_dl_routes_scaled_validation_failure_to_compatibility_once() {
    for scale in [2, 4] {
        let mut renderer = facade(scale, false);
        let profiling = crate::profiling::Recorder::new(crate::profiling::Mode::Counters);
        renderer.inner.profiling = profiling.clone();
        renderer.set_guest_writeback_with_encoding(true, crate::FramebufferEncoding::BigEndian);
        let mut memory = Memory(vec![0; B as usize + 4096]);
        let (summary, diagnostics) = submit(
            &mut renderer,
            &mut memory,
            &fill_task(A, 32, 32, 0x003f003f),
        );
        assert!(summary.renderable && diagnostics.is_empty());
        apply(&mut renderer, &mut memory);
        let (summary, diagnostics) =
            submit(&mut renderer, &mut memory, &rejected_native_projection());
        assert!(
            summary.renderable && diagnostics.is_empty(),
            "{diagnostics:?}"
        );
        let counters = profiling.snapshot().counters;
        assert_eq!(counters.get("tasks"), Some(&2));
        assert_eq!(counters.get("tasks.guest.native"), Some(&1));
        assert_eq!(counters.get("tasks.guest.fallback"), Some(&1));
        assert_eq!(counters.get("tasks.guest.compatibility"), Some(&1));
        assert_eq!(
            renderer.take_guest_writes(),
            [crate::GuestWrite {
                address: A.into(),
                bytes: vec![255, 255]
            }]
        );
        let pixels = read_texture(
            &renderer.device,
            &renderer.queue,
            &renderer.inner.framebuffers[&workload::TargetId::Guest(A.into())]
                .presentation
                .as_ref()
                .unwrap()
                .texture,
        );
        for y in 0..32 * scale {
            for x in 0..32 * scale {
                let offset = ((y * 32 * scale + x) * 4) as usize;
                let expected = if x < scale && y < scale {
                    [255; 4]
                } else {
                    [0, 0, 255, 255]
                };
                assert_eq!(&pixels[offset..offset + 4], &expected);
            }
        }
    }
}

#[test]
fn strict_layout_failure_restores_presentation_versions_before_fallback_admission() {
    for scale in [2, 4] {
        let mut renderer = facade(scale, false);
        renderer.set_guest_writeback_with_encoding(true, crate::FramebufferEncoding::BigEndian);
        let mut memory = Memory(vec![0; B as usize + 4096]);
        let (summary, diagnostics) = submit(
            &mut renderer,
            &mut memory,
            &fill_task(A, 16, 32, 0x003f003f),
        );
        assert!(summary.renderable && diagnostics.is_empty());
        apply(&mut renderer, &mut memory);
        renderer.inner.attachment_budget = 16 * 32 * 4 * (1 + u64::from(scale * scale))
            + 32 * 32 * 4 * (1 + u64::from(scale * scale));
        let (summary, diagnostics) =
            submit(&mut renderer, &mut memory, &rejected_native_projection());
        assert!(
            summary.renderable && diagnostics.is_empty(),
            "{summary:?}, {diagnostics:?}"
        );
        assert_eq!(
            renderer.take_guest_writes(),
            [crate::GuestWrite {
                address: A.into(),
                bytes: vec![255, 255]
            }]
        );
        let fb = &renderer.inner.framebuffers[&workload::TargetId::Guest(A.into())];
        let pixels = read_texture(
            &renderer.device,
            &renderer.queue,
            &fb.presentation.as_ref().unwrap().texture,
        );
        assert_eq!(&pixels[..4], &[255; 4]);
        assert_eq!(fb.layout.width, 32);
    }
}
