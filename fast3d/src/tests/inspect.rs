use std::ops::ControlFlow;

use crate::hle::{interpret, GbiUcode, InterpResult};
use crate::inspect::*;
use crate::{DataFormat, Microcode, Rdram, RdramImage};
use n64_gbi::encode::*;

impl<F: FnMut(WalkStep<'_>) -> ControlFlow<()>> WalkObserver for F {
    fn command(&mut self, step: WalkStep<'_>) -> ControlFlow<()> {
        self(step)
    }
}

pub(crate) fn equivalent<M: Rdram>(
    factory: impl Fn() -> M,
    entry: u64,
    ucode: GbiUcode,
    format: DataFormat,
) -> InterpResult {
    let ordinary = interpret(factory(), entry, ucode, format, None);
    let mut count = 0;
    let observed = interpret(
        factory(),
        entry,
        ucode,
        format,
        Some(&mut |step: WalkStep<'_>| {
            for &emission in step.emissions {
                assert_emission(&ordinary.scene, emission);
            }
            count += 1;
            ControlFlow::Continue(())
        }),
    );
    assert_eq!(count, ordinary.commands);
    assert_eq!(ordinary, observed);
    ordinary
}

pub(super) fn assert_emission(scene: &crate::scene::Scene, emission: Emission<'_>) {
    use crate::scene::SceneOp;
    match emission {
        Emission::Triangles {
            target,
            run_index,
            op_index,
            material_index,
            render_mode_index,
            index_start,
            indices,
        } => {
            let run = match (target, run_index, op_index) {
                (None, Some(index), None) => &scene.draw_runs[index as usize],
                (Some(target), None, Some(index)) => {
                    let SceneOp::Tris(run) =
                        &scene.framebuffer_pairs[target.pair_index].ops[index as usize]
                    else {
                        panic!("triangle emission must resolve to a triangle op");
                    };
                    run
                }
                _ => panic!("triangle must identify exactly one flat run or paired op"),
            };
            assert!(run.index_start <= index_start);
            assert!(index_start + indices.len() as u32 <= run.index_start + run.index_count);
            assert_eq!(
                &scene.indices[index_start as usize..index_start as usize + indices.len()],
                indices
            );
            assert_eq!(run.material_index, material_index);
            assert_eq!(run.render_mode_index, render_mode_index);
        }
        Emission::FillRect {
            target,
            op_index,
            rect,
            color_raw,
        } => {
            assert_eq!(
                scene.framebuffer_pairs[target.pair_index].ops[op_index as usize],
                SceneOp::FillRect { rect, color_raw }
            );
        }
        Emission::TexRect {
            target,
            op_index,
            rect,
            tile,
            uls,
            ult,
            dsdx,
            dtdy,
            flip,
            copy_mode,
            fb_source,
        } => {
            let SceneOp::TexRect {
                rect: actual_rect,
                tile: actual_tile,
                uls: actual_uls,
                ult: actual_ult,
                dsdx: actual_dsdx,
                dtdy: actual_dtdy,
                flip: actual_flip,
                copy_mode: actual_copy_mode,
                fb_source: actual_source,
                ..
            } = scene.framebuffer_pairs[target.pair_index].ops[op_index as usize]
            else {
                panic!("texture rectangle emission must resolve to a texture rectangle op");
            };
            assert_eq!(
                (rect, tile, uls, ult, dsdx, dtdy, flip, copy_mode, fb_source),
                (
                    actual_rect,
                    actual_tile,
                    actual_uls,
                    actual_ult,
                    actual_dsdx,
                    actual_dtdy,
                    actual_flip,
                    actual_copy_mode,
                    actual_source
                )
            );
        }
    }
}

fn bytes(words: impl IntoIterator<Item = (u32, u32)>) -> Vec<u8> {
    words
        .into_iter()
        .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
        .collect()
}

fn end(ucode: Microcode) -> (u32, u32) {
    match ucode {
        Microcode::F3dex2 => gsp_enddl(),
        Microcode::F3d => gsp_enddl_f3d(),
    }
}
fn dl(ucode: Microcode, target: u32, branch: bool) -> (u32, u32) {
    let opcode = match ucode {
        Microcode::F3dex2 => 0xde,
        Microcode::F3d => 0x06,
    };
    ((opcode << 24) | (u32::from(branch) << 16), target)
}

fn rectangle_words(ucode: Microcode, mut words: [(u32, u32); 3]) -> [(u32, u32); 3] {
    if ucode == Microcode::F3d {
        words[1].0 = u32::from(n64_gbi::consts::rsp_f3d::G_RDPHALF_1) << 24;
        words[2].0 = u32::from(n64_gbi::consts::rsp_f3d::G_RDPHALF_2) << 24;
    }
    words
}

