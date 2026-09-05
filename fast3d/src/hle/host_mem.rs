//! `HostRam` — native-endian host-pointer backend for sm64/host ports (fast3d-rs use case).
//!
//! The entire safety boundary is the module-level cfg: the web/wasm build never compiles this
//! unsafe code, and the 16-byte `Gfx` command stride is only valid on a 64-bit host. Addresses
//! here ARE raw host pointers (`u64`), so every typed read is an `unaligned` read at a byte
//! offset (N64 structs have fields at odd offsets; an aligned deref would be UB). The `'a` frame
//! witness ties any borrowed slice to the live DL backing storage so `read_bytes` can't dangle.
#![cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]

use crate::hle::math::Mat4;
use crate::hle::mem::{Command, GbiDataFormat, RawVertex, Rdram};
use core::sync::atomic::{AtomicU32, Ordering};
use core::{marker::PhantomData, ptr};
use std::borrow::Cow;

fn probe_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HELIX_DL_PROBE").is_ok())
}
// Two INDEPENDENT budgets so a flood of routine lines (set_segment / general) can never
// starve the rare-but-critical NON-IDENTITY resolve signal that Task 2 exists to detect.
static PROBE_LOGS: AtomicU32 = AtomicU32::new(0);
static PROBE_NONIDENT: AtomicU32 = AtomicU32::new(0);
fn probe_log(args: core::fmt::Arguments) {
    if probe_on() && PROBE_LOGS.fetch_add(1, Ordering::Relaxed) < 128 {
        eprintln!("[probe/host] {args}");
    }
}
fn probe_log_nonident(args: core::fmt::Arguments) {
    // Generous, separate budget — non-identity resolves are the signal, not noise.
    if probe_on() && PROBE_NONIDENT.fetch_add(1, Ordering::Relaxed) < 4096 {
        eprintln!("[probe/host] {args}");
    }
}

pub struct HostRam<'a> {
    pub segments: [u64; 16],
    _frame: PhantomData<&'a [u8]>,
}

impl<'a> HostRam<'a> {
    /// Wrap a host frame as a DL memory backend over raw native pointers. The matrix/vertex data
    /// format is the interpreter's ucode choice (`gbi.data_format`), not a backend default.
    ///
    /// # Safety
    ///
    /// Every address the interpreter resolves through this backend — the `entry` pointer, every
    /// command operand, and every struct/texture pointer reachable from the walk — must point into
    /// memory that lives at least as long as `frame` (the `'a` witness). Pointers are dereferenced
    /// with `read_unaligned`/`from_raw_parts`, so a dangling or out-of-bounds address is UB.
    pub unsafe fn new(_frame: &'a [u8]) -> Self {
        HostRam {
            segments: [0; 16],
            _frame: PhantomData,
        }
    }
}

impl<'a> Rdram for HostRam<'a> {
    #[cfg(feature = "capture")]
    fn capture_layout(&self) -> Option<crate::capture::SourceLayout> {
        Some(crate::capture::SourceLayout {
            memory: crate::capture::MemoryLayout::host_native(),
            segments: self.segments,
        })
    }

