use super::{fill_hw, headless_renderer, store_pixels, ImgHw};
use crate::inspect::WalkTermination;
use crate::{
    ClearPolicy, DataFormat, Diagnostic, DlSummary, Hardware, Microcode, NopSink, Rdram,
    RdramImage, Renderer,
};
use n64_gbi::{consts::*, encode::*};
use std::borrow::Cow;
use std::cell::RefCell;

const A: u32 = 0x100000;
const B: u32 = 0x200000;
const Z: u32 = 0x300000;

fn hw(words: impl IntoIterator<Item = (u32, u32)>) -> ImgHw {
    ImgHw {
        rdram: words
            .into_iter()
            .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
            .collect(),
    }
}

struct NoReads;

impl NoReads {
    fn bytes(&self) -> &[u8] {
        panic!("zero must not obtain a memory reader")
    }
}

impl Hardware for NoReads {
    fn rdram(&self) -> impl Rdram + '_ {
        RdramImage::new(self.bytes())
    }
}

struct ReadHw {
    image: ImgHw,
    commands: RefCell<Vec<u64>>,
    bounds: RefCell<Vec<u64>>,
}

impl Hardware for ReadHw {
    fn rdram(&self) -> impl Rdram + '_ {
        ReadMem {
            hw: self,
            inner: RdramImage::new(&self.image.rdram),
        }
    }
}

struct ReadMem<'a> {
    hw: &'a ReadHw,
    inner: RdramImage<'a>,
}

impl Rdram for ReadMem<'_> {
    fn set_segment(&mut self, seg: u32, value: u64) {
        Rdram::set_segment(&mut self.inner, seg, value);
    }
    fn resolve(&self, a: u64) -> u64 {
        self.inner.resolve(a)
    }
    fn resolve_masked(&self, a: u64) -> u64 {
        self.inner.resolve_masked(a)
    }
    fn command_stride(&self) -> u64 {
        self.inner.command_stride()
    }
    fn in_bounds(&self, a: u64, n: u64) -> bool {
        self.hw.bounds.borrow_mut().push(a);
        self.inner.in_bounds(a, n)
    }
    fn read_command(&self, a: u64) -> crate::hle::mem::Command {
        self.hw.commands.borrow_mut().push(a);
        self.inner.read_command(a)
    }
    fn read_u8(&self, _: u64) -> u8 {
        panic!("unexpected data read")
    }
    fn read_i8(&self, _: u64) -> i8 {
        panic!("unexpected data read")
    }
    fn read_i16(&self, _: u64) -> i16 {
        panic!("unexpected data read")
    }
    fn read_u16(&self, _: u64) -> u16 {
        panic!("unexpected data read")
    }
    fn read_bytes(&self, _: u64, _: usize) -> Cow<'_, [u8]> {
        panic!("unexpected data read")
    }
    fn read_matrix(&self, _: u64, _: DataFormat) -> crate::inspect::Matrix4 {
        panic!("unexpected data read")
    }
    fn is_rdram_image(&self) -> bool {
        true
    }
}

#[test]
fn zero_preserves_pixels_scenes_scanout_backend_format_and_frame_serial_without_reads() {
    let mut r = headless_renderer();
    r.begin_frame();
    r.process_dl(&fill_hw(A, 0xf801f801), 0, Microcode::F3dex2, &mut NopSink);
    r.last_backend_was_image = false;
    r.set_data_format(DataFormat::Float);
    r.inner.dither_seed = 37;
    let scenes = r.frame_scenes.clone();
    let before = store_pixels(&r, A.into());
    let serial = r.inner.frame_serial;
    let mut diags = vec![Diagnostic {
        at: 99,
        kind: crate::DiagKind::DlPastRdram,
    }];
    let saved_diags = diags.clone();
    for ucode in [Microcode::F3dex2, Microcode::F3d] {
        assert_eq!(
            r.process_dl_prefix(&NoReads, u64::MAX, ucode, &mut diags, 0),
            DlSummary {
                termination: WalkTermination::Cap,
                ..DlSummary::default()
            }
        );
        assert_eq!(r.frame_scenes, scenes);
        assert_eq!(r.last_scanout_addr, Some(A.into()));
        assert!(!r.last_backend_was_image);
        assert_eq!(r.data_format, DataFormat::Float);
        assert_eq!(r.inner.frame_serial, serial);
        assert_eq!(r.inner.dither_seed, 37);
        assert_eq!(diags, saved_diags);
        assert_eq!(store_pixels(&r, A.into()), before);
    }
    r.set_data_format(DataFormat::Fixed);
    r.process_dl_prefix(
        &hw([
            gdp_set_color_image(0, 2, 64, A),
            gdp_set_scissor(0, 0, 0, 256, 256),
            gdp_set_fill_color(0x07c107c1),
            gdp_fill_rectangle(0, 0, 124, 252),
            gsp_enddl(),
        ]),
        0,
        Microcode::F3dex2,
        &mut NopSink,
        4,
    );
    let after = store_pixels(&r, A.into());
    assert_eq!(
        &after[(32 * 64 + 48) * 4..][..4],
        &before[(32 * 64 + 48) * 4..][..4]
    );
    assert_ne!(after, before);
}