#[test]
fn dl_past_rdram_discards_scene_and_observes_fault() {
    for ucode in [Microcode::F3dex2, Microcode::F3d] {
        let fault = gdp_load_tlut(7, 3);
        let memory = bytes([
            gdp_set_color_image(0, 2, 320, 0x100000),
            gdp_fill_rectangle(0, 0, 16, 16),
            gdp_set_tile(0, 2, 0, 0x100, 7, 0, 0, 0, 0, 0, 0, 0),
            gdp_set_texture_image(0, 2, 1, 0x1000),
            fault,
            gdp_set_env_color(0xff0000ff),
            end(ucode),
        ]);
        let ordinary = interpret(
            RdramImage::new(&memory),
            0,
            ucode.into(),
            DataFormat::Fixed,
            None,
        );
        assert_eq!(ordinary.termination, WalkTermination::Bounds);
        assert_eq!(ordinary.scene, crate::hle::rsp::Scene::default());
        assert_eq!(ordinary.commands, 5);
        assert_eq!(ordinary.dropped_runs, 1);
        assert_eq!(ordinary.rdp.env, [0; 4]);
        assert_eq!(ordinary.final_diagnostics_start, ordinary.diags.len());
        assert_eq!(
            ordinary.diags,
            [crate::Diagnostic {
                at: 32,
                kind: crate::DiagKind::DlPastRdram,
            }]
        );
        for stop in [false, true] {
            let mut count = 0;
            let mut diags = vec![];
            let observed = interpret(
                RdramImage::new(&memory),
                0,
                ucode.into(),
                DataFormat::Fixed,
                Some(&mut |step: WalkStep<'_>| {
                    count += 1;
                    assert_eq!(step.diagnostics_start, diags.len());
                    diags.extend_from_slice(step.diagnostics);
                    if step.seq == 1 {
                        assert!(matches!(step.emissions, [Emission::FillRect { .. }]));
                    }
                    if step.seq == 4 {
                        assert_eq!(step.pc, 32);
                        assert_eq!(step.flow, WalkFlow::Fault);
                        assert_eq!(step.next_pc, None);
                        assert_eq!(
                            step.words,
                            &[CommandWord {
                                pc: 32,
                                w0: fault.0,
                                w1: fault.1,
                                w1_addr: u64::from(fault.1),
                            }]
                        );
                        assert_eq!(step.diagnostics, ordinary.diags);
                        assert!(step.emissions.is_empty());
                        if stop {
                            return ControlFlow::Break(());
                        }
                    }
                    ControlFlow::Continue(())
                }),
            );
            assert_eq!(count, 5);
            assert_eq!(diags, observed.diags);
            assert_eq!(observed, ordinary);
        }
    }
}

#[test]
fn every_fixture_is_observationally_equivalent() {
    for fixture in super::fixtures::FIXTURES {
        let built = (fixture.build)(fixture);
        equivalent(
            || RdramImage::new(&built.rdram),
            built.entry as u64,
            GbiUcode::F3dex2,
            DataFormat::Fixed,
        );
    }
}

#[test]
fn fixed_rectangles_have_exact_dispatches_words_and_targets() {
    for ucode in [Microcode::F3dex2, Microcode::F3d] {
        for (depth, scissor) in [(false, false), (true, false), (false, true)] {
            let mut commands = vec![gdp_set_color_image(0, 2, 320, 0x100000)];
            if depth {
                commands.push(gdp_set_depth_image(0x200000));
            }
            if scissor {
                commands.push(gdp_set_scissor(0, 0, 0, 1280, 960));
            }
            commands.extend([
                gdp_set_fill_color(0xcafecafe),
                gdp_fill_rectangle(0, 0, 1280, 960),
                end(ucode),
            ]);
            let memory = bytes(commands.clone());
            equivalent(
                || RdramImage::new(&memory),
                0,
                ucode.into(),
                DataFormat::Fixed,
            );
            let mut fills = 0;
            let summary = walk(
                RdramImage::new(&memory),
                0,
                ucode,
                DataFormat::Fixed,
                &mut |step: WalkStep<'_>| {
                    assert_eq!(
                        step.words,
                        &[CommandWord {
                            pc: step.seq as u64 * 8,
                            w0: commands[step.seq as usize].0,
                            w1: commands[step.seq as usize].1,
                            w1_addr: commands[step.seq as usize].1 as u64
                        }]
                    );
                    if scissor && step.seq >= 1 {
                        assert_eq!(step.state.scissor.lrx, 320);
                    }
                    for emission in step.emissions {
                        let Emission::FillRect {
                            target,
                            rect,
                            color_raw,
                            ..
                        } = emission
                        else {
                            panic!("{emission:?}")
                        };
                        assert_eq!(step.seq as usize, commands.len() - 2);
                        assert_eq!((rect.ulx, rect.uly, rect.lrx, rect.lry), (0, 0, 320, 240));
                        assert_eq!(*color_raw, 0xcafecafe);
                        assert_eq!(target.color_image.addr, 0x100000);
                        assert_eq!(target.depth_image, depth.then_some(0x200000));
                        fills += 1;
                    }
                    ControlFlow::Continue(())
                },
            );
            assert_eq!(summary.dispatched as usize, commands.len());
            assert_eq!(fills, 1);
        }
        for flip in [false, true] {
            let rect = rectangle_words(
                ucode,
                gsp_texture_rectangle(0, 0, 1280, 960, 0, 44, 52, 1024, 512, flip),
            );
            let commands: Vec<_> = [gdp_set_color_image(0, 2, 320, 0x100000)]
                .into_iter()
                .chain(rect)
                .chain([end(ucode)])
                .collect();
            let memory = bytes(commands.clone());
            let mut rects = 0;
            let summary = walk(
                RdramImage::new(&memory),
                0,
                ucode,
                DataFormat::Fixed,
                &mut |step: WalkStep<'_>| {
                    if step.seq == 1 {
                        assert_eq!(step.words.len(), 3);
                        for (i, word) in step.words.iter().enumerate() {
                            assert_eq!(word.pc, (i as u64 + 1) * 8);
                            assert_eq!((word.w0, word.w1), commands[i + 1]);
                        }
                        assert_eq!(step.next_pc, Some(32));
                        let [Emission::TexRect {
                            rect,
                            flip: actual,
                            uls,
                            ult,
                            dsdx,
                            dtdy,
                            ..
                        }] = step.emissions
                        else {
                            panic!("{:?}", step.emissions)
                        };
                        assert_eq!((rect.ulx, rect.uly, rect.lrx, rect.lry), (0, 0, 1280, 960));
                        assert_eq!((*uls, *ult, *dsdx, *dtdy), (44, 52, 1024, 512));
                        assert_eq!(*actual, flip);
                        rects += 1;
                    }
                    if step.seq == 2 {
                        assert_eq!(step.pc, 32);
                        assert_eq!(step.flow, WalkFlow::End);
                    }
                    ControlFlow::Continue(())
                },
            );
            assert_eq!(summary.dispatched, 3);
            assert_eq!(rects, 1);
        }
    }
}

