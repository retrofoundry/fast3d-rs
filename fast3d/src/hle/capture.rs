use super::math::Mat4;
use super::mem::{Command, RawVertex};
use crate::{DataFormat, Hardware, Microcode, Rdram, ViRegisters};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;

mod format;
mod replay;
pub use crate::scene::ColorImage;
pub use format::{Fixture, Frame, Provenance};
pub use replay::{CaptureFrame, ReplayOutput};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    Invalid(String),
    UnsupportedBackend,
    MissingSpan { address: u64, length: u64 },
    ConflictingRead { address: u64 },
    ClearPolicyMismatch,
    Gpu(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(s) => write!(f, "invalid fixture: {s}"),
            Self::UnsupportedBackend => write!(f, "memory backend has no capture layout"),
            Self::MissingSpan { address, length } => {
                write!(f, "missing memory at {address:#x}, length {length}")
            }
            Self::ConflictingRead { address } => {
                write!(f, "memory changed within a task at {address:#x}")
            }
            Self::ClearPolicyMismatch => write!(f, "frame depends on prior framebuffer contents"),
            Self::Gpu(s) => write!(f, "capture GPU error: {s}"),
        }
    }
}
impl std::error::Error for CaptureError {}

type Result<T> = std::result::Result<T, CaptureError>;
fn invalid(s: &str) -> CaptureError {
    CaptureError::Invalid(s.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressSpace {
    Image,
    Host,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteOrder {
    Big,
    Little,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixedMatrixPacking {
    SplitHalfwords,
    PackedWords,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryLayout {
    pub address_space: AddressSpace,
    pub byte_order: ByteOrder,
    pub command_word_bytes: u8,
    pub command_stride: u8,
    pub fixed_matrix_packing: FixedMatrixPacking,
}

impl MemoryLayout {
    pub const IMAGE: Self = Self {
        address_space: AddressSpace::Image,
        byte_order: ByteOrder::Big,
        command_word_bytes: 4,
        command_stride: 8,
        fixed_matrix_packing: FixedMatrixPacking::SplitHalfwords,
    };
    pub const HOST64_LE: Self = Self {
        address_space: AddressSpace::Host,
        byte_order: ByteOrder::Little,
        command_word_bytes: 8,
        command_stride: 16,
        fixed_matrix_packing: FixedMatrixPacking::PackedWords,
    };

    #[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
    pub fn host_native() -> Self {
        Self {
            byte_order: if cfg!(target_endian = "little") {
                ByteOrder::Little
            } else {
                ByteOrder::Big
            },
            ..Self::HOST64_LE
        }
    }

    fn validate(self) -> Result<()> {
        match self.address_space {
            AddressSpace::Image if self == Self::IMAGE => Ok(()),
            AddressSpace::Host
                if Self {
                    byte_order: ByteOrder::Little,
                    ..self
                } == Self::HOST64_LE =>
            {
                Ok(())
            }
            _ => Err(invalid("unsupported memory layout")),
        }
    }

    fn word(self, bytes: &[u8]) -> u64 {
        match self.byte_order {
            ByteOrder::Little => bytes.iter().rev().fold(0, |v, &b| (v << 8) | b as u64),
            ByteOrder::Big => bytes.iter().fold(0, |v, &b| (v << 8) | b as u64),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceLayout {
    pub memory: MemoryLayout,
    pub segments: [u64; 16],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySpan {
    pub address: u64,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub entry: u64,
    pub microcode: Microcode,
    pub data_format: DataFormat,
    pub order: u32,
    pub source: SourceLayout,
    pub spans: Vec<MemorySpan>,
}

impl Task {
    /// Walks the captured memory without a GPU and returns the final colour target.
    /// Missing memory or interpreter diagnostics reject the result.
    pub fn final_color_image(&self) -> Result<ColorImage> {
        let hardware = ReplayHardware::new(self, None)?;
        let result = super::interp::interpret(
            hardware.rdram(),
            self.entry,
            self.microcode.into(),
            self.data_format,
        );
        hardware.check()?;
        if let Some(diagnostic) = result.diags.first() {
            return Err(CaptureError::Invalid(format!(
                "task {}: {diagnostic}",
                self.order
            )));
        }
        Ok(result.scene.color_image)
    }

    fn validate(&self) -> Result<()> {
        self.source.memory.validate()?;
        if self.source.memory.address_space == AddressSpace::Image
            && (self.data_format != DataFormat::Fixed
                || self.source.segments.iter().any(|&s| s > u32::MAX as u64))
        {
            return Err(invalid(
                "image layout requires fixed data and 32-bit segments",
            ));
        }
        let mut end = 0;
        for span in &self.spans {
            if span.bytes.is_empty() || span.address < end {
                return Err(invalid("empty, overlapping or unordered spans"));
            }
            end = span
                .address
                .checked_add(span.bytes.len() as u64)
                .ok_or_else(|| invalid("span address overflow"))?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct ReadLog {
    source: Option<SourceLayout>,
    spans: BTreeMap<u64, Vec<u8>>,
    error: Option<CaptureError>,
}

impl ReadLog {
    fn fail(&mut self, error: CaptureError) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }

    fn insert(&mut self, address: u64, bytes: &[u8]) {
        if bytes.is_empty() || self.error.is_some() {
            return;
        }
        let Some(end) = address.checked_add(bytes.len() as u64) else {
            self.fail(invalid("read address overflow"));
            return;
        };
        let start = self
            .spans
            .range(..=address)
            .next_back()
            .filter(|(a, b)| **a + b.len() as u64 >= address)
            .map_or(address, |(&a, _)| a);
        let keys: Vec<u64> = self.spans.range(start..=end).map(|(&a, _)| a).collect();
        for &a in &keys {
            let old = &self.spans[&a];
            let lo = a.max(address);
            let hi = (a + old.len() as u64).min(end);
            if lo < hi {
                for at in lo..hi {
                    if old[(at - a) as usize] != bytes[(at - address) as usize] {
                        self.fail(CaptureError::ConflictingRead { address: at });
                        return;
                    }
                }
            }
        }
        let merged_end = keys
            .last()
            .map_or(end, |a| end.max(*a + self.spans[a].len() as u64));
        let Ok(len) = usize::try_from(merged_end - start) else {
            self.fail(invalid("snapshot too large"));
            return;
        };
        let mut merged = self.spans.remove(&start).unwrap_or_default();
        merged.resize(len, 0);
        for a in keys.into_iter().filter(|&a| a != start) {
            let old = self.spans.remove(&a).unwrap();
            merged[(a - start) as usize..(a - start) as usize + old.len()].copy_from_slice(&old);
        }
        merged[(address - start) as usize..(end - start) as usize].copy_from_slice(bytes);
        self.spans.insert(start, merged);
    }
}

pub struct RecordingHardware<'a, H> {
    hardware: &'a H,
    log: RefCell<ReadLog>,
}

impl<'a, H: Hardware> RecordingHardware<'a, H> {
    pub fn new(hardware: &'a H) -> Self {
        Self {
            hardware,
            log: RefCell::default(),
        }
    }

    pub fn finish(
        self,
        entry: u64,
        microcode: Microcode,
        data_format: DataFormat,
        order: u32,
    ) -> Result<Task> {
        let log = self.log.into_inner();
        if let Some(e) = log.error {
            return Err(e);
        }
        let task = Task {
            entry,
            microcode,
            data_format,
            order,
            source: log.source.ok_or_else(|| invalid("task was not consumed"))?,
            spans: log
                .spans
                .into_iter()
                .map(|(address, bytes)| MemorySpan { address, bytes })
                .collect(),
        };
        task.validate()?;
        Ok(task)
    }
}

impl<H: Hardware> Hardware for RecordingHardware<'_, H> {
    fn rdram(&self) -> impl Rdram + '_ {
        let inner = self.hardware.rdram();
        let mut log = self.log.borrow_mut();
        let source = inner.capture_layout();
        if log.source.is_some() {
            log.fail(invalid("recording hardware consumed more than once"));
        }
        if let Some(source) = source {
            if let Err(e) = source.memory.validate() {
                log.fail(e);
            }
            log.source = Some(source);
        } else {
            log.fail(CaptureError::UnsupportedBackend);
        }
        RecordingRdram {
            inner,
            layout: source.map_or(MemoryLayout::IMAGE, |s| s.memory),
            log: &self.log,
        }
    }
    fn vi(&self) -> Option<ViRegisters> {
        self.hardware.vi()
    }
}

pub struct RecordingRdram<'a, R> {
    inner: R,
    layout: MemoryLayout,
    log: &'a RefCell<ReadLog>,
}

trait DecodeMemory {
    fn layout(&self) -> MemoryLayout;
    fn bytes(&self, address: u64, length: usize) -> Cow<'_, [u8]>;
    fn word(&self, address: u64, length: usize) -> u64 {
        self.layout().word(&self.bytes(address, length))
    }
    fn command(&self, address: u64) -> Command {
        let layout = self.layout();
        let b = self.bytes(address, layout.command_stride as usize);
        let width = layout.command_word_bytes as usize;
        let w0 = layout.word(&b[..width]) as u32;
        let w1_addr = layout.word(&b[width..2 * width]);
        Command {
            w0,
            w1: w1_addr as u32,
            w1_addr,
        }
    }
    fn matrix(&self, address: u64, format: DataFormat) -> Mat4 {
        let bytes = self.bytes(address, 64);
        let word = |off: usize, n: usize| self.layout().word(&bytes[off..off + n]) as u32;
        let mut out = [[0.0; 4]; 4];
        for (k, v) in out.iter_mut().flatten().enumerate() {
            *v = match format {
                DataFormat::Float => f32::from_bits(word(k * 4, 4)),
                DataFormat::Fixed => {
                    let (hi, lo) = match self.layout().fixed_matrix_packing {
                        FixedMatrixPacking::SplitHalfwords => (word(k * 2, 2), word(32 + k * 2, 2)),
                        FixedMatrixPacking::PackedWords => {
                            let shift = if k % 2 == 0 { 16 } else { 0 };
                            (
                                (word(k / 2 * 4, 4) >> shift) & 0xffff,
                                (word(32 + k / 2 * 4, 4) >> shift) & 0xffff,
                            )
                        }
                    };
                    ((hi << 16 | lo) as i32) as f32 / 65536.0
                }
            };
        }
        out
    }
    fn vertex(&self, address: u64, format: DataFormat) -> RawVertex {
        let (pos_len, st_off) = match format {
            DataFormat::Fixed => (6, 8),
            DataFormat::Float => (12, 14),
        };
        let pos = self.bytes(address, pos_len);
        let rest = self.bytes(address.saturating_add(st_off), 8);
        let mut out = RawVertex {
            pos: [0.; 3],
            st: [0; 2],
            rgba: rest[4..8].try_into().unwrap(),
        };
        for (i, v) in out.pos.iter_mut().enumerate() {
            *v = match format {
                DataFormat::Fixed => self.layout().word(&pos[i * 2..i * 2 + 2]) as i16 as f32,
                DataFormat::Float => {
                    f32::from_bits(self.layout().word(&pos[i * 4..i * 4 + 4]) as u32)
                }
            };
        }
        for (i, v) in out.st.iter_mut().enumerate() {
            *v = self.layout().word(&rest[i * 2..i * 2 + 2]) as i16;
        }
        out
    }
}

impl<R: Rdram> DecodeMemory for RecordingRdram<'_, R> {
    fn layout(&self) -> MemoryLayout {
        self.layout
    }
    fn bytes(&self, address: u64, length: usize) -> Cow<'_, [u8]> {
        if self.log.borrow().error.is_some() {
            return Cow::Owned(vec![0; length]);
        }
        let bytes = self.inner.read_bytes(address, length).into_owned();
        if bytes.len() != length {
            self.log.borrow_mut().fail(CaptureError::MissingSpan {
                address,
                length: length as u64,
            });
            return Cow::Owned(vec![0; length]);
        }
        self.log.borrow_mut().insert(address, &bytes);
        Cow::Owned(bytes)
    }
}

macro_rules! decoded_reads {
    () => {
        fn read_command(&self, pc: u64) -> Command {
            self.command(pc)
        }
        fn command_stride(&self) -> u64 {
            self.layout().command_stride as u64
        }
        fn read_u8(&self, a: u64) -> u8 {
            self.word(a, 1) as u8
        }
        fn read_i8(&self, a: u64) -> i8 {
            self.word(a, 1) as i8
        }
        fn read_u16(&self, a: u64) -> u16 {
            self.word(a, 2) as u16
        }
        fn read_i16(&self, a: u64) -> i16 {
            self.word(a, 2) as i16
        }
        fn read_bytes<'s>(&'s self, a: u64, len: usize) -> Cow<'s, [u8]> {
            self.bytes(a, len)
        }
        fn read_matrix(&self, a: u64, fmt: DataFormat) -> Mat4 {
            self.matrix(a, fmt)
        }
        fn read_vertex(&self, a: u64, fmt: DataFormat) -> RawVertex {
            self.vertex(a, fmt)
        }
        fn vertex_stride(&self, fmt: DataFormat) -> u64 {
            match fmt {
                DataFormat::Fixed => 16,
                DataFormat::Float => 24,
            }
        }
        fn is_rdram_image(&self) -> bool {
            self.layout().address_space == AddressSpace::Image
        }
    };
}

impl<R: Rdram> Rdram for RecordingRdram<'_, R> {
    fn set_segment(&mut self, seg: u32, value: u64) {
        self.inner.set_segment(seg, value);
    }
    fn resolve(&self, addr: u64) -> u64 {
        self.inner.resolve(addr)
    }
    fn resolve_masked(&self, addr: u64) -> u64 {
        self.inner.resolve_masked(addr)
    }
    fn in_bounds(&self, pc: u64, stride: u64) -> bool {
        if self.log.borrow().error.is_some() {
            return false;
        }
        let valid = self.inner.in_bounds(pc, stride);
        if !valid {
            self.log.borrow_mut().fail(CaptureError::MissingSpan {
                address: pc,
                length: stride,
            });
        }
        valid
    }
    decoded_reads!();
}

pub struct ReplayHardware<'a> {
    task: &'a Task,
    vi: Option<ViRegisters>,
    error: RefCell<Option<CaptureError>>,
}

impl<'a> ReplayHardware<'a> {
    pub fn new(task: &'a Task, vi: Option<ViRegisters>) -> Result<Self> {
        task.validate()?;
        Ok(Self {
            task,
            vi,
            error: RefCell::new(None),
        })
    }
    pub fn check(&self) -> Result<()> {
        self.error.borrow().clone().map_or(Ok(()), Err)
    }
}
impl Hardware for ReplayHardware<'_> {
    fn rdram(&self) -> impl Rdram + '_ {
        ReplayRdram {
            task: self.task,
            segments: self.task.source.segments,
            error: &self.error,
        }
    }
    fn vi(&self) -> Option<ViRegisters> {
        self.vi
    }
}

