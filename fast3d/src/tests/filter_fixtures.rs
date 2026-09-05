use crate::capture::{Fixture, Provenance};
use crate::hle::gbi::GbiUcode;
use crate::{DataFormat, RdramImage};
use n64_gbi::encode::{gsp_matrix_f3d, gsp_viewport_f3d, mtx_to_bytes, Vp, VtxColored};

fn commands(bytes: &mut [u8], address: usize, words: &[(u32, u32)]) {
    for (i, (w0, w1)) in words.iter().enumerate() {
        bytes[address + i * 8..address + i * 8 + 4].copy_from_slice(&w0.to_be_bytes());
        bytes[address + i * 8 + 4..address + i * 8 + 8].copy_from_slice(&w1.to_be_bytes());
    }
}

fn memory() -> (Vec<u8>, Vec<(u32, u32)>) {
    let mut bytes = vec![0; 0x5000];
    bytes[..64].copy_from_slice(&mtx_to_bytes([
        [1.0 / 128.0, 0.0, 0.0, 0.0],
        [0.0, 1.0 / 128.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]));
    bytes[64..80].copy_from_slice(
        &Vp {
            vscale: [512, 512, 511, 0],
            vtrans: [640, 480, 511, 0],
        }
        .to_bytes(),
    );
    let prologue = vec![
        gsp_matrix_f3d(0, true, true, false),
        gsp_viewport_f3d(64),
        (0xba00_1402, 0),
        (0xba00_1301, 0x0008_0000),
        (0xb900_031d, 0x0f0a_4000),
        (0xb700_0000, 4),
    ];
    (bytes, prologue)
}

fn vertices(bytes: &mut [u8], vertices: &[(i16, i16, i16, i16)]) {
    for (i, &(x, y, s, t)) in vertices.iter().enumerate() {
        bytes[0x100 + i * 16..0x110 + i * 16].copy_from_slice(
            &VtxColored {
                x,
                y,
                z: 0,
                flag: 0,
                s,
                t,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            }
            .to_bytes(),
        );
    }
}

fn power_meter_memory() -> Vec<u8> {
    let (mut bytes, mut words) = memory();
    vertices(
        &mut bytes,
        &[
            (-32, -32, 0, 2016),
            (0, -32, 992, 2016),
            (0, 32, 992, 0),
            (-32, 32, 0, 0),
            (0, -32, 1, 2016),
            (32, -32, 1024, 2016),
            (32, 32, 1024, 0),
            (0, 32, 1, 0),
        ],
    );
    for (address, colors) in [(0x1000, [0xf801u16, 0x07c1]), (0x2000, [0x003f, 0xffc1])] {
        for i in 0..32 * 64 {
            bytes[address + i * 2..address + i * 2 + 2]
                .copy_from_slice(&colors[usize::from(i % 32 >= 16)].to_be_bytes());
        }
    }
    words.extend([
        (0xe700_0000, 0),
        (0xb600_0000, 0x0002_0000),
        (0xfcff_ffff, 0xfffc_f279),
        (0xb900_031d, 0x0f0a_7008),
        (0xba00_0c02, 0),
        (0xbb00_0001, 0xffff_ffff),
        (0x0470_0000, 0x100),
        (0xf510_0000, 0x0700_0000),
        (0xe800_0000, 0),
        (0xf510_1000, 0x0009_8250),
        (0xf200_0000, 0x0007_c0fc),
        (0xfd10_0000, 0x1000),
        (0xe600_0000, 0),
        (0xf300_0000, 0x077f_f100),
        (0xbf00_0000, 0x0000_0a14),
        (0xbf00_0000, 0x0000_141e),
        (0xfd10_0000, 0x2000),
        (0xe600_0000, 0),
        (0xf300_0000, 0x077f_f100),
        (0xbf00_0000, 0x0028_323c),
        (0xbf00_0000, 0x0028_3c46),
        (0xb800_0000, 0),
    ]);
    commands(&mut bytes, 0x4000, &words);
    bytes
}

fn castle_memory() -> Vec<u8> {
    let (mut bytes, mut words) = memory();
    vertices(
        &mut bytes,
        &[
            (-16, -16, 0, 8),
            (16, -16, 1536, 8),
            (16, 16, 1536, 8),
            (-16, 16, 0, 8),
        ],
    );
    for (address, colors) in [
        (0x1000, [0xf801u16, 0x07c1, 0x003f, 0xffff]),
        (0x2000, [0x0001, 0x003f, 0x07c1, 0xf801]),
    ] {
        for i in 0..32 * 32 {
            bytes[address + i * 2..address + i * 2 + 2]
                .copy_from_slice(&colors[(i / 32 % 2) * 2 + i % 2].to_be_bytes());
        }
    }
    words.extend([
        (0xba00_0c02, 0x2000),
        (0xe700_0000, 0),
        (0xba00_1402, 0x0010_0000),
        (0xfc26_a1ff, 0x1ffc_923c),
        (0xb900_031d, 0x0c19_2078),
        (0xba00_1001, 0x0001_0000),
        (0xb600_0000, 0x0002_0200),
        (0xe800_0000, 0),
        (0xf510_1000, 0x0009_4250),
        (0xf200_0000, 0x0007_c07c),
        (0xe800_0000, 0),
        (0xf510_1100, 0x0109_4250),
        (0xf200_0000, 0x0107_c07c),
        (0xbb00_0801, 0xffff_ffff),
        (0x0430_0000, 0x100),
        (0xfd10_0000, 0x1000),
        (0xe800_0000, 0),
        (0xf510_0000, 0x0700_0000),
        (0xe600_0000, 0),
        (0xf300_0000, 0x073f_f100),
        (0xfd10_0000, 0x2000),
        (0xe800_0000, 0),
        (0xf510_0100, 0x0700_0000),
        (0xe600_0000, 0),
        (0xf300_0000, 0x073f_f100),
        (0xbf00_0000, 0x0000_0a14),
        (0xbf00_0000, 0x0000_141e),
        (0xb800_0000, 0),
    ]);
    commands(&mut bytes, 0x4000, &words);
    bytes
}

fn fixture(bytes: Vec<u8>, source: &str) -> Fixture {
    super::capture_fixture::make(bytes, 0x4000, 320, 240, Provenance {
        decomp_revision: "sm64 1372ae1bb7cbedc03df366393188f4f05dcfc422".into(),
        source_symbols: source.into(),
        command_vector: "Hand-encoded F3D commands with synthetic addresses and framebuffer setup".into(),
        synthetic_data: "Synthetic RGBA16 color bands/checkers, screen-space geometry and matrices; power-meter vertices retain the decomp values".into(),
    })
}

fn assert_frame(fixture: Fixture, expected: impl Fn(usize, usize) -> [u8; 4]) {
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let output = pollster::block_on(fixture.replay(device, queue)).unwrap();
    assert!(
        output.diagnostics.iter().all(Vec::is_empty),
        "{:?}",
        output.diagnostics
    );
    for (i, got) in output.rgba8.as_chunks::<4>().0.iter().enumerate() {
        let expected = expected(i % 320, i / 320);
        assert!(
            got.iter().zip(expected).all(|(&a, b)| a.abs_diff(b) <= 2),
            "({}, {}): {got:?}, expected {expected:?}",
            i % 320,
            i / 320
        );
    }
}

#[test]
fn fixture_sm64_power_meter_point() {
    assert_frame(
        fixture(
            power_meter_memory(),
            "actors/power_meter/model.inc.c: dl_power_meter_base",
        ),
        |x, y| {
            if !(88..152).contains(&y) {
                return [0, 0, 0, 255];
            }
            match x {
                128..=144 => [255, 0, 0, 255],
                145..=159 => [0, 255, 0, 255],
                160..=175 => [0, 0, 255, 255],
                176..=191 => [255, 255, 0, 255],
                _ => [0, 0, 0, 255],
            }
        },
    );
}

#[test]
fn fixture_sm64_castle_trilerp() {
    assert_frame(
        fixture(
            castle_memory(),
            "levels/castle_inside/areas/1/1/model.inc.c: inside_castle_seg7_dl_07023DB0",
        ),
        |x, y| {
            if !(144..176).contains(&x) || !(104..136).contains(&y) {
                return [0, 0, 0, 255];
            }
            if x >= 165 {
                return [64, 128, 128, 255];
            }
            [
                [0, 128, 128, 255],
                [64, 64, 64, 255],
                [159, 32, 32, 255],
                [96, 96, 96, 255],
            ][(x - 144) % 4]
        },
    );
}

#[test]
fn filter_fixture_commands_preserve_load_order_and_lod_storage() {
    for bytes in [power_meter_memory(), castle_memory()] {
        let result = crate::hle::interpret(
            RdramImage::new(&bytes),
            0x4000,
            GbiUcode::F3d,
            DataFormat::Fixed,
        );
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        assert_eq!(result.scene.raw_st[0][0], 0.0);
        let mat = &result.scene.materials[0];
        if mat.lod {
            assert_eq!(mat.mip_levels.len(), 2);
            assert!(mat.mip_levels.iter().all(|m| (m.w, m.h) == (32, 32)));
            assert_eq!(mat.mip_levels[0].sampling.tmem[0], 0);
            assert_eq!(mat.mip_levels[1].sampling.tmem[0], 2048);
            assert_eq!(
                &mat.mip_levels[0].texture[..8],
                &[255, 0, 0, 255, 0, 255, 0, 255]
            );
            assert_eq!(
                &mat.mip_levels[1].texture[..8],
                &[0, 0, 0, 255, 0, 0, 255, 255]
            );
        } else {
            assert_eq!(result.scene.raw_pos.len(), 8);
            assert_eq!((mat.tex_w, mat.tex_h), (32, 64));
            assert_eq!(result.scene.draw_runs.len(), 2);
        }
    }
}