#[test]
fn calls_returns_branches_and_segmented_targets() {
    for ucode in [Microcode::F3dex2, Microcode::F3d] {
        let memory = bytes([
            dl(ucode, 24, false),
            dl(ucode, 24, false),
            end(ucode),
            gdp_set_fill_color(7),
            end(ucode),
        ]);
        let mut steps = vec![];
        let summary = walk(
            RdramImage::new(&memory),
            0,
            ucode,
            DataFormat::Fixed,
            &mut |s: WalkStep<'_>| {
                steps.push((
                    s.seq,
                    s.pc,
                    s.depth_before,
                    s.depth_after,
                    s.flow,
                    s.next_pc,
                ));
                ControlFlow::Continue(())
            },
        );
        assert_eq!(summary.dispatched, 7);
        assert_eq!(
            steps,
            vec![
                (0, 0, 0, 1, WalkFlow::Call, Some(24)),
                (1, 24, 1, 1, WalkFlow::Next, Some(32)),
                (2, 32, 1, 0, WalkFlow::Return, Some(8)),
                (3, 8, 0, 1, WalkFlow::Call, Some(24)),
                (4, 24, 1, 1, WalkFlow::Next, Some(32)),
                (5, 32, 1, 0, WalkFlow::Return, Some(16)),
                (6, 16, 0, 0, WalkFlow::End, None)
            ]
        );
        let memory = bytes([dl(ucode, 0x01000000, true), end(ucode)]);
        let mut reader = RdramImage::new(&memory);
        reader.set_segment(1, 8);
        let mut pcs = vec![];
        walk(reader, 0, ucode, DataFormat::Fixed, &mut |s: WalkStep<
            '_,
        >| {
            pcs.push(s.pc);
            if s.seq == 0 {
                assert_eq!(s.flow, WalkFlow::Branch);
                assert_eq!(s.next_pc, Some(8));
            }
            ControlFlow::Continue(())
        });
        assert_eq!(pcs, [0, 8]);
        let mut count = 0;
        let summary = walk(
            RdramImage::new(&memory),
            0,
            ucode,
            DataFormat::Fixed,
            &mut |_: WalkStep<'_>| {
                count += 1;
                ControlFlow::Break(())
            },
        );
        assert_eq!(count, 1);
        assert_eq!(summary.termination, WalkTermination::ObserverStopped);
    }
}

#[test]
fn faults_keep_successfully_read_words_and_terminal_diagnostics() {
    for ucode in [Microcode::F3dex2, Microcode::F3d] {
        let mut count = 0;
        let summary = walk(
            RdramImage::new(&[]),
            0,
            ucode,
            DataFormat::Fixed,
            &mut |_: WalkStep<'_>| {
                count += 1;
                ControlFlow::Continue(())
            },
        );
        assert_eq!(count, 0);
        assert_eq!(summary.dispatched, 0);
        assert_eq!(summary.termination, WalkTermination::Bounds);
        assert_eq!(summary.final_diagnostics_start, 0);
        for words in 1..=2 {
            let rect = rectangle_words(
                ucode,
                gsp_texture_rectangle(0, 0, 1280, 960, 0, 0, 0, 1024, 1024, false),
            );
            let memory = bytes(
                [gdp_set_color_image(0, 2, 320, 0x100000)]
                    .into_iter()
                    .chain(rect.into_iter().take(words)),
            );
            let mut diags = vec![];
            let summary = walk(
                RdramImage::new(&memory),
                0,
                ucode,
                DataFormat::Fixed,
                &mut |s: WalkStep<'_>| {
                    assert_eq!(s.diagnostics_start, diags.len());
                    diags.extend_from_slice(s.diagnostics);
                    if s.seq == 1 {
                        assert_eq!(s.words.len(), words);
                        assert_eq!(s.flow, WalkFlow::Fault);
                        assert_eq!(s.next_pc, None);
                        assert!(s.emissions.is_empty());
                    }
                    ControlFlow::Continue(())
                },
            );
            assert_eq!(summary.termination, WalkTermination::Bounds);
            assert_eq!(summary.dispatched, 2);
            assert_eq!(
                summary.diagnostics,
                vec![crate::Diagnostic {
                    at: 8,
                    kind: crate::DiagKind::TruncatedRect { fill: false }
                }]
            );
            assert_eq!(
                diags,
                summary.diagnostics[..summary.final_diagnostics_start]
            );
        }
        let memory = bytes([gdp_fill_rectangle(0, 0, 1280, 960), end(ucode)]);
        let summary = walk(
            RdramImage::new(&memory),
            0,
            ucode,
            DataFormat::Fixed,
            &mut |s: WalkStep<'_>| {
                assert!(s.emissions.is_empty());
                if s.seq == 0 {
                    assert_eq!(s.flow, WalkFlow::Next);
                }
                ControlFlow::Continue(())
            },
        );
        assert_eq!(summary.dispatched, 2);
        assert_eq!(summary.termination, WalkTermination::End);
        let memory = bytes([(0x80000000, 0), (0x80000000, 0), end(ucode)]);
        let summary = walk(
            RdramImage::new(&memory),
            0,
            ucode,
            DataFormat::Fixed,
            &mut |s: WalkStep<'_>| {
                if s.seq < 2 {
                    assert_eq!(s.flow, WalkFlow::Next);
                }
                ControlFlow::Continue(())
            },
        );
        assert_eq!(summary.diagnostics.len(), 1);
    }
}