#[test]
fn dispatch_counts_include_repeated_pcs_calls_and_atomic_rectangle_continuations() {
    let mut r = headless_renderer();
    for (ucode, call, end) in [
        (Microcode::F3dex2, 0xde000000, gsp_enddl()),
        (Microcode::F3d, 0x06000000, gsp_enddl_f3d()),
    ] {
        let image = hw([(call, 24), (call, 24), end, gdp_set_fill_color(7), end]);
        let memory = ReadHw {
            image,
            commands: RefCell::new(vec![]),
            bounds: RefCell::new(vec![]),
        };
        let s = r.process_dl_prefix(&memory, 0, ucode, &mut NopSink, 5);
        assert_eq!((s.commands, s.termination), (5, WalkTermination::Cap));
        assert_eq!(*memory.commands.borrow(), [0, 24, 32, 8, 24]);
        assert_eq!(*memory.bounds.borrow(), [0, 24, 32, 8, 24]);
        memory.commands.borrow_mut().clear();
        memory.bounds.borrow_mut().clear();
        let s = r.process_dl_prefix(&memory, 0, ucode, &mut NopSink, 1);
        assert_eq!((s.commands, s.termination), (1, WalkTermination::Cap));
        assert_eq!(*memory.commands.borrow(), [0]);
        assert_eq!(*memory.bounds.borrow(), [0]);
        let mut rect = gsp_texture_rectangle(0, 0, 128, 128, 0, 0, 0, 1024, 1024, false);
        if ucode == Microcode::F3d {
            rect[1].0 = 0xb4000000;
            rect[2].0 = 0xb3000000;
        }
        let memory = ReadHw {
            image: hw(rect.into_iter().chain([gdp_set_fill_color(1), end])),
            commands: RefCell::new(vec![]),
            bounds: RefCell::new(vec![]),
        };
        let mut diags = vec![];
        let s = r.process_dl_prefix(&memory, 0, ucode, &mut diags, 1);
        assert_eq!(
            (s.commands, s.termination, s.dropped_runs),
            (1, WalkTermination::Cap, 1)
        );
        assert_eq!(*memory.commands.borrow(), [0, 8, 16]);
        assert_eq!(*memory.bounds.borrow(), [0, 8, 16]);
        assert_eq!(
            diags,
            [Diagnostic {
                at: 0,
                kind: crate::DiagKind::DrawBeforeCimg
            }]
        );
    }
    r.set_data_format(DataFormat::Float);
    let memory = ReadHw {
        image: hw([(0xf6000000, 0), (0xe1000080, 128), gsp_enddl()]),
        commands: RefCell::new(vec![]),
        bounds: RefCell::new(vec![]),
    };
    let s = r.process_dl_prefix(&memory, 0, Microcode::F3dex2, &mut NopSink, 1);
    assert_eq!((s.commands, s.termination), (1, WalkTermination::Cap));
    assert_eq!(*memory.commands.borrow(), [0, 8]);
    assert_eq!(*memory.bounds.borrow(), [0, 8]);
}