pub struct ReplayRdram<'a> {
    task: &'a Task,
    segments: [u64; 16],
    error: &'a RefCell<Option<CaptureError>>,
}

impl ReplayRdram<'_> {
    fn fail(&self, error: CaptureError) {
        let mut slot = self.error.borrow_mut();
        if slot.is_none() {
            *slot = Some(error);
        }
    }
    fn find(&self, address: u64, length: u64) -> Option<Cow<'_, [u8]>> {
        if length == 0 {
            return Some(Cow::Borrowed(&[]));
        }
        address.checked_add(length)?;
        let i = self
            .task
            .spans
            .partition_point(|span| span.address <= address)
            .checked_sub(1)?;
        let mut parts = Vec::new();
        let mut cursor = address;
        let mut remaining = length;
        for span in &self.task.spans[i..] {
            let offset = cursor.checked_sub(span.address)?;
            let available = (span.bytes.len() as u64).checked_sub(offset)?;
            let count = remaining.min(available);
            let offset = usize::try_from(offset).ok()?;
            let count_usize = usize::try_from(count).ok()?;
            let bytes = span.bytes.get(offset..offset.checked_add(count_usize)?)?;
            if parts.is_empty() && count == remaining {
                return Some(Cow::Borrowed(bytes));
            }
            parts.push(bytes);
            cursor = cursor.checked_add(count)?;
            remaining -= count;
            if remaining == 0 {
                return Some(Cow::Owned(parts.concat()));
            }
        }
        None
    }
}

