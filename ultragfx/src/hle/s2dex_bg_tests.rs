use super::{interp, mem::RdramImage};
use crate::inspect::{Emission, WalkObserver, WalkStep, WalkTermination};
use crate::{DataFormat, Microcode, Rdram};
use std::{borrow::Cow, cell::RefCell, ops::ControlFlow};

const OBJECT: usize = 0x200;
const PALETTE: usize = 0x400;
const SOURCE: usize = 0x1000;

fn half(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
}

pub(super) fn fixture(width: u16, height: u16) -> (Vec<u8>, u64) {
    use n64_gbi::encode::{gdp_set_combine_lerp, CcPass};
    let color = CcPass {
        a: 15,
        b: 15,
        c: 31,
        d: 1,
    };
    let alpha = CcPass {
        a: 7,
        b: 7,
        c: 7,
        d: 6,
    };
    let commands = [
        (0xff100000 | (width - 1) as u32, 0x800000),
        (0xed000000, ((width as u32 * 4) << 12) | (height as u32 * 4)),
        (0xef008cf0, 0),
        gdp_set_combine_lerp(color, alpha, color, alpha),
        (0xfd100000, PALETTE as u32),
        (0xf5000100, 0x07000000),
        (0xf0000000, 0x073fc000),
        (0x0b000000, 12),
        (0x09000000, OBJECT as u32),
        (0xdf000000, 0),
    ];
    let mut bytes = vec![0; SOURCE + width as usize * height as usize + 1024];
    for (i, (a, b)) in commands.into_iter().enumerate() {
        bytes[i * 8..i * 8 + 4].copy_from_slice(&a.to_be_bytes());
        bytes[i * 8 + 4..i * 8 + 8].copy_from_slice(&b.to_be_bytes());
    }
    for (offset, value) in [
        (2, width * 4),
        (6, width * 4),
        (10, height * 4),
        (14, height * 4),
        (20, 0xfff4),
        (28, 1024),
        (30, 1024),
    ] {
        half(&mut bytes, OBJECT + offset, value);
    }
    bytes[OBJECT + 16..OBJECT + 20].copy_from_slice(&(SOURCE as u32).to_be_bytes());
    bytes[OBJECT + 22..OBJECT + 24].copy_from_slice(&[2, 1]);
    for index in 0..256u16 {
        half(
            &mut bytes,
            PALETTE + index as usize * 2,
            ((index & 31) << 11) | (((index >> 3) & 31) << 6) | (((index * 3) & 31) << 1),
        );
    }
    for y in 0..height as usize {
        for x in 0..width as usize {
            bytes[SOURCE + y * width as usize + x] = ((x * 7 + y * 13) & 255) as u8;
        }
    }
    (bytes, 64)
}

struct Traced<'a> {
    memory: RdramImage<'a>,
    reads: &'a RefCell<Vec<(u64, usize)>>,
}
impl Rdram for Traced<'_> {
    fn set_segment(&mut self, s: u32, v: u64) {
        self.memory.set_segment(s, v);
    }
    fn resolve(&self, a: u64) -> Result<u64, crate::MemoryError> {
        self.memory.resolve(a)
    }
    fn resolve_masked(&self, a: u64) -> Result<u64, crate::MemoryError> {
        self.memory.resolve_masked(a)
    }
    fn read_command(&self, a: u64) -> Result<crate::Command, crate::MemoryError> {
        self.reads.borrow_mut().push((a, 0));
        self.memory.read_command(a)
    }
    fn command_stride(&self) -> u64 {
        8
    }
    fn in_bounds(&self, a: u64, n: u64) -> bool {
        self.memory.in_bounds(a, n)
    }
    fn read_u8(&self, a: u64) -> Result<u8, crate::MemoryError> {
        self.memory.read_u8(a)
    }
    fn read_i8(&self, a: u64) -> Result<i8, crate::MemoryError> {
        self.memory.read_i8(a)
    }
    fn read_u16(&self, a: u64) -> Result<u16, crate::MemoryError> {
        self.memory.read_u16(a)
    }
    fn read_i16(&self, a: u64) -> Result<i16, crate::MemoryError> {
        self.memory.read_i16(a)
    }
    fn read_bytes(&self, a: u64, n: usize) -> Result<Cow<'_, [u8]>, crate::MemoryError> {
        self.reads.borrow_mut().push((a, n));
        self.memory.read_bytes(a, n)
    }
    fn read_matrix(&self, a: u64, f: DataFormat) -> Result<crate::Matrix, crate::MemoryError> {
        self.memory.read_matrix(a, f)
    }
    fn read_obj_bg(&self, a: u64, k: crate::ObjBgKind) -> Result<crate::ObjBg, crate::MemoryError> {
        self.reads
            .borrow_mut()
            .push((a, if k == crate::ObjBgKind::Copy { 40 } else { 36 }));
        self.memory.read_obj_bg(a, k)
    }
}