#[test]
fn end_fault_oversized_count_and_runaway_precedence() {
    let mut r = headless_renderer();
    for (ucode, end, branch) in [
        (Microcode::F3dex2, gsp_enddl(), (0xde010000, 0)),
        (Microcode::F3d, gsp_enddl_f3d(), (0x06010000, 0)),
    ] {
        for count in [1, 2, u32::MAX - 1, u32::MAX] {
            let s = r.process_dl_prefix(&hw([end]), 0, ucode, &mut NopSink, count);
            assert_eq!((s.commands, s.termination), (1, WalkTermination::End));
            let mut diags = vec![];
            let s = r.process_dl_prefix(&hw([(0xe4000000, 0)]), 0, ucode, &mut diags, count);
            assert_eq!(
                (s.commands, s.termination, s.errors),
                (1, WalkTermination::Bounds, 1)
            );
            assert_eq!(
                diags[0].kind,
                crate::DiagKind::TruncatedRect { fill: false }
            );
        }
        for (count, termination, commands) in [
            (1, WalkTermination::Cap, 1),
            (2, WalkTermination::Bounds, 1),
        ] {
            let s =
                r.process_dl_prefix(&hw([gdp_set_fill_color(0)]), 0, ucode, &mut NopSink, count);
            assert_eq!((s.commands, s.termination), (commands, termination));
        }
        let s = r.process_dl_prefix(&hw([]), 0, ucode, &mut NopSink, 1);
        assert_eq!(
            (s.commands, s.termination, s.errors),
            (0, WalkTermination::Bounds, 1)
        );
        let long = hw(std::iter::repeat_n(gdp_set_fill_color(0), 4097).chain([end]));
        let s = r.process_dl_prefix(&long, 0, ucode, &mut NopSink, 4097);
        assert_eq!((s.commands, s.termination), (4097, WalkTermination::Cap));
        let s = r.process_dl_prefix(&long, 0, ucode, &mut NopSink, 4098);
        assert_eq!((s.commands, s.termination), (4098, WalkTermination::End));
        for count in [1 << 20, (1 << 20) + 1, u32::MAX] {
            let mut diags = vec![];
            let s = r.process_dl_prefix(&hw([branch]), 0, ucode, &mut diags, count);
            assert_eq!(s.commands, 1 << 20);
            if count == 1 << 20 {
                assert_eq!(s.termination, WalkTermination::Cap);
                assert!(diags.is_empty());
            } else {
                assert_eq!(s.termination, WalkTermination::Runaway);
                assert_eq!(s.errors, 1);
                assert_eq!(diags[0].kind, crate::DiagKind::RunawayDl { cap: 1 << 20 });
            }
        }
    }
}

fn compare_prefix(
    full: &ImgHw,
    short: &ImgHw,
    entry: u64,
    count: u32,
    addresses: &[u32],
) -> Renderer {
    let mut prefix = headless_renderer();
    let mut expected = headless_renderer();
    prefix.begin_frame();
    expected.begin_frame();
    let (mut actual_diags, mut expected_diags) = (vec![], vec![]);
    let actual = prefix.process_dl_prefix(full, entry, Microcode::F3dex2, &mut actual_diags, count);
    let mut summary = expected.process_dl(short, entry, Microcode::F3dex2, &mut expected_diags);
    assert_eq!(summary.commands, count + 1);
    summary.commands = count;
    summary.termination = WalkTermination::Cap;
    assert_eq!(actual, summary);
    assert_eq!(actual_diags, expected_diags);
    assert_eq!(prefix.frame_scenes, expected.frame_scenes);
    assert_eq!(prefix.frame_scenes.len(), 1);
    assert_eq!(prefix.last_scanout_addr, expected.last_scanout_addr);
    assert_eq!(
        prefix.last_backend_was_image,
        expected.last_backend_was_image
    );
    for &addr in addresses {
        assert_eq!(
            prefix.inner.has_fb(addr.into()),
            expected.inner.has_fb(addr.into())
        );
        if expected.inner.has_fb(addr.into()) {
            assert_eq!(
                store_pixels(&prefix, addr.into()),
                store_pixels(&expected, addr.into()),
                "target {addr:#x}"
            );
        }
    }
    prefix
}

