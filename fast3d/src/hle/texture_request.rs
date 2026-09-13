use std::{borrow::Cow, collections::VecDeque, sync::Arc};

use super::{
    combiner::{tile_takes_faithful_path, validate_tile_texture},
    rdp::{Rdp, TileDescriptor},
    texdec::FormatInfo,
    tile_sampling::TileSampling,
    tmem::{BankDecoder, TMEM_BYTES},
};
use crate::{profiling::Recorder, DiagKind};

pub(crate) const MEMO_ENTRIES: usize = 256;
pub(crate) const MEMO_BYTES: usize = 1024 * 1024;

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
    key: TextureKey,
    witness: Box<[u8]>,
}

impl EncodedTextureRequest {
    pub fn input(&self) -> &EncodedInput {
        &self.input
    }
    pub fn recipe(&self) -> &DecodeRecipe {
        &self.recipe
    }
    pub fn key(&self) -> TextureKey {
        self.key
    }
    pub fn witness(&self) -> &[u8] {
        &self.witness
    }

    fn matches(&self, other: &Self, profiling: &Recorder) -> bool {
        if self.key() != other.key() {
            return false;
        }
        profiling.count("tmem.witness_comparisons", 1);
        if self.witness().len() != other.witness().len() {
            return false;
        }
        // Count the full slice passed to equality, independent of its SIMD/early-exit implementation.
        profiling.count("tmem.bytes_compared", self.witness().len() as u64);
        self.witness() == other.witness()
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

    fn payload_bytes(&self) -> usize {
        self.witness().len()
            + match self.input() {
                EncodedInput::Tmem(_) => 0,
                EncodedInput::LinearCompat { bytes, .. } => bytes.len(),
            }
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

#[derive(Clone, Debug)]
struct MemoEntry {
    recipe: DecodeRecipe,
    provenance_revision: u64,
    request: Arc<EncodedTextureRequest>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct RequestMemo {
    // Entries belong to this owned bank version. No numeric generation selects content.
    snapshot: Option<Arc<[u8; TMEM_BYTES]>>,
    provenance_revision: u64,
    entries: VecDeque<MemoEntry>,
}

impl RequestMemo {
    pub(super) fn invalidate(&mut self) {
        self.snapshot = None;
        self.entries.clear();
        // Clearing entries first prevents wrap or an equal-byte reload from aliasing an old version.
        self.provenance_revision = self.provenance_revision.wrapping_add(1);
    }

    fn payload_bytes(&self) -> usize {
        usize::from(self.snapshot.is_some()) * TMEM_BYTES
            + self
                .entries
                .iter()
                .map(|entry| entry.request.payload_bytes())
                .sum::<usize>()
    }

    fn admit(&mut self, request: Arc<EncodedTextureRequest>, profiling: &Recorder) {
        if request.payload_bytes() + TMEM_BYTES <= MEMO_BYTES {
            while self.entries.len() >= MEMO_ENTRIES
                || self.payload_bytes() + request.payload_bytes() > MEMO_BYTES
            {
                self.entries.pop_front();
                profiling.count("tmem.memo_evictions", 1);
            }
            let provenance_revision = self.provenance_revision;
            let recipe = request.recipe.clone();
            self.entries.push_back(MemoEntry {
                recipe,
                provenance_revision,
                request: request.clone(),
            });
        } else {
            profiling.count("tmem.memo_bypasses", 1);
        }
    }

    fn report(&self, profiling: &Recorder) {
        profiling.gauge("tmem.memo_entries", self.entries.len() as u64);
        profiling.gauge("tmem.memo_payload_bytes", self.payload_bytes() as u64);
        profiling.gauge(
            "tmem.memo_allocation_overhead_bytes",
            (self.entries.capacity() * std::mem::size_of::<MemoEntry>()
                + self.entries.len()
                    * (std::mem::size_of::<EncodedTextureRequest>()
                        + 2 * std::mem::size_of::<usize>())
                + usize::from(self.snapshot.is_some()) * 2 * std::mem::size_of::<usize>()
                + self
                    .entries
                    .iter()
                    .filter(|entry| {
                        matches!(entry.request.input, EncodedInput::LinearCompat { .. })
                    })
                    .count()
                    * 2
                    * std::mem::size_of::<usize>()) as u64,
        );
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
    if representation == Representation::LinearCompat {
        rdp.tmem_bank.validate_linear(tile, needed)?;
    }
    let mut memo = rdp.tmem_bank.requests.borrow_mut();
    if let Some(index) = memo.entries.iter().position(|entry| {
        entry.recipe == recipe
            && (representation != Representation::LinearCompat
                || entry.provenance_revision == memo.provenance_revision)
    }) {
        let entry = memo.entries.remove(index).unwrap();
        let source = TextureSource::Encoded(entry.request.clone());
        memo.entries.push_back(entry);
        profiling.count("tmem.memo_hits", 1);
        memo.report(profiling);
        return Ok(TextureBindingInput { source, sampling });
    }
    profiling.count("tmem.memo_misses", 1);
    let bank = memo
        .snapshot
        .get_or_insert_with(|| {
            profiling.count("tmem.snapshot_allocations", 1);
            profiling.count("tmem.snapshot_bytes", TMEM_BYTES as u64);
            Arc::new(*rdp.tmem_bank.encoded_bank())
        })
        .clone();
    let linear = if representation == Representation::LinearCompat {
        let bytes = rdp.tmem_bank.linear_bytes(tile, needed)?;
        profiling.count("tmem.linear_reconstruction_bytes", bytes.len() as u64);
        bytes
    } else {
        Vec::new()
    };
    let witness = serialize(&recipe, &bank, &linear);
    profiling.count("tmem.hashes_computed", 1);
    profiling.count("tmem.bytes_hashed", witness.len() as u64);
    let key = TextureKey {
        scheme: TextureKeyScheme::Fast3dV1,
        xxh3_64: twox_hash::XxHash3_64::oneshot(&witness),
    };
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
        recipe: recipe.clone(),
        key,
        witness: witness.into_boxed_slice(),
    });
    memo.admit(request.clone(), profiling);
    memo.report(profiling);
    Ok(TextureBindingInput {
        source: TextureSource::Encoded(request),
        sampling,
    })
}

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
                    request.witness().len(),
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
