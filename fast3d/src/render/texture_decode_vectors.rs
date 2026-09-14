use std::sync::Arc;

use super::profiling::{Bank, Request};

#[derive(Clone, Copy, PartialEq)]
enum Representation {
    Tile,
    Lookup4096x4,
    LinearCompat,
}

#[derive(Clone)]
struct DecodeRecipe {
    representation: Representation,
    output: [u32; 2],
    logical: [u32; 2],
    fmt: u8,
    siz: u8,
    base: u16,
    line: u16,
    palette: u8,
    tlut: u8,
}

enum EncodedInput {
    Tmem(Arc<[u8; 4096]>),
    LinearCompat {
        bytes: Arc<[u8]>,
        palette_bank: Arc<[u8; 4096]>,
    },
}

fn request(input: EncodedInput, recipe: DecodeRecipe) -> Request {
    let (bank, linear) = match input {
        EncodedInput::Tmem(bank) => (bank, None),
        EncodedInput::LinearCompat {
            bytes,
            palette_bank,
        } => (palette_bank, Some(bytes.to_vec())),
    };
    let mut result = Request {
        representation: match recipe.representation {
            Representation::Tile => super::profiling::Representation::Tile,
            Representation::Lookup4096x4 => super::profiling::Representation::Lookup,
            Representation::LinearCompat => super::profiling::Representation::Linear,
        },
        role: "decode-witness".into(),
        tile: Default::default(),
        tlut: recipe.tlut,
        extent: recipe.output,
        bank: Bank {
            bytes: bank.to_vec(),
            sources: Vec::new(),
            blocks: Vec::new(),
        },
        linear,
        reachable_bytes: 0,
        read_operations: 0,
        palette_bytes: 0,
        output: Vec::new(),
        rejection: None,
    };
    result.tile.fmt = recipe.fmt;
    result.tile.siz = recipe.siz;
    result.tile.width = recipe.logical[0] as u16;
    result.tile.height = recipe.logical[1] as u16;
    result.tile.tmem_addr = recipe.base;
    result.tile.line = recipe.line;
    result.tile.palette = recipe.palette;
    result
}

pub(super) fn oracle(request: &Request) -> Vec<u8> {
    request
        .decode_prepared(&request.prepare().unwrap())
        .unwrap()
}

pub(super) struct Vector {
    pub name: String,
    pub request: Request,
    pub expected: Vec<u8>,
}

const C5: [u8; 32] = [
    0, 8, 16, 24, 33, 41, 49, 57, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173,
    181, 189, 198, 206, 214, 222, 231, 239, 247, 255,
];
const C3: [u8; 8] = [0, 36, 73, 109, 146, 182, 219, 255];

fn rgba(value: u16) -> [u8; 4] {
    [
        C5[(value >> 11) as usize],
        C5[((value >> 6) & 31) as usize],
        C5[((value >> 1) & 31) as usize],
        if value & 1 == 0 { 0 } else { 255 },
    ]
}

fn recipe(fmt: u8, siz: u8, width: u32, height: u32) -> DecodeRecipe {
    DecodeRecipe {
        representation: Representation::Tile,
        output: [width, height],
        logical: [width, height],
        fmt,
        siz,
        base: 0,
        line: 0,
        palette: 0,
        tlut: 0,
    }
}

fn row_variants(
    vectors: &mut Vec<Vector>,
    name: &str,
    recipe: DecodeRecipe,
    bytes: &[u8],
    bank: [u8; 4096],
    expected: &[u8],
) {
    for representation in [Representation::Tile, Representation::LinearCompat] {
        if representation == Representation::LinearCompat && recipe.siz == 3 {
            continue;
        }
        let input = if representation == Representation::Tile {
            EncodedInput::Tmem(Arc::new(bank))
        } else {
            EncodedInput::LinearCompat {
                bytes: bytes.into(),
                palette_bank: Arc::new(bank),
            }
        };
        vectors.push(Vector {
            name: format!("{name}-{}", representation as u8),
            request: request(
                input,
                DecodeRecipe {
                    representation,
                    ..recipe.clone()
                },
            ),
            expected: expected.to_vec(),
        });
    }
}