impl DecodeMemory for ReplayRdram<'_> {
    fn layout(&self) -> MemoryLayout {
        self.task.source.memory
    }
    fn bytes(&self, address: u64, length: usize) -> Cow<'_, [u8]> {
        match self.find(address, length as u64) {
            Some(bytes) => bytes,
            None => {
                self.fail(CaptureError::MissingSpan {
                    address,
                    length: length as u64,
                });
                Cow::Owned(vec![0; length])
            }
        }
    }
}
impl Rdram for ReplayRdram<'_> {
    fn set_segment(&mut self, seg: u32, value: u64) {
        self.segments[(seg & 15) as usize] = match self.layout().address_space {
            AddressSpace::Image => value as u32 as u64,
            AddressSpace::Host => value,
        };
    }
    fn resolve(&self, addr: u64) -> u64 {
        let base = self.segments[((addr >> 24) & 15) as usize];
        match self.layout().address_space {
            AddressSpace::Image => (base as u32).wrapping_add(addr as u32 & 0x00ff_ffff) as u64,
            AddressSpace::Host if base == 0 => addr,
            AddressSpace::Host => base.checked_add(addr & 0x00ff_ffff).unwrap_or_else(|| {
                self.fail(invalid("segment address overflow"));
                0
            }),
        }
    }
    fn resolve_masked(&self, addr: u64) -> u64 {
        let resolved = self.resolve(addr);
        match self.layout().address_space {
            AddressSpace::Image => resolved & 0x00ff_fff8,
            AddressSpace::Host => resolved,
        }
    }
    fn in_bounds(&self, pc: u64, stride: u64) -> bool {
        if self.error.borrow().is_some() {
            return false;
        }
        if self.find(pc, stride).is_some() {
            true
        } else {
            self.fail(CaptureError::MissingSpan {
                address: pc,
                length: stride,
            });
            false
        }
    }
    decoded_reads!();
}

#[cfg(test)]
mod tests;