fn two_target_fills() -> ImgHw {
    hw([
        gdp_set_color_image(0, 2, 64, A),
        gdp_set_scissor(0, 0, 0, 256, 256),
        gdp_set_fill_color(0xf801f801),
        gdp_fill_rectangle(0, 0, 124, 252),
        gdp_set_fill_color(0x07c107c1),
        gdp_fill_rectangle(128, 0, 252, 252),
        gdp_set_color_image(0, 2, 64, B),
        gdp_set_fill_color(0x003f003f),
        gdp_fill_rectangle(0, 0, 252, 252),
        gsp_enddl(),
    ])
}

#[test]
fn shorter_lists_match_before_draw_mid_pair_and_after_cimg_switch() {
    let full = two_target_fills();
    let before = hw([gdp_set_color_image(0, 2, 64, A), gsp_enddl()]);
    let r = compare_prefix(&full, &before, 0, 1, &[A, B]);
    assert!(!r.inner.has_fb(A.into()));
    assert_eq!(r.last_scanout_addr, None);
    assert!(r.frame_scenes[0].framebuffer_pairs.is_empty());
    let middle = hw([
        gdp_set_color_image(0, 2, 64, A),
        gdp_set_scissor(0, 0, 0, 256, 256),
        gdp_set_fill_color(0xf801f801),
        gdp_fill_rectangle(0, 0, 124, 252),
        gsp_enddl(),
    ]);
    let r = compare_prefix(&full, &middle, 0, 4, &[A, B]);
    assert_eq!(r.last_scanout_addr, Some(A.into()));
    assert_eq!(pixel(&r, A, 16, 32), [255, 0, 0, 255]);
    assert_ne!(pixel(&r, A, 48, 32), [0, 255, 0, 255]);
    let switched = hw([
        gdp_set_color_image(0, 2, 64, A),
        gdp_set_scissor(0, 0, 0, 256, 256),
        gdp_set_fill_color(0xf801f801),
        gdp_fill_rectangle(0, 0, 124, 252),
        gdp_set_fill_color(0x07c107c1),
        gdp_fill_rectangle(128, 0, 252, 252),
        gdp_set_color_image(0, 2, 64, B),
        gsp_enddl(),
    ]);
    let r = compare_prefix(&full, &switched, 0, 7, &[A, B]);
    assert_eq!(r.last_scanout_addr, Some(A.into()));
    assert!(!r.inner.has_fb(B.into()));
    assert_eq!(pixel(&r, A, 48, 32), [0, 255, 0, 255]);
}

#[test]
fn depth_only_prefix_matches_shorter_list_and_keeps_prior_color_scanout() {
    let full = hw([
        gdp_set_depth_image(Z),
        gdp_set_color_image(0, 2, 64, Z),
        gdp_set_scissor(0, 0, 0, 256, 256),
        gdp_set_fill_color(0xfffcfffc),
        gdp_fill_rectangle(0, 0, 252, 252),
        gdp_set_color_image(0, 2, 64, A),
        gdp_set_fill_color(0xf801f801),
        gdp_fill_rectangle(0, 0, 252, 252),
        gsp_enddl(),
    ]);
    let short = hw([
        gdp_set_depth_image(Z),
        gdp_set_color_image(0, 2, 64, Z),
        gdp_set_scissor(0, 0, 0, 256, 256),
        gdp_set_fill_color(0xfffcfffc),
        gdp_fill_rectangle(0, 0, 252, 252),
        gsp_enddl(),
    ]);
    let mut r = compare_prefix(&full, &short, 0, 5, &[A, Z]);
    assert!(r.frame_scenes[0].framebuffer_pairs[0].is_depth_clear);
    assert_eq!(r.last_scanout_addr, None);
    r.process_dl(&fill_hw(A, 0xf801f801), 0, Microcode::F3dex2, &mut NopSink);
    let before = store_pixels(&r, A.into());
    let s = r.process_dl_prefix(&full, 0, Microcode::F3dex2, &mut NopSink, 5);
    assert!(!s.renderable);
    assert_eq!(r.last_scanout_addr, Some(A.into()));
    assert_eq!(store_pixels(&r, A.into()), before);
    assert_eq!(r.frame_scenes.len(), 3);
}

