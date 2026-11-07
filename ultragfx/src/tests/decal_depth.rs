use crate::render::{headless_device, headless_device_forced_fallback, SceneRenderer};
use crate::scene::{DrawRun, PrimitiveDepth, Scene};
use crate::tests::common::{render_to_pixels, scene_from_fixture};

#[derive(Clone, Copy, serde::Deserialize)]
struct Case<'a> {
    name: &'a str,
    stored_z: f32,
    stored_dz: u16,
    seed_prim: bool,
    incoming_z: f32,
    incoming_dz: u16,
    prim: bool,
    slope: f32,
    compare: bool,
    passes: bool,
}

fn scene(case: Case<'_>) -> Scene {
    let Case {
        stored_z,
        stored_dz,
        seed_prim,
        incoming_z,
        incoming_dz,
        prim,
        slope,
        compare,
        ..
    } = case;
    let template = scene_from_fixture("decal--empty-texture");
    let mut material = template.materials[0].clone();
    material.selectors = crate::hle::combiner::decode_combine(0, 0xc3);
    material.tex_enable = false;
    material.cycle_type = 0;
    material.prim = [255, 0, 0, 255];
    let mut green = material.clone();
    green.prim = [0, 255, 0, 255];
    let mut seed = crate::hle::decode_render_mode(0x20, 0, 0);
    seed.z_source_prim = seed_prim;
    let mut query = crate::hle::decode_render_mode(0xc00 | if compare { 0x10 } else { 0 }, 0, 0);
    query.z_source_prim = prim;
    let mut result = Scene {
        materials: vec![material, green],
        render_modes: vec![seed, query],
        mvp_table: vec![crate::hle::math::identity()],
        viewport_table: vec![([160.0, 120.0, 1.0 / 262144.0], [160.0, 120.0, 0.0])],
        texcoord_table: vec![[0.0, 0.0]],
        ..Default::default()
    };
    for (z, dy) in [(stored_z, 0.0), (incoming_z, slope)] {
        for [x, y] in [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
            result.raw_pos.push([x, y, z - y * 120.0 * dy]);
            result.mtx_index.push(0);
            result.viewport_index.push(0);
            result.raw_st.push([0.0; 2]);
            result.texcoord_index.push(0);
            result.cn.push(0xffffffff);
            result.light_index.push(0);
            result.light_count.push(0);
        }
    }
    result.indices = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
    result.draw_runs = [stored_dz, incoming_dz]
        .into_iter()
        .enumerate()
        .map(|(i, dz)| DrawRun {
            fog_color: [0; 4],
            prim_depth: PrimitiveDepth {
                z: (if i == 0 { stored_z } else { incoming_z } / 8.0) as u16,
                dz,
            },
            material_index: i as u32,
            render_mode_index: i as u32,
            cull: crate::hle::CullKind::None,
            unclipped_depth: false,
            index_start: i as u32 * 6,
            index_count: 6,
        })
        .collect();
    result
}

#[test]
fn depth_replay_batches_writeback_segments_before_readers() {
    let (device, queue, dual) = headless_device();
    for scale in [1, 2, 4] {
        let mut renderer =
            SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240, dual);
        renderer.set_test_scale(scale, 256 * 1024 * 1024);
        let passes = if scale > 1 { 2 } else { 1 };
        for writeback in [false, true] {
            for (decal, trailing) in [(false, false), (true, false), (true, true)] {
                let mut scene = scene(Case {
                    name: "batched-writers",
                    stored_z: 229376.0,
                    stored_dz: 32,
                    seed_prim: true,
                    incoming_z: 229632.0,
                    incoming_dz: 1,
                    prim: false,
                    slope: 0.0,
                    compare: true,
                    passes: true,
                });
                let query = scene.draw_runs.pop().unwrap();
                scene.draw_runs = vec![scene.draw_runs[0]; 4];
                if decal {
                    scene.render_modes[1].z_write = trailing;
                    scene.draw_runs.push(query);
                }
                if trailing {
                    scene.draw_runs.push(scene.draw_runs[0]);
                }
                renderer.coherence = writeback.then(Default::default);
                let recorder = crate::profiling::Recorder::new(crate::profiling::Mode::Counters);
                renderer.profiling = recorder.clone();
                let pixels = render_to_pixels(&device, &queue, &mut renderer, &scene, 320, 240);
                assert_eq!(
                    &pixels[(120 * 320 + 160) * 4..][..3],
                    if decal && !trailing {
                        [0, 255, 0]
                    } else {
                        [255, 0, 0]
                    },
                    "writeback {writeback}, decal {decal}"
                );
                let counters = recorder.snapshot().counters;
                assert_eq!(
                    counters.get("depth_delta.passes"),
                    Some(&(passes * (1 + u64::from(trailing))))
                );
                assert_eq!(
                    counters["workload.segments"],
                    passes
                        * if writeback {
                            4 + u64::from(decal) + u64::from(trailing)
                        } else {
                            1 + u64::from(decal) + u64::from(trailing)
                        }
                );
            }
        }
    }
}

