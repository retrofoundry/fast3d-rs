//! Native host-pointer memory for 64-bit ports.
#![cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]

use crate::hle::mem::{
    Command, GbiDataFormat, Matrix, MemoryError, MemoryErrorKind, RawVertex, Rdram,
};
use core::sync::atomic::{AtomicU32, Ordering};
use core::{marker::PhantomData, ptr};
use std::borrow::Cow;

fn memory_error(address: u64, length: u64, kind: MemoryErrorKind) -> MemoryError {
    MemoryError {
        address,
        length,
        kind,
    }
}

fn probe_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HELIX_DL_PROBE").is_ok())
}

static PROBE_LOGS: AtomicU32 = AtomicU32::new(0);
static PROBE_NONIDENT: AtomicU32 = AtomicU32::new(0);

fn probe_log(args: core::fmt::Arguments<'_>) {
    if probe_on() && PROBE_LOGS.fetch_add(1, Ordering::Relaxed) < 128 {
        log::trace!(target: "fast3d::host_mem", "{args}");
    }
}

fn probe_log_nonident(args: core::fmt::Arguments<'_>) {
    if probe_on() && PROBE_NONIDENT.fetch_add(1, Ordering::Relaxed) < 4096 {
        log::trace!(target: "fast3d::host_mem", "{args}");
    }
}

/// A safe descriptor for one native display-list walk.
///
/// The descriptor stores only a lifetime witness and initial raw segment bases. It performs no
/// reads and does not implement [`Rdram`]. Native memory is interpreted only by the crate's unsafe
/// host entry point. Its fixed layout uses native-endian 16-byte commands with full-width address
/// words, unaligned scalar reads, native packed Fixed matrices, 16-byte Fixed vertices, and
/// 24-byte Float vertices.
///
/// ```compile_fail
/// use fast3d::{HostRam, Rdram};
///
/// fn safe_reader(_: impl Rdram) {}
/// let frame = [];
/// safe_reader(HostRam::new(&frame));
/// ```
pub struct HostRam<'a> {
    pub segments: [u64; 16],
    _frame: PhantomData<&'a [u8]>,
}

impl<'a> HostRam<'a> {
    /// Creates a descriptor tied to the lifetime of the native frame.
    ///
    /// This constructor does not make arbitrary addresses safe to read. The unsafe host processing
    /// call requires every command and reachable input span to remain allocated, readable,
    /// initialized, in the documented native layout, and stable until that call returns.
    pub fn new(_frame: &'a [u8]) -> Self {
        Self {
            segments: [0; 16],
            _frame: PhantomData,
        }
    }

    #[cfg(feature = "capture")]
    pub fn capture_layout(&self) -> crate::capture::SourceLayout {
        crate::capture::SourceLayout {
            memory: crate::capture::MemoryLayout::host_native(),
            segments: self.segments,
        }
    }
}

pub(crate) struct HostMemory<'a> {
    ram: HostRam<'a>,
}

impl<'a> HostMemory<'a> {
    /// Creates the raw-pointer reader used only for the duration of an unsafe host walk.
    ///
    /// # Safety
    ///
    /// Every command and reachable input span read during the walk must remain allocated,
    /// readable, initialized, correctly laid out, and stable until this reader and its borrowed slices are dropped. Borrowed texture bytes must
    /// not be mutated concurrently. CIMG and ZIMG numeric identities need not be readable unless a
    /// command uses them as input.
    pub(crate) unsafe fn new(ram: HostRam<'a>) -> Self {
        Self { ram }
    }

    fn checked_span(address: u64, length: u64) -> Result<(), MemoryError> {
        address
            .checked_add(length)
            .ok_or_else(|| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
        Ok(())
    }

    unsafe fn read_unaligned<T: Copy>(address: u64) -> T {
        unsafe { ptr::read_unaligned(address as *const T) }
    }
}

impl Rdram for HostMemory<'_> {
    #[cfg(feature = "capture")]
    fn capture_layout(&self) -> Option<crate::capture::SourceLayout> {
        Some(self.ram.capture_layout())
    }

    fn set_segment(&mut self, segment: u32, value: u64) {
        probe_log(format_args!(
            "set_segment seg={} value={value:#018x}",
            segment & 0x0f
        ));
        self.ram.segments[(segment & 0x0f) as usize] = value;
    }

    fn resolve(&self, address: u64) -> Result<u64, MemoryError> {
        let segment = ((address >> 24) & 0x0f) as usize;
        let base = self.ram.segments[segment];
        let resolved = if base == 0 {
            address
        } else {
            base.checked_add(address & 0x00ff_ffff)
                .ok_or_else(|| memory_error(address, 0, MemoryErrorKind::AddressOverflow))?
        };
        if resolved != address {
            probe_log_nonident(format_args!(
                "resolve non-identity in={address:#018x} out={resolved:#018x} seg={segment} segval={base:#018x}"
            ));
        }
        Ok(resolved)
    }

    fn resolve_masked(&self, address: u64) -> Result<u64, MemoryError> {
        self.resolve(address)
    }