#[test]
fn offscreen_then_sample_prefix_matches_independent_shorter_list() {
    let full = hw([
        gdp_set_color_image(0, 2, 64, A),
        gdp_set_scissor(0, 0, 0, 256, 256),
        gdp_set_fill_color(0xf801f801),
        gdp_fill_rectangle(0, 0, 252, 252),
        gdp_set_color_image(0, 2, 64, B),
        gdp_set_cycle_type(2),
        gdp_set_texture_image(0, 2, 64, A),
        gdp_set_tile(0, 2, 16, 0, 0, 0, 2, 0, 0, 2, 0, 0),
        gdp_set_tile_size(0, 0, 0, 252, 252),
    ]
    .into_iter()
    .chain(gsp_texture_rectangle(
        0, 0, 256, 256, 0, 0, 0, 1024, 1024, false,
    ))
    .chain([
        gdp_set_fill_color(0x003f003f),
        gdp_fill_rectangle(0, 0, 252, 252),
        gsp_enddl(),
    ]));
    let short = hw([
        gdp_set_color_image(0, 2, 64, A),
        gdp_set_scissor(0, 0, 0, 256, 256),
        gdp_set_fill_color(0xf801f801),
        gdp_fill_rectangle(0, 0, 252, 252),
        gdp_set_color_image(0, 2, 64, B),
        gdp_set_cycle_type(2),
        gdp_set_texture_image(0, 2, 64, A),
        gdp_set_tile(0, 2, 16, 0, 0, 0, 2, 0, 0, 2, 0, 0),
        gdp_set_tile_size(0, 0, 0, 252, 252),
    ]
    .into_iter()
    .chain(gsp_texture_rectangle(
        0, 0, 256, 256, 0, 0, 0, 1024, 1024, false,
    ))
    .chain([gsp_enddl()]));
    let r = compare_prefix(&full, &short, 0, 10, &[A, B]);
    assert_eq!(r.last_scanout_addr, Some(B.into()));
    assert_eq!(pixel(&r, B, 32, 32), [255, 0, 0, 255]);
    assert!(r.frame_scenes[0].framebuffer_pairs[1].ops.iter().any(|op| matches!(op, crate::scene::SceneOp::TexRect { fb_source: Some(addr), .. } if *addr == A as u64)));
}

fn pixel(r: &Renderer, addr: u32, x: usize, y: usize) -> [u8; 4] {
    store_pixels(r, addr.into())[(y * 64 + x) * 4..][..4]
        .try_into()
        .unwrap()
}

#[test]
fn backward_replay_clears_touched_targets_per_frame_and_retains_color_with_persist() {
    for policy in [ClearPolicy::PerFrame, ClearPolicy::Persist] {
        let mut r = headless_renderer();
        r.config.clear_policy = policy;
        r.begin_frame();
        r.process_dl_prefix(&two_target_fills(), 0, Microcode::F3dex2, &mut NopSink, 9);
        let b = store_pixels(&r, B.into());
        assert_eq!(pixel(&r, A, 48, 32), [0, 255, 0, 255]);
        r.process_dl_prefix(&two_target_fills(), 0, Microcode::F3dex2, &mut NopSink, 4);
        assert_eq!(pixel(&r, A, 48, 32), [0, 255, 0, 255]);
        r.begin_frame();
        let serial = r.inner.frame_serial;
        let s = r.process_dl_prefix(&two_target_fills(), 0, Microcode::F3dex2, &mut NopSink, 4);
        assert!(s.renderable);
        assert_eq!(r.inner.frame_serial, serial);
        assert_eq!(r.frame_scenes.len(), 1);
        assert_eq!(r.last_scanout_addr, Some(A.into()));
        assert_eq!(pixel(&r, A, 16, 32), [255, 0, 0, 255]);
        assert_eq!(store_pixels(&r, B.into()), b);
        match policy {
            ClearPolicy::PerFrame => {
                let mut expected = headless_renderer();
                expected.begin_frame();
                expected.process_dl(
                    &hw([
                        gdp_set_color_image(0, 2, 64, A),
                        gdp_set_scissor(0, 0, 0, 256, 256),
                        gdp_set_fill_color(0xf801f801),
                        gdp_fill_rectangle(0, 0, 124, 252),
                        gsp_enddl(),
                    ]),
                    0,
                    Microcode::F3dex2,
                    &mut NopSink,
                );
                assert_eq!(
                    store_pixels(&r, A.into()),
                    store_pixels(&expected, A.into())
                );
            }
            ClearPolicy::Persist => assert_eq!(pixel(&r, A, 48, 32), [0, 255, 0, 255]),
        }
        let before = store_pixels(&r, A.into());
        let s = r.process_dl_prefix(&two_target_fills(), 0, Microcode::F3dex2, &mut NopSink, 1);
        assert!(!s.renderable);
        assert_eq!(r.last_scanout_addr, Some(A.into()));
        assert_eq!(store_pixels(&r, A.into()), before);
    }
}

