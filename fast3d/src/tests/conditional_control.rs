use std::cell::RefCell;

use crate::hle::gbi::GbiUcode;
use crate::hle::mem::{Command, GbiDataFormat, Rdram, RdramImage};
use crate::hle::{interpret, InterpResult};
use n64_gbi::{consts::*, encode::*};

use super::dl_builder::DlBuilder;

const END: (u32, u32) = (0xdf00_0000, 0);
const MARK: (u32, u32) = (0xfb00_0000, 0x1234_5678);
const CULL: (u32, u32) = (0x0300_0000, 0);
const HALF: u32 = 0xe100_0000;
const BRANCH: u32 = 0x0400_0000;

struct Traced<'a> {
    image: RdramImage<'a>,
    reads: &'a RefCell<Vec<u64>>,
}

impl Rdram for Traced<'_> {
    fn set_segment(&mut self, segment: u32, value: u64) {
        Rdram::set_segment(&mut self.image, segment, value);
    }
    fn resolve(&self, address: u64) -> Result<u64, crate::MemoryError> {
        self.image.resolve(address)
    }
    fn resolve_masked(&self, address: u64) -> Result<u64, crate::MemoryError> {
        self.image.resolve_masked(address)
    }
    fn read_command(&self, address: u64) -> Result<Command, crate::MemoryError> {
        self.reads.borrow_mut().push(address);
        self.image.read_command(address)
    }
    fn command_stride(&self) -> u64 {
        8
    }
    fn in_bounds(&self, address: u64, size: u64) -> bool {
        self.image.in_bounds(address, size)
    }
    fn read_u8(&self, address: u64) -> Result<u8, crate::MemoryError> {
        Rdram::read_u8(&self.image, address)
    }
    fn read_i8(&self, address: u64) -> Result<i8, crate::MemoryError> {
        Rdram::read_i8(&self.image, address)
    }
    fn read_u16(&self, address: u64) -> Result<u16, crate::MemoryError> {
        Rdram::read_u16(&self.image, address)
    }
    fn read_i16(&self, address: u64) -> Result<i16, crate::MemoryError> {
        Rdram::read_i16(&self.image, address)
    }
    fn read_bytes(
        &self,
        address: u64,
        size: usize,
    ) -> Result<std::borrow::Cow<'_, [u8]>, crate::MemoryError> {
        self.image.read_bytes(address, size)
    }
    fn read_matrix(
        &self,
        address: u64,
        format: GbiDataFormat,
    ) -> Result<crate::hle::math::Mat4, crate::MemoryError> {
        Rdram::read_matrix(&self.image, address, format)
    }
}

fn walk(b: DlBuilder, expected_offsets: &[u64]) -> InterpResult {
    let built = b.finish("main");
    let reads = RefCell::new(Vec::new());
    let result = interpret(
        Traced {
            image: RdramImage::new(&built.rdram),
            reads: &reads,
        },
        built.entry as u64,
        GbiUcode::F3dex2,
        GbiDataFormat::Fixed,
    );
    assert_eq!(*reads.borrow(), expected_offsets);
    result
}

fn vertex(b: &mut DlBuilder, positions: &[[i16; 3]]) -> u32 {
    b.vertices(
        &positions
            .iter()
            .map(|&[x, y, z]| VtxColored {
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
            })
            .collect::<Vec<_>>(),
    )
}

fn offsets(entry: u32, indices: &[u64]) -> Vec<u64> {
    indices.iter().map(|i| entry as u64 + i * 8).collect()
}

#[test]
fn culldl_each_side_plane_and_boundary() {
    for axis in 0..2 {
        for sign in [-1, 1] {
            for (distance, culled) in [(1, false), (2, true)] {
                let mut b = DlBuilder::new();
                let mut position = [0; 3];
                position[axis] = distance * sign;
                let v = vertex(&mut b, &[position]);
                let entry = b.list("main", &[gsp_vertex(0, 1, v), CULL, MARK, END]);
                let r = walk(
                    b,
                    &offsets(entry, if culled { &[0, 1] } else { &[0, 1, 2, 3] }),
                );
                assert!(
                    r.diags.is_empty(),
                    "{axis} {sign} {distance}: {:?}",
                    r.diags
                );
                assert_eq!(r.commands, if culled { 2 } else { 4 });
            }
        }
    }
}

