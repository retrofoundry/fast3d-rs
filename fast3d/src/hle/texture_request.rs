#[cfg(test)]
use std::borrow::Cow;

use std::sync::{Arc, OnceLock};

#[cfg(any(test, feature = "profiling"))]
use super::tmem::BankDecoder;
use super::{
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
                logical
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

#[cfg(any(test, feature = "profiling"))]
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
            let EncodedInput::Tmem(bank) = self.input();
            let witness = serialization::serialize_profiled(&self.recipe, bank, profiling);
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
    #[cfg(any(test, feature = "profiling"))]
    pub(crate) fn from_diagnostic(
        request: &crate::profiling::Request,
    ) -> Result<Self, &'static str> {
        let tile = &request.tile;
        request.validate()?;
        let bank: &[u8; TMEM_BYTES] = request
            .bank
            .bytes
            .as_slice()
            .try_into()
            .expect("validated bank");
        let representation = match request.representation {
            crate::profiling::Representation::Tile => Representation::Tile,
            crate::profiling::Representation::Lookup => Representation::Lookup4096x4,
        };
        let mut recipe = DecodeRecipe::new(
            tile,
            request.tlut,
            representation,
            TileSampling::from_tile(tile, request.tlut),
        );
        recipe.output = request.extent;
        let input = EncodedInput::Tmem(Arc::new(*bank));
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
        let (EncodedInput::Tmem(a), EncodedInput::Tmem(b)) = (&self.input, &other.input);
        serialization::matches(&self.recipe, a, b, profiling)
    }

    #[cfg(test)]
    pub(crate) fn from_test_parts(input: EncodedInput, recipe: DecodeRecipe) -> Self {
        Self {
            input,
            recipe,
            identity: OnceLock::new(),
        }
    }

    #[cfg(any(test, feature = "profiling"))]
    pub fn decode(&self) -> Vec<u8> {
        let tile = self.recipe().tile();
        let result = match self.input() {
            EncodedInput::Tmem(bank) => match self.recipe.representation {
                Representation::Tile => BankDecoder::new(bank).sample_tile(&tile, self.recipe.tlut),
                Representation::Lookup4096x4 => {
                    BankDecoder::new(bank).sampling_lookup(&tile, self.recipe.tlut)
                }
            },
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
    /// Sample an immutable framebuffer load through its TMEM byte mapping.
    Framebuffer(Arc<super::framebuffer_load::FramebufferTexture>),
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
            Self::Framebuffer(_) => panic!("framebuffer textures require GPU decoding"),
        }
    }

    pub(crate) fn matches(&self, other: &Self, profiling: &Recorder) -> bool {
        match (self, other) {
            (Self::Encoded(a), Self::Encoded(b)) => a.matches(b, profiling),
            (Self::Rgba(a), Self::Rgba(b)) => a == b,
            (Self::Framebuffer(a), Self::Framebuffer(b)) => Arc::ptr_eq(a, b) || a == b,
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
    if let Some(diagnostic) = rdp.tmem_bank.rejection(tile) {
        return Err(diagnostic.kind);
    }
    let sampling = TileSampling::from_tile(tile, tlut);
    let framebuffer = rdp.tmem_bank.has_framebuffer(tile);
    if framebuffer && !matches!((tile.fmt, tile.siz, tlut), (0 | 3, 2, _) | (2, 1, 2 | 3)) {
        return Err(DiagKind::UnsupportedFramebufferAccess {
            address: rdp.tex_image.3,
            reason: crate::FramebufferAccess::Reinterpretation,
        });
    }
    FormatInfo {
        fmt: tile.fmt,
        siz: tile.siz,
    }
    .validate()?;
    let representation = if sampling.image[2] == 1 {
        Representation::Lookup4096x4
    } else {
        Representation::Tile
    };
    let recipe = DecodeRecipe::new(tile, tlut, representation, sampling);
    let input = EncodedInput::Tmem(rdp.tmem_bank.share_bank());
    let request = EncodedTextureRequest {
        input,
        recipe,
        identity: OnceLock::new(),
    };
    let source = if framebuffer {
        TextureSource::Framebuffer(Arc::new(super::framebuffer_load::FramebufferTexture {
            encoded: request,
            tags: rdp.tmem_bank.framebuffer_tags_for(tile),
        }))
    } else {
        TextureSource::Encoded(Arc::new(request))
    };
    Ok(TextureBindingInput { source, sampling })
}

mod serialization;
#[cfg(test)]
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
            TextureSource::Framebuffer(request) => {
                self.allocation(
                    Arc::as_ptr(request) as usize,
                    4096 * std::mem::size_of::<super::framebuffer_load::Tag>(),
                    std::mem::size_of::<super::framebuffer_load::FramebufferTexture>() + arc_header,
                );
            }
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
                let EncodedInput::Tmem(bank) = request.input();
                self.allocation(bank.as_ptr() as usize, TMEM_BYTES, arc_header);
            }
        }
    }
}

#[cfg(test)]
#[path = "texture_request_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "texture_request_framebuffer_tests.rs"]
mod framebuffer_tests;