#[test]
fn full_length_and_max_prefixes_equal_ordinary_rendering() {
    for name in [
        "flat-color--white1",
        "multi-material",
        "hud-over-3d",
        "fill-texrect",
        "offscreen-then-sample",
    ] {
        let (bytes, entry) = crate::tests::fixtures::fixture(name);
        let hw = ImgHw {
            rdram: bytes.to_vec(),
        };
        let mut ordinary = headless_renderer();
        ordinary.begin_frame();
        let mut diags = vec![];
        let expected = ordinary.process_dl(&hw, entry, Microcode::F3dex2, &mut diags);
        for count in [expected.commands, expected.commands + 7, u32::MAX] {
            let mut r = headless_renderer();
            r.begin_frame();
            let mut actual_diags = vec![];
            let actual =
                r.process_dl_prefix(&hw, entry, Microcode::F3dex2, &mut actual_diags, count);
            assert_eq!(actual, expected, "{name}: {count}");
            assert_eq!(actual_diags, diags);
            assert_eq!(r.frame_scenes, ordinary.frame_scenes);
            assert_eq!(r.last_scanout_addr, ordinary.last_scanout_addr);
            let scene = &ordinary.frame_scenes[0];
            let addresses: Vec<_> = if scene.framebuffer_pairs.is_empty() {
                vec![scene.color_image.addr]
            } else {
                scene
                    .framebuffer_pairs
                    .iter()
                    .filter(|p| !p.is_depth_clear)
                    .map(|p| p.color_image.addr)
                    .collect()
            };
            for addr in addresses {
                assert_eq!(
                    store_pixels(&r, addr),
                    store_pixels(&ordinary, addr),
                    "{name}: {count}: {addr:#x}"
                );
            }
        }
    }
}

fn triangle_data() -> (crate::tests::dl_builder::DlBuilder, Vec<(u32, u32)>) {
    let mut b = crate::tests::dl_builder::DlBuilder::new();
    let vertices = b.vertices(
        &[(-1, -1), (1, -1), (1, 1), (-1, 1)].map(|(x, y)| VtxColored {
            x,
            y,
            z: 0,
            flag: 0,
            s: 0,
            t: 0,
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        }),
    );
    let viewport = b.viewport(Vp {
        vscale: [128, 128, 511, 0],
        vtrans: [128, 128, 511, 0],
    });
    let cc = CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: 4,
    };
    let ca = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 4,
    };
    (
        b,
        vec![
            gsp_viewport(viewport),
            gsp_set_geometrymode(G_SHADE | G_SHADING_SMOOTH),
            gdp_set_combine_lerp(cc, ca, cc, ca),
            gsp_vertex(0, 4, vertices),
        ],
    )
}

fn triangle_list(draws: &[(u32, u32)], paired: bool, render_mode: bool) -> (ImgHw, u64) {
    let (mut b, mut commands) = triangle_data();
    if paired {
        commands.extend([
            gdp_set_color_image(0, 2, 64, A),
            gdp_set_scissor(0, 0, 0, 256, 256),
        ]);
    }
    if render_mode {
        commands.push(gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2));
    }
    commands.extend_from_slice(draws);
    b.list("main", &commands);
    let built = b.finish("main");
    (ImgHw { rdram: built.rdram }, built.entry.into())
}