fn assert_culldl_retains_unverified_z(sign: i16) {
    for distance in [1, 2, i16::MAX] {
        let mut b = DlBuilder::new();
        let z = distance * sign;
        let v = vertex(&mut b, &[[0, 0, z], [1, 1, z]]);
        let entry = b.list("main", &[gsp_vertex(0, 2, v), (CULL.0, 2), MARK, END]);
        let r = walk(b, &offsets(entry, &[0, 1, 2, 3]));
        assert!(r.diags.is_empty(), "z={z}: {:?}", r.diags);
        assert_eq!(r.commands, 4, "z={z}");
    }
}

#[test]
fn culldl_near_z_plane_unused_until_rsp_convention_verified() {
    assert_culldl_retains_unverified_z(-1);
}

#[test]
fn culldl_far_z_plane_unused_until_rsp_convention_verified() {
    assert_culldl_retains_unverified_z(1);
}

#[test]
fn culldl_mixed_planes_not_culled() {
    let mut b = DlBuilder::new();
    let v = vertex(&mut b, &[[2, 0, 0], [0, 2, 0], [0, 0, -2]]);
    let entry = b.list("main", &[gsp_vertex(0, 3, v), (CULL.0, 4), MARK, END]);
    let r = walk(b, &offsets(entry, &[0, 1, 2, 3]));
    assert!(r.diags.is_empty(), "{:?}", r.diags);
}

#[test]
fn culldl_nested_returns_to_parent() {
    let mut b = DlBuilder::new();
    let v = vertex(&mut b, &[[2, 0, 0]]);
    let child = b.list("child", &[CULL, (0x0100_1002, 0xffff_fff0), END]);
    let parent = b.list("parent", &[gsp_displaylist(child), MARK, END]);
    let entry = b.list("main", &[gsp_vertex(0, 1, v), gsp_displaylist(parent), END]);
    let r = walk(
        b,
        &[
            entry as u64,
            entry as u64 + 8,
            parent as u64,
            child as u64,
            parent as u64 + 8,
            parent as u64 + 16,
            entry as u64 + 16,
        ],
    );
    assert!(r.diags.is_empty(), "{:?}", r.diags);
    assert_eq!(r.commands, 7);
}

#[test]
fn culldl_uses_inclusive_range() {
    for (first, last, culled) in [(0, 1, false), (1, 2, false), (1, 1, true)] {
        let mut b = DlBuilder::new();
        let v = vertex(&mut b, &[[0, 0, 0], [2, 0, 0], [0, 0, 0]]);
        let entry = b.list(
            "main",
            &[
                gsp_vertex(0, 3, v),
                (CULL.0 | (first * 2), last * 2),
                MARK,
                END,
            ],
        );
        let r = walk(
            b,
            &offsets(entry, if culled { &[0, 1] } else { &[0, 1, 2, 3] }),
        );
        assert!(r.diags.is_empty(), "{:?}", r.diags);
    }
}

#[test]
fn culldl_ignores_face_mode_and_clipratio() {
    for geom in [0, G_CULL_FRONT, G_CULL_BACK, G_CULL_BOTH | G_CLIPPING] {
        let mut b = DlBuilder::new();
        let v = vertex(&mut b, &[[2, 0, 0]]);
        let entry = b.list(
            "main",
            &[
                gsp_clear_geometrymode(u32::MAX),
                gsp_set_geometrymode(geom),
                (0xdb04_0004, 4),
                gsp_vertex(0, 1, v),
                gsp_modifyvertex(0, G_MWO_POINT_XYSCREEN, 0x0280_01e0),
                gsp_modifyvertex(0, G_MWO_POINT_ZSCREEN, 0),
                CULL,
                MARK,
                END,
            ],
        );
        let r = walk(b, &offsets(entry, &[0, 1, 2, 3, 4, 5, 6]));
        assert!(r.diags.is_empty(), "{:?}", r.diags);
    }
}

#[test]
fn branchz_below_equal_above() {
    // Identity projection and the default viewport put z=0 at raw screen Z=511.
    for (threshold, taken) in [
        (0x01fe_ffff, false),
        (0x01ff_0000, true),
        (0x01ff_0001, true),
        (0xffff_ffff, true),
    ] {
        let mut b = DlBuilder::new();
        let v = vertex(&mut b, &[[0, 0, 0]]);
        let target = b.list("target", &[MARK, END]);
        let entry = b.list(
            "main",
            &[
                gsp_vertex(3, 1, v),
                (HALF, target),
                (0x0400_f006, threshold),
                END,
                (0x0100_1002, 0xffff_fff0),
            ],
        );
        let mut expected = offsets(entry, &[0, 1, 2]);
        if taken {
            expected.extend([target as u64, target as u64 + 8]);
        } else {
            expected.push(entry as u64 + 24);
        }
        let r = walk(b, &expected);
        assert!(r.diags.is_empty(), "{threshold:x}: {:?}", r.diags);
    }
}

