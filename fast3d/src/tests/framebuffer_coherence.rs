use super::dl_builder::{Built, Command, DlBuilder};
use n64_gbi::{consts::*, encode::*};

const FB: u32 = 0x100000;
const Z: u32 = 0x200000;
const OUTPUT: u32 = 0x300000;
const VISCVG: u32 =
    IM_RD | FORCE_BL | gbl_c1(CLR_IN, A_0, CLR_BL, B_A_MEM) | gbl_c2(CLR_IN, A_0, CLR_BL, B_A_MEM);

fn combine(rgb: u32, alpha: u32) -> Command {
    let rgb = CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: rgb,
    };
    let alpha = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: alpha,
    };
    gdp_set_combine_lerp(rgb, alpha, rgb, alpha)
}

fn image(dl: &mut Vec<Command>, address: u32, width: u32, height: u32, fmt: u32, siz: u32) {
    dl.extend([
        gdp_pipe_sync(),
        gdp_set_color_image(fmt, siz, width, address),
        gdp_set_scissor(0, 0, 0, width * 4, height * 4),
    ]);
}

fn fill(dl: &mut Vec<Command>, word: u32, bounds: [u32; 4]) {
    dl.extend([
        gdp_set_cycle_type(3),
        gdp_set_render_mode(0, 0),
        gdp_set_fill_color(word),
        gdp_fill_rectangle(bounds[0] * 4, bounds[1] * 4, bounds[2] * 4, bounds[3] * 4),
    ]);
}

fn mode(dl: &mut Vec<Command>, cycle: u32, low: u32) {
    dl.extend([
        gdp_pipe_sync(),
        (
            0xef000000 | (cycle << 20) | if cycle == 2 { 0 } else { G_TC_FILT | 0xf0 },
            low,
        ),
    ]);
}

fn strips(dl: &mut Vec<Command>, source: u32, fmt: u32, copy: bool) {
    for y in (0..240).step_by(6) {
        dl.extend([
            gdp_set_texture_image(fmt, 2, 320, source),
            gdp_set_tile(fmt, 2, 80, 0, 7, 0, 0, 0, 0, 0, 0, 0),
            gdp_load_sync(),
            (
                0xf4000000 | (y * 4),
                (7 << 24) | ((319 * 4) << 12) | ((y + 5) * 4),
            ),
            gdp_pipe_sync(),
            gdp_set_tile(fmt, 2, 80, 0, 0, 0, 0, 0, 0, 0, 0, 0),
            gdp_set_tile_size(0, 0, y * 4, 319 * 4, (y + 5) * 4),
        ]);
        dl.extend(gsp_texture_rectangle(
            0,
            y * 4,
            if copy { 319 * 4 } else { 320 * 4 },
            (y + if copy { 5 } else { 6 }) * 4,
            0,
            0,
            y * 32,
            if copy { 4096 } else { 1024 },
            1024,
            false,
        ));
    }
}

fn setup() -> Vec<Command> {
    let mut dl = vec![gdp_set_depth_image(Z)];
    image(&mut dl, Z, 320, 240, 0, 2);
    fill(&mut dl, 0xfffcfffc, [0, 0, 319, 239]);
    image(&mut dl, FB, 320, 240, 0, 2);
    fill(&mut dl, 0xf801f801, [0, 0, 319, 239]);
    fill(&mut dl, 0x07c107c1, [160, 0, 319, 239]);
    fill(&mut dl, 0x003f003f, [0, 234, 319, 239]);
    mode(&mut dl, 0, G_ZS_PRIM | G_RM_OPA_SURF | G_RM_OPA_SURF2);
    dl.extend([gdp_set_env_color(0xffffffff), combine(1, 6)]);
    let modulate = CcPass {
        a: 1,
        b: ZERO_C,
        c: 5,
        d: ZERO_A,
    };
    let one = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 6,
    };
    dl.push(gdp_set_combine_lerp(modulate, one, modulate, one));
    image(&mut dl, Z, 320, 240, 0, 2);
    strips(&mut dl, FB, 0, false);
    image(&mut dl, FB, 320, 240, 0, 2);
    dl.extend([
        gdp_set_blend_color(0xffffff08),
        gdp_set_prim_depth(0xffff, 0xffff),
    ]);
    mode(&mut dl, 0, G_ZS_PRIM | VISCVG);
    dl.extend([
        gdp_set_scissor(0, 0, 0, 1280, 960),
        gdp_fill_rectangle(0, 0, 1280, 960),
        gdp_pipe_sync(),
    ]);
    mode(
        &mut dl,
        0,
        G_ZS_PRIM | gbl_c1(CLR_IN, A_0, CLR_IN, B_1) | gbl_c2(CLR_IN, A_0, CLR_IN, B_1),
    );
    dl.push(combine(1, ZERO_A));
    image(&mut dl, FB, 320, 240, 4, 1);
    strips(&mut dl, FB, 3, false);
    image(&mut dl, FB, 320, 240, 0, 2);
    dl
}

fn ready() -> Vec<Command> {
    let mut dl = Vec::new();
    mode(&mut dl, 2, 0);
    image(&mut dl, OUTPUT, 320, 240, 0, 2);
    strips(&mut dl, Z, 0, true);
    image(&mut dl, OUTPUT, 320, 240, 0, 2);
    dl
}

fn built(mut b: DlBuilder, mut dl: Vec<Command>) -> Built {
    dl.extend([gdp_pipe_sync(), (0xe9000000, 0), gsp_enddl()]);
    b.list("main", &dl);
    let mut built = b.finish("main");
    built.rdram.resize(OUTPUT as usize + 320 * 240 * 2, 0);
    built
}

