//! Exact, owned texture replacement registrations. See `docs/texture-replacements.md`.

pub use crate::hle::texture_request::{DecodeRecipe, EncodedInput, Representation};
use crate::{hle::texture_request::EncodedTextureRequest, profiling::Recorder};
use std::{collections::HashMap, sync::Arc};
pub(crate) mod rt64_v5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureKeyScheme {
    UltragfxV1,
    Rt64V5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureKey {
    pub scheme: TextureKeyScheme,
    pub xxh3_64: u64,
}

/// A numeric key and its exact, scheme-specific proof. Constructed from owned encoded data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacementTarget {
    key: TextureKey,
    witness: Arc<[u8]>,
    extent: [u32; 2],
    representation: Representation,
    palette_lanes: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplacementError {
    InvalidRecipe,
    IneligibleScheme,
    Extent,
    Length,
    ColorSpace,
}

impl ReplacementTarget {
    pub fn new(
        recipe: DecodeRecipe,
        input: EncodedInput,
        scheme: TextureKeyScheme,
    ) -> Result<Self, ReplacementError> {
        validate_recipe(&recipe, &input)?;
        let request = EncodedTextureRequest::from_owned_parts(input, recipe);
        Self::for_request(&request, scheme, &Default::default())
            .ok_or(ReplacementError::IneligibleScheme)
    }

    pub fn key(&self) -> TextureKey {
        self.key
    }
    pub fn witness(&self) -> &[u8] {
        &self.witness
    }
    pub fn extent(&self) -> [u32; 2] {
        self.extent
    }