#[test]
fn merged_triangle_prefixes_match_independently_authored_shorter_lists() {
    for paired in [false, true] {
        let (full, entry) = triangle_list(
            &[
                gsp_1triangle(0, 1, 2),
                gsp_1triangle(0, 2, 3),
                gsp_1triangle(2, 1, 0),
                gsp_enddl(),
            ],
            paired,
            true,
        );
        let setup_count = if paired { 7 } else { 5 };
        let (short, short_entry) =
            triangle_list(&[gsp_1triangle(0, 1, 2), gsp_enddl()], paired, true);
        assert_eq!(entry, short_entry);
        let r = compare_prefix(&full, &short, entry, setup_count + 1, &[0, A]);
        assert_eq!(r.frame_scenes[0].indices.len(), 3);
        let addr = if paired { A } else { 0 };
        let half = store_pixels(&r, addr.into());
        assert!(half.as_chunks::<4>().0.contains(&[255, 0, 0, 255]));
        let (short, _) = triangle_list(
            &[gsp_1triangle(0, 1, 2), gsp_1triangle(0, 2, 3), gsp_enddl()],
            paired,
            true,
        );
        let r = compare_prefix(&full, &short, entry, setup_count + 2, &[0, A]);
        let scene = &r.frame_scenes[0];
        assert_eq!(scene.indices.len(), 6);
        let runs: Vec<_> = if paired {
            scene.framebuffer_pairs[0]
                .ops
                .iter()
                .filter_map(|op| match op {
                    crate::scene::SceneOp::Tris(run) => Some(run),
                    _ => None,
                })
                .collect()
        } else {
            scene.draw_runs.iter().collect()
        };
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].index_count, 6);
        assert_ne!(store_pixels(&r, addr.into()), half);
    }
}

#[test]
fn finalization_diagnostics_and_dropped_runs_describe_only_the_prefix() {
    let (full, entry) = triangle_list(
        &[
            gsp_1triangle(0, 1, 2),
            gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
            gsp_enddl(),
        ],
        true,
        false,
    );
    let mut r = headless_renderer();
    let mut diags = vec![];
    let s = r.process_dl_prefix(&full, entry, Microcode::F3dex2, &mut diags, 7);
    assert_eq!(
        (s.commands, s.tris, s.warns, s.errors, s.dropped_runs),
        (7, 1, 1, 0, 0)
    );
    assert!(s.renderable);
    assert_eq!(
        diags,
        [Diagnostic {
            at: entry + 7 * 8,
            kind: crate::DiagKind::RenderModeNeverSet
        }]
    );
    let scene = &r.frame_scenes[0];
    assert!(!scene.mvp_table.is_empty());
    assert!(!scene.viewport_table.is_empty());
    assert!(!scene.texcoord_table.is_empty());
    let cc = CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: 1,
    };
    let ca = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 4,
    };
    let (full, entry) = triangle_list(
        &[
            gdp_set_combine_lerp(cc, ca, cc, ca),
            gsp_1triangle(0, 1, 2),
            (0x01000000, 0),
            gsp_enddl(),
        ],
        true,
        true,
    );
    diags.clear();
    let s = r.process_dl_prefix(&full, entry, Microcode::F3dex2, &mut diags, 9);
    assert_eq!(
        (s.commands, s.tris, s.dropped_runs, s.termination),
        (9, 0, 1, WalkTermination::Cap)
    );
    assert!(!s.renderable);
    assert_eq!(diags.len() as u32, s.warns + s.errors);
    assert!(diags
        .iter()
        .any(|d| d.kind == crate::DiagKind::NoTextureLoaded));
}

#[test]
fn pre_cimg_flat_prefix_uses_its_own_final_cimg_address() {
    let (full, entry) = triangle_list(
        &[
            gsp_2triangles(0, 1, 2, 0, 2, 3),
            gdp_set_color_image(0, 2, 64, A),
            gdp_set_color_image(0, 2, 64, B),
            gsp_enddl(),
        ],
        false,
        true,
    );
    let (short, _) = triangle_list(
        &[
            gsp_2triangles(0, 1, 2, 0, 2, 3),
            gdp_set_color_image(0, 2, 64, A),
            gsp_enddl(),
        ],
        false,
        true,
    );
    let r = compare_prefix(&full, &short, entry, 7, &[0, A, B]);
    assert!(r.frame_scenes[0].framebuffer_pairs.is_empty());
    assert_eq!(r.last_scanout_addr, Some(A.into()));
    assert!(!r.inner.has_fb(B.into()));
    let mut ordinary = headless_renderer();
    ordinary.begin_frame();
    ordinary.process_dl(&full, entry, Microcode::F3dex2, &mut NopSink);
    assert_eq!(ordinary.last_scanout_addr, Some(B.into()));
    assert!(!ordinary.inner.has_fb(A.into()));
    assert_eq!(
        store_pixels(&r, A.into()),
        store_pixels(&ordinary, B.into())
    );
}