#[test]
fn hardware_sweep_and_float_policy() {
    let hardware: Vec<Case> =
        serde_json::from_str(include_str!("../../tests/fixtures/decal-depth-cases.json")).unwrap();
    for fallback in [false, true] {
        let (device, queue, dual) = if fallback {
            let (device, queue) = headless_device_forced_fallback();
            (device, queue, false)
        } else {
            headless_device()
        };
        let mut renderer =
            SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240, dual);
        for &case in hardware.iter().chain(
            [
                Case {
                    name: "float-only-behind-equal-stored-4097",
                    stored_z: 4097.0,
                    stored_dz: 1,
                    seed_prim: false,
                    incoming_z: 4225.0,
                    incoming_dz: 1,
                    prim: false,
                    slope: 0.0,
                    compare: true,
                    passes: true,
                },
                Case {
                    name: "compressed-only-front-equal-stored-4097",
                    stored_z: 4097.0,
                    stored_dz: 1,
                    seed_prim: false,
                    incoming_z: 3968.0,
                    incoming_dz: 1,
                    prim: false,
                    slope: 0.0,
                    compare: true,
                    passes: false,
                },
            ]
            .iter(),
        ) {
            let pixels = render_to_pixels(&device, &queue, &mut renderer, &scene(case), 320, 240);
            let expected = if case.passes {
                [0, 255, 0]
            } else {
                [255, 0, 0]
            };
            assert_eq!(
                &pixels[(120 * 320 + 160) * 4..][..3],
                expected,
                "{}, fallback {fallback}",
                case.name
            );
        }
    }
}

#[test]
fn native_window_at_1_2_4x() {
    let (device, queue, dual) = headless_device();
    for scale in [1, 2, 4] {
        let (w, h) = (320 * scale, 240 * scale);
        let mut renderer = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, w, h, dual);
        for name in ["decal--in-front", "decal--behind"] {
            let image = render_to_pixels(
                &device,
                &queue,
                &mut renderer,
                &scene_from_fixture(name),
                w,
                h,
            );
            assert!(
                image
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p[..3] == [240, 220, 40]),
                "{name}, {scale}x"
            );
        }
        for (offset, pass) in [(-65.0, false), (-64.0, true), (64.0, true), (65.0, false)] {
            let case = Case {
                name: "native-triangle-slope",
                stored_z: 229376.0,
                stored_dz: 1,
                seed_prim: true,
                incoming_z: 229376.0 + offset,
                incoming_dz: 1,
                prim: false,
                slope: 32.0,
                compare: true,
                passes: pass,
            };
            let image = render_to_pixels(&device, &queue, &mut renderer, &scene(case), w, h);
            assert_eq!(
                &image[((h / 2 * w + w / 2) * 4) as usize..][..3],
                if pass { [0, 255, 0] } else { [255, 0, 0] },
                "{offset}, {scale}x"
            );
        }
    }
}

