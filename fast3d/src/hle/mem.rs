//! Big-endian readers over the assembled RDRAM image and the N64 fixed-point matrix decode.
//! Authentic-BE: element[i][j] decodes at k = i*4 + j with NO column word-swap (matches the
//! assembler's mtx_to_bytes). Addresses are physical offsets (no segment table).

use crate::hle::math::Mat4;
use std::borrow::Cow;

/// Binary layout of the matrices and vertices a display list points at. The F3DEX2 command
/// opcodes are identical either way — only the referenced data differs. Authentic libultra emits
/// fixed-point; the `GBI_FLOATS` builds (F3DEX_GBI_2E) used by PC ports (sm64, wafel) emit
/// floats. A runtime choice on the host-pointer backend, not a compile-time feature, so one build
/// can consume both.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GbiDataFormat {
    /// s15.16 split fixed-point matrices; `s16 ob[3]` vertices at a 16-byte stride.
    Fixed,
    /// `f32[4][4]` matrices; `f32 ob[3]` vertices at a 24-byte stride (`GBI_FLOATS`).
    Float,
}

/// One memory address space for the interpreter. `Addr = u64` holds a 32-bit
/// physical RDRAM offset OR a 64-bit host pointer.
pub trait Rdram {
    /// Opts into byte-based capture; every read must match the declared layout and segments.
    #[cfg(feature = "capture")]
    fn capture_layout(&self) -> Option<crate::capture::SourceLayout> {
        None
    }

    fn set_segment(&mut self, seg: u32, value: u64);
    /// UNMASKED resolution (SETTIMG / SETCIMG / SETZIMG).
    fn resolve(&self, addr: u64) -> u64;
    /// MASKED resolution (DL-target / vtx / mtx / viewport / light).
    ///
    /// "Masked" is a BACKEND-PRIVATE contract, not a literal mask the caller can rely on:
    /// the RDRAM-image backend applies the `& 0x00FFFFF8` physical-address alignment mask
    /// (`from_segmented_masked`), but a native-pointer backend (`HostRam`) MUST pass the
    /// address through unchanged — masking a 64-bit host pointer would corrupt it. Do NOT
    /// "fix" a pointer backend by adding the mask here; the resolved address must round-trip
    /// into `read_*`/`read_bytes` on the SAME backend.
    fn resolve_masked(&self, addr: u64) -> u64;
    fn read_command(&self, pc: u64) -> Command;
    fn command_stride(&self) -> u64;
    fn in_bounds(&self, pc: u64, stride: u64) -> bool;
    fn read_u8(&self, a: u64) -> u8;
    fn read_i8(&self, a: u64) -> i8;
    fn read_i16(&self, a: u64) -> i16;
    fn read_u16(&self, a: u64) -> u16;
    fn read_bytes<'s>(&'s self, addr: u64, len: usize) -> Cow<'s, [u8]>;
    fn read_matrix(&self, a: u64, fmt: GbiDataFormat) -> Mat4;

    /// Vertex array stride in bytes. Authentic fixed-point `Vtx` = 16; the `GBI_FLOATS`
    /// (F3DEX_GBI_2E) `Vtx` is 24 (float ob[3] + flag + tc + cn, 8-byte aligned). A backend that
    /// can consume float-GBI display lists (`HostRam`) overrides this per its `GbiDataFormat`.
    fn vertex_stride(&self, fmt: GbiDataFormat) -> u64 {
        debug_assert!(
            matches!(fmt, GbiDataFormat::Fixed),
            "default vertex_stride is Fixed-only"
        );
        16
    }
    /// Read one vertex, format-decoded. Default = authentic fixed-point layout:
    /// `s16 ob[3]@0, u16 flag@6, s16 tc[2]@8, u8 cn[4]@12`. The `GBI_FLOATS` layout
    /// (`f32 ob[3]@0, u16 flag@12, s16 tc[2]@14, u8 cn[4]@18`) is supplied by `HostRam`.
    fn read_vertex(&self, a: u64, fmt: GbiDataFormat) -> RawVertex {
        debug_assert!(
            matches!(fmt, GbiDataFormat::Fixed),
            "default read_vertex is Fixed-only"
        );
        RawVertex {
            pos: [
                self.read_i16(a) as f32,
                self.read_i16(a + 2) as f32,
                self.read_i16(a + 4) as f32,
            ],
            st: [self.read_i16(a + 8), self.read_i16(a + 10)],
            rgba: [
                self.read_u8(a + 12),
                self.read_u8(a + 13),
                self.read_u8(a + 14),
                self.read_u8(a + 15),
            ],
        }
    }

    /// True for the safe contiguous `RdramImage` backend; false for the raw-pointer `HostRam`
    /// backend. `present` derefs RDRAM only when this is true (spec §3.2 contract #1).
    fn is_rdram_image(&self) -> bool {
        false
    }
}