fn run(bytes: &[u8]) -> interp::InterpResult {
    let result = interp::interpret(
        RdramImage::new(bytes),
        0,
        Microcode::S2dex2.into(),
        DataFormat::Fixed,
        None,
    );
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert_eq!(result.termination, WalkTermination::End);
    result
}

fn rectangles(result: &interp::InterpResult) -> Vec<(crate::scene::TexRectBounds, i16, i16)> {
    result
        .scene
        .framebuffer_pairs
        .iter()
        .flat_map(|pair| &pair.ops)
        .filter_map(|op| {
            if let crate::scene::SceneOp::TexRect {
                rect, dsdx, dtdy, ..
            } = op
            {
                Some((*rect, *dsdx, *dtdy))
            } else {
                None
            }
        })
        .collect()
}

fn pixels(result: &interp::InterpResult, w: u32, h: u32) -> Vec<u8> {
    let (device, queue, dual) = crate::render::headless_device();
    let mut renderer =
        crate::render::SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, w, h, dual);
    crate::tests::common::render_to_pixels(&device, &queue, &mut renderer, &result.scene, w, h)
}

pub(super) fn palette_pixel(bytes: &[u8], x: usize, y: usize, width: usize) -> [u8; 4] {
    let index = bytes[SOURCE + y * width + x] as usize;
    let word = u16::from_be_bytes(
        bytes[PALETTE + index * 2..PALETTE + index * 2 + 2]
            .try_into()
            .unwrap(),
    );
    let expand = |v: u16| ((v << 3) | (v >> 2)) as u8;
    [
        expand((word >> 11) & 31),
        expand((word >> 6) & 31),
        expand((word >> 1) & 31),
        255,
    ]
}

#[test]
fn bg_1cyc_oot_ci8_point_with_obj_bilerp() {
    let (bytes, _) = fixture(320, 240);
    let reads = RefCell::default();
    let result = interp::interpret(
        Traced {
            memory: RdramImage::new(&bytes),
            reads: &reads,
        },
        0,
        Microcode::S2dex2.into(),
        DataFormat::Fixed,
        None,
    );
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    let rects = rectangles(&result);
    assert_eq!(rects.len(), 48);
    assert_eq!(result.rdp.tiles[0].line, 41);
    assert_eq!(result.rdp.tex_image.2, 159);
    for (i, (rect, ds, dt)) in rects.iter().enumerate() {
        assert_eq!(
            (rect.ulx, rect.lrx, rect.uly, rect.lry),
            (0, 1276, i as i32 * 20, ((i + 1) * 5).min(239) as i32 * 4)
        );
        assert_eq!((*ds, *dt), (1024, 1024));
    }
    assert_eq!(
        reads
            .borrow()
            .iter()
            .filter(|&&(a, n)| a >= SOURCE as u64 && n == 1928)
            .count(),
        47
    );
    let output = pixels(&result, 320, 240);
    // rt64's (frameMax - 1) & ~3 gives 319x239 coverage at unity scale.
    for y in 0..239 {
        for x in 0..319 {
            assert_eq!(
                &output[(y * 320 + x) * 4..(y * 320 + x + 1) * 4],
                &palette_pixel(&bytes, x, y, 320),
                "{x},{y}"
            );
        }
    }
    let bank = result.rdp.tmem_bank.profile_bank().bytes;
    for i in 0..256 {
        for lane in 0..4 {
            assert_eq!(
                &bank[2048 + i * 8 + lane * 2..2050 + i * 8 + lane * 2],
                &bytes[PALETTE + i * 2..PALETTE + i * 2 + 2]
            );
        }
    }
}

