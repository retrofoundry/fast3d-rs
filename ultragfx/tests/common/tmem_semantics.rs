#[derive(Clone)]
pub struct Format {
    pub name: &'static str,
    pub fmt: u32,
    pub siz: u32,
    pub encoded: Vec<u8>,
    pub pixels: Vec<[u8; 4]>,
}

pub fn formats() -> Vec<Format> {
    vec![
        Format {
            name: "rgba16",
            fmt: 0,
            siz: 2,
            encoded: vec![0xf8, 1, 7, 0xc1, 0, 0x3f, 0xff, 0xff],
            pixels: vec![
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [255; 4],
            ],
        },
        Format {
            name: "rgba32",
            fmt: 0,
            siz: 3,
            encoded: vec![
                17, 43, 89, 255, 101, 151, 199, 255, 211, 71, 31, 255, 239, 227, 181, 255,
            ],
            pixels: vec![
                [17, 43, 89, 255],
                [101, 151, 199, 255],
                [211, 71, 31, 255],
                [239, 227, 181, 255],
            ],
        },
        Format {
            name: "i4",
            fmt: 4,
            siz: 0,
            encoded: vec![0x37, 0xbf],
            pixels: vec![[51; 4], [119; 4], [187; 4], [255; 4]],
        },
        Format {
            name: "i8",
            fmt: 4,
            siz: 1,
            encoded: vec![37, 91, 173, 241],
            pixels: vec![[37; 4], [91; 4], [173; 4], [241; 4]],
        },
        Format {
            name: "ia4",
            fmt: 3,
            siz: 0,
            encoded: vec![0x37, 0xbf],
            pixels: vec![
                [36, 36, 36, 255],
                [109, 109, 109, 255],
                [182, 182, 182, 255],
                [255; 4],
            ],
        },
        Format {
            name: "ia8",
            fmt: 3,
            siz: 1,
            encoded: vec![0x3f, 0x7f, 0xbf, 0xff],
            pixels: vec![
                [51, 51, 51, 255],
                [119, 119, 119, 255],
                [187, 187, 187, 255],
                [255; 4],
            ],
        },
        Format {
            name: "ia16",
            fmt: 3,
            siz: 2,
            encoded: vec![37, 255, 91, 255, 173, 255, 241, 255],
            pixels: vec![
                [37, 37, 37, 255],
                [91, 91, 91, 255],
                [173, 173, 173, 255],
                [241, 241, 241, 255],
            ],
        },
        Format {
            name: "ci4",
            fmt: 2,
            siz: 0,
            encoded: vec![0x01, 0x23],
            pixels: vec![
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [255; 4],
            ],
        },
        Format {
            name: "ci8",
            fmt: 2,
            siz: 1,
            encoded: vec![0, 1, 2, 3],
            pixels: vec![
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [255; 4],
            ],
        },
    ]
}

pub const FIXTURES: [&str; 3] = ["tmem-layouts", "tmem-tlut-mutation", "tmem-roles-lifetime"];
pub const TLUT_COLORS: [[u8; 4]; 5] = [
    [255, 0, 0, 255],
    [0, 255, 0, 255],
    [0, 255, 0, 255],
    [0, 255, 0, 255],
    [0, 0, 255, 255],
];

pub fn expected_pixels(name: &str) -> Vec<u8> {
    let mut pixels = [0, 0, 0, 255].repeat(320 * 240);
    let mut put = |x: usize, y: usize, color: [u8; 4]| {
        let offset = (y * 320 + x) * 4;
        pixels[offset..offset + 4].copy_from_slice(&color);
    };
    match name {
        "tmem-layouts" => {
            for (row, format) in formats().iter().enumerate() {
                for y in 0..3 {
                    for x in 0..16 {
                        let [r, g, b, a] = format.pixels[x % 4];
                        put(16 + x, 8 + row * 8 + y, [r, g, b, 255]);
                        put(48 + x, 8 + row * 8 + y, [a, a, a, 255]);
                    }
                }
            }
        }
        "tmem-tlut-mutation" => {
            for (draw, color) in TLUT_COLORS.into_iter().enumerate() {
                for y in 24..28 {
                    for x in 16 + draw * 16..24 + draw * 16 {
                        put(x, y, color);
                    }
                }
            }
        }
        "tmem-roles-lifetime" => {
            for (draw, value) in [37, 91, 173].into_iter().enumerate() {
                for y in 112..128 {
                    for x in 136 + draw * 16..152 + draw * 16 {
                        put(x, y, [value, value, value, 255]);
                    }
                }
            }
        }
        _ => panic!("unregistered fixture {name}"),
    }
    pixels
}

pub fn assert_pixels(name: &str, pixels: &[u8]) {
    let expected = expected_pixels(name);
    assert_eq!(pixels.len(), expected.len(), "{name}");
    for (i, (actual, expected)) in pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected.as_chunks::<4>().0)
        .enumerate()
    {
        assert_eq!(actual, expected, "{name} pixel ({},{})", i % 320, i / 320);
    }
}