    pub(crate) fn for_request(
        request: &EncodedTextureRequest,
        scheme: TextureKeyScheme,
        profiling: &Recorder,
    ) -> Option<Self> {
        match scheme {
            TextureKeyScheme::UltragfxV1 => {
                let (key, witness) = request.identity_profiled(profiling);
                Some(Self {
                    key: TextureKey {
                        scheme: TextureKeyScheme::UltragfxV1,
                        xxh3_64: key.xxh3_64,
                    },
                    witness,
                    extent: request.recipe().allocation_extent(),
                    representation: request.recipe().representation,
                    palette_lanes: request.recipe().palette_lanes,
                })
            }
            TextureKeyScheme::Rt64V5 => {
                profiling.count("replacement.v5_admission_checks", 1);
                rt64_v5::alias(request.recipe(), request.input(), profiling)
            }
        }
    }
}

fn validate_recipe(recipe: &DecodeRecipe, input: &EncodedInput) -> Result<(), ReplacementError> {
    let invalid = ReplacementError::InvalidRecipe;
    if (crate::hle::texdec::FormatInfo {
        fmt: recipe.fmt,
        siz: recipe.siz,
    })
    .validate_tlut(recipe.tlut)
    .is_err()
        || recipe
            .output
            .into_iter()
            .chain(recipe.logical)
            .any(|x| x == 0 || x > 4096)
        || recipe.base > 511
        || recipe.line > 511
        || recipe.palette > 15
        || recipe.tlut > 3
        || (recipe.fmt != 2 && recipe.tlut == 1)
        || (((recipe.fmt != 2 && recipe.tlut < 2) || recipe.siz != 0) && recipe.palette != 0)
    {
        return Err(invalid);
    }
    let EncodedInput::Tmem(bank) = input;
    if recipe.palette_lanes
        != crate::hle::texture_request::palette_lanes(
            recipe,
            bank,
            &crate::hle::tmem::Tmem::palette_nonreplicated_bytes(bank),
            &Default::default(),
        )
    {
        return Err(invalid);
    }
    match (recipe.representation, input) {
        (Representation::Tile, EncodedInput::Tmem(_)) if recipe.output == recipe.logical => Ok(()),
        (Representation::Lookup4096x4, EncodedInput::Tmem(_)) if recipe.output == [4096, 4] => {
            Ok(())
        }
        _ => Err(invalid),
    }
}

/// Pixels are straight-alpha RGBA8. Only `Rgba8Unorm` is accepted; no colour conversion occurs.
#[derive(Clone, Debug)]
pub struct ReplacementImage {
    pub extent: [u32; 2],
    pub pixels: Arc<[u8]>,
    pub format: wgpu::TextureFormat,
}

impl ReplacementImage {
    pub(crate) fn scale(
        &self,
        target: &ReplacementTarget,
        max_dimension: u32,
    ) -> Result<u32, ReplacementError> {
        if self.format != wgpu::TextureFormat::Rgba8Unorm {
            return Err(ReplacementError::ColorSpace);
        }
        let [w, h] = self.extent;
        let [native_w, native_h] = target.extent;
        if w == 0
            || h == 0
            || w > max_dimension
            || h > max_dimension
            || native_w == 0
            || native_h == 0
        {
            return Err(ReplacementError::Extent);
        }
        let scale = w / native_w;
        if scale == 0
            || scale > (i32::MAX as u32) / (2 * (1 << 15))
            || native_w.checked_mul(scale) != Some(w)
            || native_h.checked_mul(scale) != Some(h)
            || ((target.representation == Representation::Lookup4096x4 || target.palette_lanes)
                && scale != 1)
        {
            return Err(ReplacementError::Extent);
        }
        if u64::from(w)
            .checked_mul(u64::from(h))
            .and_then(|x| x.checked_mul(4))
            != Some(self.pixels.len() as u64)
        {
            return Err(ReplacementError::Length);
        }
        Ok(scale)
    }
}

#[derive(Clone, Debug)]
pub enum ReplacementStatus {
    Ready(ReplacementImage),
    /// Uses native output until a new provider snapshot is installed at a frame boundary.
    Pending,
}

#[derive(Clone, Debug)]
pub struct ReplacementRegistration {
    pub target: ReplacementTarget,
    pub status: ReplacementStatus,
}

/// Immutable in-memory provider. Within one scheme, the last exact registration wins.
/// Revisions are consumer labels; installing any new snapshot invalidates previous decisions.
#[derive(Debug)]
pub struct ReplacementProvider {
    revision: u64,
    registrations: HashMap<TextureKey, Vec<ReplacementRegistration>>,
    wants_v5: bool,
}

impl ReplacementProvider {
    pub fn new(
        revision: u64,
        registrations: impl IntoIterator<Item = ReplacementRegistration>,
    ) -> Self {
        let mut result = Self {
            revision,
            registrations: HashMap::new(),
            wants_v5: false,
        };
        for registration in registrations {
            result.wants_v5 |= registration.target.key.scheme == TextureKeyScheme::Rt64V5;
            result
                .registrations
                .entry(registration.target.key)
                .or_default()
                .push(registration);
        }
        result
    }
    pub(crate) fn profile(&self, stats: &Recorder) {
        let mut witnesses = std::collections::HashSet::new();
        let mut pixels = std::collections::HashSet::new();
        let mut witness_bytes = 0;
        let mut pixel_bytes = 0;
        let mut entries = 0;
        for registration in self.registrations.values().flatten() {
            entries += 1;
            if witnesses.insert(Arc::as_ptr(&registration.target.witness)) {
                witness_bytes += registration.target.witness.len() as u64;
            }
            if let ReplacementStatus::Ready(image) = &registration.status {
                if pixels.insert(Arc::as_ptr(&image.pixels)) {
                    pixel_bytes += image.pixels.len() as u64;
                }
            }
        }
        stats.gauge("replacement.provider_entries", entries);
        stats.gauge("replacement.provider_witness_bytes", witness_bytes);
        stats.gauge("replacement.provider_pixel_bytes", pixel_bytes);
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub(crate) fn resolve(
        &self,
        request: &EncodedTextureRequest,
        v5_allowed: bool,
        profiling: &Recorder,
    ) -> (ReplacementTarget, Option<&ReplacementStatus>) {
        self.resolve_with(
            |scheme| {
                if scheme == TextureKeyScheme::Rt64V5 && !v5_allowed {
                    return None;
                }
                ReplacementTarget::for_request(request, scheme, profiling)
            },
            profiling,
        )
    }

    fn resolve_with(
        &self,
        mut target: impl FnMut(TextureKeyScheme) -> Option<ReplacementTarget>,
        profiling: &Recorder,
    ) -> (ReplacementTarget, Option<&ReplacementStatus>) {
        let native = target(TextureKeyScheme::UltragfxV1).unwrap();
        if let Some(status) = self.lookup(&native, profiling) {
            return (native, Some(status));
        }
        if self.wants_v5 {
            if let Some(alias) = target(TextureKeyScheme::Rt64V5) {
                let status = self.lookup(&alias, profiling);
                return (alias, status);
            }
        }
        (native, None)
    }

    pub(crate) fn lookup(
        &self,
        target: &ReplacementTarget,
        profiling: &Recorder,
    ) -> Option<&ReplacementStatus> {
        profiling.count("replacement.provider_calls", 1);
        self.registrations
            .get(&target.key)?
            .iter()
            .rev()
            .find_map(|registration| {
                profiling.count("replacement.witness_comparisons", 1);
                profiling.count(
                    "replacement.bytes_compared",
                    registration.target.witness.len() as u64,
                );
                (registration.target == *target).then_some(&registration.status)
            })
    }
}

#[cfg(test)]
mod tests;