#[test]
fn stored_delta_follows_successful_depth_writer() {
    for fallback in [false, true] {
        let (device, queue, dual) = if fallback {
            let (device, queue) = headless_device_forced_fallback();
            (device, queue, false)
        } else {
            headless_device()
        };
        let mut renderer =
            SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240, dual);
        for (label, z, test, write, discard, offset, pass) in [
            ("farther-fails", 28675, true, true, false, 9.0, false),
            ("equal-fails-less", 28672, true, true, false, 9.0, false),
            ("update-off", 28672, false, false, false, 9.0, false),
            ("alpha-discard", 28672, false, true, true, 9.0, false),
            ("last-writer", 28672, false, true, false, 256.0, true),
            (
                "last-writer-outside",
                28672,
                false,
                true,
                false,
                257.0,
                false,
            ),
        ] {
            let mut scene = scene(Case {
                name: label,
                stored_z: 229376.0,
                stored_dz: 1,
                seed_prim: true,
                incoming_z: 229376.0 + offset,
                incoming_dz: 1,
                prim: false,
                slope: 0.0,
                compare: true,
                passes: pass,
            });
            let mut writer = scene.draw_runs[0];
            let mut mode = scene.render_modes[0];
            mode.z_test = test;
            mode.z_write = write;
            if discard {
                mode.alpha_compare = crate::hle::AlphaCompare::Threshold;
                let mut material = scene.materials[0].clone();
                material.prim[3] = 0;
                material.blend_color[3] = 128;
                scene.materials.push(material);
                writer.material_index = 2;
            }
            scene.render_modes.push(mode);
            writer.render_mode_index = 2;
            writer.prim_depth = PrimitiveDepth { z, dz: 32 };
            scene.draw_runs.insert(1, writer);
            let image = render_to_pixels(&device, &queue, &mut renderer, &scene, 320, 240);
            assert_eq!(
                &image[(120 * 320 + 160) * 4..][..3],
                if pass { [0, 255, 0] } else { [255, 0, 0] },
                "{label}, fallback {fallback}"
            );
        }
        for (offset, pass) in [(64.0, true), (65.0, false)] {
            let mut surface = scene(Case {
                name: "stored-triangle",
                stored_z: 229376.0,
                stored_dz: 1,
                seed_prim: false,
                incoming_z: 229376.0 + offset,
                incoming_dz: 1,
                prim: false,
                slope: 0.0,
                compare: true,
                passes: pass,
            });
            for v in &mut surface.raw_pos[..4] {
                v[2] -= v[1] * 120.0 * 32.0;
            }
            let image = render_to_pixels(&device, &queue, &mut renderer, &surface, 320, 240);
            assert_eq!(
                &image[(120 * 320 + 160) * 4..][..3],
                if pass { [0, 255, 0] } else { [255, 0, 0] },
                "stored triangle {offset}, fallback {fallback}"
            );
        }
    }
}

#[test]
fn triangle_delta_uses_unclamped_signed_native_slopes() {
    let (device, queue, dual) = headless_device();
    for (sx, sy) in [(1, 1), (2, 2), (4, 4), (2, 1), (1, 4)] {
        let (w, h) = (320 * sx, 240 * sy);
        let mut renderer = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, w, h, dual);
        for (slope, offset, pass) in [
            (32.0, 64.0, true),
            (32.0, 65.0, false),
            (-32.0, 32.0, true),
            (-32.0, 33.0, false),
        ] {
            let scene = scene(Case {
                name: "signed-native-slope",
                stored_z: 229376.0,
                stored_dz: 1,
                seed_prim: true,
                incoming_z: 229376.0 + offset,
                incoming_dz: 1,
                prim: false,
                slope,
                compare: true,
                passes: pass,
            });
            let image = render_to_pixels(&device, &queue, &mut renderer, &scene, w, h);
            assert_eq!(
                &image[((h / 2 * w + w / 2) * 4) as usize..][..3],
                if pass { [0, 255, 0] } else { [255, 0, 0] },
                "{slope}, {offset}, {sx}x{sy}"
            );
        }
        let mut unclamped = scene(Case {
            name: "unclamped-slope",
            stored_z: 262112.0,
            stored_dz: 1,
            seed_prim: true,
            incoming_z: 262160.0,
            incoming_dz: 1,
            prim: false,
            slope: 64.0,
            compare: true,
            passes: true,
        });
        unclamped.draw_runs[1].unclipped_depth = true;
        let image = render_to_pixels(&device, &queue, &mut renderer, &unclamped, w, h);
        assert_eq!(
            &image[((h / 2 * w + w / 2) * 4) as usize..][..3],
            [0, 255, 0],
            "unclamped {sx}x{sy}"
        );
        for (offset, pass) in [(64.0, true), (65.0, false)] {
            let mut x_slope = scene(Case {
                name: "native-x-slope",
                stored_z: 229376.0,
                stored_dz: 1,
                seed_prim: true,
                incoming_z: 229376.0 + offset,
                incoming_dz: 1,
                prim: false,
                slope: 0.0,
                compare: true,
                passes: pass,
            });
            for v in &mut x_slope.raw_pos[4..] {
                v[2] += v[0] * 160.0 * 32.0;
            }
            let image = render_to_pixels(&device, &queue, &mut renderer, &x_slope, w, h);
            assert_eq!(
                &image[((h / 2 * w + w / 2) * 4) as usize..][..3],
                if pass { [0, 255, 0] } else { [255, 0, 0] },
                "native X {offset}, {sx}x{sy}"
            );
        }
    }
}