#[test]
fn bg_1cyc_wrap_split_state_matches_reference() {
    let (bytes, _) = fixture(320, 240);
    let reads = RefCell::default();
    let result = interp::interpret(
        Traced {
            memory: RdramImage::new(&bytes),
            reads: &reads,
        },
        0,
        Microcode::S2dex2.into(),
        DataFormat::Fixed,
        None,
    );
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    let loads: Vec<_> = reads
        .borrow()
        .iter()
        .copied()
        .filter(|&(a, n)| a >= SOURCE as u64 - 320 && n > 0)
        .collect();
    assert_eq!(
        &loads[47..],
        &[
            (SOURCE as u64 - 320, 648),
            (SOURCE as u64, 8),
            (SOURCE as u64 + 239 * 320, 320),
            (SOURCE as u64 + 235 * 320, 1288)
        ]
    );
    let bank = result.rdp.tmem_bank.profile_bank().bytes;
    let mut expected = vec![0; 4096];
    for row in 0..6 {
        for col in 0..328 {
            let source_row = if row == 5 { 0 } else { 235 + row };
            let offset = (source_row * 320 + col) % (320 * 240);
            expected[row * 328 + (col ^ if row & 1 != 0 { 4 } else { 0 })] = bytes[SOURCE + offset];
        }
    }
    for i in 0..256 {
        for lane in 0..4 {
            expected[2048 + i * 8 + lane * 2..2050 + i * 8 + lane * 2]
                .copy_from_slice(&bytes[PALETTE + i * 2..PALETTE + i * 2 + 2]);
        }
    }
    assert_eq!(bank, expected);
    assert_eq!(
        (
            result.rdp.tiles[0].cms,
            result.rdp.tiles[0].cmt,
            result.rdp.tiles[0].masks,
            result.rdp.tiles[0].maskt
        ),
        (1, 1, 15, 15)
    );
    assert_eq!(
        (
            result.rdp.tiles[0].uls,
            result.rdp.tiles[0].ult,
            result.rdp.tiles[0].lrs,
            result.rdp.tiles[0].lrt
        ),
        (0, 0, 0, 0)
    );
    assert_eq!(
        (
            result.rdp.tiles[7].line,
            result.rdp.tiles[7].tmem_addr,
            result.rdp.tiles[7].lrs,
            result.rdp.tiles[7].lrt
        ),
        (41, 0, 640, 15)
    );
    assert_eq!(
        result.rdp.tiles[0],
        super::rdp::TileDescriptor {
            fmt: 2,
            siz: 1,
            line: 41,
            cms: 1,
            cmt: 1,
            masks: 15,
            maskt: 15,
            width: 1,
            height: 1,
            ..Default::default()
        }
    );
    assert_eq!(
        result.rdp.tiles[7],
        super::rdp::TileDescriptor {
            siz: 2,
            line: 41,
            lrs: 640,
            lrt: 15,
            width: 161,
            height: 4,
            ..Default::default()
        }
    );
    assert_eq!(result.rdp.tex_image, (0, 2, 159, SOURCE as u64 + 235 * 320));
}

#[test]
fn bg_1cyc_palette_alpha_does_not_replace_combiner_alpha() {
    let (bytes, _) = fixture(64, 32);
    let result = run(&bytes);
    let output = pixels(&result, 64, 32);
    assert_eq!(
        &output[(13 * 64 + 17) * 4..(13 * 64 + 18) * 4],
        &palette_pixel(&bytes, 17, 13, 64)
    );
}