#[test]
fn writeback_cvg_policy_survives_later_othermode() {
    for triangle in [false, true] {
        let mut b = DlBuilder::new();
        let vertices = b.vertices(&[[-1, -1], [1, -1], [0, 1]].map(|[x, y]| VtxColored {
            x,
            y,
            z: 0,
            flag: 0,
            s: 0,
            t: 0,
            r: 64,
            g: 128,
            b: 192,
            a: 0,
        }));
        let mut dl = Vec::new();
        image(&mut dl, FB, 32, 24, 0, 2);
        dl.extend([
            combine(3, 3),
            gsp_vertex(0, 3, vertices),
            (0xef00_0000 | G_TC_FILT | 0xf0, 0),
        ]);
        dl.push(if triangle {
            gsp_1triangle(0, 1, 2)
        } else {
            gdp_fill_rectangle(0, 0, 32, 32)
        });
        dl.push((0xef00_0000 | G_TC_FILT | 0xf0, CVG_DST_FULL));
        dl.push(if triangle {
            gsp_1triangle(0, 1, 2)
        } else {
            gdp_fill_rectangle(32, 0, 64, 32)
        });
        dl.push((0xef00_0000 | G_TC_FILT | 0x30, CVG_DST_SAVE));
        let built = built(b, dl);
        let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        let modes: Vec<_> = result
            .scene
            .framebuffer_pairs
            .iter()
            .flat_map(|pair| &pair.ops)
            .filter_map(|op| {
                let index = match op {
                    crate::scene::SceneOp::TexRect {
                        render_mode_index, ..
                    } => *render_mode_index,
                    crate::scene::SceneOp::Tris(run) => run.render_mode_index,
                    _ => return None,
                };
                Some(result.scene.render_modes[index as usize])
            })
            .collect();
        assert_eq!(modes.len(), 2);
        assert_ne!(modes[0], modes[1]);
        assert_eq!(modes[0].cvg_dst, 0);
        assert_eq!(modes[1].cvg_dst, 2);
    }
}

#[test]
fn pause_setup_and_ready_record_without_rejections() {
    let built = built(DlBuilder::new(), [setup(), ready()].concat());
    let mut state = crate::hle::interp::FramebufferState::default();
    state.targets.seed_owners(&Default::default());
    let result = crate::hle::interp::interpret_with_framebuffers(
        crate::RdramImage::new(&built.rdram),
        built.entry.into(),
        crate::hle::gbi::GbiUcode::F3dex2,
        crate::hle::mem::GbiDataFormat::Fixed,
        Default::default(),
        state,
        None,
    );
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert_eq!(result.dropped_runs, 0);
    assert_eq!(
        result
            .scene
            .framebuffer_pairs
            .iter()
            .flat_map(|pair| &pair.ops)
            .filter(|op| matches!(op, crate::scene::SceneOp::TexRect { .. }))
            .count(),
        121
    );
}

#[test]
fn depth_alias_requires_depth_access_to_reject() {
    for cycle in [0, G_CYC_FILL, G_CYC_COPY] {
        for address in [Z, Z + 16, Z - 16] {
            for flags in [0, Z_CMP, Z_UPD, Z_CMP | Z_UPD] {
                let rejected = flags != 0 && cycle == 0;
                let mut dl = vec![gdp_set_depth_image(Z)];
                image(&mut dl, address, 8, 4, 0, 2);
                mode(&mut dl, cycle, flags);
                dl.extend([combine(3, 3), gdp_set_prim_color(0, 0, 0xff0000ff)]);
                if cycle == G_CYC_COPY {
                    strips(&mut dl, FB, 0, true);
                } else {
                    dl.push(gdp_fill_rectangle(0, 0, 32, 16));
                }
                let built = built(DlBuilder::new(), dl);
                let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
                assert_eq!(
                    result.dropped_runs,
                    u32::from(rejected),
                    "{cycle}, {address:x}, {flags:x}: {:?}",
                    result.diags
                );
                assert_eq!(
                    result.diags.len(),
                    usize::from(rejected),
                    "{:?}",
                    result.diags
                );
                if rejected {
                    assert!(matches!(
                        result.diags[0].kind,
                        crate::DiagKind::UnsupportedDepthAlias { .. }
                    ));
                }
            }
        }
    }
}

#[test]
fn fill_and_copy_cycles_leave_depth_unwritten() {
    for cycle in [G_CYC_FILL, G_CYC_COPY] {
        let mut dl = vec![gdp_set_depth_image(Z)];
        image(&mut dl, Z, 320, 240, 0, 2);
        mode(&mut dl, 0, G_RM_OPA_SURF | G_RM_OPA_SURF2);
        dl.extend([
            combine(3, 3),
            gdp_set_prim_color(0, 0, 0xff0000ff),
            gdp_fill_rectangle(0, 0, 1280, 960),
        ]);
        image(&mut dl, OUTPUT, 320, 240, 0, 2);
        mode(&mut dl, cycle, G_ZS_PRIM | Z_UPD);
        if cycle == G_CYC_FILL {
            dl.push(gdp_fill_rectangle(0, 0, 1276, 956));
            mode(&mut dl, G_CYC_COPY, 0);
        }
        strips(&mut dl, Z, 0, true);
        let built = built(DlBuilder::new(), dl);
        let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
        assert!(result.diags.is_empty(), "{cycle}: {:?}", result.diags);
        assert_eq!(result.dropped_runs, 0);
    }
}

fn coverage_fill(cycle: u32) -> Built {
    let mut dl = Vec::new();
    image(&mut dl, FB, 8, 4, 0, 2);
    fill(&mut dl, 0x00010001, [0, 0, 7, 3]);
    dl.extend([
        combine(1, 1),
        gdp_set_blend_color(0xffffff08),
        gdp_set_prim_depth(0xffff, 0xffff),
    ]);
    let render_mode = if cycle == 0 {
        VISCVG
    } else {
        IM_RD
            | FORCE_BL
            | gbl_c1(CLR_FOG, A_FOG, CLR_IN, B_1MA)
            | gbl_c2(CLR_IN, A_0, CLR_BL, B_A_MEM)
    };
    dl.push(gdp_set_fog_color(0x000000ff));
    mode(&mut dl, cycle, G_ZS_PRIM | render_mode);
    dl.push(gdp_fill_rectangle(0, 0, 32, 16));
    built(DlBuilder::new(), dl)
}

#[test]
fn viscvg_ignores_unloaded_inherited_texture() {
    let built = coverage_fill(0);
    let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert_eq!(result.dropped_runs, 0);
    assert!(!result.scene.materials.last().unwrap().tex_enable);
}

