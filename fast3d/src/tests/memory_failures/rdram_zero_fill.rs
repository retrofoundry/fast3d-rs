use crate::hle::{gbi::GbiUcode, interp::interpret};
use crate::{DataFormat, DiagKind, RdramImage, Severity};
use n64_gbi::{consts::*, encode::*};

fn image(commands: &[(u32, u32)]) -> Vec<u8> {
    commands
        .iter()
        .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
        .collect()
}

#[test]
fn every_rsp_and_rdp_data_reader_warns_and_continues() {
    for (ucode, commands, address, length) in [
        (GbiUcode::F3dex2, vec![gsp_vertex(0, 3, 0x1000)], 0x1000, 16),
        (
            GbiUcode::F3dex2,
            vec![gsp_matrix(0x1000, false, true, false)],
            0x1000,
            64,
        ),
        (GbiUcode::F3dex2, vec![(0xdc08_0008, 0x1000)], 0x1000, 2),
        (GbiUcode::F3dex2, vec![(0xdc08_060a, 0x1000)], 0x1000, 1),
        (GbiUcode::F3dex2, vec![(0xdc08_000a, 0x1000)], 0x1008, 1),
        (
            GbiUcode::F3dex2,
            vec![(0xfd10_0000, 0x1000), (0xf300_0000, 0)],
            0x1000,
            8,
        ),
        (
            GbiUcode::F3dex2,
            vec![(0xfd10_0000, 0x1000), (0xf400_0000, 0)],
            0x1000,
            8,
        ),
        (
            GbiUcode::F3dex2,
            vec![(0xfd10_0000, 0x1000), (0xf000_0000, 0x0700_4000)],
            0x1000,
            4,
        ),
        (
            GbiUcode::F3d,
            vec![gsp_vertex_f3d(0, 3, 0x1000)],
            0x1000,
            16,
        ),
        (
            GbiUcode::F3d,
            vec![gsp_matrix_f3d(0x1000, false, true, false)],
            0x1000,
            64,
        ),
        (GbiUcode::F3d, vec![(0x039e_0000, 0x1000)], 0x1000, 64),
    ] {
        let mut commands = commands;
        commands.push(if ucode == GbiUcode::F3d {
            gsp_enddl_f3d()
        } else {
            gsp_enddl()
        });
        let bytes = image(&commands);
        let result = interpret(RdramImage::new(&bytes), 0, ucode, DataFormat::Fixed, None);
        assert_eq!(
            result.termination,
            crate::inspect::WalkTermination::End,
            "{commands:x?}: {:?}",
            result.diags
        );
        assert_eq!(result.commands as usize, commands.len());
        assert_eq!(result.diags.len(), 1, "{:?}", result.diags);
        assert_eq!(
            result.diags[0].kind,
            DiagKind::RdramReadZeroFilled { address, length }
        );
        assert_eq!(result.diags[0].kind.severity(), Severity::Warn);
    }
}

#[test]
fn k0_segment_textures_and_palettes_read_masked_physical_bytes() {
    for ucode in [GbiUcode::F3d, GbiUcode::F3dex2] {
        for load in [0xf300_0000, 0xf400_0000, 0xf000_0000] {
            let mut bytes = image(&[
                (0xfd10_0000, 0x0100_0100),
                (load, if load == 0xf000_0000 { 0x0700_4000 } else { 0 }),
                if ucode == GbiUcode::F3d {
                    gsp_enddl_f3d()
                } else {
                    gsp_enddl()
                },
            ]);
            bytes.resize(0x100, 0);
            bytes.extend([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]);
            let mut expected = None;
            for segment in [0, 0x8000_0000] {
                let mut memory = RdramImage::new(&bytes);
                memory.set_segment(1, segment);
                let result = interpret(memory, 0, ucode, DataFormat::Fixed, None);
                assert_eq!(result.termination, crate::inspect::WalkTermination::End);
                assert!(result.diags.is_empty(), "{:?}", result.diags);
                assert!(result.rdp.tmem_bank.encoded_bank().iter().any(|&b| b != 0));
                if let Some(expected) = &expected {
                    assert_eq!(&result.rdp.tmem_bank, expected);
                } else {
                    expected = Some(result.rdp.tmem_bank);
                }
            }
        }
    }
}