#[test]
fn cap_and_cancellation_precedence() {
    for ucode in [Microcode::F3dex2, Microcode::F3d] {
        for (last, expected, stop) in [
            (end(ucode), WalkTermination::End, false),
            (gdp_set_fill_color(3), WalkTermination::Cap, false),
            ((0xe4000000, 0), WalkTermination::Bounds, false),
            (
                gdp_set_fill_color(3),
                WalkTermination::ObserverStopped,
                true,
            ),
        ] {
            let mut commands = vec![gdp_set_fill_color(1); MAX_DISPATCHES as usize - 1];
            commands.push(last);
            if expected != WalkTermination::Bounds {
                commands.push(end(ucode));
            }
            let memory = bytes(commands);
            let mut count = 0;
            let reads = std::cell::RefCell::new(vec![]);
            let summary = walk(
                CountingReader {
                    inner: RdramImage::new(&memory),
                    reads: &reads,
                    wide: false,
                },
                0,
                ucode,
                DataFormat::Fixed,
                &mut |s: WalkStep<'_>| {
                    count += 1;
                    if stop && s.seq + 1 == MAX_DISPATCHES {
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    }
                },
            );
            assert_eq!(count, MAX_DISPATCHES);
            assert_eq!(summary.dispatched, MAX_DISPATCHES);
            assert_eq!(summary.termination, expected);
            assert_eq!(
                reads
                    .borrow()
                    .iter()
                    .filter(|(kind, _, _)| *kind == "command")
                    .count(),
                MAX_DISPATCHES as usize
            );
        }
        let memory = bytes([dl(ucode, 0, true)]);
        let reads = std::cell::RefCell::new(vec![]);
        let summary = walk(
            CountingReader {
                inner: RdramImage::new(&memory),
                reads: &reads,
                wide: false,
            },
            0,
            ucode,
            DataFormat::Fixed,
            &mut |_: WalkStep<'_>| ControlFlow::Continue(()),
        );
        assert_eq!(summary.dispatched, MAX_DISPATCHES);
        assert_eq!(summary.termination, WalkTermination::Cap);
        assert!(summary.diagnostics.is_empty());
        assert_eq!(
            reads
                .borrow()
                .iter()
                .filter(|(kind, _, _)| *kind == "command")
                .count(),
            MAX_DISPATCHES as usize
        );
        let memory = bytes([gdp_set_fill_color(1), gdp_set_fill_color(2), end(ucode)]);
        let summary = walk(
            RdramImage::new(&memory),
            0,
            ucode,
            DataFormat::Fixed,
            &mut |s: WalkStep<'_>| {
                assert_eq!(s.state.fill_color_raw, s.seq + 1);
                if s.seq == 1 {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        );
        assert_eq!(summary.dispatched, 2);
        assert_eq!(summary.termination, WalkTermination::ObserverStopped);
    }
}

