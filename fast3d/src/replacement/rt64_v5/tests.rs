use super::*;
use crate::hle::texture_request::EncodedTextureRequest;
use std::{collections::BTreeSet, sync::Arc};

fn recipe() -> DecodeRecipe {
    DecodeRecipe {
        representation: Representation::Tile,
        output: [6, 2],
        logical: [6, 2],
        fmt: 4,
        siz: 1,
        base: 1,
        line: 2,
        palette: 0,
        tlut: 0,
        palette_lanes: false,
    }
}

#[test]
fn partial_word_and_split_bank_literals() {
    let mut r = recipe();
    for (width, odd) in [
        (1, vec![28]),
        (2, vec![28, 29]),
        (3, vec![28, 29, 30]),
        (4, vec![28, 29, 30, 31]),
        (5, vec![24, 28, 29, 30, 31]),
        (6, vec![24, 25, 28, 29, 30, 31]),
        (7, vec![24, 25, 26, 28, 29, 30, 31]),
    ] {
        r.output[0] = width;
        r.logical = r.output;
        let expected: Vec<_> = (8..8 + width as usize).chain(odd).collect();
        assert_eq!(
            spans(&r).unwrap().into_iter().flatten().collect::<Vec<_>>(),
            expected
        );
    }
    r.fmt = 0;
    r.siz = 3;
    r.output = [3, 2];
    r.logical = r.output;
    assert_eq!(
        spans(&r).unwrap().into_iter().flatten().collect::<Vec<_>>(),
        [
            8, 9, 10, 11, 12, 13, 24, 25, 28, 29, 30, 31, 2056, 2057, 2058, 2059, 2060, 2061, 2072,
            2073, 2076, 2077, 2078, 2079
        ]
    );
    let mut bank = [0; 4096];
    for (a, value) in [
        (8, 11),
        (9, 12),
        (10, 21),
        (11, 22),
        (12, 31),
        (13, 32),
        (2056, 13),
        (2057, 14),
        (2058, 23),
        (2059, 24),
        (2060, 33),
        (2061, 34),
        (28, 41),
        (29, 42),
        (30, 51),
        (31, 52),
        (24, 61),
        (25, 62),
        (2076, 43),
        (2077, 44),
        (2078, 53),
        (2079, 54),
        (2072, 63),
        (2073, 64),
    ] {
        bank[a] = value;
    }
    let request =
        EncodedTextureRequest::from_owned_parts(EncodedInput::Tmem(Arc::new(bank)), r.clone());
    assert_eq!(
        request.decode(),
        [
            11, 12, 13, 14, 21, 22, 23, 24, 31, 32, 33, 34, 41, 42, 43, 44, 51, 52, 53, 54, 61, 62,
            63, 64
        ]
    );
    let target = alias(&r, request.input(), &Recorder::default()).unwrap();
    assert_eq!(
        &target.witness[..24],
        [
            11, 12, 21, 22, 31, 32, 61, 62, 41, 42, 51, 52, 13, 14, 23, 24, 33, 34, 63, 64, 43, 44,
            53, 54
        ]
    );
    assert_eq!(&target.witness[24..], [3, 0, 2, 0, 0, 0, 0, 0, 2, 0, 3, 0]);
}

#[test]
fn admitted_spans_cover_exactly_the_decoder_reads() {
    let mut admitted = 0;
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
        for width in 1..=32 {
            for height in 1..=4 {
                for base in [0, 1, 253, 254, 255, 256, 509, 510, 511] {
                    for line in [1, 2, 4, 8] {
                        let r = DecodeRecipe {
                            fmt,
                            siz,
                            output: [width, height],
                            logical: [width, height],
                            base,
                            line,
                            tlut: if fmt == 2 { 2 } else { 0 },
                            ..recipe()
                        };
                        let Some(selected) = spans(&r) else {
                            continue;
                        };
                        let selected: BTreeSet<_> = selected.into_iter().flatten().collect();
                        let mut reads = BTreeSet::new();
                        let mask = if fmt == 2 || siz == 3 { 2047 } else { 4095 };
                        for y in 0..height as usize {
                            for x in 0..width as usize {
                                for byte in 0..if siz >= 2 { 2 } else { 1 } {
                                    let address = (usize::from(base) * 8
                                        + ((y * usize::from(line) * 8
                                            + ((x << siz.min(2)) >> 1)
                                            + byte)
                                            ^ if y & 1 != 0 { 4 } else { 0 }))
                                        & mask;
                                    reads.insert(address);
                                    if siz == 3 {
                                        reads.insert(address | 2048);
                                    }
                                }
                            }
                        }
                        assert_eq!(selected, reads, "{r:?}");
                        admitted += 1;
                    }
                }
            }
        }
    }
    assert_eq!(admitted, 14096);
    eprintln!("admitted literal-address comparisons: {admitted}");
}