/// Format-agnostic decoded vertex: position widened to f32, texcoords as raw s10.5, color/normal
/// bytes verbatim. Lets `set_vertex` consume both the authentic fixed-point and the GBI_FLOATS
/// vertex layouts through one path.
pub struct RawVertex {
    pub pos: [f32; 3],
    pub st: [i16; 2],
    pub rgba: [u8; 4],
}

pub struct Command {
    pub w0: u32,
    pub w1: u32,
    pub w1_addr: u64,
}

pub struct RdramImage<'a> {
    pub bytes: &'a [u8],
    /// Segment base table. Zero-init: the default table is an identity map, so
    /// from_segmented(a) == a & 0x00FFFFFF and from_segmented_masked == old masked().
    pub segments: [u32; 16],
}

impl<'a> RdramImage<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        RdramImage {
            bytes,
            segments: [0u32; 16],
        }
    }

    /// G_MW_SEGMENT store: RAW value, no mask, no resolution.
    pub fn set_segment(&mut self, seg: u32, value: u32) {
        self.segments[(seg & 0xF) as usize] = value;
    }

    /// UNMASKED resolution. For SETTIMG/SETCIMG/SETZIMG.
    pub fn from_segmented(&self, a: u32) -> u32 {
        self.segments[((a >> 24) & 0x0F) as usize].wrapping_add(a & 0x00FF_FFFF)
    }

    /// MASKED resolution (& 0x00FFFFF8). For DL target / vtx / mtx / viewport.
    pub fn from_segmented_masked(&self, a: u32) -> u32 {
        self.from_segmented(a) & 0x00FF_FFF8
    }

    pub fn read_i16(&self, off: usize) -> i16 {
        i16::from_be_bytes([self.bytes[off], self.bytes[off + 1]])
    }
    pub fn read_u16(&self, off: usize) -> u16 {
        u16::from_be_bytes([self.bytes[off], self.bytes[off + 1]])
    }
    pub fn read_u8(&self, off: usize) -> u8 {
        self.bytes[off]
    }
    pub fn read_i8(&self, off: usize) -> i8 {
        self.read_u8(off) as i8
    }

    pub fn read_slice(&self, addr: u32, len: usize) -> &[u8] {
        let start = addr as usize;
        let end = (start + len).min(self.bytes.len());
        &self.bytes[start..end]
    }

    /// Layout: [16 s16 integer at k*2][16 u16 frac at 32+k*2], k=i*4+j, big-endian, NO j^1.
    pub fn read_matrix(&self, off: usize) -> Mat4 {
        let mut m = [[0.0f32; 4]; 4];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                let k = i * 4 + j; // NO j^1
                let int_v = self.read_i16(off + k * 2) as i32;
                let frac_v = self.read_u16(off + 32 + k * 2) as i32;
                let full = (int_v << 16) | frac_v;
                *cell = full as f32 / 65536.0;
            }
        }
        m
    }
}

impl<'a> Rdram for RdramImage<'a> {
    #[cfg(feature = "capture")]
    fn capture_layout(&self) -> Option<crate::capture::SourceLayout> {
        Some(crate::capture::SourceLayout {
            memory: crate::capture::MemoryLayout::IMAGE,
            segments: self.segments.map(u64::from),
        })
    }

