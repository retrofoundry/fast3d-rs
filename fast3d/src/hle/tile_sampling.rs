use super::rdp::TileDescriptor;

/// Draw-time tile state. Rows match the WGSL uniform layout; bounds are raw 10.2 values.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TileSampling {
    pub bounds: [i32; 4],
    /// S/T shifts followed by S/T mask fields.
    pub shift_mask: [u32; 4],
    /// S/T clamp/mirror flags followed by the logical clamp extents.
    pub modes: [u32; 4],
    /// Decoded image extent, representation (0 = tile, 1 = TMEM lookup, 2 = normalized image), padding.
    pub image: [u32; 4],
    /// TMEM base and line in bytes, format, size.
    pub tmem: [u32; 4],
    /// Palette index, TLUT format, padding.
    pub palette: [u32; 4],
}

impl Default for TileSampling {
    fn default() -> Self {
        Self::from_tile(&TileDescriptor::default(), 0)
    }
}

impl TileSampling {
    pub fn from_tile(tile: &TileDescriptor, tlut_fmt: u8) -> Self {
        let width = u32::from(tile.width.max(1));
        let height = u32::from(tile.height.max(1));
        let mut sampling = Self {
            bounds: [
                tile.uls.into(),
                tile.ult.into(),
                tile.lrs.into(),
                tile.lrt.into(),
            ],
            shift_mask: [
                tile.shifts.into(),
                tile.shiftt.into(),
                tile.masks.into(),
                tile.maskt.into(),
            ],
            modes: [tile.cms.into(), tile.cmt.into(), width, height],
            image: [width, height, 0, 0],
            tmem: [
                u32::from(tile.tmem_addr) * 8,
                u32::from(tile.line) * 8,
                tile.fmt.into(),
                tile.siz.into(),
            ],
            palette: [tile.palette.into(), tlut_fmt.into(), 0, 0],
        };
        for axis in 0..2 {
            let mask = sampling.shift_mask[axis + 2];
            if mask != 0 && sampling.modes[axis] & 2 == 0 && (1u32 << mask) > sampling.image[axis] {
                sampling.image[2] = 1;
            }
        }
        sampling
    }

    pub fn allocation_extent(self) -> [u32; 2] {
        if self.image[2] == 1 {
            [4096, 4]
        } else {
            [self.image[0], self.image[1]]
        }
    }
}
