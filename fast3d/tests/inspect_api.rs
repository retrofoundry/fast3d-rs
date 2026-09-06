use std::ops::ControlFlow;

use fast3d::inspect::{walk, Emission, WalkObserver, WalkStep, WalkTermination};
use fast3d::{DataFormat, Microcode, RdramImage};

#[derive(Default)]
struct Observer {
    count: u32,
    fills: u32,
    stop: bool,
}

impl WalkObserver for Observer {
    fn command(&mut self, step: WalkStep<'_>) -> ControlFlow<()> {
        assert_eq!(step.seq, self.count);
        self.count += 1;
        let s = step.state;
        let _ = (
            s.geometry_mode,
            s.geometry_names,
            s.texture,
            s.modelview_depth,
            s.modelview,
            s.projection,
            s.viewport_scale,
            s.viewport_translation,
            s.light_count,
            s.lights,
            s.ambient,
            s.lookat_axes,
            s.tiles,
            s.load_via_tile,
            s.texture_image,
            s.combine_l,
            s.combine_h,
            s.other_mode_h,
            s.other_mode_l,
            s.prim_color,
            s.env_color,
            s.fog_color,
            s.blend_color,
            s.fill_color_raw,
            s.color_image,
            s.depth_image,
            s.scissor,
        );
        let _ = (
            step.pc,
            step.words,
            step.depth_before,
            step.depth_after,
            step.flow,
            step.next_pc,
            step.diagnostics_start,
            step.diagnostics,
        );
        for emission in step.emissions {
            match emission {
                Emission::Triangles {
                    target,
                    index_start,
                    indices,
                } => {
                    let _ = (target, index_start, indices);
                }
                Emission::FillRect {
                    target,
                    rect,
                    color_raw,
                } => {
                    assert_eq!(target.pair_index, 0);
                    assert_eq!(target.color_image.addr, 0x100000);
                    assert_eq!(target.depth_image, None);
                    assert!(!target.is_depth_clear);
                    assert_eq!((rect.ulx, rect.uly, rect.lrx, rect.lry), (0, 0, 320, 240));
                    assert_eq!(*color_raw, 0xcafecafe);
                    self.fills += 1;
                }
                Emission::TexRect {
                    target,
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
                    let _ = (
                        target, rect, tile, uls, ult, dsdx, dtdy, flip, copy_mode, fb_source,
                    );
                }
            }
        }
        if self.stop {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

fn run(
    words: &[(u32, u32)],
    microcode: Microcode,
    observer: &mut dyn WalkObserver,
) -> fast3d::inspect::WalkSummary {
    let bytes: Vec<u8> = words
        .iter()
        .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
        .collect();
    walk(
        RdramImage::new(&bytes),
        0,
        microcode,
        DataFormat::Fixed,
        observer,
    )
}

#[test]
fn public_walk_without_renderer() {
    for (microcode, end) in [
        (Microcode::F3dex2, 0xdf000000),
        (Microcode::F3d, 0xb8000000),
    ] {
        let mut observer = Observer::default();
        let summary = run(&[(end, 0)], microcode, &mut observer);
        assert_eq!(summary.termination, WalkTermination::End);
        assert_eq!(summary.dispatched, 1);
        assert_eq!(summary.tris, 0);
        assert_eq!(summary.dropped_runs, 0);
        assert!(summary.diagnostics.is_empty());
        assert_eq!(summary.final_diagnostics_start, 0);
        let mut observer = Observer::default();
        let summary = run(
            &[
                (0xff10013f, 0x100000),
                (0xf7000000, 0xcafecafe),
                (0xf65003c0, 0),
                (end, 0),
            ],
            microcode,
            &mut observer,
        );
        assert_eq!(summary.dispatched, 4);
        assert_eq!(observer.fills, 1);
        let mut observer = Observer {
            stop: true,
            ..Observer::default()
        };
        let summary = run(&[(0xf7000000, 1), (end, 0)], microcode, &mut observer);
        assert_eq!(summary.termination, WalkTermination::ObserverStopped);
        assert_eq!(summary.dispatched, 1);
    }
}