    fn set_segment(&mut self, seg: u32, value: u64) {
        self.segments[(seg & 0xF) as usize] = value as u32;
    }
    fn resolve(&self, addr: u64) -> u64 {
        self.from_segmented(addr as u32) as u64
    }
    fn resolve_masked(&self, addr: u64) -> u64 {
        self.from_segmented_masked(addr as u32) as u64
    }
    fn read_command(&self, pc: u64) -> Command {
        let p = pc as usize;
        let w0 = u32::from_be_bytes([
            self.bytes[p],
            self.bytes[p + 1],
            self.bytes[p + 2],
            self.bytes[p + 3],
        ]);
        let w1 = u32::from_be_bytes([
            self.bytes[p + 4],
            self.bytes[p + 5],
            self.bytes[p + 6],
            self.bytes[p + 7],
        ]);
        Command {
            w0,
            w1,
            w1_addr: w1 as u64,
        }
    }
    fn command_stride(&self) -> u64 {
        8
    }
    fn in_bounds(&self, pc: u64, stride: u64) -> bool {
        pc + stride <= self.bytes.len() as u64
    }
    fn read_u8(&self, a: u64) -> u8 {
        self.bytes[a as usize]
    }
    fn read_i8(&self, a: u64) -> i8 {
        self.bytes[a as usize] as i8
    }
    fn read_i16(&self, a: u64) -> i16 {
        let p = a as usize;
        i16::from_be_bytes([self.bytes[p], self.bytes[p + 1]])
    }
    fn read_u16(&self, a: u64) -> u16 {
        let p = a as usize;
        u16::from_be_bytes([self.bytes[p], self.bytes[p + 1]])
    }
    fn read_bytes<'s>(&'s self, addr: u64, len: usize) -> Cow<'s, [u8]> {
        let s = addr as usize;
        let e = (s + len).min(self.bytes.len());
        Cow::Borrowed(&self.bytes[s..e])
    }
    fn read_matrix(&self, a: u64, fmt: GbiDataFormat) -> Mat4 {
        debug_assert!(
            matches!(fmt, GbiDataFormat::Fixed),
            "RdramImage is Fixed-only (web)"
        );
        RdramImage::read_matrix(self, a as usize)
    }
    fn is_rdram_image(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod mem_tests {
    use super::*;

    #[test]
    fn read_i8_sign_extends() {
        let r = RdramImage::new(&[0x7f, 0x80, 0xff]);
        assert_eq!(r.read_i8(0), 127);
        assert_eq!(r.read_i8(1), -128);
        assert_eq!(r.read_i8(2), -1);
    }

    #[test]
    fn rdramimage_dlmemory_basics() {
        let bytes = vec![0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0xAA, 0xBB];
        let r = RdramImage::new(&bytes);
        let c = Rdram::read_command(&r, 0);
        assert_eq!(
            (c.w0, c.w1, c.w1_addr),
            (0x1234_5678, 0x9ABC_DEF0, 0x9ABC_DEF0u64)
        );
        assert_eq!(Rdram::command_stride(&r), 8);
        assert!(Rdram::in_bounds(&r, 0, 8));
        assert!(!Rdram::in_bounds(&r, 8, 8));
        assert_eq!(Rdram::read_u16(&r, 8), 0xAABB);
        assert_eq!(&*Rdram::read_bytes(&r, 8, 2), &[0xAA, 0xBB]);
    }

    #[test]
    fn rdramimage_read_matrix_translation_row3_is_stable() {
        let m = [
            [1.0f32, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [1.0, 2.0, 3.0, 1.0],
        ];
        let bytes = n64_gbi::encode::mtx_to_bytes(m);
        let r = RdramImage::new(&bytes);
        assert_eq!(Rdram::read_matrix(&r, 0, GbiDataFormat::Fixed), m);
    }
}