#[test]
fn bg_1cyc_origin_clip_scale_and_flip() {
    let (mut bytes, _) = fixture(64, 32);
    for (offset, value) in [
        (0, 64 * 32 + 32),
        (4, (-8i16) as u16),
        (8, 31 * 32),
        (12, (-4i16) as u16),
        (14, 24 * 4),
        (26, 1),
        (28, 512),
        (30, 512),
    ] {
        half(&mut bytes, OBJECT + offset, value);
    }
    bytes[OBJECT + 32..OBJECT + 36].copy_from_slice(&(-32i32).to_be_bytes());
    let result = run(&bytes);
    let output = pixels(&result, 64, 32);
    for y in 0..23 {
        for x in 0..62 {
            assert_eq!(
                &output[(y * 64 + x) * 4..(y * 64 + x + 1) * 4],
                &palette_pixel(&bytes, 31 - x / 2, y.div_ceil(2), 64),
                "{x},{y}"
            );
        }
    }
}

#[derive(Default)]
struct BackgroundObserver {
    bg_pc: u64,
    stop: bool,
    calls: usize,
    emissions: Vec<(crate::scene::TexRectBounds, u32, u32)>,
    final_line: u16,
}
impl WalkObserver for BackgroundObserver {
    fn command(&mut self, step: WalkStep<'_>) -> ControlFlow<()> {
        if step.pc == self.bg_pc {
            self.calls += 1;
            assert_eq!(step.words.len(), 1);
            for emission in step.emissions {
                if let Emission::TexRect {
                    rect,
                    material_index,
                    render_mode_index,
                    ..
                } = emission
                {
                    self.emissions
                        .push((*rect, *material_index, *render_mode_index));
                } else {
                    panic!("background emitted non-rectangle");
                }
            }
            self.final_line = step.state.tiles[7].line;
            if self.stop {
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    }
}

#[test]
fn bg_dispatch_is_atomic_for_observer_and_prefix() {
    let (bytes, bg_pc) = fixture(320, 240);
    let reads = RefCell::default();
    let mut observer = BackgroundObserver {
        bg_pc,
        stop: true,
        ..Default::default()
    };
    let result = interp::interpret(
        Traced {
            memory: RdramImage::new(&bytes),
            reads: &reads,
        },
        0,
        Microcode::S2dex2.into(),
        DataFormat::Fixed,
        Some(&mut observer),
    );
    assert_eq!(result.termination, WalkTermination::ObserverStopped);
    assert_eq!(result.commands, 9);
    assert_eq!(observer.calls, 1);
    assert_eq!(observer.emissions.len(), 48);
    assert_eq!(observer.final_line, 41);
    assert!(!reads.borrow().contains(&(bg_pc + 8, 0)));
    assert!(observer
        .emissions
        .windows(2)
        .all(|pair| pair[0].0.lry == pair[1].0.uly));
    struct Hardware<'a> {
        bytes: &'a [u8],
        reads: &'a RefCell<Vec<(u64, usize)>>,
    }
    impl crate::Hardware for Hardware<'_> {
        fn rdram(&self) -> impl Rdram + '_ {
            Traced {
                memory: RdramImage::new(self.bytes),
                reads: self.reads,
            }
        }
    }
    reads.borrow_mut().clear();
    let (device, queue, _) = crate::render::headless_device();
    let mut renderer = crate::Renderer::with_device(
        device,
        queue,
        crate::PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 320,
            height: 240,
        },
        crate::RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            clear_policy: crate::ClearPolicy::PerFrame,
            power_preference: wgpu::PowerPreference::LowPower,
        },
    );
    let before = renderer.rdp.clone();
    let mut diags = Vec::new();
    let summary = renderer.process_dl_prefix(
        &Hardware {
            bytes: &bytes,
            reads: &reads,
        },
        0,
        Microcode::S2dex2,
        &mut diags,
        9,
    );
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(summary.termination, WalkTermination::Cap);
    assert_eq!(summary.commands, 9);
    assert!(summary.renderable);
    assert_eq!(renderer.rdp, before);
    assert!(!reads.borrow().contains(&(bg_pc + 8, 0)));
    assert_eq!(renderer.frame_scenes[0].draw_origins.len(), 48);
}

