use crate::scene::{ColorImage, DrawOrigin, DrawRun, Scene, SceneOp, Scissor};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TargetId {
    Legacy,
    Guest(u64),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Operation {
    pub draw: SceneOp,
    pub scissor: Scissor,
    pub pc: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TargetWorkload {
    pub id: TargetId,
    pub color_image: ColorImage,
    pub depth_image: Option<u64>,
    pub logical_extent: (u32, u32),
    pub depth_clear: bool,
    pub operations: Vec<Operation>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Workload {
    pub targets: Vec<TargetWorkload>,
}

impl Workload {
    pub fn new(scene: &Scene) -> Self {
        let triangles: Vec<_> = scene
            .draw_origins
            .iter()
            .filter(|origin| origin.rectangle.is_none())
            .collect();
        let rectangles: std::collections::HashMap<_, _> = scene
            .draw_origins
            .iter()
            .filter_map(|origin| origin.rectangle.map(|key| (key, origin)))
            .collect();
        let mut targets = Vec::new();
        if !scene.draw_runs.is_empty() {
            let mut operations = Vec::new();
            for run in &scene.draw_runs {
                push_triangles(&mut operations, &triangles, run, legacy_scissor());
            }
            targets.push(TargetWorkload {
                id: TargetId::Legacy,
                color_image: ColorImage::default(),
                depth_image: None,
                logical_extent: super::PAIRLESS_LOGICAL_EXTENT,
                depth_clear: false,
                operations,
            });
        }
        for (pair_index, pair) in scene.framebuffer_pairs.iter().enumerate() {
            let mut scissor = pair.active_scissor;
            let mut operations = Vec::new();
            for (op_index, draw) in pair.ops.iter().enumerate() {
                match draw {
                    SceneOp::SetScissor(value) => scissor = *value,
                    SceneOp::Tris(run) => push_triangles(&mut operations, &triangles, run, scissor),
                    _ => {
                        let origin = rectangles.get(&(pair_index, op_index));
                        operations.push(Operation {
                            draw: draw.clone(),
                            scissor,
                            pc: origin.map(|origin| origin.pc),
                        });
                    }
                }
            }
            targets.push(TargetWorkload {
                id: TargetId::Guest(pair.color_image.addr),
                color_image: pair.color_image,
                depth_image: pair.depth_image,
                logical_extent: super::pair_render_extent(pair),
                depth_clear: pair.is_depth_clear,
                operations,
            });
        }
        Self { targets }
    }
}

fn legacy_scissor() -> Scissor {
    Scissor {
        lrx: 320,
        lry: 240,
        ..Scissor::default()
    }
}

fn push_triangles(
    operations: &mut Vec<Operation>,
    origins: &[&DrawOrigin],
    run: &DrawRun,
    scissor: Scissor,
) {
    let end = run.index_start + run.index_count;
    let mut start = run.index_start;
    let first = origins.partition_point(|origin| origin.indices.end <= start);
    for origin in origins[first..]
        .iter()
        .take_while(|origin| origin.indices.start < end)
    {
        if start < origin.indices.start {
            operations.push(Operation {
                draw: SceneOp::Tris(DrawRun {
                    index_start: start,
                    index_count: origin.indices.start - start,
                    ..*run
                }),
                scissor,
                pc: None,
            });
            start = origin.indices.start;
        }
        let next = origin.indices.end.min(end);
        operations.push(Operation {
            draw: SceneOp::Tris(DrawRun {
                index_start: start,
                index_count: next - start,
                ..*run
            }),
            scissor: origin.scissor,
            pc: Some(origin.pc),
        });
        start = next;
    }
    if start < end {
        operations.push(Operation {
            draw: SceneOp::Tris(DrawRun {
                index_start: start,
                index_count: end - start,
                ..*run
            }),
            scissor,
            pc: None,
        });
    }
}