#[test]
fn culled_draw_opens_a_clear_only_pair_without_triangle_emission() {
    let (full, entry) = triangle_list(
        &[
            gsp_set_geometrymode(G_CULL_BOTH),
            gsp_1triangle(0, 1, 2),
            gsp_clear_geometrymode(G_CULL_BOTH),
            gsp_1triangle(0, 1, 2),
            gsp_enddl(),
        ],
        true,
        true,
    );
    let (short, _) = triangle_list(
        &[
            gsp_set_geometrymode(G_CULL_BOTH),
            gsp_1triangle(0, 1, 2),
            gsp_enddl(),
        ],
        true,
        true,
    );
    let empty = compare_prefix(&full, &short, entry, 9, &[A]);
    assert!(empty.frame_scenes[0].indices.is_empty());
    assert_eq!(empty.frame_scenes[0].framebuffer_pairs.len(), 1);
    assert_eq!(empty.last_scanout_addr, Some(A.into()));
    let mut r = headless_renderer();
    r.process_dl(&fill_hw(A, 0xf801f801), 0, Microcode::F3dex2, &mut NopSink);
    let before = store_pixels(&r, A.into());
    r.begin_frame();
    let s = r.process_dl_prefix(&full, entry, Microcode::F3dex2, &mut NopSink, 9);
    assert_eq!(s.tris, 0);
    assert!(s.renderable);
    assert_ne!(store_pixels(&r, A.into()), before);
    assert_eq!(store_pixels(&r, A.into()), store_pixels(&empty, A.into()));
}

#[test]
fn counting_observer_finalizes_scenes_and_preserves_terminal_precedence_without_gpu() {
    fn interpret(hw: &ImgHw, entry: u64, count: u32) -> crate::hle::InterpResult {
        crate::hle::interpret(
            RdramImage::new(&hw.rdram),
            entry,
            Microcode::F3dex2.into(),
            DataFormat::Fixed,
            Some(&mut crate::PrefixObserver { remaining: count }),
        )
    }
    let (full, entry) = triangle_list(
        &[
            gsp_1triangle(0, 1, 2),
            gsp_1triangle(0, 2, 3),
            gsp_1triangle(2, 1, 0),
            gsp_enddl(),
        ],
        true,
        true,
    );
    let (short, _) = triangle_list(
        &[gsp_1triangle(0, 1, 2), gsp_1triangle(0, 2, 3), gsp_enddl()],
        true,
        true,
    );
    let prefix = interpret(&full, entry, 9);
    let expected = crate::hle::interpret(
        RdramImage::new(&short.rdram),
        entry,
        Microcode::F3dex2.into(),
        DataFormat::Fixed,
        None,
    );
    assert_eq!(prefix.scene, expected.scene);
    assert_eq!(prefix.diags, expected.diags);
    assert_eq!(prefix.commands, 9);
    assert_eq!(prefix.termination, WalkTermination::ObserverStopped);
    assert_eq!(prefix.scene.indices.len(), 6);
    assert_eq!(
        interpret(&hw([gsp_enddl()]), 0, 1).termination,
        WalkTermination::End
    );
    assert_eq!(
        interpret(&hw([(0xe4000000, 0)]), 0, 1).termination,
        WalkTermination::Bounds
    );
    assert_eq!(interpret(&hw([]), 0, 1).commands, 0);
    let long = hw(std::iter::repeat_n(gdp_set_fill_color(0), 4097).chain([gsp_enddl()]));
    let prefix = interpret(&long, 0, 4097);
    assert_eq!(
        (prefix.commands, prefix.termination),
        (4097, WalkTermination::ObserverStopped)
    );
    let runaway = interpret(&hw([(0xde010000, 0)]), 0, (1 << 20) + 1);
    assert_eq!(
        (runaway.commands, runaway.termination),
        (1 << 20, WalkTermination::Runaway)
    );
    assert_eq!(
        runaway.diags,
        [Diagnostic {
            at: 0,
            kind: crate::DiagKind::RunawayDl { cap: 1 << 20 }
        }]
    );
}