#[test]
fn bg_observation_adds_no_reads() {
    let (bytes, bg_pc) = fixture(320, 240);
    let reads = [RefCell::default(), RefCell::default()];
    let mut observer = BackgroundObserver {
        bg_pc,
        ..Default::default()
    };
    let ordinary = interp::interpret(
        Traced {
            memory: RdramImage::new(&bytes),
            reads: &reads[0],
        },
        0,
        Microcode::S2dex2.into(),
        DataFormat::Fixed,
        None,
    );
    let observed = interp::interpret(
        Traced {
            memory: RdramImage::new(&bytes),
            reads: &reads[1],
        },
        0,
        Microcode::S2dex2.into(),
        DataFormat::Fixed,
        Some(&mut observer),
    );
    assert_eq!(ordinary, observed);
    assert_eq!(reads[0], reads[1]);
    assert_eq!(observer.emissions.len(), 48);
}

#[test]
fn bg_object_and_texture_failures_stop_decomposition() {
    use crate::tests::memory_failures::StrictMemory;
    let (bytes, _) = fixture(320, 240);
    for (end, access, address, length) in [
        (OBJECT + 19, crate::MemoryAccess::ObjBg, OBJECT + 16, 4),
        (OBJECT + 35, crate::MemoryAccess::ObjBg, OBJECT + 32, 4),
        (
            SOURCE + 2000,
            crate::MemoryAccess::Texture,
            SOURCE + 1600,
            1928,
        ),
    ] {
        let result = interp::interpret(
            StrictMemory::new(&bytes[..end]),
            if end < SOURCE { 64 } else { 0 },
            Microcode::S2dex2.into(),
            DataFormat::Fixed,
            None,
        );
        assert_eq!(result.termination, WalkTermination::Bounds);
        assert!(result.diags.iter().any(|d| matches!(d.kind,crate::DiagKind::MemoryRead{access:a,error} if a==access && error.address==address as u64 && error.length==length)));
        assert_eq!(result.scene, crate::scene::Scene::default());
        assert!(!result.commits_rdp());
    }
}

#[test]
fn bg_invalid_dimensions_and_capacity_reject_without_loading() {
    for (offset, value) in [(2, 0), (10, 0), (6, 0), (14, 0), (2, 65532)] {
        let (mut bytes, _) = fixture(320, 240);
        half(&mut bytes, OBJECT + offset, value);
        if value != 0 {
            half(&mut bytes, OBJECT + 6, value);
        }
        let reads = RefCell::default();
        let result = interp::interpret(
            Traced {
                memory: RdramImage::new(&bytes),
                reads: &reads,
            },
            0,
            Microcode::S2dex2.into(),
            DataFormat::Fixed,
            None,
        );
        assert_eq!(result.termination, WalkTermination::Rejected);
        assert!(result.diags.iter().any(|d| matches!(
            d.kind,
            crate::DiagKind::UnsupportedCommandParameters { opcode: 9 }
        )));
        assert!(!reads
            .borrow()
            .iter()
            .any(|&(address, length)| address >= SOURCE as u64 && length > 0));
    }
}

fn copy_fixture(width: u16, height: u16) -> Vec<u8> {
    let (mut bytes, bg_pc) = fixture(width, height);
    bytes.resize(SOURCE + width as usize * height as usize * 2 + 1024, 0);
    bytes[16..20].copy_from_slice(&0xef200cf0u32.to_be_bytes());
    bytes[bg_pc as usize..bg_pc as usize + 4].copy_from_slice(&0x0a000000u32.to_be_bytes());
    bytes[OBJECT + 22..OBJECT + 24].copy_from_slice(&[0, 2]);
    let tmem_w = width / 4 + 3;
    let tmem_h = (512 / tmem_w) * 4;
    let product = u32::from(tmem_h) * u32::from(width / 2);
    for (offset, value) in [
        (28, tmem_w),
        (30, tmem_h),
        (32, product as u16),
        (34, tmem_h - 1),
        (36, width / 2),
        (38, (product >> 16) as u16),
    ] {
        half(&mut bytes, OBJECT + offset, value);
    }
    for y in 0..height as usize {
        for x in 0..width as usize {
            let word = (((x * 3 + y) & 31) << 11)
                | (((x + y * 5) & 31) << 6)
                | (((x * 7 + y * 11) & 31) << 1)
                | 1;
            half(
                &mut bytes,
                SOURCE + (y * width as usize + x) * 2,
                word as u16,
            );
        }
    }
    bytes
}

