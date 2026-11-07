use super::super::{
    interp::interpret_profiled,
    rdp::{Rdp, TileDescriptor},
};
use crate::{
    profiling::{CensusLoad, Mode, Recorder},
    tests::dl_builder::DlBuilder,
    DataFormat, Microcode, RdramImage,
};
use n64_gbi::encode::{gdp_set_texture_image, gdp_set_tile, gsp_enddl};

struct Load {
    image_siz: u8,
    width: u16,
    tile: TileDescriptor,
    opcode: u8,
    bounds: [u16; 4],
}

impl Load {
    fn run(&self, source: &[u8]) -> (Rdp, CensusLoad) {
        let mut dl = DlBuilder::new();
        let address = dl.bytes(8, source);
        let tile = &self.tile;
        let [s, t, sh, th] = self.bounds.map(u32::from);
        dl.list(
            "main",
            &[
                gdp_set_texture_image(0, self.image_siz.into(), self.width.into(), address),
                gdp_set_tile(
                    tile.fmt.into(),
                    tile.siz.into(),
                    tile.line.into(),
                    tile.tmem_addr.into(),
                    7,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                ),
                (
                    (u32::from(self.opcode) << 24) | (s << 12) | t,
                    (7 << 24) | (sh << 12) | th,
                ),
                gsp_enddl(),
            ],
        );
        let built = dl.finish("main");
        let recorder = Recorder::new(Mode::Census);
        let result = interpret_profiled(
            RdramImage::new(&built.rdram),
            built.entry.into(),
            Microcode::F3dex2.into(),
            DataFormat::Fixed,
            Rdp::default(),
            Default::default(),
            None,
            recorder.clone(),
        );
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        let loads = recorder.drain().census_loads;
        assert_eq!(loads.len(), 1);
        (result.rdp, loads.into_iter().next().unwrap())
    }
}

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn assert_bank(rdp: &Rdp, low: &str, high: &str, name: &str) {
    let mut expected = [0; 4096];
    for (base, bytes) in [(0, hex(low)), (0x800, hex(high))] {
        expected[base..base + bytes.len()].copy_from_slice(&bytes);
    }
    for (address, (&actual, expected)) in rdp
        .tmem_bank
        .encoded_bank()
        .iter()
        .zip(expected)
        .enumerate()
    {
        assert_eq!(actual, expected, "{name}: TMEM[{address:03x}]");
    }
}

pub(crate) struct LiteralLoad {
    pub name: String,
    pub rdp: Rdp,
    pub tile: TileDescriptor,
    pub expected: Vec<u8>,
}

pub(crate) fn literal_load_vectors() -> Vec<LiteralLoad> {
    let mut vectors = Vec::new();
    for (image, fmt, siz, count, low, high) in [
        (1, 4, 1, 16, "0102030405060708090a0b0c0d0e0f10", ""),
        (2, 0, 2, 8, "0102030405060708090a0b0c0d0e0f10", ""),
        (3, 0, 3, 4, "01020506090a0d0e", "030407080b0c0f10"),
        (2, 4, 1, 8, "090a0b0c0d0e0f10", ""),
        (
            1,
            4,
            2,
            16,
            "01020304050607080000000000000000090a0b0c0d0e0f10",
            "",
        ),
        (
            2,
            0,
            3,
            8,
            "0102050600000000090a0d0e",
            "03040708000000000b0c0f10",
        ),
        (3, 0, 2, 4, "090a0b0c0d0e0f10", ""),
        (3, 3, 3, 4, "090a0b0c0d0e0f10", ""),
        (
            1,
            0,
            3,
            16,
            "01020506000000000000000000000000090a0d0e",
            "030407080000000000000000000000000b0c0f10",
        ),
    ] {
        for opcode in [0xf3, 0xf4] {
            let name = format!("load-{opcode:02x}-image{image}-fmt{fmt}-siz{siz}");
            let load = Load {
                image_siz: image,
                width: 32,
                tile: TileDescriptor {
                    fmt,
                    siz,
                    ..Default::default()
                },
                opcode,
                bounds: [0, 0, (count - 1) * if opcode == 0xf4 { 4 } else { 1 }, 0],
            };
            let (rdp, observed) = load.run(&(1..=32).collect::<Vec<_>>());
            assert_eq!(
                observed.source,
                (1..=16).collect::<Vec<_>>(),
                "{name}: source words"
            );
            assert_bank(&rdp, low, high, &name);
            let (fmt, siz, expected) = if high.is_empty() {
                (
                    4,
                    1,
                    hex(low)
                        .into_iter()
                        .flat_map(|v| [v; 4])
                        .collect::<Vec<_>>(),
                )
            } else {
                let low = hex(low);
                let high = hex(high);
                (
                    0,
                    3,
                    low.as_chunks::<2>()
                        .0
                        .iter()
                        .zip(high.as_chunks::<2>().0)
                        .flat_map(|(lo, hi)| [lo[0], lo[1], hi[0], hi[1]])
                        .collect::<Vec<_>>(),
                )
            };
            vectors.push(LiteralLoad {
                name,
                rdp,
                tile: TileDescriptor {
                    fmt,
                    siz,
                    width: (expected.len() / 4) as u16,
                    height: 1,
                    ..Default::default()
                },
                expected,
            });
        }
    }
    vectors.extend(tlut_spacing_vectors());
    vectors.extend(tlut_image_vectors());
    vectors
}

