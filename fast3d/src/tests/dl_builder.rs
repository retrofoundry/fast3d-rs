use std::collections::BTreeMap;

use n64_gbi::encode::{mtx_to_bytes, Vp, VtxColored};
use n64_gbi::gu::Mtx4;

pub(crate) type Command = (u32, u32);

pub(crate) struct Built {
    pub rdram: Vec<u8>,
    pub entry: u32,
}

pub(crate) struct DlBuilder {
    rdram: Vec<u8>,
    lists: BTreeMap<&'static str, u32>,
}

pub(crate) struct Light {
    pub color: [u8; 3],
    pub direction: [i8; 3],
}

impl DlBuilder {
    pub fn new() -> Self {
        Self {
            rdram: vec![0; 16],
            lists: BTreeMap::new(),
        }
    }

    pub fn bytes(&mut self, alignment: usize, bytes: &[u8]) -> u32 {
        assert!(matches!(alignment, 8 | 16));
        self.rdram
            .resize(self.rdram.len().next_multiple_of(alignment), 0);
        let addr = u32::try_from(self.rdram.len()).unwrap();
        self.rdram.extend_from_slice(bytes);
        addr
    }

    pub fn vertices(&mut self, vertices: &[VtxColored]) -> u32 {
        self.bytes(
            16,
            &vertices
                .iter()
                .flat_map(VtxColored::to_bytes)
                .collect::<Vec<_>>(),
        )
    }

    pub fn matrix(&mut self, matrix: Mtx4) -> u32 {
        self.bytes(16, &mtx_to_bytes(matrix))
    }

    pub fn viewport(&mut self, viewport: Vp) -> u32 {
        self.bytes(8, &viewport.to_bytes())
    }

    pub fn lights(&mut self, ambient: [u8; 3], lights: &[Light]) -> u32 {
        assert!(lights.len() <= 7);
        let mut bytes = Vec::new();
        for light in lights {
            bytes.extend(light_bytes(light.color, light.direction));
        }
        bytes.extend(&light_bytes(ambient, [0; 3])[..8]);
        self.bytes(8, &bytes)
    }

    pub fn look_at(&mut self, axes: ([i8; 3], [i8; 3])) -> u32 {
        let bytes: Vec<_> = [axes.0, axes.1]
            .into_iter()
            .flat_map(|axis| light_bytes([0; 3], axis))
            .collect();
        self.bytes(8, &bytes)
    }

    pub fn list(&mut self, name: &'static str, commands: &[Command]) -> u32 {
        assert!(
            !self.lists.contains_key(name),
            "duplicate display list: {name}"
        );
        let bytes: Vec<_> = commands
            .iter()
            .flat_map(|&(w0, w1)| [w0.to_be_bytes(), w1.to_be_bytes()].concat())
            .collect();
        let addr = self.bytes(8, &bytes);
        self.lists.insert(name, addr);
        addr
    }

    pub fn address(&self, name: &str) -> u32 {
        self.lists[name]
    }

    pub fn segment(&self, segment: u8, name: &str) -> Command {
        assert!(segment < 16);
        n64_gbi::encode::gsp_segment(segment, self.address(name))
    }

    pub fn finish(self, entry: &str) -> Built {
        let entry = self.address(entry);
        Built {
            rdram: self.rdram,
            entry,
        }
    }
}

pub(crate) fn seg(segment: u8, offset: u32) -> u32 {
    assert!(segment < 16 && offset < 1 << 24);
    (u32::from(segment) << 24) | offset
}

fn light_bytes([r, g, b]: [u8; 3], [x, y, z]: [i8; 3]) -> [u8; 16] {
    [
        r, g, b, 0, r, g, b, 0, x as u8, y as u8, z as u8, 0, 0, 0, 0, 0,
    ]
}

#[derive(Clone, Copy)]
pub(crate) enum TexelFormat {
    Rgba16,
    Ia16,
    Ia8,
    Ia4,
    I8,
    I4,
    Ci8,
    Ci4,
}

pub(crate) struct PackedTexture {
    pub texels: Vec<u8>,
    pub palette: Vec<u8>,
}