fn copy_pixel(bytes: &[u8], x: usize, y: usize, width: usize) -> [u8; 4] {
    let offset = SOURCE + (y * width + x) * 2;
    let value = u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap());
    let expand = |v: u16| ((v << 3) | (v >> 2)) as u8;
    [
        expand((value >> 11) & 31),
        expand((value >> 6) & 31),
        expand((value >> 1) & 31),
        255,
    ]
}

#[test]
fn bg_copy_oot_rgba16_320x240() {
    let bytes = copy_fixture(320, 240);
    let result = run(&bytes);
    let rects = rectangles(&result);
    assert_eq!(rects.len(), 40);
    for (i, (rect, ds, dt)) in rects.iter().enumerate() {
        assert_eq!((*ds, *dt), (0x1000, 0x0400));
        assert_eq!(
            (rect.ulx, rect.lrx, rect.uly, rect.lry),
            (0, 1276, (i * 24) as i32, (i * 24 + 20) as i32)
        );
    }
    let output = pixels(&result, 320, 240);
    for y in 0..240 {
        for x in 0..320 {
            assert_eq!(
                &output[(y * 320 + x) * 4..(y * 320 + x + 1) * 4],
                &copy_pixel(&bytes, x, y, 320),
                "{x},{y}"
            );
        }
    }
}

#[test]
fn bg_copy_partial_last_strip() {
    let mut bytes = copy_fixture(320, 241);
    bytes[12..16].copy_from_slice(&((1280u32 << 12) | 968).to_be_bytes());
    let result = run(&bytes);
    let rects = rectangles(&result);
    assert_eq!(rects.len(), 41);
    let last = rects.last().unwrap().0;
    assert_eq!((last.uly, last.lry), (960, 960));
    assert_eq!(
        rects
            .iter()
            .filter(|(r, _, _)| r.uly <= 960 && r.lry >= 960)
            .count(),
        1
    );
    let output = pixels(&result, 320, 242);
    for x in 0..320 {
        assert_eq!(
            &output[(240 * 320 + x) * 4..(240 * 320 + x + 1) * 4],
            &copy_pixel(&bytes, x, 240, 320)
        );
    }
    let mut blank = result.clone();
    blank.scene.framebuffer_pairs[0]
        .ops
        .retain(|op| !matches!(op, crate::scene::SceneOp::TexRect { .. }));
    let clear = pixels(&blank, 320, 242);
    assert_eq!(&output[241 * 320 * 4..], &clear[241 * 320 * 4..]);
}

#[test]
fn bg_copy_smaller_frame_and_quake_clip() {
    let mut bytes = copy_fixture(64, 32);
    for (offset, value) in [
        (4, (-12i16) as u16),
        (6, 40 * 4),
        (12, (-8i16) as u16),
        (14, 20 * 4),
    ] {
        half(&mut bytes, OBJECT + offset, value);
    }
    bytes.copy_within(64..80, 104);
    for (i, (w0, w1)) in [
        (0xef300cf0u32, 0u32),
        (0xf7000000, 0x08430843),
        (0xf60fc07c, 0),
        (0xed004004, (128 << 12) | 64),
        (0xef200cf0, 0),
    ]
    .into_iter()
    .enumerate()
    {
        bytes[64 + i * 8..68 + i * 8].copy_from_slice(&w0.to_be_bytes());
        bytes[68 + i * 8..72 + i * 8].copy_from_slice(&w1.to_be_bytes());
    }
    let result = run(&bytes);
    let output = pixels(&result, 64, 32);
    let mut blank = result.clone();
    blank.scene.framebuffer_pairs[0]
        .ops
        .retain(|op| !matches!(op, crate::scene::SceneOp::TexRect { .. }));
    let clear = pixels(&blank, 64, 32);
    for y in 0..32 {
        for x in 0..64 {
            let offset = (y * 64 + x) * 4;
            if (1..32).contains(&x) && (1..16).contains(&y) {
                assert_eq!(
                    &output[offset..offset + 4],
                    &copy_pixel(&bytes, x + 3, y + 2, 64),
                    "{x},{y}"
                );
            } else {
                assert_eq!(
                    &output[offset..offset + 4],
                    &clear[offset..offset + 4],
                    "{x},{y}"
                );
            }
        }
    }
}