#[test]
fn triangles_report_index_growth_even_when_runs_coalesce() {
    use crate::hle::consts::{G_RM_OPA_SURF, G_RM_OPA_SURF2};
    for ucode in [Microcode::F3dex2, Microcode::F3d] {
        for paired in [false, true] {
            for doubled in [false, true] {
                for cull in [0, 1, 2] {
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
                    let mut commands = vec![];
                    if paired {
                        commands.push(gdp_set_color_image(0, 2, 320, 0x100000));
                    }
                    if cull != 0 {
                        let mask = match ucode {
                            Microcode::F3dex2 => {
                                if cull == 1 {
                                    0x200
                                } else {
                                    0x600
                                }
                            }
                            Microcode::F3d => {
                                if cull == 1 {
                                    0x1000
                                } else {
                                    0x3000
                                }
                            }
                        };
                        commands.push(match ucode {
                            Microcode::F3dex2 => gsp_set_geometrymode(mask),
                            Microcode::F3d => gsp_set_geometrymode_f3d(mask),
                        });
                    }
                    commands.push(gdp_set_combine_lerp(cc, ca, cc, ca));
                    commands.push(match ucode {
                        Microcode::F3dex2 => gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
                        Microcode::F3d => gdp_set_render_mode_f3d(G_RM_OPA_SURF, G_RM_OPA_SURF2),
                    });
                    commands.push(match ucode {
                        Microcode::F3dex2 => gsp_vertex(0, 4, 0x100),
                        Microcode::F3d => gsp_vertex_f3d(0, 4, 0x100),
                    });
                    if doubled {
                        commands.push(match ucode {
                            Microcode::F3dex2 => gsp_2triangles(0, 1, 2, 0, 2, 3),
                            Microcode::F3d => gsp_quad_f3d(0, 1, 2, 3),
                        });
                    } else {
                        commands.extend(match ucode {
                            Microcode::F3dex2 => [gsp_1triangle(0, 1, 2), gsp_1triangle(0, 2, 3)],
                            Microcode::F3d => {
                                [gsp_1triangle_f3d(0, 1, 2), gsp_1triangle_f3d(0, 2, 3)]
                            }
                        });
                    }
                    commands.push(end(ucode));
                    let dispatches = commands.len();
                    let mut memory = bytes(commands);
                    memory.resize(0x140, 0);
                    for (i, (x, y)) in [(0, 0), (20, 0), (20, 20), (0, 20)].into_iter().enumerate()
                    {
                        let vertex = VtxColored {
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
                        };
                        memory[0x100 + i * 16..0x110 + i * 16].copy_from_slice(&vertex.to_bytes());
                    }
                    let mut indices = vec![];
                    let mut emission_lengths = vec![];
                    let result = interpret(
                        RdramImage::new(&memory),
                        0,
                        ucode.into(),
                        DataFormat::Fixed,
                        Some(&mut |s: WalkStep<'_>| {
                            for e in s.emissions {
                                let Emission::Triangles {
                                    target,
                                    index_start,
                                    indices: new,
                                    ..
                                } = e
                                else {
                                    panic!("{e:?}")
                                };
                                assert_eq!(*index_start as usize, indices.len());
                                assert_eq!(target.is_some(), paired);
                                assert_eq!(
                                    s.seq as usize,
                                    dispatches - 1 - (if doubled { 1 } else { 2 })
                                        + emission_lengths.len()
                                );
                                emission_lengths.push(new.len());
                                indices.extend_from_slice(new);
                            }
                            ControlFlow::Continue(())
                        }),
                    );
                    assert_eq!(result.commands as usize, dispatches);
                    assert_eq!(indices, result.scene.indices);
                    if cull == 2 {
                        assert!(indices.is_empty());
                    } else {
                        assert_eq!(emission_lengths, if doubled { vec![6] } else { vec![3, 3] });
                        assert_eq!(
                            indices,
                            if cull == 1 {
                                vec![2, 1, 0, 3, 2, 0]
                            } else {
                                vec![0, 1, 2, 0, 2, 3]
                            }
                        );
                        if !paired {
                            assert_eq!(result.scene.draw_runs.len(), 1);
                        } else {
                            assert_eq!(result.scene.framebuffer_pairs[0].ops.len(), 1);
                            assert!(
                                matches!(&result.scene.framebuffer_pairs[0].ops[0],crate::hle::SceneOp::Tris(run) if run.index_count==6)
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn float_rectangles_consume_continuations_and_preserve_sentinel() {
    for tex in [false, true] {
        let mut commands = vec![
            gdp_set_color_image(0, 2, 320, 0x100000),
            gdp_set_fill_color(0x12345678),
        ];
        commands.push((if tex { 0xe4000500 } else { 0xf6000500 }, 960));
        commands.push((0xe1000000, if tex { 0x000b000d } else { 0 }));
        if tex {
            commands.push((0xf1000000, 0x04000200));
        }
        commands.extend([gdp_set_prim_color(0, 0, 0x1234abcd), gsp_enddl()]);
        let memory = bytes(commands.clone());
        equivalent(
            || RdramImage::new(&memory),
            0,
            GbiUcode::F3dex2,
            DataFormat::Float,
        );
        let summary = walk(
            RdramImage::new(&memory),
            0,
            Microcode::F3dex2,
            DataFormat::Float,
            &mut |s: WalkStep<'_>| {
                if s.seq == 2 {
                    assert_eq!(s.words.len(), if tex { 3 } else { 2 });
                    assert_eq!(s.emissions.len(), 1);
                    match s.emissions[0] {
                        Emission::FillRect {
                            rect,
                            color_raw,
                            target,
                            ..
                        } => {
                            assert!(!tex);
                            assert_eq!((rect.ulx, rect.uly, rect.lrx, rect.lry), (0, 0, 320, 240));
                            assert_eq!(color_raw, 0x12345678);
                            assert_eq!(target.color_image.addr, 0x100000);
                        }
                        Emission::TexRect {
                            rect,
                            tile,
                            uls,
                            ult,
                            dsdx,
                            dtdy,
                            flip,
                            copy_mode,
                            fb_source,
                            ..
                        } => {
                            assert!(tex);
                            assert_eq!((rect.ulx, rect.uly, rect.lrx, rect.lry), (0, 0, 1280, 960));
                            assert_eq!((tile, uls, ult, dsdx, dtdy), (0, 11, 13, 1024, 512));
                            assert!(!flip);
                            assert!(!copy_mode);
                            assert_eq!(fb_source, None);
                        }
                        other => panic!("{other:?}"),
                    }
                    for (i, w) in s.words.iter().enumerate() {
                        assert_eq!(w.pc, (i as u64 + 2) * 8);
                        assert_eq!((w.w0, w.w1), commands[i + 2]);
                    }
                }
                if s.seq == 3 {
                    assert_eq!(s.state.prim_color, [0x12, 0x34, 0xab, 0xcd]);
                }
                ControlFlow::Continue(())
            },
        );
        assert_eq!(summary.dispatched, 5);
    }
    let memory = bytes([(0xf6000500, 960)]);
    let summary = walk(
        RdramImage::new(&memory),
        0,
        Microcode::F3dex2,
        DataFormat::Float,
        &mut |s: WalkStep<'_>| {
            assert_eq!(s.words.len(), 1);
            assert_eq!(s.flow, WalkFlow::Fault);
            assert!(s.emissions.is_empty());
            ControlFlow::Continue(())
        },
    );
    assert_eq!(summary.termination, WalkTermination::Bounds);
}

struct CountingReader<'a> {
    inner: RdramImage<'a>,
    reads: &'a std::cell::RefCell<Vec<(&'static str, u64, usize)>>,
    wide: bool,
}
impl Rdram for CountingReader<'_> {
    fn set_segment(&mut self, s: u32, v: u64) {
        Rdram::set_segment(&mut self.inner, s, v);
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
        self.reads.borrow_mut().push(("bounds", a, n as usize));
        self.inner.in_bounds(a, n)
    }
    fn read_command(&self, a: u64) -> crate::hle::mem::Command {
        self.reads.borrow_mut().push(("command", a, 8));
        let mut c = self.inner.read_command(a);
        if self.wide && c.w0 >> 24 == 0xff {
            c.w1_addr = 0x1234_5678_0010_0000;
        }
        c
    }
    fn read_u8(&self, a: u64) -> u8 {
        self.reads.borrow_mut().push(("u8", a, 1));
        Rdram::read_u8(&self.inner, a)
    }
    fn read_i8(&self, a: u64) -> i8 {
        self.reads.borrow_mut().push(("i8", a, 1));
        Rdram::read_i8(&self.inner, a)
    }
    fn read_i16(&self, a: u64) -> i16 {
        self.reads.borrow_mut().push(("i16", a, 2));
        Rdram::read_i16(&self.inner, a)
    }
    fn read_u16(&self, a: u64) -> u16 {
        self.reads.borrow_mut().push(("u16", a, 2));
        Rdram::read_u16(&self.inner, a)
    }
    fn read_bytes(&self, a: u64, n: usize) -> std::borrow::Cow<'_, [u8]> {
        self.reads.borrow_mut().push(("bytes", a, n));
        self.inner.read_bytes(a, n)
    }
    fn read_matrix(&self, a: u64, f: DataFormat) -> Matrix4 {
        self.reads.borrow_mut().push(("matrix", a, 64));
        Rdram::read_matrix(&self.inner, a, f)
    }
}

#[test]
fn observation_does_not_add_reads_or_truncate_operands() {
    use std::cell::RefCell;
    let memory = bytes(
        [gdp_set_color_image(0, 2, 320, 0x100000)]
            .into_iter()
            .chain(gsp_texture_rectangle(
                0, 0, 1280, 960, 0, 0, 0, 1024, 1024, false,
            ))
            .chain([gsp_enddl()]),
    );
    let ordinary_reads = RefCell::new(vec![]);
    let observed_reads = RefCell::new(vec![]);
    let ordinary = interpret(
        CountingReader {
            inner: RdramImage::new(&memory),
            reads: &ordinary_reads,
            wide: true,
        },
        0,
        GbiUcode::F3dex2,
        DataFormat::Fixed,
        None,
    );
    let observed = interpret(
        CountingReader {
            inner: RdramImage::new(&memory),
            reads: &observed_reads,
            wide: true,
        },
        0,
        GbiUcode::F3dex2,
        DataFormat::Fixed,
        Some(&mut |s: WalkStep<'_>| {
            if s.seq == 0 {
                assert_eq!(s.words[0].w1_addr, 0x1234_5678_0010_0000);
            }
            ControlFlow::Continue(())
        }),
    );
    assert_eq!(ordinary, observed);
    assert_eq!(ordinary_reads, observed_reads);
    for stop in [2, MAX_DISPATCHES] {
        let memory = bytes(std::iter::repeat_n(
            gdp_set_fill_color(1),
            stop as usize + 1,
        ));
        let reads = RefCell::new(vec![]);
        let summary = walk(
            CountingReader {
                inner: RdramImage::new(&memory),
                reads: &reads,
                wide: false,
            },
            0,
            Microcode::F3dex2,
            DataFormat::Fixed,
            &mut |s: WalkStep<'_>| {
                if stop == 2 && s.seq == 1 {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        );
        assert_eq!(summary.dispatched, stop);
        assert_eq!(
            reads
                .borrow()
                .iter()
                .filter(|(kind, _, _)| *kind == "command")
                .count(),
            stop as usize
        );
        assert!(reads
            .borrow()
            .iter()
            .all(|(_, a, _)| *a < (stop as u64) * 8));
    }
}

#[test]
fn post_command_projection_preserves_values_and_owned_snapshots() {
    for ucode in [Microcode::F3dex2, Microcode::F3d] {
        let geom = match ucode {
            Microcode::F3dex2 => gsp_set_geometrymode(0x200),
            Microcode::F3d => gsp_set_geometrymode_f3d(0x200),
        };
        let texture = match ucode {
            Microcode::F3dex2 => gsp_texture(123, 456, 2, 3, true),
            Microcode::F3d => gsp_texture_f3d(123, 456, 2, 3, true),
        };
        let commands = [
            geom,
            texture,
            gdp_set_texture_image(0, 2, 320, 0x180),
            gdp_set_prim_color(0, 0, 0x11223344),
            gdp_set_env_color(0x55667788),
            gdp_set_fog_color(0x99aabbcc),
            gdp_set_blend_color(0xddeeff12),
            gdp_set_fill_color(0xabcdef01),
            gdp_set_color_image(0, 2, 640, 0x100000),
            gdp_set_depth_image(0x200000),
            gdp_set_scissor(1, 4, 8, 1200, 800),
            (0xfc123456, 0x12345678),
            (0xef123456, 0x87654321),
            gdp_set_tile(3, 1, 8, 16, 3, 4, 2, 5, 6, 1, 7, 8),
            gdp_set_tile_size(3, 4, 8, 64, 128),
            end(ucode),
        ];
        let memory = bytes(commands);
        let mut early = None;
        walk(
            RdramImage::new(&memory),
            0,
            ucode,
            DataFormat::Fixed,
            &mut |s: WalkStep<'_>| {
                if s.seq == 0 {
                    let name = match ucode {
                        Microcode::F3dex2 => "G_CULL_FRONT",
                        Microcode::F3d => "G_SHADING_SMOOTH",
                    };
                    assert!(s
                        .state
                        .geometry_names
                        .iter()
                        .any(|f| f.mask == 0x200 && f.name == name));
                    assert!(s
                        .state
                        .geometry_names
                        .windows(2)
                        .all(|p| p[0].mask < p[1].mask));
                    assert!(s
                        .state
                        .geometry_names
                        .iter()
                        .all(|f| f.mask.is_power_of_two()));
                    early = Some((
                        s.state.prim_color,
                        s.state.tiles.clone(),
                        *s.state.modelview,
                    ));
                }
                if s.flow == WalkFlow::End {
                    let v = s.state;
                    assert_eq!(
                        v.texture,
                        TextureState {
                            tile: 3,
                            level: 2,
                            on: true,
                            sc: 123,
                            tc: 456
                        }
                    );
                    assert_eq!(
                        v.texture_image,
                        ColorImage {
                            fmt: 0,
                            siz: 2,
                            width: 320,
                            addr: 0x180
                        }
                    );
                    assert_eq!(v.prim_color, [0x11, 0x22, 0x33, 0x44]);
                    assert_eq!(v.env_color, [0x55, 0x66, 0x77, 0x88]);
                    assert_eq!(v.fog_color, [0x99, 0xaa, 0xbb, 0xcc]);
                    assert_eq!(v.blend_color, [0xdd, 0xee, 0xff, 0x12]);
                    assert_eq!(v.fill_color_raw, 0xabcdef01);
                    assert_eq!(
                        v.color_image,
                        ColorImage {
                            fmt: 0,
                            siz: 2,
                            width: 640,
                            addr: 0x100000
                        }
                    );
                    assert_eq!(v.depth_image, 0x200000);
                    assert_eq!(
                        v.scissor,
                        Scissor {
                            ulx: 1,
                            uly: 2,
                            lrx: 300,
                            lry: 200,
                            mode: 1
                        }
                    );
                    assert_eq!((v.combine_l, v.combine_h), (0xfc123456, 0x12345678));
                    assert_eq!((v.other_mode_h, v.other_mode_l), (0x123456, 0x87654321));
                    let t = &v.tiles[3];
                    assert_eq!(
                        (t.fmt, t.siz, t.line, t.tmem_addr, t.palette),
                        (3, 1, 8, 16, 4)
                    );
                    assert_eq!(
                        (t.cms, t.cmt, t.masks, t.maskt, t.shifts, t.shiftt),
                        (1, 2, 7, 5, 8, 6)
                    );
                    assert_eq!((t.uls, t.ult, t.lrs, t.lrt), (4, 8, 64, 128));
                    assert_eq!((t.width, t.height), (16, 31));
                    assert!(!v.load_via_tile);
                    let saved = early.as_ref().unwrap();
                    assert_ne!(saved.0, v.prim_color);
                    assert_ne!(saved.1, *v.tiles);
                    assert_eq!(saved.2, *v.modelview);
                }
                ControlFlow::Continue(())
            },
        );
    }
}