    fn read_command(&self, address: u64) -> Result<Command, MemoryError> {
        Self::checked_span(address, 16)?;
        let second = address + 8;
        let w0_word = unsafe { Self::read_unaligned::<u64>(address) };
        let w1_word = unsafe { Self::read_unaligned::<u64>(second) };
        Ok(Command {
            w0: w0_word as u32,
            w1: w1_word as u32,
            w1_addr: w1_word,
        })
    }

    fn command_stride(&self) -> u64 {
        16
    }

    fn in_bounds(&self, address: u64, length: u64) -> bool {
        address.checked_add(length).is_some()
    }

    fn read_u8(&self, address: u64) -> Result<u8, MemoryError> {
        Self::checked_span(address, 1)?;
        Ok(unsafe { Self::read_unaligned(address) })
    }

    fn read_i8(&self, address: u64) -> Result<i8, MemoryError> {
        Self::checked_span(address, 1)?;
        Ok(unsafe { Self::read_unaligned(address) })
    }

    fn read_i16(&self, address: u64) -> Result<i16, MemoryError> {
        Self::checked_span(address, 2)?;
        Ok(unsafe { Self::read_unaligned(address) })
    }

    fn read_u16(&self, address: u64) -> Result<u16, MemoryError> {
        Self::checked_span(address, 2)?;
        Ok(unsafe { Self::read_unaligned(address) })
    }

    fn read_bytes(&self, address: u64, length: usize) -> Result<Cow<'_, [u8]>, MemoryError> {
        let length_u64 = length as u64;
        Self::checked_span(address, length_u64)?;
        if length == 0 {
            return Ok(Cow::Borrowed(&[]));
        }
        if length > isize::MAX as usize {
            return Err(memory_error(
                address,
                length_u64,
                MemoryErrorKind::AddressOverflow,
            ));
        }
        let bytes = unsafe { std::slice::from_raw_parts(address as *const u8, length) };
        Ok(Cow::Borrowed(bytes))
    }

    fn read_matrix(&self, address: u64, format: GbiDataFormat) -> Result<Matrix, MemoryError> {
        Self::checked_span(address, 64)?;
        let mut matrix = [[0.0; 4]; 4];
        match format {
            GbiDataFormat::Float => {
                for k in 0..16u64 {
                    matrix[k as usize / 4][k as usize % 4] =
                        unsafe { Self::read_unaligned::<f32>(address + k * 4) };
                }
            }
            GbiDataFormat::Fixed => {
                for (row_index, row) in matrix.iter_mut().enumerate() {
                    for column_pair in 0..2 {
                        let word_index = (row_index * 2 + column_pair) as u64;
                        let integer =
                            unsafe { Self::read_unaligned::<u32>(address + word_index * 4) };
                        let fraction =
                            unsafe { Self::read_unaligned::<u32>(address + (8 + word_index) * 4) };
                        row[column_pair * 2] =
                            (((integer & 0xffff_0000) | (fraction >> 16)) as i32) as f32 / 65536.0;
                        row[column_pair * 2 + 1] =
                            (((integer << 16) | (fraction & 0xffff)) as i32) as f32 / 65536.0;
                    }
                }
            }
        }
        Ok(matrix)
    }

    fn vertex_stride(&self, format: GbiDataFormat) -> Result<u64, MemoryError> {
        Ok(match format {
            GbiDataFormat::Fixed => 16,
            GbiDataFormat::Float => 24,
        })
    }

    fn read_vertex(&self, address: u64, format: GbiDataFormat) -> Result<RawVertex, MemoryError> {
        let length = self.vertex_stride(format)?;
        Self::checked_span(address, length)?;
        match format {
            GbiDataFormat::Float => Ok(RawVertex {
                pos: unsafe {
                    [
                        Self::read_unaligned(address),
                        Self::read_unaligned(address + 4),
                        Self::read_unaligned(address + 8),
                    ]
                },
                st: [unsafe { Self::read_unaligned(address + 14) }, unsafe {
                    Self::read_unaligned(address + 16)
                }],
                rgba: unsafe {
                    [
                        Self::read_unaligned(address + 18),
                        Self::read_unaligned(address + 19),
                        Self::read_unaligned(address + 20),
                        Self::read_unaligned(address + 21),
                    ]
                },
            }),
            GbiDataFormat::Fixed => Ok(RawVertex {
                pos: [
                    unsafe { Self::read_unaligned::<i16>(address) } as f32,
                    unsafe { Self::read_unaligned::<i16>(address + 2) } as f32,
                    unsafe { Self::read_unaligned::<i16>(address + 4) } as f32,
                ],
                st: [unsafe { Self::read_unaligned(address + 8) }, unsafe {
                    Self::read_unaligned(address + 10)
                }],
                rgba: unsafe {
                    [
                        Self::read_unaligned(address + 12),
                        Self::read_unaligned(address + 13),
                        Self::read_unaligned(address + 14),
                        Self::read_unaligned(address + 15),
                    ]
                },
            }),
        }
    }
}