#[test]
fn branchz_uses_load_viewport_and_modified_z() {
    for (modify, taken) in [
        (None, false),
        (Some(0x0031_ffff), true),
        (Some(0x0032_0000), true),
        (Some(0x0032_0001), false),
    ] {
        let mut b = DlBuilder::new();
        let v = vertex(&mut b, &[[0, 0, 1]]);
        let projection = b.matrix(n64_gbi::gu::gu_scale(1.0, 1.0, 0.5));
        let old = b.viewport(Vp {
            vscale: [640, 480, 100, 0],
            vtrans: [640, 480, 200, 0],
        });
        let new = b.viewport(Vp {
            vscale: [640, 480, 10, 0],
            vtrans: [640, 480, 20, 0],
        });
        let target = b.list("target", &[MARK, END]);
        let mut commands = vec![
            gsp_matrix(projection, true, true, false),
            gsp_viewport(old),
            gsp_vertex(0, 1, v),
            gsp_viewport(new),
        ];
        if let Some(value) = modify {
            commands.push(gsp_modifyvertex(0, G_MWO_POINT_ZSCREEN, value));
        }
        commands.extend([(HALF, target), (BRANCH, 50 << 16), END]);
        let count = commands.len() as u64;
        let entry = b.list("main", &commands);
        let mut expected = offsets(entry, &(0..count - 1).collect::<Vec<_>>());
        if taken {
            expected.extend([target as u64, target as u64 + 8]);
        } else {
            expected.push(entry as u64 + (count - 1) * 8);
        }
        let r = walk(b, &expected);
        assert!(r.diags.is_empty(), "{:?}", r.diags);
    }
}

#[test]
fn conditional_invalid_vertex_rejects_task() {
    for conditional in [
        CULL,
        (CULL.0 | 2, 0),
        (CULL.0, 2),
        (CULL.0 | 2, 4),
        (CULL.0, 0xfffe),
        (BRANCH, 0),
        (BRANCH | 0xffe, 0),
    ] {
        let mut b = DlBuilder::new();
        let v = vertex(&mut b, &[[0, 0, 0]]);
        let entry = b.list(
            "main",
            &[
                (0xff10_0003, 0x1000),
                (0xf601_0010, 0),
                gsp_vertex(1, 1, v),
                (HALF, 0),
                conditional,
                (0x0100_1002, 0xffff_fff0),
                END,
            ],
        );
        let r = walk(b, &offsets(entry, &[0, 1, 2, 3, 4]));
        assert_eq!(r.scene, crate::hle::Scene::default());
        assert_eq!(r.diags.len(), 1);
        assert_eq!(r.diags[0].at, entry as u64 + 32);
        assert_eq!(r.diags[0].kind.severity(), crate::Severity::Error);
        assert_eq!(r.dropped_runs, 1);
    }
}

#[test]
fn branchz_requires_half1_each_task() {
    let mut previous = DlBuilder::new();
    let entry = previous.list("main", &[(HALF, 0x1234_5678), END]);
    assert!(walk(previous, &offsets(entry, &[0, 1])).diags.is_empty());
    let mut b = DlBuilder::new();
    let v = vertex(&mut b, &[[0, 0, 0]]);
    let entry = b.list("main", &[gsp_vertex(0, 1, v), (BRANCH, 0), MARK, END]);
    let r = walk(b, &offsets(entry, &[0, 1]));
    assert_eq!(r.scene, crate::hle::Scene::default());
    assert_eq!(r.diags.len(), 1);
    assert_eq!(r.diags[0].kind.severity(), crate::Severity::Error);
}

#[test]
fn branchz_segmented_target_returns_to_original_caller() {
    let mut b = DlBuilder::new();
    let v = vertex(&mut b, &[[0, 0, 0]]);
    let target = b.list("target", &[MARK, END]);
    let child = b.list(
        "child",
        &[
            (HALF, 0x0700_000b),
            (BRANCH, 512 << 16),
            (0x0100_1002, 0xffff_fff0),
            END,
        ],
    );
    let entry = b.list(
        "main",
        &[
            gsp_vertex(0, 1, v),
            gsp_segment(7, target - 8),
            gsp_displaylist(child),
            END,
        ],
    );
    let r = walk(
        b,
        &[
            entry as u64,
            entry as u64 + 8,
            entry as u64 + 16,
            child as u64,
            child as u64 + 8,
            target as u64,
            target as u64 + 8,
            entry as u64 + 24,
        ],
    );
    assert!(r.diags.is_empty(), "{:?}", r.diags);
}