#[test]
fn bg_copy_scroll_and_horizontal_flip() {
    let mut bytes = copy_fixture(64, 32);
    for flip in [0, 1] {
        for (offset, value) in [(0, 60 * 32), (8, 30 * 32), (26, flip)] {
            half(&mut bytes, OBJECT + offset, value);
        }
        let result = run(&bytes);
        let output = pixels(&result, 64, 32);
        for y in 0..32 {
            for x in 0..64 {
                let source_x = if flip == 0 {
                    (60 + x) % 64
                } else {
                    (60 + 63 - x) % 64
                };
                assert_eq!(
                    &output[(y * 64 + x) * 4..(y * 64 + x + 1) * 4],
                    &copy_pixel(&bytes, source_x, (30 + y) % 32, 64),
                    "flip={flip} {x},{y}"
                );
            }
        }
    }
}

#[test]
#[ignore = "needs an RSP reference fixture"]
fn bg_copy_terminal_state_matches_rsp_reference() {
    panic!("COPY residual-strip TMEM and tile state requires an RSP-produced fixture");
}

#[test]
fn bg_1cyc_loadblock_selector_uses_tile_transfer() {
    let (mut bytes, _) = fixture(64, 32);
    let tile = run(&bytes);
    half(&mut bytes, OBJECT + 20, 0x0033);
    assert_eq!(run(&bytes), tile);
}

#[test]
fn bg_image_k0_segment_reads_masked_physical_bytes() {
    for mut bytes in [fixture(8, 2).0, copy_fixture(8, 2)] {
        let expected = run(&bytes);
        bytes[OBJECT + 16..OBJECT + 20]
            .copy_from_slice(&(0x0100_0000 | SOURCE as u32).to_be_bytes());
        let mut memory = RdramImage::new(&bytes);
        memory.set_segment(1, 0x8000_0000);
        let result =
            interp::interpret(memory, 0, Microcode::S2dex2.into(), DataFormat::Fixed, None);
        assert_eq!(result.termination, WalkTermination::End);
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        assert_eq!(result.rdp.tmem_bank, expected.rdp.tmem_bank);
        assert_eq!(result.scene.materials.len(), expected.scene.materials.len());
        for (actual, expected) in result.scene.materials.iter().zip(&expected.scene.materials) {
            assert_eq!(actual.texture.decode(), expected.texture.decode());
        }
    }
}

#[test]
fn bg_copy_missing_final_field_reports_exact_span() {
    use crate::tests::memory_failures::StrictMemory;
    let bytes = copy_fixture(64, 32);
    let result = interp::interpret(
        StrictMemory::new(&bytes[..OBJECT + 39]),
        64,
        Microcode::S2dex2.into(),
        DataFormat::Fixed,
        None,
    );
    assert_eq!(result.termination, WalkTermination::Bounds);
    assert_eq!(result.commands, 1);
    assert!(
        matches!(result.diags[0].kind,crate::DiagKind::MemoryRead{access:crate::MemoryAccess::ObjBg,error}
        if error.address==(OBJECT+38) as u64 && error.length==2)
    );
    assert_eq!(result.scene, crate::scene::Scene::default());
}

#[test]
fn bg_copy_quake_clip_reads_only_the_aligned_source() {
    let mut bytes = copy_fixture(64, 32);
    bytes.truncate(SOURCE + 64 * 32 * 2);
    half(&mut bytes, OBJECT + 4, (-4i16) as u16);
    let result = run(&bytes);
    let output = pixels(&result, 64, 32);
    for y in 0..32 {
        for x in 0..63 {
            assert_eq!(
                &output[(y * 64 + x) * 4..(y * 64 + x + 1) * 4],
                &copy_pixel(&bytes, x + 1, y, 64)
            );
        }
    }
}
