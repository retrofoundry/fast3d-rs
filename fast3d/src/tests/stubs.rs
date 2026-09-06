use crate::diag::{DiagKind, Severity};
use crate::hle::gbi::GbiUcode;
use crate::hle::mem::{GbiDataFormat, RdramImage};
use crate::hle::{interpret, InterpResult, Scene};

fn walk(commands: &[(u32, u32)], ucode: GbiUcode) -> InterpResult {
    let bytes: Vec<_> = commands
        .iter()
        .flat_map(|(w0, w1)| w0.to_be_bytes().into_iter().chain(w1.to_be_bytes()))
        .collect();
    interpret(RdramImage::new(&bytes), 0, ucode, GbiDataFormat::Fixed)
}

const FILL: (u32, u32) = (0xf601_0010, 0);
const CIMG: (u32, u32) = (0xff10_0003, 0x1000);
const END: (u32, u32) = (0xdf00_0000, 0);

#[test]
fn f3dex2_known_stub_inventory() {
    for (opcode, severity, dropped) in [
        (0x08, Severity::Error, 1),
        (0xd6, Severity::Warn, 0),
        (0xd3, Severity::Error, 0),
        (0xd4, Severity::Error, 0),
        (0xd5, Severity::Error, 0),
        (0xdd, Severity::Error, 0),
    ] {
        let r = walk(
            &[((opcode as u32) << 24, 0xffff_ffff), END],
            GbiUcode::F3dex2,
        );
        assert_eq!(r.diags.len(), 1, "opcode {opcode:02x}: {:?}", r.diags);
        assert!(!matches!(r.diags[0].kind, DiagKind::UnknownOpcode(_)));
        assert_eq!(r.diags[0].kind.severity(), severity);
        assert_eq!(r.dropped_runs, dropped);
    }
}

#[test]
fn spnoop_e0_is_silent() {
    let r = walk(&[(0xe000_0000, 0), END], GbiUcode::F3dex2);
    assert!(r.diags.is_empty(), "{:?}", r.diags);
    assert_eq!(r.commands, 2);
    assert_eq!(r.dropped_runs, 0);
}

#[test]
fn load_ucode_rejects_following_stream() {
    for prefix in [vec![], vec![(0xe100_0000, 0xffff_fff0), (0xe000_0000, 0)]] {
        let mut commands = vec![CIMG, FILL];
        commands.extend(prefix);
        commands.extend([(0xdd00_07ff, 0xffff_ffe0), FILL, END]);
        let r = walk(&commands, GbiUcode::F3dex2);
        assert_eq!(r.scene, Scene::default());
        assert_eq!(r.commands as usize, commands.len() - 2);
        assert_eq!(r.diags.len(), 1, "{:?}", r.diags);
        assert_eq!(r.diags[0].kind.severity(), Severity::Error);
        assert_eq!(r.dropped_runs, 1);
    }
}

#[test]
fn special_rejects_task_without_reading_operands() {
    for w0 in [0xd312_3456, 0xd456_789a, 0xd5bc_def0] {
        let r = walk(
            &[CIMG, FILL, (w0, 0xffff_ffff), FILL, END],
            GbiUcode::F3dex2,
        );
        assert_eq!(r.scene, Scene::default());
        assert_eq!(r.commands, 3);
        assert_eq!(r.dropped_runs, 1);
        assert_eq!(r.diags.len(), 1);
        assert_eq!(r.diags[0].at, 16);
    }
}

#[test]
fn unsupported_move_state_rejects_task() {
    for (ucode, w0, expected) in [
        (
            GbiUcode::F3dex2,
            0xdc00_007f,
            DiagKind::UnhandledMovemem(0x7f),
        ),
        (
            GbiUcode::F3dex2,
            0xdb7f_0000,
            DiagKind::UnhandledMoveword(0x7f),
        ),
        (GbiUcode::F3d, 0x037f_0000, DiagKind::UnhandledMovemem(0x7f)),
        (
            GbiUcode::F3d,
            0xbc00_007f,
            DiagKind::UnhandledMoveword(0x7f),
        ),
    ] {
        let end = if ucode == GbiUcode::F3d {
            (0xb800_0000, 0)
        } else {
            END
        };
        let r = walk(&[CIMG, FILL, (w0, 0xffff_ffff), FILL, end], ucode);
        assert_eq!(r.scene, Scene::default(), "{ucode:?}, {w0:08x}");
        assert_eq!(r.commands, 3);
        assert_eq!(r.dropped_runs, 1);
        assert_eq!(r.diags.len(), 1);
        assert_eq!(r.diags[0].kind, expected);
        assert_eq!(expected.severity(), Severity::Error);
    }
}