#[test]
fn half1_then_texrect_then_branch() {
    let mut b = DlBuilder::new();
    let v = vertex(&mut b, &[[0, 0, 0]]);
    let target = b.list("target", &[MARK, END]);
    let entry = b.list(
        "main",
        &[
            (0xff10_0003, 0x1000),
            gsp_vertex(0, 1, v),
            (HALF, target),
            (0xe401_0010, 0),
            (HALF, 0xffff_fff0),
            (0xf100_0000, 0x0400_0400),
            (BRANCH, 512 << 16),
            (0x0100_1002, 0xffff_fff0),
            END,
        ],
    );
    let mut expected = offsets(entry, &[0, 1, 2, 3, 4, 5, 6]);
    expected.extend([target as u64, target as u64 + 8]);
    let r = walk(b, &expected);
    assert!(r.diags.is_empty(), "{:?}", r.diags);
    assert_eq!(r.commands, 7);
    assert!(matches!(
        r.scene.framebuffer_pairs[0].ops.as_slice(),
        [crate::hle::SceneOp::TexRect { .. }]
    ));
}

#[test]
fn conditional_loop_hits_dispatch_cap() {
    let mut b = DlBuilder::new();
    let v = vertex(&mut b, &[[0, 0, 0]]);
    let body = b.list("body", &[(BRANCH, 512 << 16)]);
    b.list(
        "main",
        &[gsp_vertex(0, 1, v), (HALF, body), gsp_branchlist(body)],
    );
    let built = b.finish("main");
    let r = crate::hle::interpret_rdram(&built.rdram, built.entry);
    assert_eq!(r.commands, 1 << 20);
    assert_eq!(
        r.diags,
        vec![crate::Diagnostic {
            at: body as u64,
            kind: crate::DiagKind::RunawayDl { cap: 1 << 20 }
        }]
    );
}

struct NativeCommands {
    commands: Vec<(u32, u64)>,
    base: u64,
    position: [f32; 3],
    matrix: crate::hle::math::Mat4,
    reads: RefCell<Vec<u64>>,
}

impl Rdram for &NativeCommands {
    fn set_segment(&mut self, _: u32, _: u64) {
        panic!("unexpected segment write")
    }
    fn resolve(&self, address: u64) -> Result<u64, crate::MemoryError> {
        Ok(address)
    }
    fn resolve_masked(&self, address: u64) -> Result<u64, crate::MemoryError> {
        Ok(address)
    }
    fn read_command(&self, address: u64) -> Result<Command, crate::MemoryError> {
        self.reads.borrow_mut().push(address);
        if !self.in_bounds(address, 16) {
            return Err(crate::MemoryError {
                address,
                length: 16,
                kind: crate::MemoryErrorKind::OutOfBounds,
            });
        }
        let (w0, w1_addr) = self.commands[((address - self.base) / 16) as usize];
        Ok(Command {
            w0,
            w1: w1_addr as u32,
            w1_addr,
        })
    }
    fn command_stride(&self) -> u64 {
        16
    }
    fn in_bounds(&self, address: u64, size: u64) -> bool {
        address
            .checked_sub(self.base)
            .and_then(|a| a.checked_add(size))
            .is_some_and(|end| end <= self.commands.len() as u64 * 16)
    }
    fn read_u8(&self, _: u64) -> Result<u8, crate::MemoryError> {
        panic!("unexpected byte read")
    }
    fn read_i8(&self, _: u64) -> Result<i8, crate::MemoryError> {
        panic!("unexpected byte read")
    }
    fn read_u16(&self, _: u64) -> Result<u16, crate::MemoryError> {
        panic!("unexpected halfword read")
    }
    fn read_i16(&self, _: u64) -> Result<i16, crate::MemoryError> {
        panic!("unexpected halfword read")
    }
    fn read_bytes(
        &self,
        _: u64,
        _: usize,
    ) -> Result<std::borrow::Cow<'_, [u8]>, crate::MemoryError> {
        panic!("unexpected byte read")
    }
    fn read_matrix(
        &self,
        address: u64,
        _: GbiDataFormat,
    ) -> Result<crate::hle::math::Mat4, crate::MemoryError> {
        assert_eq!(address, 0x2000);
        Ok(self.matrix)
    }
    fn vertex_stride(&self, _: GbiDataFormat) -> Result<u64, crate::MemoryError> {
        Ok(24)
    }
    fn read_vertex(
        &self,
        address: u64,
        _: GbiDataFormat,
    ) -> Result<crate::hle::mem::RawVertex, crate::MemoryError> {
        assert_eq!(address, 0x1000);
        Ok(crate::hle::mem::RawVertex {
            pos: self.position,
            st: [0; 2],
            rgba: [255; 4],
        })
    }
}