#[test]
fn zero_filled_matrix_and_texture_replace_previous_values() {
    use crate::Rdram;
    let mut bytes = image(&[
        gsp_matrix(0x100, false, true, false),
        gsp_matrix(0x200, false, true, false),
        gsp_vertex(0, 1, 0x100),
        gdp_set_texture_image(0, 2, 4, 0x100),
        gdp_load_block(0, 0, 0, 3, 0),
        gdp_set_texture_image(0, 2, 4, 0x13c),
        gdp_load_block(0, 0, 0, 3, 0),
        gsp_enddl(),
    ]);
    bytes.resize(0x140, 0xff);
    let result = interpret(
        RdramImage::new(&bytes),
        0,
        GbiUcode::F3dex2,
        DataFormat::Fixed,
        None,
    );
    assert_eq!(result.termination, crate::inspect::WalkTermination::End);
    assert_eq!(
        result.scene.mvp_table[*result.scene.mtx_index.last().unwrap() as usize],
        [[0.0; 4]; 4]
    );
    assert_eq!(
        &result.rdp.tmem_bank.encoded_bank()[..8],
        &[255, 255, 255, 255, 0, 0, 0, 0]
    );
    assert_eq!(result.diags.len(), 1);
    let memory = RdramImage::new(&bytes);
    let matrix = memory.read_matrix(0x138, DataFormat::Fixed).unwrap();
    assert_eq!(matrix[0], [-1.0; 4]);
    assert_eq!(matrix[1..], [[0.0; 4]; 3]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn missing_vertices_draw_nothing_and_preserve_the_next_draw() {
    struct Image(Vec<u8>);
    impl crate::Hardware for Image {
        fn rdram(&self) -> impl crate::Rdram + '_ {
            RdramImage::new(&self.0)
        }
    }
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let mut baseline = None;
    for (bad_address, segment, physical, skip_bad_draw) in [
        (0x200, 0, 0x200, true),
        (0x200, 0, 0x200, false),
        (0x130, 0, 0x140, false),
        (0x138, 0, 0x138, false),
        (0x05dd_6dc8, 0x000e_82a0, 0x00eb_f068, false),
    ] {
        let commands = [
            gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
            (0xfcff_ffff, 0xfffe_793c),
            gsp_segment(5, segment),
            gsp_vertex(0, 3, 0xd0),
            gsp_vertex(0, 3, bad_address),
            if skip_bad_draw {
                gsp_spnoop()
            } else {
                gsp_1triangle(0, 1, 2)
            },
            gsp_vertex(0, 3, bad_address),
            gsp_vertex(0, 3, 0x100),
            gsp_1triangle(0, 1, 2),
            gsp_enddl(),
        ];
        let mut bytes = image(&commands);
        bytes.resize(0x140, 0);
        for (offset, x, y, color) in [
            (0xd0, -1, -1, [0, 0, 255]),
            (0xe0, 0, 0, [0, 0, 255]),
            (0xf0, -1, 1, [0, 0, 255]),
            (0x100, 0, 0, [255, 0, 0]),
            (0x110, 1, -1, [255, 0, 0]),
            (0x120, 1, 1, [255, 0, 0]),
            (0x130, 1, 1, [255, 0, 0]),
        ] {
            let vertex = VtxColored {
                x,
                y,
                z: 0,
                flag: 0,
                s: 0,
                t: 0,
                r: color[0],
                g: color[1],
                b: color[2],
                a: 255,
            };
            bytes[offset..offset + 16].copy_from_slice(&vertex.to_bytes());
        }
        if segment != 0 {
            bytes.resize(4 * 1024 * 1024, 0);
        }
        let mut renderer = crate::Renderer::with_device(
            device.clone(),
            queue.clone(),
            crate::PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 64,
                height: 64,
            },
            crate::RendererConfig {
                resolution_multiplier: 1,
                sample_count: 1,
                present_mode: wgpu::PresentMode::Fifo,
                format: None,
                clear_policy: crate::ClearPolicy::PerFrame,
                power_preference: wgpu::PowerPreference::LowPower,
            },
        );
        let mut diags = Vec::new();
        let summary = renderer.process_dl(&Image(bytes), 0, crate::Microcode::F3dex2, &mut diags);
        assert!(summary.renderable, "{summary:?}: {diags:?}");
        assert_eq!(summary.termination, crate::inspect::WalkTermination::End);
        assert_eq!(summary.errors, 0);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].at, 32);
        assert_eq!(
            diags[0].kind,
            DiagKind::RdramReadZeroFilled {
                address: physical,
                length: 16
            }
        );
        let pixels = crate::tests::common::pixels_from_render(
            &device,
            &queue,
            64,
            64,
            wgpu::TextureFormat::Rgba8Unorm,
            |view| renderer.present_last_to(view),
        );
        assert!(pixels.as_chunks::<4>().0.contains(&[255, 0, 0, 255]));
        assert!(!pixels.as_chunks::<4>().0.contains(&[0, 0, 255, 255]));
        if let Some(expected) = &baseline {
            assert_eq!(&pixels, expected);
        } else {
            baseline = Some(pixels);
        }
    }
}

#[test]
fn command_fetch_after_zero_filled_data_still_rejects_the_task() {
    let bytes = image(&[gsp_vertex(0, 3, 0x1000), gsp_branchlist(0x2000)]);
    let result = interpret(
        RdramImage::new(&bytes),
        0,
        GbiUcode::F3dex2,
        DataFormat::Fixed,
        None,
    );
    assert_eq!(result.termination, crate::inspect::WalkTermination::Bounds);
    assert!(result.scene.raw_pos.is_empty());
    assert!(matches!(
        result.diags.as_slice(),
        [
            crate::Diagnostic {
                kind: DiagKind::RdramReadZeroFilled { .. },
                ..
            },
            crate::Diagnostic {
                kind: DiagKind::MemoryRead {
                    access: crate::MemoryAccess::Command,
                    ..
                },
                ..
            }
        ]
    ));
}

#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
#[test]
fn host_vertex_range_stays_strict() {
    let vertices = [0u8; 16];
    let address = vertices.as_ptr() as u64;
    for offset in [8, 16] {
        let commands = [[0x0100_3006u64, address + offset], [0xdf00_0000, 0]];
        let entry = commands.as_ptr() as u64;
        let ranges = [
            entry..entry + std::mem::size_of_val(&commands) as u64,
            address..address + 16,
        ];
        let result = interpret(
            unsafe {
                crate::hle::host_mem::HostMemory::new(crate::HostRam::with_ranges(&[], &ranges))
            },
            entry,
            GbiUcode::F3dex2,
            DataFormat::Fixed,
            None,
        );
        assert_eq!(result.commands, 1);
        assert!(result.scene.raw_pos.is_empty());
        assert!(matches!(
            result.diags.as_slice(),
            [crate::Diagnostic {
                kind: DiagKind::MemoryRead {
                    access: crate::MemoryAccess::Vertex,
                    ..
                },
                ..
            }]
        ));
    }
}
