use crate::hle::{gbi::GbiUcode, interp::interpret, mem::GbiDataFormat};
use crate::RdramImage;

fn image(commands: &[(u32, u32)]) -> Vec<u8> {
    commands
        .iter()
        .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
        .collect()
}

#[test]
fn failed_command_discards_recorded_operations() {
    let bytes = image(&[(0xff10_003f, 0x1000), (0xf600_4004, 0)]);
    let result = interpret(
        RdramImage::new(&bytes),
        0,
        GbiUcode::F3dex2,
        GbiDataFormat::Fixed,
    );
    assert!(result.scene.framebuffer_pairs.is_empty());
    assert!(result.scene.indices.is_empty());
    assert_eq!(result.commands, 3);
    assert_eq!(result.dropped_runs, 1);
}

#[test]
fn bad_vertex_input_returns_error_without_partial_vertices() {
    let bytes = image(&[(0x0100_2004, 16), (0xdf00_0000, 0)]);
    let result = interpret(
        RdramImage::new(&bytes),
        0,
        GbiUcode::F3dex2,
        GbiDataFormat::Fixed,
    );
    assert!(!result.diags.is_empty());
    assert!(result.scene.raw_pos.is_empty());
    assert_eq!(result.commands, 1);
}

#[test]
fn image_operand_bounds_all_read_kinds() {
    use crate::{DiagKind, Diagnostic, MemoryAccess as A, MemoryError, MemoryErrorKind as K};
    type OperandFailure = (&'static [(u32, u32)], A, u64, u64, u64, u32);
    let cases: &[OperandFailure] = &[
        (&[], A::Command, 0, 0, 8, 1),
        (&[(0xe400_0000, 0)], A::Continuation, 0, 8, 8, 1),
        (
            &[(0xe400_0000, 0), (0xe100_0000, 0)],
            A::Continuation,
            0,
            16,
            8,
            1,
        ),
        (&[(0x0100_1002, 0x1000)], A::Vertex, 0, 0x1000, 16, 1),
        (&[(0xda38_0002, 0x1000)], A::Matrix, 0, 0x1000, 64, 1),
        (&[(0xdc08_0008, 0x1000)], A::Viewport, 0, 0x1000, 2, 1),
        (&[(0xdc08_060a, 0x1000)], A::Light, 0, 0x1000, 1, 1),
        (&[(0xdc08_000a, 0x1000)], A::LookAt, 0, 0x1008, 1, 1),
        (
            &[(0xfd10_0000, 0x1000), (0xf300_0000, 0)],
            A::Texture,
            8,
            0x1000,
            8,
            2,
        ),
        (
            &[(0xfd10_0000, 0x1000), (0xf400_0000, 0)],
            A::Texture,
            8,
            0x1000,
            8,
            2,
        ),
        (
            &[(0xfd10_0000, 0x1000), (0xf000_0000, 0x0700_4000)],
            A::Tlut,
            8,
            0x1000,
            4,
            2,
        ),
    ];
    for &(commands, access, at, address, length, attempted) in cases {
        let bytes = image(commands);
        let result = interpret(
            RdramImage::new(&bytes),
            0,
            GbiUcode::F3dex2,
            GbiDataFormat::Fixed,
        );
        assert_eq!(
            result.diags,
            [Diagnostic {
                at,
                kind: DiagKind::MemoryRead {
                    access,
                    error: MemoryError {
                        address,
                        length,
                        kind: K::OutOfBounds
                    },
                }
            }],
            "{access:?}"
        );
        assert_eq!(result.commands, attempted, "{access:?}");
        assert!(result.scene.indices.is_empty());
        assert!(result.scene.framebuffer_pairs.is_empty());
    }
}

#[test]
fn unsupported_float_image_is_diagnostic() {
    use crate::{DiagKind, MemoryAccess, MemoryErrorKind};
    for (word, access) in [
        (0x0100_1002, MemoryAccess::Vertex),
        (0xda38_0002, MemoryAccess::Matrix),
    ] {
        let bytes = image(&[(word, 8), (0xdf00_0000, 0)]);
        let result = interpret(
            RdramImage::new(&bytes),
            0,
            GbiUcode::F3dex2,
            GbiDataFormat::Float,
        );
        assert!(
            matches!(result.diags.as_slice(), [crate::Diagnostic { at: 0,
            kind: DiagKind::MemoryRead { access: a, error }
        }] if *a == access && error.address == 8 && error.kind == MemoryErrorKind::UnsupportedFormat)
        );
        assert_eq!(result.commands, 1);
        assert_eq!(result.summary(false).tris, 0);
    }
}

#[test]
fn image_address_overflow_is_diagnostic() {
    use crate::{DiagKind, MemoryAccess, MemoryError, MemoryErrorKind};
    let result = interpret(
        RdramImage::new(&[]),
        u64::MAX - 3,
        GbiUcode::F3dex2,
        GbiDataFormat::Fixed,
    );
    assert_eq!(
        result.diags,
        [crate::Diagnostic {
            at: u64::MAX - 3,
            kind: DiagKind::MemoryRead {
                access: MemoryAccess::Command,
                error: MemoryError {
                    address: u64::MAX - 3,
                    length: 8,
                    kind: MemoryErrorKind::AddressOverflow
                },
            },
        }]
    );
    assert_eq!(result.commands, 1);
}

#[cfg(not(target_arch = "wasm32"))]
fn renderer() -> crate::Renderer {
    let (device, queue) = crate::render::headless_device_forced_fallback();
    crate::Renderer::with_device(
        device,
        queue,
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
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn scanout(renderer: &mut crate::Renderer) -> Vec<u8> {
    let (device, queue) = (renderer.device().clone(), renderer.queue().clone());
    super::common::pixels_from_render(
        &device,
        &queue,
        64,
        64,
        wgpu::TextureFormat::Rgba8Unorm,
        |view| renderer.present_last_to(view),
    )
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn failed_task_submits_no_operations() {
    struct Image(Vec<u8>);
    impl crate::Hardware for Image {
        fn rdram(&self) -> impl crate::Rdram + '_ {
            RdramImage::new(&self.0)
        }
    }
    let good = Image(image(&[
        (0xe300_0a01, 0x0030_0000),
        (0xff10_003f, 0x1000),
        (0xed00_0000, 0x0010_0100),
        (0xf700_0000, 0xf801_f801),
        (0xf60f_c0fc, 0),
        (0xdf00_0000, 0),
    ]));
    let bad = Image(image(&[
        (0xe300_0a01, 0x0030_0000),
        (0xff10_003f, 0x1000),
        (0xed00_0000, 0x0010_0100),
        (0xf700_0000, 0x07c1_07c1),
        (0xf60f_c0fc, 0),
        (0xff10_003f, 0x2000),
        (0xf60f_c0fc, 0),
        (0x0100_1002, 0x3000),
    ]));
    let mut renderer = renderer();
    renderer.begin_frame();
    assert!(
        renderer
            .process_dl(&good, 0, crate::Microcode::F3dex2, &mut crate::NopSink)
            .renderable
    );
    let before = scanout(&mut renderer);
    assert!(before.as_chunks::<4>().0.contains(&[255, 0, 0, 255]));
    let summary = renderer.process_dl(&bad, 0, crate::Microcode::F3dex2, &mut crate::NopSink);
    assert_eq!(
        (
            summary.commands,
            summary.errors,
            summary.tris,
            summary.dropped_runs,
            summary.renderable
        ),
        (8, 1, 0, 2, false)
    );
    assert_eq!(renderer.frame_scenes.len(), 1);
    assert_eq!(renderer.last_scanout_addr, Some(0x1000));
    assert!(!renderer.inner.has_fb(0x2000));
    assert_eq!(scanout(&mut renderer), before);
}

#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
#[test]
fn host_entry_preserves_high_pointer_and_numeric_words() {
    use crate::{DataFormat, HostRam, Microcode, NopSink};
    for format in [DataFormat::Fixed, DataFormat::Float] {
        let mut renderer = renderer();
        renderer.set_data_format(format);
        let matrix: Vec<u8> = match format {
            DataFormat::Fixed => n64_gbi::encode::mtx_to_bytes(crate::hle::math::identity())
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|word| u32::from_be_bytes(*word).to_ne_bytes())
                .collect(),
            DataFormat::Float => crate::hle::math::identity()
                .into_iter()
                .flatten()
                .flat_map(f32::to_ne_bytes)
                .collect(),
        };
        let mut vertex = Vec::new();
        match format {
            DataFormat::Fixed => {
                vertex.extend([1i16, -2, 3].into_iter().flat_map(i16::to_ne_bytes))
            }
            DataFormat::Float => {
                vertex.extend([1.5f32, -2.25, 3.75].into_iter().flat_map(f32::to_ne_bytes))
            }
        }
        vertex.extend(0u16.to_ne_bytes());
        vertex.extend([32i16, -64].into_iter().flat_map(i16::to_ne_bytes));
        vertex.extend([10, 20, 30, 255]);
        vertex.resize(if format == DataFormat::Fixed { 16 } else { 24 }, 0);
        let child: [[u64; 2]; 4] = [
            [0xda38_0002, matrix.as_ptr() as u64],
            [0x0100_1002, vertex.as_ptr() as u64],
            [0xff10_003f, u64::MAX - 3],
            [0xdf00_0000, 0],
        ];
        let root = [
            [0xde00_0000u64, child.as_ptr() as u64],
            [0xfb00_0000, 0x1234_5678_1020_3040],
            [0xdf00_0000, 0],
        ];
        let entry = root.as_ptr() as u64;
        assert!(entry > u32::MAX as u64);
        // SAFETY: all reachable arrays remain allocated, initialized and stable through this call.
        let summary = unsafe {
            renderer.process_dl_host(HostRam::new(&[]), entry, Microcode::F3dex2, &mut NopSink)
        };
        assert_eq!((summary.commands, summary.errors), (7, 0));
        let scene = &renderer.frame_scenes[0];
        assert_eq!(scene.color_image.addr, u64::MAX - 3);
        assert_eq!(scene.color_image.width, 64);
        assert_eq!(
            scene.raw_pos,
            [if format == DataFormat::Fixed {
                [1., -2., 3.]
            } else {
                [1.5, -2.25, 3.75]
            }]
        );
        assert_eq!(scene.raw_st, [[32., -64.]]);
        assert_eq!(scene.cn, [u32::from_le_bytes([10, 20, 30, 255])]);
        assert_eq!(
            scene.mvp_table[scene.mtx_index[0] as usize],
            crate::hle::math::identity()
        );
    }
}

#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
#[test]
fn host_present_never_reads_guest() {
    let mut renderer = renderer();
    let mut commands = vec![
        [0xe300_0a01u64, 0x0030_0000],
        [0xff10_003f, 0x1234_5678_0000_1000],
        [0xed00_0000, 0x0010_0100],
        [0xfeed_beef_f700_0000, 0x1234_5678_f801_f801],
        [0xf60f_c0fc, 0],
        [0xdf00_0000, 0],
    ];
    // SAFETY: commands are stable through consumption; CIMG is only a GPU target identity.
    let summary = unsafe {
        renderer.process_dl_host(
            crate::HostRam::new(&[]),
            commands.as_ptr() as u64,
            crate::Microcode::F3dex2,
            &mut crate::NopSink,
        )
    };
    assert!(summary.renderable);
    commands.fill([0, 0]);
    drop(commands);
    let pixels = scanout(&mut renderer);
    assert!(pixels.as_chunks::<4>().0.contains(&[255, 0, 0, 255]));
    renderer.present_last().unwrap();
}

#[test]
fn compound_reads_leave_prior_rsp_state_intact() {
    use crate::hle::{
        gbi::f3dex2::F3DEX2_CONSTS,
        rdp::Rdp,
        rsp::{Rsp, Scene},
    };
    let mut rsp = Rsp::new(F3DEX2_CONSTS, GbiDataFormat::Fixed);
    let mut scene = Scene::default();
    let rdp = Rdp::default();
    let mut vertex = [0; 16];
    vertex[..2].copy_from_slice(&7i16.to_be_bytes());
    rsp.set_vertex(&RdramImage::new(&vertex), 0, 1, 0, &rdp, &mut scene)
        .unwrap();
    let before = scene.clone();
    vertex[..2].copy_from_slice(&9i16.to_be_bytes());
    assert!(rsp
        .set_vertex(&RdramImage::new(&vertex), 0, 2, 0, &rdp, &mut scene)
        .is_err());
    assert_eq!(scene, before);

    let before = (rsp.lights, rsp.ambient_col, rsp.light_version);
    assert!(rsp.set_light(&RdramImage::new(&[255, 64]), 0, 0).is_err());
    assert_eq!((rsp.lights, rsp.ambient_col, rsp.light_version), before);
    rsp.set_num_lights_direct(1);
    let before = (rsp.lights, rsp.ambient_col, rsp.light_version);
    assert!(rsp.set_light(&RdramImage::new(&[255; 10]), 0, 0).is_err());
    assert_eq!((rsp.lights, rsp.ambient_col, rsp.light_version), before);
    let before = (rsp.lookat_axes, rsp.lookat_version);
    assert!(rsp.set_lookat(&RdramImage::new(&[127; 10]), 0, 0).is_err());
    assert_eq!((rsp.lookat_axes, rsp.lookat_version), before);

    rsp.finish(&mut scene);
    let before = scene.clone();
    assert!(rsp.set_viewport(&RdramImage::new(&[0; 15]), 0).is_err());
    assert!(rsp.matrix(&RdramImage::new(&[0; 63]), 0, 3).is_err());
    assert!(rsp.force_matrix(&RdramImage::new(&[0; 63]), 0).is_err());
    rsp.finish(&mut scene);
    assert_eq!(scene, before);
}

#[test]
fn failed_texture_and_palette_loads_preserve_previous_bytes() {
    for load in [0xf300_0000, 0xf400_0000, 0xf000_0000] {
        let word = if load == 0xf000_0000 {
            0x0700_c000
        } else {
            0x0700_0000
        };
        let prefix = [(0xfd10_0000, 0x80), (load, word)];
        let mut bytes = image(&[prefix[0], prefix[1], (0xdf00_0000, 0)]);
        bytes.resize(0x80, 0);
        bytes.extend([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]);
        let prior = interpret(
            RdramImage::new(&bytes),
            0,
            GbiUcode::F3dex2,
            GbiDataFormat::Fixed,
        );
        assert!(prior.diags.is_empty(), "{load:x}: {:?}", prior.diags);
        let fault = image(&[(0xfd10_0000, 0x84), (load, word)]);
        bytes[16..16 + fault.len()].copy_from_slice(&fault);
        let failed = interpret(
            RdramImage::new(&bytes),
            0,
            GbiUcode::F3dex2,
            GbiDataFormat::Fixed,
        );
        assert_eq!(failed.summary(false).errors, 1);
        assert_eq!(failed.rdp.tmem, prior.rdp.tmem);
        assert_eq!(failed.rdp.tmem_bank, prior.rdp.tmem_bank);
    }
}

struct AddressMemory {
    base: u64,
    commands: Vec<crate::Command>,
    command_stride: u64,
    vertex_stride: u64,
}

impl crate::Rdram for AddressMemory {
    fn set_segment(&mut self, _: u32, _: u64) {}
    fn resolve(&self, address: u64) -> Result<u64, crate::MemoryError> {
        Ok(address)
    }
    fn resolve_masked(&self, address: u64) -> Result<u64, crate::MemoryError> {
        Ok(address)
    }
    fn read_command(&self, address: u64) -> Result<crate::Command, crate::MemoryError> {
        let index = address
            .checked_sub(self.base)
            .map(|offset| offset / self.command_stride)
            .and_then(|index| usize::try_from(index).ok());
        index
            .and_then(|index| self.commands.get(index))
            .copied()
            .ok_or(crate::MemoryError {
                address,
                length: self.command_stride,
                kind: crate::MemoryErrorKind::Unavailable,
            })
    }
    fn command_stride(&self) -> u64 {
        self.command_stride
    }
    fn in_bounds(&self, _: u64, _: u64) -> bool {
        true
    }
    fn read_u8(&self, address: u64) -> Result<u8, crate::MemoryError> {
        RdramImage::new(&[]).read_u8(address)
    }
    fn read_i8(&self, address: u64) -> Result<i8, crate::MemoryError> {
        RdramImage::new(&[]).read_i8(address)
    }
    fn read_i16(&self, address: u64) -> Result<i16, crate::MemoryError> {
        RdramImage::new(&[]).read_i16(address)
    }
    fn read_u16(&self, address: u64) -> Result<u16, crate::MemoryError> {
        RdramImage::new(&[]).read_u16(address)
    }
    fn read_bytes(
        &self,
        address: u64,
        length: usize,
    ) -> Result<std::borrow::Cow<'_, [u8]>, crate::MemoryError> {
        Err(crate::MemoryError {
            address,
            length: length as u64,
            kind: crate::MemoryErrorKind::Unavailable,
        })
    }
    fn read_matrix(
        &self,
        address: u64,
        _: GbiDataFormat,
    ) -> Result<crate::Matrix, crate::MemoryError> {
        Err(crate::MemoryError {
            address,
            length: 64,
            kind: crate::MemoryErrorKind::Unavailable,
        })
    }
    fn vertex_stride(&self, _: GbiDataFormat) -> Result<u64, crate::MemoryError> {
        Ok(self.vertex_stride)
    }
}

fn address_memory(commands: &[(u32, u64)]) -> AddressMemory {
    AddressMemory {
        base: 0,
        command_stride: 8,
        vertex_stride: 16,
        commands: commands
            .iter()
            .map(|&(w0, w1_addr)| crate::Command {
                w0,
                w1: w1_addr as u32,
                w1_addr,
            })
            .collect(),
    }
}

#[test]
fn walk_checks_pc_vertex_and_texture_arithmetic() {
    use crate::{MemoryAccess as A, MemoryErrorKind as K};
    let mut pc = address_memory(&[(0, 0)]);
    pc.base = 8;
    pc.command_stride = u64::MAX;
    let mut vertices = address_memory(&[(0x0100_2004, 16)]);
    vertices.vertex_stride = u64::MAX;
    let tile = address_memory(&[(0xfd10_0000, u64::MAX - 4), (0xf401_0000, 0)]);
    for (memory, access, at, address, length) in [
        (pc, A::Command, 8, 8, u64::MAX),
        (vertices, A::Vertex, 0, 16, u64::MAX),
        (tile, A::Texture, 8, u64::MAX - 4, 8),
    ] {
        let entry = memory.base;
        let result = interpret(memory, entry, GbiUcode::F3dex2, GbiDataFormat::Fixed);
        assert_eq!(
            result.diags,
            [crate::Diagnostic {
                at,
                kind: crate::DiagKind::MemoryRead {
                    access,
                    error: crate::MemoryError {
                        address,
                        length,
                        kind: K::AddressOverflow
                    },
                }
            }]
        );
        assert!(result.scene.indices.is_empty());
        assert!(result.scene.framebuffer_pairs.is_empty());
    }
}

#[test]
fn advisory_bounds_do_not_suppress_read_failures() {
    let memory = address_memory(&[(0, 0)]);
    assert!(crate::Rdram::in_bounds(&memory, 8, 8));
    let result = interpret(memory, 0, GbiUcode::F3dex2, GbiDataFormat::Fixed);
    assert_eq!(result.commands, 2);
    assert_eq!(
        result.diags,
        [crate::Diagnostic {
            at: 8,
            kind: crate::DiagKind::MemoryRead {
                access: crate::MemoryAccess::Command,
                error: crate::MemoryError {
                    address: 8,
                    length: 8,
                    kind: crate::MemoryErrorKind::Unavailable
                },
            }
        }]
    );
}