fn native(commands: &[(u32, u64)]) -> NativeCommands {
    NativeCommands {
        commands: commands.to_vec(),
        base: 0x1234_5678_0000_0000,
        position: [0.0; 3],
        matrix: crate::hle::math::identity(),
        reads: RefCell::default(),
    }
}

#[test]
fn branchz_half1_preserves_host_address() {
    let mut mem = native(&[
        (0x0100_1002, 0x1000),
        (HALF, 0),
        (BRANCH, 512 << 16),
        (0x0100_1002, 0xffff_fff0),
        (MARK.0, MARK.1 as u64),
        (END.0, 0),
    ]);
    mem.commands[1].1 = mem.base + 4 * 16;
    let r = interpret(&mem, mem.base, GbiUcode::F3dex2, GbiDataFormat::Float);
    assert!(r.diags.is_empty(), "{:?}", r.diags);
    assert_eq!(
        *mem.reads.borrow(),
        [0, 16, 32, 64, 80].map(|offset| mem.base + offset)
    );
    assert_eq!(r.commands, 5);
}

#[test]
fn conditional_invalid_transform_rejects_task() {
    for (position, w, conditional) in [
        ([f32::NAN, 0.0, 0.0], 1.0, CULL),
        ([0.0, f32::INFINITY, 0.0], 1.0, CULL),
        ([0.0, 0.0, f32::NEG_INFINITY], 1.0, (BRANCH, u32::MAX)),
        ([0.0; 3], f32::NAN, (BRANCH, u32::MAX)),
        ([0.0; 3], 0.0, (BRANCH, u32::MAX)),
    ] {
        for modify in [false, true] {
            let mut commands = vec![(0xda38_0007, 0x2000), (0x0100_1002, 0x1000), (HALF, 0)];
            if modify {
                commands.push((0x021c_0000, 0));
            }
            commands.extend([
                (conditional.0, conditional.1 as u64),
                (MARK.0, MARK.1 as u64),
                (END.0, 0),
            ]);
            let mut mem = native(&commands);
            mem.position = position;
            mem.matrix[3][3] = w;
            let r = interpret(&mem, mem.base, GbiUcode::F3dex2, GbiDataFormat::Float);
            assert_eq!(
                r.diags,
                vec![crate::Diagnostic {
                    at: mem.base + (commands.len() as u64 - 3) * 16,
                    kind: crate::DiagKind::InvalidVertexTransform { index: 0 },
                }],
                "{position:?} w={w} modify={modify}"
            );
            assert_eq!(r.scene, crate::hle::Scene::default());
            assert_eq!(r.commands as usize, commands.len() - 2);
        }
    }
}

#[test]
fn conditional_homogeneous_projection() {
    for (x, z, threshold, culled, taken) in [
        (2.0, 0.0, 511 << 16, false, true),
        (2.25, 0.0, 511 << 16, true, false),
        (0.0, 1.0, 766 << 16, false, false),
        (0.0, 1.0, 767 << 16, false, true),
    ] {
        let mut mem = native(&[
            (0xda38_0007, 0x2000),
            (0x0100_1002, 0x1000),
            (CULL.0, CULL.1 as u64),
            (HALF, 0),
            (BRANCH, threshold),
            (END.0, 0),
            (MARK.0, MARK.1 as u64),
            (END.0, 0),
        ]);
        mem.position = [x, 0.0, z];
        mem.matrix[3][3] = 2.0;
        mem.commands[3].1 = mem.base + 6 * 16;
        let r = interpret(&mem, mem.base, GbiUcode::F3dex2, GbiDataFormat::Float);
        assert!(r.diags.is_empty(), "{:?}", r.diags);
        let indices: &[u64] = if culled {
            &[0, 1, 2]
        } else if taken {
            &[0, 1, 2, 3, 4, 6, 7]
        } else {
            &[0, 1, 2, 3, 4, 5]
        };
        assert_eq!(
            *mem.reads.borrow(),
            indices
                .iter()
                .map(|i| mem.base + i * 16)
                .collect::<Vec<_>>()
        );
    }
}