#[test]
fn load_sizes_follow_literal_angrylion_banks_and_source_counts() {
    for vector in literal_load_vectors() {
        assert_eq!(
            vector.rdp.tmem_bank.sample_tile(&vector.tile, 0).unwrap(),
            vector.expected,
            "{}",
            vector.name
        );
    }
}

#[test]
fn load_bank_sorting_rotates_halfwords_and_splits_yuv_bytes() {
    for (fmt, low, high) in [
        (4, "11121b1c1d1e1f20191a", ""),
        (1, "01030d0f191b1d1f090b", "02040e101a1c1e200a0c"),
    ] {
        for opcode in [0xf3, 0xf4] {
            let (rdp, observed) = Load {
                image_siz: 3,
                width: 32,
                tile: TileDescriptor {
                    fmt,
                    siz: 1,
                    ..Default::default()
                },
                opcode,
                bounds: [0, 0, if opcode == 0xf4 { 28 } else { 7 }, 0],
            }
            .run(&(1..=32).collect::<Vec<_>>());
            assert_eq!(observed.source.len(), 32);
            assert_bank(&rdp, low, high, "halfword rotation");
        }
    }
}

#[test]
fn load_source_offsets_and_odd_rows_keep_image_sizing() {
    for (opcode, bounds, expected) in [
        (0xf3, [3, 1, 10, 0], "1415161718191a1b"),
        (0xf4, [12, 4, 40, 8], "1415161718191a1b28292a2b24252627"),
    ] {
        let (rdp, observed) = Load {
            image_siz: 1,
            width: 16,
            tile: TileDescriptor {
                fmt: 4,
                siz: 1,
                line: 1,
                ..Default::default()
            },
            opcode,
            bounds,
        }
        .run(&(1..=64).collect::<Vec<_>>());
        assert_eq!(observed.source[0], 0x14);
        assert_eq!(observed.source.len(), if opcode == 0xf4 { 24 } else { 8 });
        assert_bank(&rdp, expected, "", "load-offset");
    }
}

#[test]
fn load_dxt_rounding_flips_the_third_and_fourth_words() {
    let (rdp, _) = Load {
        image_siz: 1,
        width: 32,
        tile: TileDescriptor {
            fmt: 4,
            siz: 1,
            ..Default::default()
        },
        opcode: 0xf3,
        bounds: [0, 0, 31, 0x555],
    }
    .run(&(1..=32).collect::<Vec<_>>());
    assert_bank(
        &rdp,
        "0102030405060708090a0b0c0d0e0f1015161718111213141d1e1f20191a1b1c",
        "",
        "dxt-rounding",
    );
}