#[test]
fn matrix_viewport_light_and_lookat_projection_uses_decoded_units() {
    let model = [
        [1., 0., 0., 0.],
        [0., 2., 0., 0.],
        [0., 0., 3., 0.],
        [4., 5., 6., 1.],
    ];
    let projection = [
        [2., 0., 0., 0.],
        [0., 3., 0., 0.],
        [0., 0., 4., 0.],
        [7., 8., 9., 1.],
    ];
    let commands = [
        gsp_matrix_f3d(0x200, false, true, true),
        gsp_matrix_f3d(0x240, true, true, false),
        gsp_viewport_f3d(0x280),
        gsp_numlights_f3d(1),
        gsp_light_f3d(0, 0x290),
        gsp_light_f3d(1, 0x2a0),
        gsp_lookat_f3d(0, 0x2b0),
        gsp_lookat_f3d(1, 0x2c0),
        gsp_enddl_f3d(),
    ];
    let mut memory = bytes(commands);
    memory.resize(0x300, 0);
    memory[0x200..0x240].copy_from_slice(&mtx_to_bytes(model));
    memory[0x240..0x280].copy_from_slice(&mtx_to_bytes(projection));
    memory[0x280..0x290].copy_from_slice(
        &Vp {
            vscale: [400, 800, 256, 0],
            vtrans: [40, 80, 512, 0],
        }
        .to_bytes(),
    );
    memory[0x290..0x293].copy_from_slice(&[255, 128, 64]);
    memory[0x298..0x29b].copy_from_slice(&[127, 0, 0]);
    memory[0x2a0..0x2a3].copy_from_slice(&[32, 16, 8]);
    memory[0x2b8..0x2bb].copy_from_slice(&[0, 127, 0]);
    memory[0x2c8..0x2cb].copy_from_slice(&[0, 0, 127]);
    walk(
        RdramImage::new(&memory),
        0,
        Microcode::F3d,
        DataFormat::Fixed,
        &mut |s: WalkStep<'_>| {
            if s.flow == WalkFlow::End {
                let v = s.state;
                assert_eq!(v.modelview_depth, 2);
                assert_eq!(*v.modelview, model);
                assert_eq!(*v.projection, projection);
                assert_eq!(v.viewport_scale, [100., 200., 0.25]);
                assert_eq!(v.viewport_translation, [10., 20., 0.5]);
                assert_eq!(v.light_count, 1);
                assert_eq!(v.lights[0], ([1., 0., 0.], [1., 128. / 255., 64. / 255.]));
                assert_eq!(v.ambient, [32. / 255., 16. / 255., 8. / 255.]);
                assert_eq!(*v.lookat_axes, [[0., 1., 0.], [0., 0., 1.]]);
            }
            ControlFlow::Continue(())
        },
    );
}

