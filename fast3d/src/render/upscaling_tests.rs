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
        let mut renderer = SceneRenderer::new(&device, FORMAT, 193, 131, dual);
        renderer.set_test_scale(scale, BUDGET);
        let mut depth_scene = fill_scene(Z, 193, 131, 0xfffcfffc);
        depth_scene.framebuffer_pairs[0].is_depth_clear = true;
        depth_scene.framebuffer_pairs[0].depth_image = Some(Z.into());
        renderer.render_into_store(&device, &queue, &depth_scene, ClearPolicy::Persist);
        for height in [131, 157, 31] {
            let mut draw = fill_scene(
                A,
                193,
                height,
                if height == 31 { 0xf801f801 } else { 0x07c107c1 },
            );
            depth_scene.framebuffer_pairs[0].size_extent.1 = height;
            depth_scene.framebuffer_pairs[0].active_scissor.lry = height as i32;
            depth_scene.framebuffer_pairs[0].ops.clear();
            renderer.render_into_store(&device, &queue, &depth_scene, ClearPolicy::Persist);
            draw.framebuffer_pairs[0].depth_image = Some(Z.into());
            renderer.render_into_store(&device, &queue, &draw, ClearPolicy::Persist);
            let retained = height.max(if height == 31 { 157 } else { height });
            let fb = &renderer.framebuffers[&workload::TargetId::Guest(A.into())];
            assert_eq!(
                (fb.color.width(), fb.color.height(), fb.height),
                (193, retained, retained)
            );
            let descriptor = renderer.target_descriptors.get(A.into(), false).unwrap();
            assert_eq!(
                descriptor.range().unwrap(),
                u64::from(A)..u64::from(A) + 193 * u64::from(retained) * 2
            );
            let depth = &renderer.depthbuffers[&workload::TargetId::Guest(Z.into())];
            assert_eq!(
                (depth.texture.width(), depth.texture.height()),
                (193, retained)
            );
            for presentation in [
                fb.presentation.as_ref().map(|p| &p.texture),
                depth.presentation.as_ref().map(|p| &p.texture),
            ] {
                assert_eq!(
                    presentation.map(|p| (p.width(), p.height())),
                    (scale > 1).then_some((193 * scale, retained * scale))
                );
            }
        }
        let vi = crate::ViRegisters {
            origin: A + 193 * 7 * 2,
            width: 193,
            status: 2,
            v_start: 34,
            y_scale: 1024,
            ..Default::default()
        };
        let source =
            crate::select_scanout_source(Some(vi), true, None, renderer.scanout_images()).unwrap();
        assert_eq!(source.rows, Some(7..24));
        let pixels =
            common::pixels_from_render(&device, &queue, 193 * scale, 17 * scale, FORMAT, |view| {
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
    let mut renderer = Renderer::with_device(
        device,
        queue,
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
    renderer.inner.set_test_scale(scale, BUDGET);
    renderer
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
            for scale in [1, 2, 3, 4] {
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
        let mut renderer = facade(scale, false);
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