pub(crate) fn pack_texels(format: TexelFormat, width: usize, pixels: &[[u8; 4]]) -> PackedTexture {
    assert!(width > 0 && pixels.len().is_multiple_of(width));
    let mut packed = PackedTexture {
        texels: Vec::new(),
        palette: Vec::new(),
    };
    let mut colors = Vec::new();
    for row in pixels.chunks(width) {
        let mut nibbles = Vec::new();
        for &pixel @ [r, g, b, a] in row {
            let intensity = ((u16::from(r) + u16::from(g) + u16::from(b)) / 3) as u8;
            match format {
                TexelFormat::Rgba16 => packed.texels.extend(rgba16(pixel)),
                TexelFormat::Ia16 => packed.texels.extend([intensity, a]),
                TexelFormat::Ia8 => packed.texels.push((intensity & 0xf0) | (a >> 4)),
                TexelFormat::I8 => packed.texels.push(intensity),
                TexelFormat::Ia4 => nibbles.push(((intensity >> 5) << 1) | (a >> 7)),
                TexelFormat::I4 => nibbles.push(intensity >> 4),
                TexelFormat::Ci8 | TexelFormat::Ci4 => {
                    let index = colors
                        .iter()
                        .position(|color| *color == pixel)
                        .unwrap_or_else(|| {
                            colors.push(pixel);
                            packed.palette.extend(rgba16(pixel));
                            colors.len() - 1
                        });
                    let limit = if matches!(format, TexelFormat::Ci4) {
                        16
                    } else {
                        256
                    };
                    assert!(index < limit, "too many palette colors");
                    if limit == 16 {
                        nibbles.push(index as u8);
                    } else {
                        packed.texels.push(index as u8);
                    }
                }
            }
        }
        packed.texels.extend(
            nibbles
                .chunks(2)
                .map(|pair| pair[0] << 4 | pair.get(1).copied().unwrap_or(0)),
        );
    }
    packed
}

fn rgba16([r, g, b, a]: [u8; 4]) -> [u8; 2] {
    ((u16::from(r / 8) << 11)
        | (u16::from(g / 8) << 6)
        | (u16::from(b / 8) << 1)
        | u16::from(a >= 128))
    .to_be_bytes()
}

#[test]
fn texel_formats_preserve_channels_and_row_boundaries() {
    use TexelFormat::*;
    let pixels = [
        [255, 0, 0, 255],
        [0, 255, 0, 0],
        [0, 0, 255, 128],
        [255, 255, 255, 127],
        [0, 0, 0, 128],
        [96, 96, 96, 255],
    ];
    for (format, expected) in [
        (
            Rgba16,
            vec![0xf8, 1, 7, 0xc0, 0, 0x3f, 0xff, 0xfe, 0, 1, 0x63, 0x19],
        ),
        (
            Ia16,
            vec![85, 255, 85, 0, 85, 128, 255, 127, 0, 128, 96, 255],
        ),
        (Ia8, vec![0x5f, 0x50, 0x58, 0xf7, 0x08, 0x6f]),
        (Ia4, vec![0x54, 0x50, 0xe1, 0x70]),
        (I8, vec![85, 85, 85, 255, 0, 96]),
        (I4, vec![0x55, 0x50, 0xf0, 0x60]),
        (Ci8, vec![0, 1, 2, 3, 4, 5]),
        (Ci4, vec![0x01, 0x20, 0x34, 0x50]),
    ] {
        let packed = pack_texels(format, 3, &pixels);
        assert_eq!(packed.texels, expected);
        if matches!(format, Ci4 | Ci8) {
            assert_eq!(
                packed.palette,
                [0xf8, 1, 7, 0xc0, 0, 0x3f, 0xff, 0xfe, 0, 1, 0x63, 0x19]
            );
        }
    }
}

#[test]
fn light_and_look_at_layouts() {
    use n64_gbi::{encode::gsp_enddl, gu::gu_look_at_reflect};
    let mut b = DlBuilder::new();
    b.bytes(8, &[42]);
    let lights = b.lights(
        [16, 32, 48],
        &[Light {
            color: [255, 128, 64],
            direction: [-100, 0, 0],
        }],
    );
    let look_at = b.look_at(gu_look_at_reflect([0., 0., 100., 0., 0., 0., 0., 1., 0.]));
    b.list("main", &[gsp_enddl()]);
    let built = b.finish("main");
    assert_eq!(lights % 8, 0);
    assert_eq!(
        &built.rdram[lights as usize..lights as usize + 24],
        &[
            255, 128, 64, 0, 255, 128, 64, 0, 156, 0, 0, 0, 0, 0, 0, 0, 16, 32, 48, 0, 16, 32, 48,
            0
        ]
    );
    assert_eq!(
        &built.rdram[look_at as usize + 8..look_at as usize + 11],
        &[127, 0, 0]
    );
    assert_eq!(
        &built.rdram[look_at as usize + 24..look_at as usize + 27],
        &[0, 127, 0]
    );
    assert!(crate::hle::interpret_rdram(&built.rdram, built.entry)
        .diags
        .is_empty());
}