    /// Store a raw 64-bit segment base. We deliberately do NOT mask the value.
    ///
    /// CONFIRMED (SP3b HELIX_DL_PROBE, 2026-06-25): sm64's PC port uses identity address
    /// translation (VIRTUAL_TO_PHYSICAL / segmented_to_virtual are identity; load_segment is a
    /// no-op) and emits NO `gSPSegment`/`G_MW_SEGMENT` into its gfx DLs — the probe saw zero
    /// `set_segment` calls and zero non-identity `resolve`s across the captured frames, so
    /// `segments` stay zero during interpret and `resolve` is pure pass-through. We keep the
    /// full-width store (no 24-bit mask) so that if a future DL ever sets a host-pointer
    /// segment base it is not truncated. (fast3d's F3DEX2 `MoveWord SEGMENT` masks to 24-bit,
    /// which would corrupt a 64-bit host pointer — hence the deliberate divergence here.)
    fn set_segment(&mut self, seg: u32, value: u64) {
        probe_log(format_args!(
            "set_segment seg={} value={:#018x}",
            seg & 0xF,
            value
        ));
        self.segments[(seg & 0xF) as usize] = value;
    }
    fn resolve(&self, a: u64) -> u64 {
        let s = ((a >> 24) & 0x0F) as usize;
        let out = if self.segments[s] != 0 {
            self.segments[s] + (a & 0x00FF_FFFF)
        } else {
            a
        };
        if out != a {
            probe_log_nonident(format_args!(
                "resolve NON-IDENTITY in={a:#018x} out={out:#018x} seg={s} segval={:#018x}",
                self.segments[s]
            ));
        }
        out
    }
    fn resolve_masked(&self, a: u64) -> u64 {
        self.resolve(a) // NO &0x00FFFFF8 on a pointer
    }
    fn read_command(&self, pc: u64) -> Command {
        let p = pc as *const usize;
        let w0 = unsafe { ptr::read_unaligned(p) } as u32;
        let w1u = unsafe { ptr::read_unaligned(p.add(1)) };
        Command {
            w0,
            w1: w1u as u32,
            w1_addr: w1u as u64,
        }
    }
    fn command_stride(&self) -> u64 {
        16
    }
    fn in_bounds(&self, _pc: u64, _stride: u64) -> bool {
        true // host DL is G_ENDDL-terminated (fast3d run_dl); DISPATCH_CAP guards runaway
    }
    fn read_u8(&self, a: u64) -> u8 {
        unsafe { ptr::read_unaligned(a as *const u8) }
    }
    fn read_i8(&self, a: u64) -> i8 {
        unsafe { ptr::read_unaligned(a as *const i8) }
    }
    fn read_i16(&self, a: u64) -> i16 {
        unsafe { ptr::read_unaligned(a as *const i16) }
    }
    fn read_u16(&self, a: u64) -> u16 {
        unsafe { ptr::read_unaligned(a as *const u16) }
    }
    fn read_bytes<'s>(&'s self, a: u64, len: usize) -> Cow<'s, [u8]> {
        Cow::Borrowed(unsafe { std::slice::from_raw_parts(a as *const u8, len) })
    }
    fn read_matrix(&self, a: u64, fmt: GbiDataFormat) -> Mat4 {
        match fmt {
            // Row-major `f32[4][4]`, read verbatim (GBI_FLOATS guMtxF2L is a memcpy).
            GbiDataFormat::Float => {
                let mut m = [[0.0f32; 4]; 4];
                for k in 0..16 {
                    m[k / 4][k % 4] = unsafe { ptr::read_unaligned((a as *const f32).add(k)) };
                }
                m
            }
            // s15.16 split: 16 native i32 words, [0..8] integer halves, [8..16] fraction halves;
            // element (i,j) = (int16 << 16 | frac16) / 65536. Native-endian (matches fast3d).
            GbiDataFormat::Fixed => {
                let word =
                    |k: usize| unsafe { ptr::read_unaligned((a as *const i32).add(k)) as u32 };
                let mut m = [[0.0f32; 4]; 4];
                for (i, row) in m.iter_mut().enumerate() {
                    for c in 0..2 {
                        let int = word(i * 2 + c);
                        let frac = word(8 + i * 2 + c);
                        row[c * 2] = (((int & 0xFFFF_0000) | (frac >> 16)) as i32) as f32 / 65536.0;
                        row[c * 2 + 1] = (((int << 16) | (frac & 0xFFFF)) as i32) as f32 / 65536.0;
                    }
                }
                m
            }
        }
    }

    /// `GBI_FLOATS` (F3DEX_GBI_2E) `Vtx` is 24 bytes (float ob[3] + flag + tc + cn, padded by the
    /// `long long` union alignment); authentic fixed-point `Vtx` is 16.
    fn vertex_stride(&self, fmt: GbiDataFormat) -> u64 {
        match fmt {
            GbiDataFormat::Float => 24,
            GbiDataFormat::Fixed => 16,
        }
    }

    /// Float layout: `f32 ob[3]@0`, `flag@12`, `tc[2]@14`, `cn[4]@18`. Fixed layout: `s16 ob[3]@0`,
    /// `flag@6`, `tc[2]@8`, `cn[4]@12`. Reading a float vertex as s16 misreads every position.
    fn read_vertex(&self, a: u64, fmt: GbiDataFormat) -> RawVertex {
        match fmt {
            GbiDataFormat::Float => RawVertex {
                pos: unsafe {
                    [
                        ptr::read_unaligned(a as *const f32),
                        ptr::read_unaligned((a + 4) as *const f32),
                        ptr::read_unaligned((a + 8) as *const f32),
                    ]
                },
                st: [self.read_i16(a + 14), self.read_i16(a + 16)],
                rgba: [
                    self.read_u8(a + 18),
                    self.read_u8(a + 19),
                    self.read_u8(a + 20),
                    self.read_u8(a + 21),
                ],
            },
            GbiDataFormat::Fixed => RawVertex {
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
            },
        }
    }
}