#[cfg(feature = "capture")]
#[test]
fn conditional_capture_records_only_reached_commands_and_typed_inputs() {
    use crate::capture::{RecordingHardware, ReplayHardware};
    use crate::Hardware;

    struct Image(Vec<u8>);
    impl Hardware for Image {
        fn rdram(&self) -> impl Rdram + '_ {
            RdramImage::new(&self.0)
        }
    }

    for (cull, taken) in [(true, true), (true, false), (false, true), (false, false)] {
        let mut b = DlBuilder::new();
        let projection = b.matrix(n64_gbi::gu::gu_scale(1.0, 1.0, 1.0));
        let vp = b.viewport(Vp {
            vscale: [640, 480, 511, 0],
            vtrans: [640, 480, 511, 0],
        });
        let v = vertex(&mut b, &[[if cull && taken { 2 } else { 0 }, 0, 0]]);
        let later_v = vertex(&mut b, &[[1, 1, 1]]);
        let target = b.list("target", &[MARK, END]);
        let control = if cull {
            CULL
        } else {
            (BRANCH, if taken { 512 << 16 } else { 510 << 16 })
        };
        let entry = b.list(
            "main",
            &[
                gsp_matrix(projection, true, true, false),
                gsp_viewport(vp),
                gsp_vertex(0, 1, v),
                (HALF, target),
                control,
                gsp_vertex(1, 1, later_v),
                END,
            ],
        );
        let built = b.finish("main");
        let hardware = Image(built.rdram);
        let recording = RecordingHardware::new(&hardware);
        let result = interpret(
            recording.rdram(),
            entry as u64,
            GbiUcode::F3dex2,
            GbiDataFormat::Fixed,
        );
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        let task = recording
            .finish(
                entry as u64,
                crate::Microcode::F3dex2,
                crate::DataFormat::Fixed,
                0,
            )
            .unwrap();
        let mut expected = std::collections::BTreeSet::new();
        for (address, length) in [(projection, 64), (vp, 16), (v, 6), (v + 8, 8), (entry, 40)] {
            expected.extend(address as u64..(address + length) as u64);
        }
        if taken && !cull {
            expected.extend(target as u64..target as u64 + 16);
        }
        if !taken {
            expected.extend(entry as u64 + 40..entry as u64 + 56);
            expected.extend(later_v as u64..later_v as u64 + 6);
            expected.extend(later_v as u64 + 8..later_v as u64 + 16);
        }
        let recorded: std::collections::BTreeSet<_> = task
            .spans
            .iter()
            .flat_map(|span| span.address..span.address + span.bytes.len() as u64)
            .collect();
        assert_eq!(recorded, expected, "cull={cull} taken={taken}");
        drop(hardware);
        let replay = ReplayHardware::new(&task, None).unwrap();
        assert_eq!(
            interpret(
                replay.rdram(),
                entry as u64,
                GbiUcode::F3dex2,
                GbiDataFormat::Fixed
            ),
            result
        );
        replay.check().unwrap();
    }
}

#[test]
fn branchz_preserves_load_matrix() {
    let mut b = DlBuilder::new();
    let old = b.matrix(n64_gbi::gu::gu_scale(1.0, 1.0, 0.5));
    let new = b.matrix(n64_gbi::gu::gu_scale(1.0, 1.0, 4.0));
    let vp = b.viewport(Vp {
        vscale: [640, 480, 100, 0],
        vtrans: [640, 480, 200, 0],
    });
    let v = vertex(&mut b, &[[0, 0, 1]]);
    let target = b.list("target", &[MARK, END]);
    let entry = b.list(
        "main",
        &[
            gsp_matrix(old, true, true, false),
            gsp_viewport(vp),
            gsp_vertex(0, 1, v),
            gsp_matrix(new, true, true, false),
            (HALF, target),
            (BRANCH, 255 << 16),
            (0x0100_1002, 0xffff_fff0),
            END,
        ],
    );
    let mut expected = offsets(entry, &[0, 1, 2, 3, 4, 5]);
    expected.extend([target as u64, target as u64 + 8]);
    let r = walk(b, &expected);
    assert!(r.diags.is_empty(), "{:?}", r.diags);
}
