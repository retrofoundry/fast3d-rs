use std::{
    borrow::Cow,
    sync::{Arc, OnceLock},
};

use super::{
    combiner::{tile_takes_faithful_path, validate_tile_texture},
    rdp::{Rdp, TileDescriptor},
    texdec::FormatInfo,
    tile_sampling::TileSampling,
    tmem::{BankDecoder, TMEM_BYTES},
};
use crate::{profiling::Recorder, DiagKind};

#[cfg_attr(not(any(test, feature = "profiling")), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureKeyScheme {
    Fast3dV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureKey {
    pub scheme: TextureKeyScheme,
    pub xxh3_64: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Representation {
    Tile,
    Lookup4096x4,
    LinearCompat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeRecipe {
    pub representation: Representation,
    pub output: [u32; 2],
    pub logical: [u32; 2],
    pub fmt: u8,
    pub siz: u8,
    /// TMEM base and stride in 64-bit words, as serialized by Fast3dV1.
    pub base: u16,
    pub line: u16,
    pub palette: u8,
    pub tlut: u8,
}

impl DecodeRecipe {
    fn tile(&self) -> TileDescriptor {
        TileDescriptor {
            fmt: self.fmt,
            siz: self.siz,
            width: if self.representation == Representation::Tile {
                self.output[0]
            } else {
                self.logical[0]
            } as u16,
            height: if self.representation == Representation::Tile {
                self.output[1]
            } else {
                self.logical[1]
            } as u16,
            tmem_addr: self.base,
            line: self.line,
            palette: self.palette,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug)]
pub enum EncodedInput {
    Tmem(Arc<[u8; TMEM_BYTES]>),
    LinearCompat {
        bytes: Arc<[u8]>,
        palette_bank: Arc<[u8; TMEM_BYTES]>,
    },
}

#[derive(Debug)]
pub struct EncodedTextureRequest {
    input: EncodedInput,
    recipe: DecodeRecipe,
    identity: OnceLock<TextureIdentity>,
}

#[cfg_attr(not(any(test, feature = "profiling")), allow(dead_code))]
#[derive(Debug)]
struct TextureIdentity {
    key: TextureKey,
    witness: Box<[u8]>,
}

// Rendering starts consuming content identity with T4 residency.
#[cfg_attr(not(any(test, feature = "profiling")), allow(dead_code))]
impl EncodedTextureRequest {
    pub fn key(&self) -> TextureKey {
        self.key_profiled(&Default::default())
    }
    pub fn witness(&self) -> &[u8] {
        &self.identity(&Default::default()).witness
    }

    pub(crate) fn key_profiled(&self, profiling: &Recorder) -> TextureKey {
        self.identity(profiling).key
    }

    fn identity(&self, profiling: &Recorder) -> &TextureIdentity {
        self.identity.get_or_init(|| {
            let (bank, linear) = match self.input() {
                EncodedInput::Tmem(bank) => (bank, &[][..]),
                EncodedInput::LinearCompat {
                    bytes,
                    palette_bank,
                } => (palette_bank, &bytes[..]),
            };
            let witness = serialize(&self.recipe, bank, linear);
            profiling.count("tmem.hashes_computed", 1);
            profiling.count("tmem.bytes_hashed", witness.len() as u64);
            TextureIdentity {
                key: TextureKey {
                    scheme: TextureKeyScheme::Fast3dV1,
                    xxh3_64: twox_hash::XxHash3_64::oneshot(&witness),
                },
                witness: witness.into_boxed_slice(),
            }
        })
    }
}

impl EncodedTextureRequest {
    pub fn input(&self) -> &EncodedInput {
        &self.input
    }
    pub fn recipe(&self) -> &DecodeRecipe {
        &self.recipe
    }
    fn matches(&self, other: &Self, profiling: &Recorder) -> bool {
        if self.recipe != other.recipe {
            return false;
        }
        profiling.count("tmem.encoded_comparisons", 1);
        let equal = |a: &[u8], b: &[u8]| {
            if a.len() != b.len() {
                return false;
            }
            profiling.count("tmem.bytes_compared", a.len() as u64);
            a == b
        };
        // Positional reuse can conservatively compare complete owned inputs without content keys.
        match (&self.input, &other.input) {
            (EncodedInput::Tmem(a), EncodedInput::Tmem(b)) => equal(&a[..], &b[..]),
            (
                EncodedInput::LinearCompat {
                    bytes: a,
                    palette_bank: pa,
                },
                EncodedInput::LinearCompat {
                    bytes: b,
                    palette_bank: pb,
                },
            ) => equal(a, b) && (self.recipe.fmt != 2 || equal(&pa[2048..], &pb[2048..])),
            _ => false,
        }
    }

    pub fn decode(&self) -> Vec<u8> {
        let tile = self.recipe().tile();
        let result = match self.input() {
            EncodedInput::Tmem(bank) => match self.recipe.representation {
                Representation::Tile => BankDecoder::new(bank).sample_tile(&tile, self.recipe.tlut),
                Representation::Lookup4096x4 => {
                    BankDecoder::new(bank).sampling_lookup(&tile, self.recipe.tlut)
                }
                Representation::LinearCompat => unreachable!(),
            },
            EncodedInput::LinearCompat {
                bytes,
                palette_bank,
            } => FormatInfo {
                fmt: self.recipe.fmt,
                siz: self.recipe.siz,
            }
            .decode(
                bytes,
                self.recipe.logical[0],
                self.recipe.logical[1],
                &palette_bank[2048..],
                self.recipe.palette,
                self.recipe.tlut,
            ),
        };
        result.expect("owned texture requests are validated before construction")
    }
}

impl PartialEq for EncodedTextureRequest {
    fn eq(&self, other: &Self) -> bool {
        self.matches(other, &Default::default())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextureSource {
    Encoded(Arc<EncodedTextureRequest>),
    Rgba(Arc<[u8]>),
}

impl From<Vec<u8>> for TextureSource {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Rgba(bytes.into())
    }
}

impl TextureSource {
    pub fn decode(&self) -> Cow<'_, [u8]> {
        match self {
            Self::Encoded(request) => Cow::Owned(request.decode()),
            Self::Rgba(bytes) => Cow::Borrowed(bytes),
        }
    }

    pub(crate) fn decode_profiled(&self, profiling: &Recorder, role: &str) -> Cow<'_, [u8]> {
        let _span = profiling.span("decode");
        let pixels = self.decode();
        if let Self::Encoded(request) = self {
            profiling.count("tmem.cpu_decode_executions", 1);
            profiling.count("tmem.cpu_decode_output_bytes", pixels.len() as u64);
            #[cfg(any(test, feature = "profiling"))]
            profiling.decode(
                format!(
                    "{:?}.{}-{}.tlut{}.{}x{}.{role}",
                    request.recipe().representation,
                    request.recipe().fmt,
                    request.recipe().siz,
                    request.recipe().tlut,
                    request.recipe().output[0],
                    request.recipe().output[1]
                ),
                Some(pixels.len()),
            );
            #[cfg(not(any(test, feature = "profiling")))]
            let _ = (request, role);
        }
        pixels
    }

    pub(crate) fn matches(&self, other: &Self, profiling: &Recorder) -> bool {
        match (self, other) {
            (Self::Encoded(a), Self::Encoded(b)) => a.matches(b, profiling),
            (Self::Rgba(a), Self::Rgba(b)) => a == b,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextureBindingInput {
    pub source: TextureSource,
    pub sampling: TileSampling,
}

impl TextureBindingInput {
    pub(crate) fn matches(&self, other: &Self, profiling: &Recorder) -> bool {
        self.sampling == other.sampling && self.source.matches(&other.source, profiling)
    }
}

pub(crate) fn prepare(
    rdp: &Rdp,
    tile: &TileDescriptor,
    tlut: u8,
    profiling: &Recorder,
) -> Result<TextureBindingInput, DiagKind> {
    let _span = profiling.span("texture_request");
    validate_tile_texture(rdp, tile)?;
    if let Some(diagnostic) = rdp.tmem_bank.rejection(tile) {
        return Err(diagnostic.kind);
    }
    let sampling = TileSampling::from_tile(tile, tlut);
    let logical = [u32::from(tile.width.max(1)), u32::from(tile.height.max(1))];
    let representation = if sampling.image[2] == 1 {
        Representation::Lookup4096x4
    } else if tile_takes_faithful_path(rdp, tile, logical[0]) {
        Representation::Tile
    } else {
        Representation::LinearCompat
    };
    let recipe = DecodeRecipe {
        representation,
        output: if representation == Representation::Tile {
            [u32::from(tile.width), u32::from(tile.height)]
        } else {
            sampling.allocation_extent()
        },
        logical,
        fmt: tile.fmt,
        siz: tile.siz,
        base: tile.tmem_addr,
        line: tile.line,
        palette: if tile.fmt == 2 && tile.siz == 0 {
            tile.palette
        } else {
            0
        },
        tlut: if tile.fmt == 2 { tlut } else { 0 },
    };
    let needed = FormatInfo {
        fmt: tile.fmt,
        siz: tile.siz,
    }
    .tmem_bytes(logical[0], logical[1]);
    let linear = if representation == Representation::LinearCompat {
        let bytes = rdp.tmem_bank.linear_bytes(tile, needed)?;
        profiling.count("tmem.linear_reconstruction_bytes", bytes.len() as u64);
        bytes
    } else {
        Vec::new()
    };
    let bank = rdp.tmem_bank.share_bank();
    let input = if representation == Representation::LinearCompat {
        EncodedInput::LinearCompat {
            bytes: linear.into(),
            palette_bank: bank,
        }
    } else {
        EncodedInput::Tmem(bank)
    };
    let request = Arc::new(EncodedTextureRequest {
        input,
        recipe,
        identity: OnceLock::new(),
    });
    Ok(TextureBindingInput {
        source: TextureSource::Encoded(request),
        sampling,
    })
}

#[cfg_attr(not(any(test, feature = "profiling")), allow(dead_code))]
fn serialize(recipe: &DecodeRecipe, bank: &[u8; TMEM_BYTES], linear: &[u8]) -> Vec<u8> {
    let mut selected = [false; TMEM_BYTES];
    let mut palette = [false; TMEM_BYTES];
    let mut mark = |rel: usize, odd: bool, parity: usize| {
        let mask = if recipe.fmt == 2 || recipe.siz == 3 {
            2047
        } else {
            4095
        };
        let address = |offset| {
            ((usize::from(recipe.base) * 8) + ((rel + offset) ^ if odd { 4 } else { 0 })) & mask
        };
        let a = address(0);
        selected[a] = true;
        if recipe.siz >= 2 {
            selected[address(1)] = true;
        }
        if recipe.siz == 3 {
            selected[a | 2048] = true;
            selected[address(1) | 2048] = true;
        }
        if recipe.fmt == 2 {
            let index = if recipe.siz == 0 {
                (bank[a] >> if parity == 0 { 4 } else { 0 }) & 15
            } else {
                bank[a]
            };
            let p = 2048 + usize::from(recipe.palette) * 128 + usize::from(index) * 8;
            selected[p & 4095] = true;
            selected[(p + 1) & 4095] = true;
        }
    };
    match recipe.representation {
        Representation::Tile => {
            for y in 0..recipe.output[1] as usize {
                for x in 0..recipe.output[0] as usize {
                    mark(
                        y * usize::from(recipe.line) * 8 + ((x << recipe.siz.min(2)) >> 1),
                        y & 1 != 0,
                        x & 1,
                    );
                }
            }
        }
        Representation::Lookup4096x4 => {
            for odd in [false, true] {
                for parity in 0..2 {
                    for rel in 0..TMEM_BYTES {
                        mark(rel, odd, parity);
                    }
                }
            }
        }
        Representation::LinearCompat => {
            if recipe.fmt == 2 {
                for x in 0..(recipe.logical[0] as usize * recipe.logical[1] as usize) {
                    let index = if recipe.siz == 0 {
                        (linear[x / 2] >> if x & 1 == 0 { 4 } else { 0 }) & 15
                    } else {
                        linear[x]
                    };
                    let p = 2048 + usize::from(recipe.palette) * 128 + usize::from(index) * 8;
                    palette[p & 4095] = true;
                    palette[(p + 1) & 4095] = true;
                }
            }
        }
    }
    let mut out = Vec::with_capacity(
        56 + selected.iter().filter(|&&v| v).count() * 3
            + linear.len()
            + palette.iter().filter(|&&v| v).count() * 3,
    );
    out.extend_from_slice(b"fast3d-tmem-key\0");
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&[recipe.representation as u8, 0]);
    for dimension in recipe.output.into_iter().chain(recipe.logical) {
        out.extend_from_slice(&dimension.to_le_bytes());
    }
    out.extend_from_slice(&[recipe.fmt, recipe.siz]);
    out.extend_from_slice(&recipe.base.to_le_bytes());
    out.extend_from_slice(&recipe.line.to_le_bytes());
    out.extend_from_slice(&[recipe.palette, recipe.tlut]);
    let pairs = |out: &mut Vec<u8>, selected: &[bool; TMEM_BYTES]| {
        out.extend_from_slice(&(selected.iter().filter(|&&v| v).count() as u32).to_le_bytes());
        for (address, &included) in selected.iter().enumerate() {
            if included {
                out.extend_from_slice(&(address as u16).to_le_bytes());
                out.push(bank[address]);
            }
        }
    };
    pairs(&mut out, &selected);
    out.extend_from_slice(&(linear.len() as u32).to_le_bytes());
    out.extend_from_slice(linear);
    pairs(&mut out, &palette);
    out
}

#[derive(Default)]
pub(crate) struct TextureMemory {
    allocations: std::collections::HashSet<usize>,
    pub(crate) payload_bytes: usize,
    pub(crate) overhead_bytes: usize,
}

impl TextureMemory {
    fn allocation(&mut self, address: usize, bytes: usize, overhead: usize) -> bool {
        if !self.allocations.insert(address) {
            return false;
        }
        self.payload_bytes += bytes;
        self.overhead_bytes += overhead;
        true
    }

    pub(crate) fn include(&mut self, source: &TextureSource) {
        let arc_header = 2 * std::mem::size_of::<usize>();
        match source {
            TextureSource::Rgba(bytes) => {
                self.allocation(bytes.as_ptr() as usize, bytes.len(), arc_header);
            }
            TextureSource::Encoded(request) => {
                if !self.allocation(
                    Arc::as_ptr(request) as usize,
                    request
                        .identity
                        .get()
                        .map_or(0, |identity| identity.witness.len()),
                    std::mem::size_of::<EncodedTextureRequest>() + arc_header,
                ) {
                    return;
                }
                let bank = match request.input() {
                    EncodedInput::Tmem(bank) => bank,
                    EncodedInput::LinearCompat {
                        bytes,
                        palette_bank,
                    } => {
                        self.allocation(bytes.as_ptr() as usize, bytes.len(), arc_header);
                        palette_bank
                    }
                };
                self.allocation(bank.as_ptr() as usize, TMEM_BYTES, arc_header);
            }
        }
    }
}

#[cfg(test)]
#[path = "texture_request_tests.rs"]
mod tests;