#[test]
fn admission_boundaries_are_explicit() {
    let r = recipe();
    let excluded = [
        DecodeRecipe {
            line: 0,
            ..r.clone()
        },
        DecodeRecipe {
            line: 1,
            output: [9, 2],
            logical: [9, 2],
            ..r.clone()
        },
        DecodeRecipe {
            base: 511,
            ..r.clone()
        },
        DecodeRecipe {
            siz: 0,
            output: [3, 2],
            logical: [3, 2],
            ..r.clone()
        },
        DecodeRecipe {
            fmt: 2,
            tlut: 0,
            ..r.clone()
        },
        DecodeRecipe {
            fmt: 2,
            tlut: 1,
            ..r.clone()
        },
        DecodeRecipe {
            tlut: 2,
            ..r.clone()
        },
        DecodeRecipe {
            fmt: 1,
            ..r.clone()
        },
        DecodeRecipe {
            representation: Representation::Lookup4096x4,
            ..r.clone()
        },
        DecodeRecipe {
            output: [6, 0],
            logical: [6, 0],
            ..r.clone()
        },
    ];
    for r in excluded {
        assert!(spans(&r).is_none(), "{r:?}");
    }
    let end = DecodeRecipe {
        base: 510,
        line: 1,
        output: [4, 2],
        logical: [4, 2],
        ..r
    };
    assert_eq!(spans(&end).unwrap(), [4080..4084, 4092..4096]);
    let over = DecodeRecipe {
        output: [5, 2],
        logical: [5, 2],
        ..end
    };
    assert!(spans(&over).is_some()); // The odd tail reads 4088 and 4092..4096, all in-bank.
}

#[test]
fn relocation_requires_equal_resolved_input_and_interpretation() {
    let r = DecodeRecipe {
        fmt: 2,
        siz: 0,
        palette: 1,
        tlut: 2,
        output: [4, 2],
        logical: [4, 2],
        ..recipe()
    };
    let mut a = [0; 4096];
    a[8] = 0x12;
    a[9] = 0x21;
    a[28] = 0x21;
    a[29] = 0x12;
    a[2184..2192].copy_from_slice(&[0xf8, 1, 0xf8, 1, 0xf8, 1, 0xf8, 1]);
    a[2192..2200].copy_from_slice(&[0, 0x3f, 0, 0x3f, 0, 0x3f, 0, 0x3f]);
    let mut b = [0; 4096];
    b[40..42].copy_from_slice(&a[8..10]);
    b[60..62].copy_from_slice(&a[28..30]);
    b[2440..2456].copy_from_slice(&a[2184..2200]);
    let moved = DecodeRecipe {
        base: 5,
        palette: 3,
        ..r.clone()
    };
    let input_a = EncodedInput::Tmem(Arc::new(a));
    let input_b = EncodedInput::Tmem(Arc::new(b));
    let a = alias(&r, &input_a, &Recorder::default()).unwrap();
    let b = alias(&moved, &input_b, &Recorder::default()).unwrap();
    assert_eq!(a, b);
    let req_a = EncodedTextureRequest::from_owned_parts(input_a, r.clone());
    let req_b = EncodedTextureRequest::from_owned_parts(input_b, moved);
    assert_ne!(req_a.key(), req_b.key());
    assert_eq!(req_a.decode(), req_b.decode());
    let ia = DecodeRecipe { tlut: 3, ..r };
    assert_ne!(a, alias(&ia, req_a.input(), &Recorder::default()).unwrap());
}