fn tlut_spacing_vectors() -> Vec<LiteralLoad> {
    let mut vectors = Vec::new();
    for (siz, low, high, pixel) in [
        (0, "", "01020102010201020304030403040304", [3, 3, 3, 4]),
        (
            1,
            "",
            "010201020102010200000000000000000304030403040304",
            [0; 4],
        ),
        (
            2,
            "",
            "01020102010201020000000000000000000000000000000000000000000000000304030403040304",
            [0; 4],
        ),
        (
            3,
            "010201020000000000000000000000000000000000000000000000000000000003040304",
            "010201020000000000000000000000000000000000000000000000000000000003040304",
            [0; 4],
        ),
    ] {
        let (rdp, _) = Load {
            image_siz: 2,
            width: 32,
            tile: TileDescriptor {
                siz,
                tmem_addr: 0x100,
                ..Default::default()
            },
            opcode: 0xf0,
            bounds: [0, 0, 4, 0],
        }
        .run(&[1, 2, 3, 4]);
        assert_bank(&rdp, low, high, "tlut-spacing");
        let tile = TileDescriptor {
            fmt: 3,
            siz: 2,
            tmem_addr: 0x101,
            width: 1,
            height: 1,
            ..Default::default()
        };
        vectors.push(LiteralLoad {
            name: format!("tlut-tile-size{siz}"),
            rdp,
            tile,
            expected: pixel.to_vec(),
        });
    }
    vectors
}

fn tlut_image_vectors() -> Vec<LiteralLoad> {
    let mut vectors = Vec::new();
    for (siz, high, source_len, pixel) in [
        (1, "0102010201020102", 2, [1, 1, 1, 2]),
        (2, "0102010201020102030403040304030405060506050605060708070807080708090a090a090a090a0b0c0b0c0b0c0b0c0d0e0d0e0d0e0d0e0f100f100f100f10", 16, [1, 1, 1, 2]),
        (3, "090a090a090a090a191a191a191a191a", 26, [9, 9, 9, 10]),
    ] {
        let (rdp, observed) = Load {
            image_siz: siz, width: 32,
            tile: TileDescriptor { tmem_addr: 0x100, ..Default::default() },
            opcode: 0xf0, bounds: [0, 0, 28, 0],
        }.run(&(1..=32).collect::<Vec<_>>());
        assert_eq!(observed.source.len(), source_len, "image size {siz}");
        assert_bank(&rdp, "", high, "tlut-image-size");
        let tile = TileDescriptor { fmt: 3, siz: 2, tmem_addr: 0x100, width: 1, height: 1, ..Default::default() };
        vectors.push(LiteralLoad { name: format!("tlut-image-size{siz}"), rdp, tile, expected: pixel.to_vec() });
    }
    vectors
}

#[test]
fn canonical_sdk_tlut_replicates_all_256_slots() {
    let source: Vec<_> = (0u8..=255).flat_map(|v| [v, v ^ 255]).collect();
    let (rdp, observed) = Load {
        image_siz: 2,
        width: 1,
        tile: TileDescriptor {
            tmem_addr: 0x100,
            ..Default::default()
        },
        opcode: 0xf0,
        bounds: [0, 0, 1020, 0],
    }
    .run(&source);
    assert_eq!(observed.source, source);
    assert!(rdp.tmem_bank.encoded_bank()[..0x800]
        .iter()
        .all(|v| *v == 0));
    for (index, slot) in rdp
        .tmem_bank
        .palette()
        .as_chunks::<8>()
        .0
        .iter()
        .enumerate()
    {
        assert_eq!(slot.as_slice(), [index as u8, index as u8 ^ 255].repeat(4));
    }
}

#[test]
fn reference_crashing_load_inputs_keep_ultragfx_defined_behavior() {
    for opcode in [0xf0, 0xf3, 0xf4] {
        for (fmt, siz) in [(4, 0), (0, 3), (1, 2)] {
            let (rdp, _) = Load {
                image_siz: 0,
                width: 32,
                tile: TileDescriptor {
                    fmt,
                    siz,
                    ..Default::default()
                },
                opcode,
                bounds: [0, 0, 4, 0],
            }
            .run(&(1..=32).collect::<Vec<_>>());
            let expected = if opcode == 0xf0 {
                "01020102010201020304030403040304"
            } else {
                "0102030405060708"
            };
            assert_bank(&rdp, expected, "", "image4 extension");
        }
    }
    let (rdp, _) = Load {
        image_siz: 2,
        width: 32,
        tile: TileDescriptor {
            tmem_addr: 0x100,
            ..Default::default()
        },
        opcode: 0xf0,
        bounds: [0, 0, 4, 4],
    }
    .run(&(1..=68).collect::<Vec<_>>());
    assert_bank(
        &rdp,
        "",
        "41424142414241424344434443444344",
        "multirow TLUT extension",
    );
}
