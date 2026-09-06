//! Fallible memory readers for N64 byte images and consumer-provided address spaces.

use std::borrow::Cow;
use std::ops::Range;

/// A decoded 4×4 matrix in the renderer's row-vector convention.
pub type Matrix = [[f32; 4]; 4];

/// Binary layout of matrices and vertices referenced by display-list commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GbiDataFormat {
    /// Big-endian s15.16 split matrices and 16-byte fixed-point vertices in an image backend.
    Fixed,
    /// Native `f32` matrices and 24-byte `GBI_FLOATS` vertices in a host backend.
    Float,
}

/// Why a memory operation could not produce its complete requested value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MemoryErrorKind {
    OutOfBounds,
    AddressOverflow,
    UnsupportedFormat,
    Unavailable,
}

/// A failed memory operation and the exact byte span it requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryError {
    pub address: u64,
    pub length: u64,
    pub kind: MemoryErrorKind,
}

fn memory_error(address: u64, length: u64, kind: MemoryErrorKind) -> MemoryError {
    MemoryError {
        address,
        length,
        kind,
    }
}

/// One memory address space for the interpreter.
///
/// Reads return decoded values rather than guest-layout structs. Implementations must return an
/// error unless the complete requested value is available. `in_bounds` is advisory: a successful
/// query does not make a later read infallible. The interpreter does not catch panics from a
/// consumer implementation.
pub trait Rdram {
    /// Describes the byte and address layout used when recording memory reads.
    #[cfg(feature = "capture")]
    fn capture_layout(&self) -> Option<crate::capture::SourceLayout> {
        None
    }

    /// Stores the raw segment value selected by the low four bits of `segment`.
    fn set_segment(&mut self, segment: u32, value: u64);

    /// Resolves an image operand without the display-list alignment mask.
    fn resolve(&self, address: u64) -> Result<u64, MemoryError>;

    /// Resolves an address operand using the backend's masked-address convention.
    ///
    /// `RdramImage` applies `& 0x00ff_fff8` after 32-bit segmented resolution. Native host
    /// backends preserve the full pointer because masking it would change its identity.
    fn resolve_masked(&self, address: u64) -> Result<u64, MemoryError>;

    fn read_command(&self, address: u64) -> Result<Command, MemoryError>;
    fn command_stride(&self) -> u64;
    fn in_bounds(&self, address: u64, length: u64) -> bool;
    fn read_u8(&self, address: u64) -> Result<u8, MemoryError>;
    fn read_i8(&self, address: u64) -> Result<i8, MemoryError>;
    fn read_i16(&self, address: u64) -> Result<i16, MemoryError>;
    fn read_u16(&self, address: u64) -> Result<u16, MemoryError>;
    fn read_bytes(&self, address: u64, length: usize) -> Result<Cow<'_, [u8]>, MemoryError>;
    fn read_matrix(&self, address: u64, format: GbiDataFormat) -> Result<Matrix, MemoryError>;

    /// Returns the byte stride for the requested vertex layout.
    ///
    /// The default image layout supports only 16-byte Fixed vertices.
    fn vertex_stride(&self, format: GbiDataFormat) -> Result<u64, MemoryError> {
        match format {
            GbiDataFormat::Fixed => Ok(16),
            GbiDataFormat::Float => Err(memory_error(0, 0, MemoryErrorKind::UnsupportedFormat)),
        }
    }

    /// Decodes one Fixed vertex using this backend's scalar readers.
    ///
    /// The default layout is `s16 pos[3]@0`, `u16 flag@6`, `s16 st[2]@8`, and
    /// `u8 rgba[4]@12`, with a 16-byte stride. Float requires a backend override.
    fn read_vertex(&self, address: u64, format: GbiDataFormat) -> Result<RawVertex, MemoryError> {
        if format != GbiDataFormat::Fixed {
            return Err(memory_error(
                address,
                24,
                MemoryErrorKind::UnsupportedFormat,
            ));
        }
        read_fixed_vertex(self, address)
    }

    /// Allows VI addresses to be interpreted as physical offsets in this memory image.
    ///
    /// This does not authorize memory reads during presentation.
    fn is_rdram_image(&self) -> bool {
        false
    }
}