#[test]
fn repeated_unsupported_draws_keep_first_operands_and_count_each_drop() {
    let r = walk(
        &[
            CIMG,
            (0x0800_0204, 0x1234),
            (0x0806_080a, 0x5678),
            (0xd600_0001, 0xffff_fffe),
            (0xd600_0002, 0xffff_ffff),
            FILL,
            END,
        ],
        GbiUcode::F3dex2,
    );
    assert_eq!(r.diags.len(), 2);
    assert_eq!(r.diags[0].at, 8);
    assert_eq!(
        r.diags[0].kind,
        DiagKind::UnsupportedCommand {
            opcode: 0x08,
            w0: 0x0800_0204,
            w1: 0x1234
        }
    );
    assert_eq!(r.diags[1].at, 24);
    assert_eq!(
        r.diags[1].kind,
        DiagKind::UnsupportedCommand {
            opcode: 0xd6,
            w0: 0xd600_0001,
            w1: 0xffff_fffe
        }
    );
    assert_eq!(r.dropped_runs, 2);
    assert_eq!(r.scene.framebuffer_pairs.len(), 1);
    assert_eq!(r.scene.framebuffer_pairs[0].ops.len(), 1);
}

#[test]
fn diagnostic_severity_and_rollup() {
    let r = walk(
        &[
            CIMG,
            (0xd680_0001, 0xffff_ffff),
            (0x0800_0200, 0),
            (0x0802_0400, 0),
            FILL,
            END,
        ],
        GbiUcode::F3dex2,
    );
    assert_eq!(
        r.summary(true),
        crate::DlSummary {
            commands: 6,
            tris: 0,
            warns: 1,
            errors: 1,
            dropped_runs: 2,
            renderable: true,
        }
    );
    let rejected = walk(&[CIMG, FILL, (0xd500_0000, 0), END], GbiUcode::F3dex2);
    assert_eq!(rejected.scene, Scene::default());
    assert_eq!(
        rejected.summary(false),
        crate::DlSummary {
            commands: 3,
            tris: 0,
            warns: 0,
            errors: 1,
            dropped_runs: 1,
            renderable: false,
        }
    );
    assert_eq!(
        DiagKind::UnsupportedTextureFormat { fmt: 1, siz: 2 }.severity(),
        Severity::Error
    );
}

struct CommandMemory<'a>(&'a [(u32, u64)]);

impl crate::Rdram for CommandMemory<'_> {
    fn set_segment(&mut self, _: u32, _: u64) {
        panic!("unexpected write")
    }
    fn resolve(&self, addr: u64) -> u64 {
        addr
    }
    fn resolve_masked(&self, addr: u64) -> u64 {
        addr
    }
    fn read_command(&self, pc: u64) -> crate::hle::mem::Command {
        let (w0, w1_addr) = self.0[pc as usize / 16];
        crate::hle::mem::Command {
            w0,
            w1: w1_addr as u32,
            w1_addr,
        }
    }
    fn command_stride(&self) -> u64 {
        16
    }
    fn in_bounds(&self, pc: u64, stride: u64) -> bool {
        pc.checked_add(stride)
            .is_some_and(|end| end <= self.0.len() as u64 * 16)
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
    fn read_bytes(&self, _: u64, _: usize) -> std::borrow::Cow<'_, [u8]> {
        panic!("unexpected data read")
    }
    fn read_matrix(&self, _: u64, _: GbiDataFormat) -> crate::hle::math::Mat4 {
        panic!("unexpected data read")
    }
}

#[test]
fn load_ucode_latch_preserves_full_address_across_calls_and_rects() {
    let commands = [
        (0xe100_0000, 0x1234_5678_ffff_fff0),
        (0xde00_0000, 10 * 16),
        (CIMG.0, CIMG.1 as u64),
        (0xe401_0010, 0),
        (0xe100_0000, 0x0020_0040),
        (0xf100_0000, 0x0400_0400),
        (0xdd00_07ff, 0x9876_5432_ffff_ffe0),
        (FILL.0, 0),
        (END.0, 0),
        (0, 0),
        (0xe100_0000, 0x1234_5678_aaaa_aaa0),
        (0xe000_0000, 0),
        (END.0, 0),
    ];
    let r = interpret(
        CommandMemory(&commands),
        0,
        GbiUcode::F3dex2,
        GbiDataFormat::Fixed,
    );
    assert_eq!(r.scene, Scene::default());
    assert_eq!(r.commands, 8);
    assert_eq!(r.dropped_runs, 1);
    assert_eq!(
        r.diags,
        vec![crate::Diagnostic {
            at: 6 * 16,
            kind: DiagKind::UnsupportedMicrocodeLoad {
                w0: 0xdd00_07ff,
                w1: 0x9876_5432_ffff_ffe0,
                data_address: Some(0x1234_5678_aaaa_aaa0),
            },
        }]
    );
    let next = walk(&[(0xdd00_0000, 0xffff_ffff), END], GbiUcode::F3dex2);
    assert!(matches!(
        next.diags[0].kind,
        DiagKind::UnsupportedMicrocodeLoad {
            data_address: None,
            ..
        }
    ));
}

#[test]
fn unsupported_stubs_preserve_full_raw_operands_without_data_access() {
    for opcode in [0x08, 0xd3, 0xd4, 0xd5, 0xd6] {
        let w0 = (opcode as u32) << 24 | 0x0012_3456;
        let w1 = 0x1234_5678_ffff_ffff;
        let commands = [(w0, w1), (END.0, 0)];
        let r = interpret(
            CommandMemory(&commands),
            0,
            GbiUcode::F3dex2,
            GbiDataFormat::Fixed,
        );
        assert_eq!(
            r.diags[0].kind,
            DiagKind::UnsupportedCommand { opcode, w0, w1 }
        );
    }
}
