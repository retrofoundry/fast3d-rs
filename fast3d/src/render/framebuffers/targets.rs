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
pub(crate) struct TargetDescriptors(BTreeMap<(u64, bool), ImageDescriptor>);

impl TargetDescriptors {
    pub(crate) fn record(
        &mut self,
        address: u64,
        layout: ImageLayout,
        height: u32,
        depth: bool,
    ) -> Result<ImageDescriptor, DiagKind> {
        let old = self.0.get(&(address, depth));
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
        self.0.insert((address, depth), descriptor);
        Ok(descriptor)
    }

    pub(crate) fn overlap(
        &self,
        address: u64,
        length: u64,
    ) -> Result<Option<ImageDescriptor>, DiagKind> {
        let range = checked_range(address, length)?;
        Ok(self.0.values().copied().find(|image| {
            image
                .range()
                .is_ok_and(|known| range.start < known.end && known.start < range.end)
        }))
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
            .0
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
        if self.0.values().any(|other| {
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
        self.0.get(&(address, depth)).copied()
    }
}