fn read_fixed_vertex(
    memory: &(impl Rdram + ?Sized),
    address: u64,
) -> Result<RawVertex, MemoryError> {
    address
        .checked_add(16)
        .ok_or_else(|| memory_error(address, 16, MemoryErrorKind::AddressOverflow))?;
    Ok(RawVertex {
        pos: [
            memory.read_i16(address)? as f32,
            memory.read_i16(address + 2)? as f32,
            memory.read_i16(address + 4)? as f32,
        ],
        st: [
            memory.read_i16(address + 8)?,
            memory.read_i16(address + 10)?,
        ],
        rgba: [
            memory.read_u8(address + 12)?,
            memory.read_u8(address + 13)?,
            memory.read_u8(address + 14)?,
            memory.read_u8(address + 15)?,
        ],
    })
}

/// A decoded vertex. This is not a guest-memory layout and must not be used for casting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RawVertex {
    pub pos: [f32; 3],
    pub st: [i16; 2],
    pub rgba: [u8; 4],
}

/// A decoded display-list command. This is not a guest-memory layout and must not be cast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    pub w0: u32,
    pub w1: u32,
    pub w1_addr: u64,
}

/// A safe, contiguous big-endian RDRAM image.
pub struct RdramImage<'a> {
    pub bytes: &'a [u8],
    /// Raw 32-bit segment bases. Zero bases preserve the operand's physical low 24 bits.
    pub segments: [u32; 16],
}