pub(super) fn component_vectors() -> Vec<Vector> {
    let mut vectors = Vec::new();
    for page in 0..32u32 {
        for fmt in [0, 3] {
            let mut bytes = Vec::new();
            let mut expected = Vec::new();
            for value in page * 2048..(page + 1) * 2048 {
                bytes.extend((value as u16).to_be_bytes());
                expected.extend(if fmt == 0 {
                    rgba(value as u16)
                } else {
                    [
                        (value >> 8) as u8,
                        (value >> 8) as u8,
                        (value >> 8) as u8,
                        value as u8,
                    ]
                });
            }
            row_variants(
                &mut vectors,
                &format!("component-{fmt}-16-{page}"),
                recipe(fmt, 2, 2048, 1),
                &bytes,
                bytes.as_slice().try_into().unwrap(),
                &expected,
            );
        }
    }
    for (fmt, siz) in [(3, 0), (3, 1), (4, 0), (4, 1)] {
        let bytes: Vec<u8> = (0..=255).collect();
        let mut bank = [0; 4096];
        bank[..256].copy_from_slice(&bytes);
        let expected: Vec<_> = bytes
            .iter()
            .flat_map(|&byte| {
                if siz == 1 {
                    if fmt == 4 {
                        vec![byte; 4]
                    } else {
                        vec![
                            byte / 16 * 17,
                            byte / 16 * 17,
                            byte / 16 * 17,
                            byte % 16 * 17,
                        ]
                    }
                } else {
                    [byte / 16, byte % 16]
                        .into_iter()
                        .flat_map(|nibble| {
                            if fmt == 4 {
                                [nibble * 17; 4]
                            } else {
                                [
                                    C3[(nibble / 2) as usize],
                                    C3[(nibble / 2) as usize],
                                    C3[(nibble / 2) as usize],
                                    if nibble & 1 == 0 { 0 } else { 255 },
                                ]
                            }
                        })
                        .collect()
                }
            })
            .collect();
        row_variants(
            &mut vectors,
            &format!("component-{fmt}-{siz}"),
            recipe(fmt, siz, if siz == 0 { 512 } else { 256 }, 1),
            &bytes,
            bank,
            &expected,
        );
    }
    for channel in 0..4 {
        let mut bank = [0; 4096];
        let mut expected = Vec::new();
        for value in 0..=255usize {
            let mut pixel = [17, 73, 149, 211];
            pixel[channel] = value as u8;
            bank[value * 2..value * 2 + 2].copy_from_slice(&pixel[..2]);
            bank[2048 + value * 2..2050 + value * 2].copy_from_slice(&pixel[2..]);
            expected.extend(pixel);
        }
        row_variants(
            &mut vectors,
            &format!("component-rgba32-{channel}"),
            recipe(0, 3, 256, 1),
            &[],
            bank,
            &expected,
        );
    }
    for mode in 0..4 {
        let pages = if mode >= 2 { 256 } else { 1 };
        for page in 0..pages {
            let mut bank = [0; 4096];
            let bytes: Vec<u8> = (0..=255).collect();
            bank[..256].copy_from_slice(&bytes);
            let mut expected = Vec::new();
            for index in 0..256usize {
                let value = ((page << 8) | index) as u16;
                bank[2048 + index * 8..2050 + index * 8].copy_from_slice(&value.to_be_bytes());
                expected.extend(match mode {
                    2 => rgba(value),
                    3 => [page as u8, page as u8, page as u8, index as u8],
                    _ => [0; 4],
                });
            }
            let mut descriptor = recipe(2, 1, 256, 1);
            descriptor.tlut = mode;
            row_variants(
                &mut vectors,
                &format!("component-ci8-{mode}-{page}"),
                descriptor,
                &bytes,
                bank,
                &expected,
            );
        }
        for palette in 0..16 {
            let mut bank = [0; 4096];
            let bytes = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
            bank[..8].copy_from_slice(&bytes);
            let mut expected = Vec::new();
            for index in 0..16usize {
                let value = ((palette * 16 + index) * 257) as u16;
                let address = 2048 + palette * 128 + index * 8;
                bank[address..address + 2].copy_from_slice(&value.to_be_bytes());
                expected.extend(match mode {
                    2 => rgba(value),
                    3 => [value as u8; 4],
                    _ => [0; 4],
                });
            }
            let mut descriptor = recipe(2, 0, 16, 1);
            descriptor.palette = palette as u8;
            descriptor.tlut = mode;
            row_variants(
                &mut vectors,
                &format!("component-ci4-{mode}-{palette}"),
                descriptor,
                &bytes,
                bank,
                &expected,
            );
        }
    }
    vectors
}

