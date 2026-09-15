#[cfg(test)]
use std::borrow::Cow;

use std::sync::{Arc, OnceLock};

#[cfg(test)]
use super::tmem::BankDecoder;
use super::{
    combiner::{tile_takes_faithful_path, validate_tile_texture},
    rdp::{Rdp, TileDescriptor},
    texdec::FormatInfo,
    tile_sampling::TileSampling,
    tmem::TMEM_BYTES,
};
use crate::{profiling::Recorder, DiagKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureKeyScheme {
    Fast3dV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    pub(crate) fn new(
        tile: &TileDescriptor,
        tlut: u8,
        representation: Representation,
        sampling: TileSampling,
    ) -> Self {
        let logical = [u32::from(tile.width.max(1)), u32::from(tile.height.max(1))];
        Self {
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
        }
    }
}

#[cfg(test)]
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

#[derive(Debug)]
struct TextureIdentity {
    key: TextureKey,
    witness: Arc<[u8]>,
}

impl EncodedTextureRequest {
    #[cfg(test)]
    pub fn key(&self) -> TextureKey {
        self.key_profiled(&Default::default())
    }
    #[cfg(test)]
    pub fn witness(&self) -> &[u8] {
        &self.identity(&Default::default()).witness
    }

    #[cfg(test)]
    pub(crate) fn witness_profiled(&self, profiling: &Recorder) -> Arc<[u8]> {
        self.identity(profiling).witness.clone()
    }

    pub(crate) fn key_profiled(&self, profiling: &Recorder) -> TextureKey {
        self.identity(profiling).key
    }

    pub(crate) fn identity_profiled(&self, profiling: &Recorder) -> (TextureKey, Arc<[u8]>) {
        let identity = self.identity(profiling);
        (identity.key, identity.witness.clone())
    }

    fn identity(&self, profiling: &Recorder) -> &TextureIdentity {
        if let Some(identity) = self.identity.get() {
            profiling.count("tmem.identity_memo_hits", 1);
            return identity;
        }
        self.identity.get_or_init(|| {
            let (bank, linear) = match self.input() {
                EncodedInput::Tmem(bank) => (bank, &[][..]),
                EncodedInput::LinearCompat {
                    bytes,
                    palette_bank,
                } => (palette_bank, &bytes[..]),
            };
            let witness = serialization::serialize_profiled(&self.recipe, bank, linear, profiling);
            profiling.count("tmem.hashes_computed", 1);
            profiling.count("tmem.bytes_hashed", witness.len() as u64);
            TextureIdentity {
                key: TextureKey {
                    scheme: TextureKeyScheme::Fast3dV1,
                    xxh3_64: twox_hash::XxHash3_64::oneshot(&witness),
                },
                witness: witness.into(),
            }
        })
    }
}

impl EncodedTextureRequest {
    #[cfg(any(test, all(feature = "profiling", feature = "capture")))]
    pub(crate) fn from_diagnostic(request: &crate::profiling::Request) -> Result<Self, DiagKind> {
        let tile = &request.tile;
        let fi = FormatInfo {
            fmt: tile.fmt,
            siz: tile.siz,
        };
        fi.validate()?;
        let unavailable = DiagKind::TextureBytesUnavailable {
            tmem_addr: tile.tmem_addr,
        };
        if request.rejection.is_some()
            || request
                .extent
                .iter()
                .any(|&extent| extent == 0 || extent > 4096)
            || tile.width == 0
            || tile.width > 4096
            || tile.height == 0
            || tile.height > 4096
            || tile.tmem_addr > 511
            || tile.line > 511
            || tile.palette > 15
            || request.tlut > 3
            || tile.masks > 15
            || tile.maskt > 15
        {
            return Err(unavailable);
        }
        let bank: &[u8; TMEM_BYTES] = request
            .bank
            .bytes
            .as_slice()
            .try_into()
            .map_err(|_| unavailable)?;
        let representation = match request.representation {
            crate::profiling::Representation::Tile => Representation::Tile,
            crate::profiling::Representation::Lookup => Representation::Lookup4096x4,
            crate::profiling::Representation::Linear => Representation::LinearCompat,
        };
        let mut recipe = DecodeRecipe::new(
            tile,
            request.tlut,
            representation,
            TileSampling::from_tile(tile, request.tlut),
        );
        recipe.output = request.extent;
        let input = if representation == Representation::LinearCompat {
            if request.extent != recipe.logical || (tile.fmt == 0 && tile.siz == 3) {
                return Err(unavailable);
            }
            let bytes = request.prepare()?;
            if bytes.len() != fi.tmem_bytes(recipe.logical[0], recipe.logical[1]) {
                return Err(unavailable);
            }
            EncodedInput::LinearCompat {
                bytes: bytes.into(),
                palette_bank: Arc::new(*bank),
            }
        } else {
            if representation == Representation::Lookup4096x4 && request.extent != [4096, 4] {
                return Err(unavailable);
            }
            EncodedInput::Tmem(Arc::new(*bank))
        };
        Ok(Self {
            input,
            recipe,
            identity: OnceLock::new(),
        })
    }

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
        // Capture admission conservatively compares complete owned inputs without content keys.
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

    #[cfg(test)]
    pub(crate) fn from_test_parts(input: EncodedInput, recipe: DecodeRecipe) -> Self {
        Self {
            input,
            recipe,
            identity: OnceLock::new(),
        }
    }

    #[cfg(test)]
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
    #[cfg(test)]
    pub fn decode(&self) -> Cow<'_, [u8]> {
        match self {
            Self::Encoded(request) => Cow::Owned(request.decode()),
            Self::Rgba(bytes) => Cow::Borrowed(bytes),
        }
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

#[cfg(test)]
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
    let recipe = DecodeRecipe::new(tile, tlut, representation, sampling);
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

mod serialization;
#[cfg(any(test, feature = "profiling"))]
pub(crate) use serialization::serialize;

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
                    0,
                    std::mem::size_of::<EncodedTextureRequest>() + arc_header,
                ) {
                    return;
                }
                if let Some(identity) = request.identity.get() {
                    self.allocation(
                        identity.witness.as_ptr() as usize,
                        identity.witness.len(),
                        arc_header,
                    );
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