#[test]
fn rejected_draws_and_final_warnings_have_distinct_diagnostic_slices() {
    use crate::DiagKind;
    for ucode in [Microcode::F3dex2, Microcode::F3d] {
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
        for rejected in [false, true] {
            let combine = if rejected {
                gdp_set_combine_lerp(CcPass { d: 1, ..cc }, ca, CcPass { d: 1, ..cc }, ca)
            } else {
                gdp_set_combine_lerp(cc, ca, cc, ca)
            };
            let vtx = match ucode {
                Microcode::F3dex2 => gsp_vertex(0, 3, 0x100),
                Microcode::F3d => gsp_vertex_f3d(0, 3, 0x100),
            };
            let tri = match ucode {
                Microcode::F3dex2 => gsp_1triangle(0, 1, 2),
                Microcode::F3d => gsp_1triangle_f3d(0, 1, 2),
            };
            let mut memory = bytes([combine, vtx, tri, end(ucode)]);
            memory.resize(0x130, 0);
            let mut step_diagnostics = vec![];
            let summary = walk(
                RdramImage::new(&memory),
                0,
                ucode,
                DataFormat::Fixed,
                &mut |s: WalkStep<'_>| {
                    assert_eq!(s.diagnostics_start, step_diagnostics.len());
                    step_diagnostics.extend_from_slice(s.diagnostics);
                    if s.seq == 2 {
                        assert_eq!(s.flow, WalkFlow::Next);
                        assert_eq!(s.emissions.is_empty(), rejected);
                    }
                    assert!(s
                        .diagnostics
                        .iter()
                        .all(|d| d.kind != DiagKind::RenderModeNeverSet));
                    ControlFlow::Continue(())
                },
            );
            assert_eq!(
                summary.diagnostics[..summary.final_diagnostics_start],
                step_diagnostics
            );
            if rejected {
                assert!(summary
                    .diagnostics
                    .iter()
                    .any(|d| d.kind == DiagKind::NoTextureLoaded));
                assert_eq!(summary.tris, 0);
            } else {
                assert_eq!(summary.tris, 1);
                assert_eq!(
                    summary.diagnostics[summary.final_diagnostics_start..],
                    [crate::Diagnostic {
                        at: 24,
                        kind: DiagKind::RenderModeNeverSet
                    }]
                );
            }
        }
        for before_cimg in [false, true] {
            let mut commands = vec![];
            if !before_cimg {
                commands.push(gdp_set_color_image(0, 2, 320, 0x100000));
                let cc = CcPass { d: 1, ..cc };
                commands.push(gdp_set_combine_lerp(cc, ca, cc, ca));
            }
            commands.extend(rectangle_words(
                ucode,
                gsp_texture_rectangle(0, 0, 1280, 960, 0, 0, 0, 1024, 1024, false),
            ));
            commands.push(end(ucode));
            let memory = bytes(commands);
            let summary = walk(
                RdramImage::new(&memory),
                0,
                ucode,
                DataFormat::Fixed,
                &mut |s: WalkStep<'_>| {
                    assert!(s.emissions.is_empty());
                    if s.words.len() == 3 {
                        assert_eq!(s.flow, WalkFlow::Next);
                    }
                    ControlFlow::Continue(())
                },
            );
            assert_eq!(summary.termination, WalkTermination::End);
            let kind = if before_cimg {
                DiagKind::DrawBeforeCimg
            } else {
                DiagKind::NoTextureLoaded
            };
            assert_eq!(
                summary
                    .diagnostics
                    .iter()
                    .filter(|d| d.kind == kind)
                    .count(),
                1
            );
        }
    }
}