#[test]
fn viscvg_render_mode_switch_rebuilds_triangle_texture_dependencies() {
    let mut b = DlBuilder::new();
    let vertices = b.vertices(&[[-1, -1], [1, -1], [1, 1]].map(|[x, y]| VtxColored {
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
    let mut dl = Vec::new();
    image(&mut dl, FB, 8, 8, 0, 2);
    mode(&mut dl, 0, VISCVG);
    dl.extend([
        combine(1, 1),
        gsp_vertex(0, 3, vertices),
        gsp_1triangle(0, 1, 2),
        gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
        gsp_1triangle(0, 1, 2),
        gdp_set_render_mode(VISCVG, 0),
        gsp_1triangle(0, 1, 2),
    ]);
    let built = built(b, dl);
    let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
    assert_eq!(result.dropped_runs, 1);
    assert_eq!(result.scene.indices.len(), 6);
    assert_eq!(result.diags.len(), 1);
    assert_eq!(result.diags[0].kind, crate::DiagKind::NoTextureLoaded);
}

#[cfg(not(target_arch = "wasm32"))]
mod gpu {
    use super::*;
    use crate::{Hardware, Rdram, RdramImage, Renderer};

    struct Memory(Vec<u8>);
    impl Hardware for Memory {
        fn rdram(&self) -> impl Rdram + '_ {
            RdramImage::new(&self.0)
        }
    }

    fn renderer(fallback: bool) -> Renderer {
        let (device, queue) = if fallback {
            crate::render::headless_device_forced_fallback()
        } else {
            let (device, queue, _) = crate::render::headless_device();
            (device, queue)
        };
        let mut r = Renderer::with_device(
            device,
            queue,
            crate::PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 320,
                height: 240,
            },
            crate::RendererConfig {
                clear_policy: crate::ClearPolicy::Persist,
                resolution_multiplier: 1,
                sample_count: 1,
                present_mode: wgpu::PresentMode::Fifo,
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                power_preference: wgpu::PowerPreference::LowPower,
            },
        );
        r.set_guest_writeback(true);
        r
    }

    fn run(r: &mut Renderer, memory: &mut Memory, built: Built) {
        run_microcode(r, memory, built, crate::Microcode::F3dex2);
    }

    fn run_microcode(
        r: &mut Renderer,
        memory: &mut Memory,
        built: Built,
        microcode: crate::Microcode,
    ) {
        let entry = built.entry;
        let end = built.rdram[entry as usize..]
            .as_chunks::<8>()
            .0
            .iter()
            .position(|c| c[0] == 0xdf)
            .unwrap()
            * 8
            + entry as usize
            + 8;
        memory.0[..end].copy_from_slice(&built.rdram[..end]);
        let mut diagnostics = Vec::new();
        let result = r.process_dl_with_bindings(
            memory,
            u64::from(entry),
            microcode,
            &mut diagnostics,
            &[
                (0x1000, crate::Microcode::S2dex2),
                (0x2000, crate::Microcode::F3dex2NoN),
                (0x3000, crate::Microcode::F3dex2),
            ],
        );
        assert_eq!(result.termination, crate::inspect::WalkTermination::End);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(result.dropped_runs, 0);
        for patch in r.take_guest_writes() {
            memory.0[patch.address as usize..patch.address as usize + patch.bytes.len()]
                .copy_from_slice(&patch.bytes);
        }
    }

    fn word(memory: &Memory, address: u32, pixel: usize) -> u16 {
        let start = address as usize + pixel * 2;
        u16::from_ne_bytes(memory.0[start..start + 2].try_into().unwrap())
    }

    #[test]
    fn writeback_copy_alpha_rejection_demotes_prior_coverage() {
        for fallback in [false, true] {
            let mut r = renderer(fallback);
            for copy in [false, true] {
                r.reset();
                let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
                let mut b = DlBuilder::new();
                let transparent = b.bytes(8, &[0; 32]);
                let mut dl = Vec::new();
                image(&mut dl, FB, 32, 24, 0, 2);
                mode(&mut dl, 0, AA_EN);
                dl.extend([
                    combine(3, 3),
                    gdp_set_prim_color(0, 0, 0x4080c000),
                    gdp_fill_rectangle(0, 0, 128, 96),
                ]);
                if copy {
                    mode(&mut dl, 2, AC_THRESHOLD);
                    dl.extend(gdp_load_texture_block(0, 2, 4, 4, transparent, 2, 0, 2, 0));
                    dl.extend(gsp_texture_rectangle(
                        32, 32, 60, 60, 0, 0, 0, 4096, 1024, false,
                    ));
                }
                run(&mut r, &mut memory, built(b, dl));
                assert_eq!(
                    word(&memory, FB, 12 * 32 + 12),
                    if copy { 0x4430 } else { 0x4431 },
                    "fallback={fallback}, copy={copy}"
                );
                assert_eq!(word(&memory, FB, 3 * 32 + 3), 0x4431);
            }
        }
    }

    #[test]
    fn writeback_full_triangle_ignores_opacity() {
        full_writeback_ignores_opacity(true);
    }

    #[test]
    fn writeback_full_rectangle_ignores_opacity() {
        full_writeback_ignores_opacity(false);
    }

    fn full_writeback_ignores_opacity(triangle: bool) {
        for fallback in [false, true] {
            let mut r = renderer(fallback);
            let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
            for cycle in [0, 1] {
                for alpha in [0, 4, 128, 255] {
                    r.reset();
                    let mut b = DlBuilder::new();
                    let viewport = b.viewport(Vp {
                        vscale: [64, 48, 511, 0],
                        vtrans: [64, 48, 511, 0],
                    });
                    let vertices =
                        b.vertices(&[[-1, -1], [1, -1], [0, 1]].map(|[x, y]| VtxColored {
                            x,
                            y,
                            z: 0,
                            flag: 0,
                            s: 0,
                            t: 0,
                            r: 64,
                            g: 128,
                            b: 192,
                            a: alpha,
                        }));
                    let mut dl = Vec::new();
                    image(&mut dl, FB, 32, 24, 0, 2);
                    mode(&mut dl, cycle, AA_EN);
                    dl.extend([combine(4, 4), gsp_viewport(viewport)]);
                    if triangle {
                        dl.extend([gsp_vertex(0, 3, vertices), gsp_1triangle(0, 1, 2)]);
                    } else {
                        dl.extend([
                            combine(3, 3),
                            gdp_set_prim_color(0, 0, 0x4080c000 | u32::from(alpha)),
                            gdp_fill_rectangle(0, 0, 128, 96),
                        ]);
                    }
                    run(&mut r, &mut memory, built(b, dl));
                    assert_eq!(
                        word(&memory, FB, 12 * 32 + 16),
                        0x4431,
                        "fallback={fallback}, triangle={triangle}, alpha={alpha}"
                    );
                }
            }
        }
    }

    #[test]
    fn writeback_uses_native_target_with_upscaled_presentation() {
        for fallback in [false, true] {
            let mut native_bytes = None;
            for scale in [1, 3] {
                let mut r = renderer(fallback);
                r.target = crate::PresentTarget::Headless {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    width: 320 * scale,
                    height: 240 * scale,
                };
                r.reconfigure(crate::RendererConfig {
                    resolution_multiplier: scale,
                    ..r.config
                });
                let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
                let mut dl = Vec::new();
                image(&mut dl, FB, 32, 24, 0, 2);
                mode(&mut dl, 0, CVG_DST_FULL);
                dl.extend([
                    combine(3, 3),
                    gdp_set_prim_color(0, 0, 0x4080c000),
                    gdp_fill_rectangle(0, 0, 128, 96),
                ]);
                run(&mut r, &mut memory, built(DlBuilder::new(), dl));
                assert_eq!(word(&memory, FB, 12 * 32 + 16), 0x4431);
                assert_eq!(
                    &memory.0[FB as usize + 32 * 24 * 2..FB as usize + 32 * 24 * 2 + 4],
                    &[0, 0, 0, 0]
                );
                let mut b = DlBuilder::new();
                let viewport = b.viewport(Vp {
                    vscale: [64, 16, 511, 0],
                    vtrans: [64, 16, 511, 0],
                });
                let vertices = b.vertices(&[[-1, -1], [1, -1], [0, 1]].map(|[x, y]| VtxColored {
                    x,
                    y,
                    z: 0,
                    flag: 0,
                    s: 0,
                    t: 0,
                    r: if x < 0 { 0 } else { 255 },
                    g: 128,
                    b: 192,
                    a: 0,
                }));
                let mut dl = Vec::new();
                image(&mut dl, FB, 32, 8, 0, 2);
                mode(&mut dl, 0, AA_EN);
                dl.extend([
                    combine(4, 4),
                    gsp_viewport(viewport),
                    gsp_vertex(0, 3, vertices),
                    gsp_1triangle(0, 1, 2),
                ]);
                run(&mut r, &mut memory, built(b, dl));
                assert_eq!(word(&memory, FB, 32 + 16) & 1, 1);
                let bytes = memory.0[FB as usize..FB as usize + 32 * 24 * 2].to_vec();
                if let Some(expected) = &native_bytes {
                    assert_eq!(&bytes, expected);
                } else {
                    native_bytes = Some(bytes);
                }
            }
        }
    }

    #[test]
    fn writeback_full_mode_fixes_edges_and_keeps_unknown_clamp_edges() {
        for fallback in [false, true] {
            let mut r = renderer(fallback);
            r.set_guest_writeback_with_encoding(true, crate::FramebufferEncoding::BigEndian);
            let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
            let mut dl = Vec::new();
            image(&mut dl, FB, 32, 24, 0, 2);
            mode(&mut dl, 0, AA_EN);
            dl.extend([
                combine(3, 3),
                gdp_set_prim_color(0, 0, 0x4080c000),
                gdp_fill_rectangle(9, 9, 21, 21),
            ]);
            mode(&mut dl, 0, AA_EN | CVG_DST_FULL);
            dl.push(gdp_fill_rectangle(49, 9, 61, 21));
            mode(&mut dl, 0, CVG_DST_SAVE | IM_RD);
            run(&mut r, &mut memory, built(DlBuilder::new(), dl));
            let pixel = |p: usize| &memory.0[FB as usize + p * 2..FB as usize + p * 2 + 2];
            assert_eq!(pixel(5 * 32 + 5), &[0x44, 0x30]);
            assert_eq!(pixel(5 * 32 + 15), &[0x44, 0x31]);
            assert_eq!(pixel(0), &[0, 0]);
        }
    }

    fn pixels(r: &mut Renderer, address: u32, width: u32, height: u32) -> Vec<u8> {
        let device = r.device().clone();
        let queue = r.queue().clone();
        crate::tests::common::pixels_from_render(
            &device,
            &queue,
            width,
            height,
            wgpu::TextureFormat::Rgba8Unorm,
            |view| {
                let mut encoder = device.create_command_encoder(&Default::default());
                r.inner
                    .scanout(&device, &mut encoder, view, u64::from(address), None);
                queue.submit([encoder.finish()]);
            },
        )
    }

    #[test]
    fn canonical_writeback_round_trip_across_tasks_and_reconfigure() {
        for fallback in [false, true] {
            let mut r = renderer(fallback);
            r.set_guest_writeback_with_encoding(true, crate::FramebufferEncoding::BigEndian);
            r.reconfigure(r.config);
            let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
            let mut dl = Vec::new();
            image(&mut dl, FB, 2, 1, 0, 2);
            fill(&mut dl, 0xf801_07c1, [0, 0, 1, 0]);
            run(&mut r, &mut memory, built(DlBuilder::new(), dl));
            assert_eq!(
                &memory.0[FB as usize..FB as usize + 4],
                &[0xf8, 0x01, 0x07, 0xc1]
            );
            assert_eq!(memory.rdram().read_u16(FB.into()).unwrap(), 0xf801);
            assert_eq!(memory.rdram().read_u16(u64::from(FB) + 2).unwrap(), 0x07c1);
            memory.0[FB as usize..FB as usize + 2].copy_from_slice(&[0x00, 0x3f]);
            let mut dl = Vec::new();
            image(&mut dl, FB, 2, 1, 0, 2);
            mode(&mut dl, 0, 0);
            dl.extend([
                combine(3, 3),
                gdp_set_prim_color(0, 0, 0xff0000ff),
                gdp_fill_rectangle(4, 0, 8, 4),
            ]);
            run(&mut r, &mut memory, built(DlBuilder::new(), dl));
            assert_eq!(
                &memory.0[FB as usize..FB as usize + 4],
                &[0x00, 0x3f, 0xf8, 0x01]
            );
            assert_eq!(pixels(&mut r, FB, 2, 1), [0, 0, 255, 255, 255, 0, 0, 255]);
            r.reset();
            let mut dl = Vec::new();
            image(&mut dl, FB, 2, 1, 0, 2);
            fill(&mut dl, 0xf801_07c1, [0, 0, 1, 0]);
            let built = built(DlBuilder::new(), dl);
            let entry = u64::from(built.entry);
            run(&mut r, &mut memory, built);
            assert_eq!(
                &memory.0[FB as usize..FB as usize + 4],
                &[0xf8, 0x01, 0x07, 0xc1]
            );
            let summary = r.process_dl(
                &memory,
                entry,
                crate::Microcode::F3dex2,
                &mut crate::NopSink,
            );
            assert_eq!(summary.termination, crate::inspect::WalkTermination::End);
            r.set_guest_writeback(true);
            assert!(r.take_guest_writes().is_empty());
            assert!(r.inner.coherence.as_ref().unwrap().owners.0.is_empty());
            let summary = r.process_dl(
                &memory,
                entry,
                crate::Microcode::F3dex2,
                &mut crate::NopSink,
            );
            assert_eq!(summary.termination, crate::inspect::WalkTermination::End);
            r.set_guest_writeback(true);
            let patches = r.take_guest_writes();
            assert_eq!(patches.len(), 1);
            #[cfg(target_endian = "little")]
            assert_eq!(patches[0].bytes, [0x01, 0xf8, 0xc1, 0x07]);
            #[cfg(target_endian = "big")]
            assert_eq!(patches[0].bytes, [0xf8, 0x01, 0x07, 0xc1]);
        }
    }

    #[test]
    fn non_clamps_depth_and_survives_bound_switches_with_guest_writeback() {
        for fallback in [false, true] {
            let mut r = renderer(fallback);
            if !fallback
                && !r
                    .device()
                    .features()
                    .contains(wgpu::Features::DEPTH_CLIP_CONTROL)
            {
                continue;
            }
            let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
            for (z, clamped) in [(-2, 0x0000), (2, 0xfffc)] {
                for microcode in [crate::Microcode::F3dex2, crate::Microcode::F3dex2NoN] {
                    r.reset();
                    let mut b = DlBuilder::new();
                    let matrix = b.matrix(crate::hle::math::identity());
                    let viewport = b.viewport(Vp {
                        vscale: [16, 16, 511, 0],
                        vtrans: [16, 16, 511, 0],
                    });
                    let vertices =
                        b.vertices(&[[-1, -1, z], [1, -1, z], [0, 1, z]].map(|[x, y, z]| {
                            VtxColored {
                                x,
                                y,
                                z,
                                flag: 0,
                                s: 0,
                                t: 0,
                                r: 255,
                                g: 255,
                                b: 255,
                                a: 255,
                            }
                        }));
                    let mut dl = vec![gdp_set_depth_image(Z)];
                    image(&mut dl, Z, 8, 8, 0, 2);
                    fill(&mut dl, 0x80008000, [0, 0, 7, 7]);
                    image(&mut dl, FB, 8, 8, 0, 2);
                    fill(&mut dl, 0x00010001, [0, 0, 7, 7]);
                    mode(&mut dl, 0, Z_UPD);
                    dl.extend([
                        combine(3, 3),
                        gdp_set_prim_color(0, 0, 0xff0000ff),
                        gsp_viewport(viewport),
                        gsp_vertex(0, 3, vertices),
                        gsp_1triangle(0, 1, 2),
                    ]);
                    if microcode == crate::Microcode::F3dex2NoN {
                        dl.extend([
                            (0xe1000000, 0x8000),
                            (0xdd0007ff, 0x1000),
                            (0xdd0007ff, 0x2000),
                            gsp_matrix(matrix, false, true, false),
                            gsp_vertex(0, 3, vertices),
                            gdp_set_prim_color(0, 0, 0x00ff00ff),
                            gsp_1triangle(0, 1, 2),
                            (0xdd0007ff, 0x3000),
                            gsp_matrix(matrix, false, true, false),
                            gsp_vertex(0, 3, vertices),
                            gdp_set_prim_color(0, 0, 0x0000ffff),
                            gsp_1triangle(0, 1, 2),
                        ]);
                    }
                    run_microcode(&mut r, &mut memory, built(b, dl), microcode);
                    let rgba = pixels(&mut r, FB, 8, 8);
                    let green: Vec<_> = rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .enumerate()
                        .filter_map(|(p, color)| (*color == [0, 255, 0, 255]).then_some(p))
                        .collect();
                    if microcode == crate::Microcode::F3dex2NoN {
                        assert!(
                            !green.is_empty(),
                            "{microcode:?}, z={z}, fallback={fallback}"
                        );
                        assert!(rgba
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .all(|p| *p == [0, 255, 0, 255] || *p == [0, 0, 0, 255]));
                        for p in green {
                            assert_eq!(word(&memory, FB, p), 0x07c1);
                            assert_eq!(word(&memory, Z, p), clamped, "z={z}, fallback={fallback}");
                        }
                    } else {
                        assert!(rgba.as_chunks::<4>().0.iter().all(|p| *p == [0, 0, 0, 255]));
                        assert!((0..64).all(|p| word(&memory, Z, p) == 0x8000));
                    }
                }
            }
        }
    }

    #[test]
    fn non_keeps_clipping_vertices_behind_the_eye() {
        for fallback in [false, true] {
            let mut r = renderer(fallback);
            if !fallback
                && !r
                    .device()
                    .features()
                    .contains(wgpu::Features::DEPTH_CLIP_CONTROL)
            {
                continue;
            }
            let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
            let mut coverage = Vec::new();
            for microcode in [crate::Microcode::F3dex2, crate::Microcode::F3dex2NoN] {
                r.reset();
                let mut b = DlBuilder::new();
                // clip = (x, y, z / 2, z): the near plane is the eye plane, w = 0.
                let projection = b.matrix([
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 0.5, 1.0],
                    [0.0, 0.0, 0.0, 0.0],
                ]);
                let modelview = b.matrix(crate::hle::math::identity());
                let viewport = b.viewport(Vp {
                    vscale: [64, 64, 511, 0],
                    vtrans: [64, 64, 511, 0],
                });
                let vertices =
                    b.vertices(&[[-5, -5, 10], [5, -5, 10], [0, 5, -5]].map(|[x, y, z]| {
                        VtxColored {
                            x,
                            y,
                            z,
                            flag: 0,
                            s: 0,
                            t: 0,
                            r: 255,
                            g: 255,
                            b: 255,
                            a: 255,
                        }
                    }));
                let mut dl = vec![gdp_set_depth_image(Z)];
                image(&mut dl, Z, 32, 32, 0, 2);
                fill(&mut dl, 0xfffcfffc, [0, 0, 31, 31]);
                image(&mut dl, FB, 32, 32, 0, 2);
                fill(&mut dl, 0x00010001, [0, 0, 31, 31]);
                mode(&mut dl, 0, Z_UPD);
                dl.extend([
                    combine(3, 3),
                    gdp_set_prim_color(0, 0, 0x00ff00ff),
                    gsp_viewport(viewport),
                    gsp_matrix(projection, true, true, false),
                    gsp_matrix(modelview, false, true, false),
                    gsp_vertex(0, 3, vertices),
                    gsp_1triangle(0, 1, 2),
                ]);
                run_microcode(&mut r, &mut memory, built(b, dl), microcode);
                let rgba = pixels(&mut r, FB, 32, 32);
                coverage.push(
                    rgba.as_chunks::<4>()
                        .0
                        .iter()
                        .map(|p| p[1] == 255)
                        .collect::<Vec<_>>(),
                );
            }
            assert!(coverage[0][..32].iter().all(|&covered| covered));
            assert_eq!(coverage[0], coverage[1], "fallback={fallback}");
        }
    }

    #[cfg(feature = "capture")]
    #[test]
    fn non_capture_replays_with_the_recorded_depth_clip_control() {
        use crate::capture::{CaptureFrame, Fixture, Provenance};
        let mut r = renderer(false);
        if !r
            .device()
            .features()
            .contains(wgpu::Features::DEPTH_CLIP_CONTROL)
        {
            return;
        }
        let mut b = DlBuilder::new();
        // clip z = z / 4: the red triangle spans NDC z -3 to 1 and the green one sits at -0.5.
        // Clamping the red vertices instead of its fragments would lift its depth past the green.
        let projection = b.matrix([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.25, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        let viewport = b.viewport(Vp {
            vscale: [64, 64, 511, 0],
            vtrans: [64, 64, 511, 0],
        });
        let vertices = b.vertices(
            &[
                [-1, -1, -12],
                [1, -1, -12],
                [-1, 1, 4],
                [-1, -1, -2],
                [1, -1, -2],
                [-1, 1, -2],
            ]
            .map(|[x, y, z]| VtxColored {
                x,
                y,
                z,
                flag: 0,
                s: 0,
                t: 0,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            }),
        );
        let mut dl = vec![gdp_set_depth_image(Z)];
        image(&mut dl, Z, 32, 32, 0, 2);
        fill(&mut dl, 0xfffcfffc, [0, 0, 31, 31]);
        image(&mut dl, FB, 32, 32, 0, 2);
        fill(&mut dl, 0x00010001, [0, 0, 31, 31]);
        mode(&mut dl, 0, Z_UPD);
        dl.extend([
            combine(3, 3),
            gdp_set_prim_color(0, 0, 0xff0000ff),
            gsp_viewport(viewport),
            gsp_matrix(projection, true, true, false),
            gsp_vertex(0, 6, vertices),
            gsp_1triangle(0, 1, 2),
        ]);
        mode(&mut dl, 0, Z_CMP);
        dl.extend([gdp_set_prim_color(0, 0, 0x00ff00ff), gsp_1triangle(3, 4, 5)]);
        let built = built(b, dl);
        let memory = Memory(built.rdram);
        let mut capture = CaptureFrame::begin(&mut r, 0, 0, Provenance::default());
        let mut diagnostics = Vec::new();
        capture
            .process_dl(
                &mut r,
                &memory,
                u64::from(built.entry),
                crate::Microcode::F3dex2NoN,
                crate::DataFormat::Fixed,
                &mut diagnostics,
            )
            .unwrap();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let fixture = capture.present_last(&mut r).unwrap();
        let fixture = Fixture::from_bytes(&fixture.to_bytes().unwrap()).unwrap();
        let (device, queue, _) = crate::render::headless_device();
        let recorded = pollster::block_on(fixture.replay(device, queue)).unwrap();
        let headless = pollster::block_on(fixture.replay_headless()).unwrap();
        assert_eq!(headless.rgba8, recorded.rgba8);
        let mut clamped = fixture.clone();
        clamped.frame.dual_source_blending = false;
        let (device, queue) = crate::render::headless_device_forced_fallback();
        let error = pollster::block_on(clamped.replay(device, queue)).unwrap_err();
        assert!(error.to_string().contains("depth-clip"), "{error}");
    }

    #[test]
    fn primitive_depth_keeps_triangle_near_plane_clipping() {
        let mut r = renderer(true);
        r.set_guest_writeback(false);
        let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
        let mut outputs = Vec::new();
        for source in [G_ZS_PIXEL, G_ZS_PRIM] {
            r.reset();
            let mut b = DlBuilder::new();
            let viewport = b.viewport(Vp {
                vscale: [16, 16, 511, 0],
                vtrans: [16, 16, 511, 0],
            });
            let vertices =
                b.vertices(
                    &[[-1, -1, -2], [1, -1, 0], [1, 1, 0]].map(|[x, y, z]| VtxColored {
                        x,
                        y,
                        z,
                        flag: 0,
                        s: 0,
                        t: 0,
                        r: 255,
                        g: 255,
                        b: 255,
                        a: 255,
                    }),
                );
            let mut dl = Vec::new();
            image(&mut dl, FB, 8, 8, 0, 2);
            fill(&mut dl, 0x00010001, [0, 0, 7, 7]);
            mode(&mut dl, 0, source | G_RM_OPA_SURF | G_RM_OPA_SURF2);
            dl.extend([
                combine(3, 3),
                gdp_set_prim_color(0, 0, 0xff0000ff),
                gdp_set_prim_depth(0x2000, 0),
                gsp_viewport(viewport),
                gsp_vertex(0, 3, vertices),
                gsp_1triangle(0, 1, 2),
            ]);
            run(&mut r, &mut memory, built(b, dl));
            outputs.push(pixels(&mut r, FB, 8, 8));
        }
        assert!(outputs[0].as_chunks::<4>().0.contains(&[255, 0, 0, 255]));
        assert!(outputs[0].as_chunks::<4>().0.contains(&[0, 0, 0, 255]));
        assert_eq!(outputs[0], outputs[1]);
    }

    #[test]
    fn consecutive_decals_compare_against_prior_writes_without_coherence() {
        for fallback in [true, false] {
            for second_writes in [false, true] {
                let mut r = renderer(fallback);
                r.set_guest_writeback(false);
                let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
                let mut dl = vec![gdp_set_depth_image(Z)];
                image(&mut dl, Z, 8, 8, 0, 2);
                fill(&mut dl, 0x40004000, [0, 0, 7, 7]);
                image(&mut dl, FB, 8, 8, 0, 2);
                fill(&mut dl, 0x00010001, [0, 0, 7, 7]);
                for (z, color, write) in [
                    (0x6003, 0xff0000ff, true),
                    (0x6006, 0x00ff00ff, second_writes),
                ] {
                    mode(
                        &mut dl,
                        0,
                        G_ZS_PRIM | ZMODE_DEC | Z_CMP | if write { Z_UPD } else { 0 },
                    );
                    dl.extend([
                        combine(3, 3),
                        gdp_set_prim_color(0, 0, color),
                        gdp_set_prim_depth(z, 0),
                        gdp_fill_rectangle(8, 8, 24, 24),
                    ]);
                }
                run(&mut r, &mut memory, built(DlBuilder::new(), dl));
                assert_eq!(&pixels(&mut r, FB, 8, 8)[45 * 4..46 * 4], &[0, 255, 0, 255]);
                let (_, _, depth) = crate::tests::depth_persistence::depth_pixels(
                    r.device(),
                    r.queue(),
                    &r.inner,
                    Z.into(),
                );
                assert_eq!(
                    depth[45],
                    if second_writes {
                        196656.0 / 262144.0
                    } else {
                        196632.0 / 262144.0
                    }
                );
            }
        }
    }

    #[test]
    fn depth_reading_decals_share_a_segment_without_coherence() {
        let mut segments = Vec::new();
        for decals in [1, 3] {
            let mut r = renderer(true);
            r.set_guest_writeback(false);
            let recorder = crate::profiling::Recorder::new(crate::profiling::Mode::Counters);
            r.inner.profiling = recorder.clone();
            let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
            let mut dl = vec![gdp_set_depth_image(Z)];
            image(&mut dl, Z, 8, 8, 0, 2);
            fill(&mut dl, 0x40004000, [0, 0, 7, 7]);
            image(&mut dl, FB, 8, 8, 0, 2);
            fill(&mut dl, 0x00010001, [0, 0, 7, 7]);
            mode(&mut dl, 0, G_ZS_PRIM | ZMODE_DEC | Z_CMP);
            dl.extend([
                combine(3, 3),
                gdp_set_prim_color(0, 0, 0xff0000ff),
                gdp_set_prim_depth(0x4000, 0),
            ]);
            for _ in 0..decals {
                dl.push(gdp_fill_rectangle(8, 8, 24, 24));
            }
            run(&mut r, &mut memory, built(DlBuilder::new(), dl));
            segments.push(recorder.snapshot().counters["workload.segments"]);
        }
        assert_eq!(segments[0], segments[1]);
    }

    #[test]
    fn pause_setup_writeback_and_ready_restore_saved_world() {
        let mut r = renderer(true);
        let setup = built(DlBuilder::new(), setup());
        let mut memory = Memory(vec![0; setup.rdram.len()]);
        run(&mut r, &mut memory, setup);
        for p in 0..320 * 240 {
            let expected = if p / 320 >= 234 {
                0x003f
            } else if p % 320 >= 160 {
                0x07c1
            } else {
                0xf801
            };
            assert_eq!(word(&memory, Z, p), expected, "saved pixel {p}");
        }
        assert!(memory.0[FB as usize..FB as usize + 320 * 240]
            .iter()
            .all(|&v| v == 0xff));
        let saved = memory.0[Z as usize..Z as usize + 320 * 240 * 2].to_vec();
        run(&mut r, &mut memory, built(DlBuilder::new(), ready()));
        assert_eq!(
            &memory.0[OUTPUT as usize..OUTPUT as usize + saved.len()],
            saved
        );
        assert_eq!(&memory.0[Z as usize..Z as usize + saved.len()], saved);
    }

    #[test]
    fn viscvg_writes_full_coverage_words() {
        let mut r = renderer(true);
        for cycle in [0, 1] {
            r.reset();
            let built = coverage_fill(cycle);
            let mut memory = Memory(vec![0; built.rdram.len()]);
            run(&mut r, &mut memory, built);
            for p in 0..32 {
                assert_eq!(word(&memory, FB, p), 0xffff);
            }
        }
    }

    #[test]
    fn primitive_depth_draw_kinds_compare_and_update_independently() {
        for fallback in [true, false] {
            let mut r = renderer(fallback);
            let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
            for kind in 0..6 {
                let decal = kind >= 4;
                for blend in [false, true] {
                    for flags in [0, Z_CMP, Z_UPD, Z_CMP | Z_UPD] {
                        for (z, passes, packed) in if decal {
                            [(0x6003, true, 0x4004), (0x2000, false, 0x1000)]
                        } else {
                            [(0x1000, true, 0x0800), (0x6000, false, 0x4000)]
                        } {
                            for high in [0, 0x8000] {
                                r.reset();
                                let mut b = DlBuilder::new();
                                let viewport = b.viewport(Vp {
                                    vscale: [16, 16, 511, 0],
                                    vtrans: [16, 16, 511, 0],
                                });
                                let vertices =
                                    b.vertices(&[[-1, -1], [1, -1], [1, 1], [-1, 1]].map(
                                        |[x, y]| VtxColored {
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
                                        },
                                    ));
                                let mut dl = vec![gdp_set_depth_image(Z)];
                                image(&mut dl, Z, 8, 8, 0, 2);
                                let initial = if decal { 0x4000 } else { 0x1000 };
                                fill(&mut dl, initial * 0x10001, [0, 0, 7, 7]);
                                image(&mut dl, FB, 8, 8, 0, 2);
                                fill(&mut dl, 0x00010001, [0, 0, 7, 7]);
                                let blender = if blend {
                                    FORCE_BL
                                        | gbl_c1(CLR_IN, A_IN, CLR_MEM, B_1MA)
                                        | gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA)
                                } else {
                                    G_RM_OPA_SURF | G_RM_OPA_SURF2
                                };
                                mode(
                                    &mut dl,
                                    u32::from(kind == 3),
                                    G_ZS_PRIM | flags | blender | if decal { ZMODE_DEC } else { 0 },
                                );
                                dl.extend([
                                    combine(3, 3),
                                    gdp_set_prim_color(0, 0, 0xff0000ff),
                                    gdp_set_prim_depth(z | high, 0xabcd),
                                ]);
                                match kind {
                                    0 | 4 => dl.extend([
                                        gsp_viewport(viewport),
                                        gsp_vertex(0, 4, vertices),
                                        gsp_1triangle(0, 1, 2),
                                    ]),
                                    1 | 5 => dl.extend(gsp_texture_rectangle(
                                        8, 8, 24, 24, 0, 0, 0, 1024, 1024, false,
                                    )),
                                    _ => dl.push(gdp_fill_rectangle(8, 8, 24, 24)),
                                }
                                run(&mut r, &mut memory, built(b, dl));
                                let visible = flags & Z_CMP == 0 || passes;
                                let label = format!("kind {kind}, flags {flags:x}, z {:x}, blend {blend}, fallback {fallback}", z|high);
                                assert_eq!(
                                    word(&memory, FB, 45),
                                    if visible { 0xf801 } else { 1 },
                                    "{label}"
                                );
                                assert_eq!(
                                    word(&memory, Z, 45),
                                    if visible && flags & Z_UPD != 0 {
                                        packed
                                    } else {
                                        initial as u16
                                    },
                                    "{label}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn depth_probe(dl: &mut Vec<Command>, width: u32, height: u32, z: u32) {
        image(dl, OUTPUT, width, height, 0, 2);
        fill(dl, 0x00010001, [0, 0, width - 1, height - 1]);
        mode(dl, 0, G_ZS_PRIM | Z_CMP | G_RM_OPA_SURF | G_RM_OPA_SURF2);
        dl.extend([
            combine(3, 3),
            gdp_set_prim_color(0, 0, 0x00ff00ff),
            gdp_set_prim_depth(z, 0),
            gdp_fill_rectangle(0, 0, width * 4, height * 4),
        ]);
    }

    #[test]
    fn alias_color_write_invalidates_depth_and_imports_latest_or_guest_bytes() {
        for offset in [0, 16] {
            for next_task in [false, true] {
                let mut r = renderer(true);
                let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
                let mut dl = vec![gdp_set_depth_image(Z)];
                image(&mut dl, Z, 8, 4, 0, 2);
                fill(&mut dl, 0, [0, 0, 7, 3]);
                image(&mut dl, Z + offset, 8, 4, 0, 2);
                mode(&mut dl, 0, G_RM_OPA_SURF | G_RM_OPA_SURF2);
                dl.extend([
                    combine(3, 3),
                    gdp_set_prim_color(0, 0, 0xffffffff),
                    gdp_fill_rectangle(0, 0, 32, 16),
                ]);
                if next_task {
                    run(&mut r, &mut memory, built(DlBuilder::new(), dl));
                    assert_eq!(word(&memory, Z + offset, 0), 0xffff);
                    memory.0[Z as usize + offset as usize..Z as usize + offset as usize + 2]
                        .copy_from_slice(&0u16.to_ne_bytes());
                    dl = vec![gdp_set_depth_image(Z)];
                }
                depth_probe(&mut dl, 8, 4, 0x4000);
                run(&mut r, &mut memory, built(DlBuilder::new(), dl));
                for p in 0..32 {
                    let overwritten = p >= offset as usize / 2;
                    let foreign = next_task && p == offset as usize / 2;
                    assert_eq!(
                        word(&memory, OUTPUT, p),
                        if overwritten && !foreign { 0x07c1 } else { 1 },
                        "offset {offset}, next_task {next_task}, pixel {p}"
                    );
                }
            }
        }
    }

    #[test]
    fn portrait_color_fill_before_zimg_preserves_literal_depth_words() {
        let mut r = renderer(true);
        let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
        let portrait_depth = FB + 64 * 112 * 2;
        let mut dl = vec![gdp_set_depth_image(Z)];
        image(&mut dl, portrait_depth, 64, 112, 0, 2);
        fill(&mut dl, 0xfffcfffc, [0, 0, 63, 111]);
        image(&mut dl, FB, 64, 112, 0, 2);
        fill(&mut dl, 0x00010001, [0, 0, 63, 111]);
        dl.push(gdp_set_depth_image(portrait_depth));
        depth_probe(&mut dl, 64, 112, 0x7fff);
        run(&mut r, &mut memory, built(DlBuilder::new(), dl));
        for p in 0..64 * 112 {
            assert_eq!(word(&memory, portrait_depth, p), 0xfffc);
            assert_eq!(word(&memory, OUTPUT, p), 0x07c1);
        }
        let (_, _, depth) = crate::tests::depth_persistence::depth_pixels(
            r.device(),
            r.queue(),
            &r.inner,
            portrait_depth.into(),
        );
        assert!(depth.iter().all(|&z| z == 262143.0 / 262144.0));
    }
}