impl<'a> RdramImage<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            segments: [0; 16],
        }
    }

    /// Stores the low 32 bits of a raw segment value without resolving or masking it.
    pub fn set_segment(&mut self, segment: u32, value: u64) {
        self.segments[(segment & 0x0f) as usize] = value as u32;
    }

    /// Performs 32-bit segmented resolution without the display-list alignment mask.
    ///
    /// The segment base plus the low 24-bit offset intentionally wraps as a `u32`, matching N64
    /// address arithmetic. Inputs outside the 32-bit image address space are rejected.
    pub fn from_segmented(&self, address: u64) -> Result<u64, MemoryError> {
        let address = u32::try_from(address)
            .map_err(|_| memory_error(address, 0, MemoryErrorKind::AddressOverflow))?;
        let segment = ((address >> 24) & 0x0f) as usize;
        Ok(self.segments[segment].wrapping_add(address & 0x00ff_ffff) as u64)
    }

    /// Resolves a 32-bit segmented address, then applies `& 0x00ff_fff8`.
    pub fn from_segmented_masked(&self, address: u64) -> Result<u64, MemoryError> {
        Ok(self.from_segmented(address)? & 0x00ff_fff8)
    }

    pub fn read_i16(&self, address: u64) -> Result<i16, MemoryError> {
        let bytes = self.read_slice(address, 2)?;
        Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
    }

    pub fn read_u16(&self, address: u64) -> Result<u16, MemoryError> {
        let bytes = self.read_slice(address, 2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    pub fn read_u8(&self, address: u64) -> Result<u8, MemoryError> {
        Ok(self.read_slice(address, 1)?[0])
    }

    pub fn read_i8(&self, address: u64) -> Result<i8, MemoryError> {
        Ok(self.read_u8(address)? as i8)
    }

    pub fn read_slice(&self, address: u64, length: usize) -> Result<&[u8], MemoryError> {
        let range = self.range(address, length as u64)?;
        Ok(&self.bytes[range])
    }

    fn decode_matrix(&self, address: u64) -> Result<Matrix, MemoryError> {
        let bytes = self.read_slice(address, 64)?;
        let mut matrix = [[0.0; 4]; 4];
        for (row_index, row) in matrix.iter_mut().enumerate() {
            for (column_index, cell) in row.iter_mut().enumerate() {
                let k = row_index * 4 + column_index;
                let integer = i16::from_be_bytes([bytes[k * 2], bytes[k * 2 + 1]]) as i32;
                let fraction = u16::from_be_bytes([bytes[32 + k * 2], bytes[33 + k * 2]]) as i32;
                *cell = ((integer << 16) | fraction) as f32 / 65536.0;
            }
        }
        Ok(matrix)
    }

    fn range(&self, address: u64, length: u64) -> Result<Range<usize>, MemoryError> {
        let end = address
            .checked_add(length)
            .ok_or_else(|| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
        if end > self.bytes.len() as u64 {
            return Err(memory_error(address, length, MemoryErrorKind::OutOfBounds));
        }
        let start = usize::try_from(address)
            .map_err(|_| memory_error(address, length, MemoryErrorKind::OutOfBounds))?;
        let end = usize::try_from(end)
            .map_err(|_| memory_error(address, length, MemoryErrorKind::OutOfBounds))?;
        Ok(start..end)
    }
}

impl Rdram for RdramImage<'_> {
    #[cfg(feature = "capture")]
    fn capture_layout(&self) -> Option<crate::capture::SourceLayout> {
        Some(crate::capture::SourceLayout {
            memory: crate::capture::MemoryLayout::IMAGE,
            segments: self.segments.map(u64::from),
        })
    }

    fn set_segment(&mut self, segment: u32, value: u64) {
        RdramImage::set_segment(self, segment, value);
    }

    fn resolve(&self, address: u64) -> Result<u64, MemoryError> {
        self.from_segmented(address)
    }

    fn resolve_masked(&self, address: u64) -> Result<u64, MemoryError> {
        self.from_segmented_masked(address)
    }

    fn read_command(&self, address: u64) -> Result<Command, MemoryError> {
        let bytes = self.read_slice(address, 8)?;
        let w0 = u32::from_be_bytes(bytes[..4].try_into().unwrap());
        let w1 = u32::from_be_bytes(bytes[4..].try_into().unwrap());
        Ok(Command {
            w0,
            w1,
            w1_addr: u64::from(w1),
        })
    }

    fn command_stride(&self) -> u64 {
        8
    }

    fn in_bounds(&self, address: u64, length: u64) -> bool {
        address
            .checked_add(length)
            .is_some_and(|end| end <= self.bytes.len() as u64)
    }

    fn read_u8(&self, address: u64) -> Result<u8, MemoryError> {
        RdramImage::read_u8(self, address)
    }

    fn read_i8(&self, address: u64) -> Result<i8, MemoryError> {
        RdramImage::read_i8(self, address)
    }

    fn read_i16(&self, address: u64) -> Result<i16, MemoryError> {
        RdramImage::read_i16(self, address)
    }

    fn read_u16(&self, address: u64) -> Result<u16, MemoryError> {
        RdramImage::read_u16(self, address)
    }

    fn read_bytes(&self, address: u64, length: usize) -> Result<Cow<'_, [u8]>, MemoryError> {
        Ok(Cow::Borrowed(self.read_slice(address, length)?))
    }

    fn read_matrix(&self, address: u64, format: GbiDataFormat) -> Result<Matrix, MemoryError> {
        match format {
            GbiDataFormat::Fixed => self.decode_matrix(address),
            GbiDataFormat::Float => Err(memory_error(
                address,
                64,
                MemoryErrorKind::UnsupportedFormat,
            )),
        }
    }

    fn read_vertex(&self, address: u64, format: GbiDataFormat) -> Result<RawVertex, MemoryError> {
        if format != GbiDataFormat::Fixed {
            return Err(memory_error(
                address,
                24,
                MemoryErrorKind::UnsupportedFormat,
            ));
        }
        self.range(address, 16)?;
        read_fixed_vertex(self, address)
    }

    fn is_rdram_image(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_reads_are_big_endian_and_exact() {
        let bytes = [0x12, 0x34, 0x80, 0xff, 0, 0, 0, 0];
        let image = RdramImage::new(&bytes);

        assert_eq!(image.read_u16(0), Ok(0x1234));
        assert_eq!(image.read_i8(2), Ok(-128));
        assert_eq!(image.read_slice(6, 2), Ok(&bytes[6..8]));
        assert_eq!(
            image.read_slice(7, 2),
            Err(memory_error(7, 2, MemoryErrorKind::OutOfBounds))
        );
    }

    #[test]
    fn image_range_addition_cannot_wrap() {
        let image = RdramImage::new(&[]);
        assert_eq!(
            image.read_slice(u64::MAX, 2),
            Err(memory_error(u64::MAX, 2, MemoryErrorKind::AddressOverflow,))
        );
        assert!(!image.in_bounds(u64::MAX, 2));
    }
}
