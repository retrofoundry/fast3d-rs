use super::ImageLayout;
use crate::{diag::FramebufferAccess, DiagKind};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImageDescriptor {
    pub address: u64,
    pub layout: ImageLayout,
    pub height: u32,
    pub generation: u64,
    pub depth: bool,
}

impl ImageLayout {
    pub(crate) fn row_bytes(self) -> Option<u64> {
        u64::from(self.width)
            .checked_mul(4u64.checked_shl(u32::from(self.siz))?)
            .map(|bits| bits.div_ceil(8))
    }
}

pub(crate) fn checked_range(address: u64, length: u64) -> Result<std::ops::Range<u64>, DiagKind> {
    address
        .checked_add(length)
        .map(|end| address..end)
        .ok_or(DiagKind::FramebufferRangeOverflow { address, length })
}

impl ImageDescriptor {
    pub(crate) fn range(self) -> Result<std::ops::Range<u64>, DiagKind> {
        let length = self
            .layout
            .row_bytes()
            .and_then(|row| row.checked_mul(u64::from(self.height)))
            .ok_or(DiagKind::FramebufferRangeOverflow {
                address: self.address,
                length: u64::MAX,
            })?;
        checked_range(self.address, length)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TargetDescriptors {
    images: BTreeMap<(u64, bool), ImageDescriptor>,
    writes: BTreeMap<(u64, bool), u64>,
    coherent: bool,
    /// Coherent mode: bytes each image owns from earlier tasks, and images drawn in this one.
    owned: Vec<(std::ops::Range<u64>, (u64, bool, ImageLayout))>,
    drawn: std::collections::BTreeSet<(u64, bool)>,
    versions: Vec<ImageDescriptor>,
}

impl TargetDescriptors {
    pub(crate) fn record(
        &mut self,
        address: u64,
        layout: ImageLayout,
        height: u32,
        depth: bool,
    ) -> Result<ImageDescriptor, DiagKind> {
        let old = self.images.get(&(address, depth));
        let compatible = old.is_some_and(|old| old.layout == layout);
        let descriptor = ImageDescriptor {
            address,
            layout,
            depth,
            height: if compatible {
                height.max(old.unwrap().height)
            } else {
                height
            },
            generation: old.map_or(1, |old| old.generation + u64::from(!compatible)),
        };
        descriptor.range()?;
        if let Some(old) = old.filter(|_| !compatible) {
            self.versions.retain(|version| {
                !(version.address == old.address
                    && version.depth == old.depth
                    && version.layout == old.layout)
            });
            self.versions.push(*old);
        }
        self.images.insert((address, depth), descriptor);
        Ok(descriptor)
    }

    pub(crate) fn overlap(
        &self,
        address: u64,
        length: u64,
    ) -> Result<Option<ImageDescriptor>, DiagKind> {
        let range = checked_range(address, length)?;
        Ok(self.images.values().copied().find(|image| {
            image
                .range()
                .is_ok_and(|known| range.start < known.end && known.start < range.end)
        }))
    }

    pub(crate) fn written(&mut self, address: u64, depth: bool) {
        let next = self.writes.values().copied().max().unwrap_or(0) + 1;
        self.writes.insert((address, depth), next);
        self.drawn.insert((address, depth));
    }

    pub(crate) fn seed_owners(&mut self, owners: &super::coherence::Ownership) {
        self.coherent = true;
        self.writes.clear();
        self.drawn.clear();
        self.owned.clear();
        for (&start, range) in owners.0.iter().filter(|(_, range)| !range.foreign) {
            let writer = range.writer;
            self.owned.push((
                start..start + range.bytes.len() as u64,
                (writer.address, writer.depth, writer.layout),
            ));
            self.writes
                .entry((range.writer.address, range.writer.depth))
                .and_modify(|generation| *generation = (*generation).max(range.generation))
                .or_insert(range.generation);
        }
    }

    pub(crate) fn load_source(
        &self,
        address: u64,
        length: u64,
    ) -> Result<Option<ImageDescriptor>, DiagKind> {
        let range = checked_range(address, length)?;
        let source = self
            .images
            .values()
            .chain(self.versions.iter().filter(|_| self.coherent))
            .filter(|source| {
                source
                    .range()
                    .is_ok_and(|r| r.start < range.end && range.start < r.end)
            })
            .filter(|source| {
                !self.coherent
                    || self.drawn.contains(&(source.address, source.depth))
                    || self.owned.iter().any(|(owned, key)| {
                        *key == (source.address, source.depth, source.layout)
                            && owned.start < range.end
                            && range.start < owned.end
                    })
            })
            .max_by_key(|source| {
                (
                    self.coherent
                        && source.range().is_ok_and(|known| {
                            known.start <= range.start && range.end <= known.end
                        }),
                    self.coherent
                        && !source.depth
                        && (source.layout.fmt, source.layout.siz) == (0, 2),
                    self.writes
                        .get(&(source.address, source.depth))
                        .copied()
                        .unwrap_or(0),
                )
            })
            .copied();
        let Some(source) = source else {
            return Ok(None);
        };
        let fail = |reason| DiagKind::UnsupportedFramebufferAccess { address, reason };
        if source.depth {
            return Err(fail(FramebufferAccess::DepthTexture));
        }
        if (source.layout.fmt, source.layout.siz) != (0, 2) {
            return Err(fail(FramebufferAccess::Reinterpretation));
        }
        let extent = source.range()?;
        if address < extent.start || range.end > extent.end {
            return Err(fail(FramebufferAccess::Extent));
        }
        Ok(Some(source))
    }

    pub(crate) fn source(
        &self,
        address: u64,
        target: u64,
        layout: ImageLayout,
        height: u32,
    ) -> Result<Option<ImageDescriptor>, DiagKind> {
        let request = ImageDescriptor {
            address,
            layout,
            height,
            generation: 0,
            depth: false,
        };
        let range = request.range()?;
        let matches: Vec<_> = self
            .images
            .values()
            .copied()
            .filter(|image| {
                image
                    .range()
                    .is_ok_and(|known| range.start < known.end && known.start < range.end)
            })
            .collect();
        let fail = |reason| DiagKind::UnsupportedFramebufferAccess { address, reason };
        if address == target {
            return Err(fail(FramebufferAccess::SameTarget));
        }
        if matches.is_empty() {
            return Ok(None);
        }
        if matches.len() != 1 {
            return Err(fail(FramebufferAccess::Overlap));
        }
        let source = matches[0];
        if source.address != address {
            return Err(fail(FramebufferAccess::InteriorOffset));
        }
        if source.depth {
            return Err(fail(FramebufferAccess::DepthTexture));
        }
        if source.layout != layout {
            return Err(fail(FramebufferAccess::Reinterpretation));
        }
        let view = source.range()?;
        if self.images.values().any(|other| {
            *other != source
                && other
                    .range()
                    .is_ok_and(|range| view.start < range.end && range.start < view.end)
        }) {
            return Err(fail(FramebufferAccess::Overlap));
        }
        if height > source.height {
            return Err(fail(FramebufferAccess::Extent));
        }
        Ok(Some(source))
    }

    pub(crate) fn get(&self, address: u64, depth: bool) -> Option<ImageDescriptor> {
        self.images.get(&(address, depth)).copied()
    }
}