pub(super) fn layout_vectors() -> Vec<Vector> {
    let mut vectors = Vec::new();
    let bank = Arc::new(std::array::from_fn(|i| {
        ((i * 73 + i / 7 + (i >> 8) * 31) & 255) as u8
    }));
    for (fmt, siz) in [
        (0, 2),
        (0, 3),
        (2, 0),
        (2, 1),
        (3, 0),
        (3, 1),
        (3, 2),
        (4, 0),
        (4, 1),
    ] {
        for base in [0, 1, 255, 256, 511] {
            for line in [0, 1, 3, 511] {
                for mode in if fmt == 2 {
                    &[0, 1, 2, 3][..]
                } else {
                    &[3][..]
                } {
                    for representation in [Representation::Tile, Representation::Lookup4096x4] {
                        let descriptor = DecodeRecipe {
                            representation,
                            output: if representation == Representation::Tile {
                                [19, 5]
                            } else {
                                [4096, 4]
                            },
                            logical: [19, 5],
                            base,
                            line,
                            palette: 15,
                            tlut: *mode,
                            ..recipe(fmt, siz, 19, 5)
                        };
                        let request = request(EncodedInput::Tmem(bank.clone()), descriptor);
                        let expected = oracle(&request);
                        vectors.push(Vector {
                            name: format!(
                                "layout-{fmt}-{siz}-{base}-{line}-{mode}-{}",
                                representation as u8
                            ),
                            request,
                            expected,
                        });
                    }
                }
            }
        }
    }
    for (fmt, siz) in [
        (0, 2),
        (2, 0),
        (2, 1),
        (3, 0),
        (3, 1),
        (3, 2),
        (4, 0),
        (4, 1),
    ] {
        let descriptor = DecodeRecipe {
            representation: Representation::LinearCompat,
            base: 511,
            line: 511,
            palette: 15,
            tlut: 3,
            ..recipe(fmt, siz, 3, 3)
        };
        let length = (9usize << siz).div_ceil(2);
        let request = request(
            EncodedInput::LinearCompat {
                bytes: bank[..length].into(),
                palette_bank: bank.clone(),
            },
            descriptor,
        );
        let expected = oracle(&request);
        vectors.push(Vector {
            name: format!("layout-linear-odd-{fmt}-{siz}"),
            request,
            expected,
        });
    }
    vectors
}

pub(super) fn literal_layout_vectors() -> Vec<Vector> {
    let mut vectors = Vec::new();
    let mut bank = [0xee; 4096];
    for (address, value) in [0, 1, 2, 28, 29, 30, 48, 49, 50]
        .into_iter()
        .zip([0x12, 0x34, 0x50, 0x67, 0x89, 0xa0, 0xbc, 0xde, 0xf0])
    {
        bank[address] = value;
    }
    vectors.push(Vector {
        name: "literal-odd-width-padded-rows".into(),
        request: request(
            EncodedInput::Tmem(Arc::new(bank)),
            DecodeRecipe {
                line: 3,
                ..recipe(4, 0, 5, 3)
            },
        ),
        expected: [
            17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255,
        ]
        .into_iter()
        .flat_map(|v| [v; 4])
        .collect(),
    });
    let mut bank = [0; 4096];
    for (address, value) in [
        (2046, 1),
        (2047, 2),
        (4094, 3),
        (4095, 4),
        (0, 5),
        (1, 6),
        (2048, 7),
        (2049, 8),
    ] {
        bank[address] = value;
    }
    vectors.push(Vector {
        name: "literal-rgba32-bank-wrap".into(),
        request: request(
            EncodedInput::Tmem(Arc::new(bank)),
            DecodeRecipe {
                base: 255,
                line: 2,
                ..recipe(0, 3, 5, 1)
            },
        ),
        expected: vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8],
    });
    let mut bank = [193; 4096];
    bank[4088] = 1;
    bank[4092] = 5;
    vectors.push(Vector {
        name: "literal-zero-line-nonci-tlut".into(),
        request: request(
            EncodedInput::Tmem(Arc::new(bank)),
            DecodeRecipe {
                base: 511,
                tlut: 3,
                ..recipe(4, 1, 1, 3)
            },
        ),
        expected: vec![1, 1, 1, 1, 5, 5, 5, 5, 1, 1, 1, 1],
    });
    let mut bank = [0x12; 4096];
    bank[0] = 0xab;
    bank[4095] = 0xcd;
    let mut rotated = bank;
    rotated.rotate_left(4088);
    let mut expected = Vec::new();
    for odd in [false, true] {
        let mut plane = rotated;
        if odd {
            for word in plane.as_chunks_mut::<8>().0 {
                word.rotate_left(4);
            }
        }
        for low in [false, true] {
            for byte in plane {
                expected.extend([if low { byte % 16 * 17 } else { byte / 16 * 17 }; 4]);
            }
        }
    }
    vectors.push(Vector {
        name: "literal-lookup-four-planes-wrap".into(),
        request: request(
            EncodedInput::Tmem(Arc::new(bank)),
            DecodeRecipe {
                representation: Representation::Lookup4096x4,
                base: 511,
                output: [4096, 4],
                ..recipe(4, 0, 1, 1)
            },
        ),
        expected,
    });
    let bytes = [0x12, 0x34, 0x56, 0x78, 0x90];
    vectors.push(Vector {
        name: "literal-linear-odd-width-nibbles".into(),
        request: request(
            EncodedInput::LinearCompat {
                bytes: bytes.into(),
                palette_bank: Arc::new([0; 4096]),
            },
            DecodeRecipe {
                representation: Representation::LinearCompat,
                base: 511,
                line: 511,
                ..recipe(4, 0, 3, 3)
            },
        ),
        expected: [17, 34, 51, 68, 85, 102, 119, 136, 153]
            .into_iter()
            .flat_map(|v| [v; 4])
            .collect(),
    });
    vectors
}