#[test]
fn cancellation_on_call_never_fetches_target() {
    for ucode in [Microcode::F3dex2, Microcode::F3d] {
        let memory = bytes([
            dl(ucode, 16, false),
            end(ucode),
            gdp_set_fill_color(7),
            end(ucode),
        ]);
        let reads = std::cell::RefCell::new(vec![]);
        let summary = walk(
            CountingReader {
                inner: RdramImage::new(&memory),
                reads: &reads,
                wide: false,
            },
            0,
            ucode,
            DataFormat::Fixed,
            &mut |s: WalkStep<'_>| {
                assert_eq!(s.flow, WalkFlow::Call);
                assert_eq!(s.depth_after, 1);
                assert_eq!(s.next_pc, Some(16));
                ControlFlow::Break(())
            },
        );
        assert_eq!(summary.dispatched, 1);
        assert_eq!(summary.termination, WalkTermination::ObserverStopped);
        assert!(reads.borrow().iter().all(|(_, a, _)| *a == 0));
    }
}

#[test]
fn load_tile_and_f3d_only_geometry_flags_are_projected() {
    for ucode in [Microcode::F3dex2, Microcode::F3d] {
        let geom = match ucode {
            Microcode::F3dex2 => gsp_set_geometrymode(0x80000200),
            Microcode::F3d => gsp_set_geometrymode_f3d(0x80400202),
        };
        let mut memory = bytes([
            geom,
            gdp_set_texture_image(0, 2, 4, 0x100),
            gdp_set_tile(0, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0),
            (0xf4000000, 0x0000c00c),
            end(ucode),
        ]);
        memory.resize(0x120, 0xff);
        walk(
            RdramImage::new(&memory),
            0,
            ucode,
            DataFormat::Fixed,
            &mut |s: WalkStep<'_>| {
                assert_ne!(s.state.geometry_mode & 0x80000000, 0);
                assert!(s
                    .state
                    .geometry_names
                    .iter()
                    .all(|f| f.name != "G_CULL_BOTH"));
                if ucode == Microcode::F3d {
                    for name in ["G_TEXTURE_ENABLE", "G_POINT_LIGHTING"] {
                        assert!(s.state.geometry_names.iter().any(|f| f.name == name));
                    }
                }
                if s.seq >= 3 {
                    assert!(s.state.load_via_tile);
                }
                ControlFlow::Continue(())
            },
        );
    }
}

#[test]
fn internal_observation_is_uncapped_and_keeps_configured_segments() {
    let memory = bytes(
        std::iter::repeat_n(gdp_set_fill_color(1), MAX_DISPATCHES as usize).chain([gsp_enddl()]),
    );
    let result = equivalent(
        || RdramImage::new(&memory),
        0,
        GbiUcode::F3dex2,
        DataFormat::Fixed,
    );
    assert_eq!(result.commands, MAX_DISPATCHES + 1);
    assert_eq!(result.termination, WalkTermination::End);
    for ucode in [Microcode::F3dex2, Microcode::F3d] {
        let memory = bytes([dl(ucode, 0x01000000, true), end(ucode)]);
        let result = equivalent(
            || {
                let mut mem = RdramImage::new(&memory);
                mem.set_segment(1, 8);
                mem
            },
            0,
            ucode.into(),
            DataFormat::Fixed,
        );
        assert_eq!(result.commands, 2);
        assert_eq!(result.termination, WalkTermination::End);
    }
}

#[test]
fn texture_rectangle_keeps_prior_framebuffer_source_and_copy_mode() {
    for ucode in [Microcode::F3dex2, Microcode::F3d] {
        let commands = [
            gdp_set_color_image(0, 2, 320, 0x100000),
            gdp_set_scissor(0, 0, 0, 1280, 960),
            gdp_set_fill_color(0x12345678),
            gdp_fill_rectangle(0, 0, 1280, 960),
            gdp_set_color_image(0, 2, 320, 0x200000),
            gdp_set_texture_image(0, 2, 320, 0x100000),
            (0xef200000, 0),
        ];
        let memory = bytes(
            commands
                .into_iter()
                .chain(rectangle_words(
                    ucode,
                    gsp_texture_rectangle(0, 0, 1280, 960, 0, 0, 0, 4096, 1024, false),
                ))
                .chain([end(ucode)]),
        );
        let mut rects = 0;
        let summary = walk(
            RdramImage::new(&memory),
            0,
            ucode,
            DataFormat::Fixed,
            &mut |s: WalkStep<'_>| {
                for emission in s.emissions {
                    if let Emission::TexRect {
                        target,
                        fb_source,
                        copy_mode,
                        ..
                    } = emission
                    {
                        assert_eq!(s.seq, 7);
                        assert_eq!(target.pair_index, 1);
                        assert_eq!(target.color_image.addr, 0x200000);
                        assert_eq!(*fb_source, Some(0x100000));
                        assert!(*copy_mode);
                        rects += 1;
                    }
                }
                ControlFlow::Continue(())
            },
        );
        assert_eq!(summary.dispatched, 9);
        assert_eq!(rects, 1);
    }
}
