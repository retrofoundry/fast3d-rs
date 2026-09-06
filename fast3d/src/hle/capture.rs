use super::math::Mat4;
use super::mem::{Command, RawVertex};
use crate::{DataFormat, Hardware, MemoryError, MemoryErrorKind, Microcode, Rdram, ViRegisters};
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

fn memory_error(address: u64, length: u64, kind: MemoryErrorKind) -> MemoryError {
    MemoryError {
        address,
        length,
        kind,
    }
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
            .filter(|(a, b)| {
                u64::try_from(b.len())
                    .ok()
                    .and_then(|length| a.checked_add(length))
                    .is_some_and(|span_end| span_end >= address)
            })
            .map_or(address, |(&a, _)| a);
        let key_count = self.spans.range(start..=end).count();
        let mut keys = Vec::new();
        if keys.try_reserve_exact(key_count).is_err() {
            self.fail(invalid("snapshot allocation failed"));
            return;
        }
        keys.extend(self.spans.range(start..=end).map(|(&a, _)| a));
        for &a in &keys {
            let old = &self.spans[&a];
            let Some(old_end) = u64::try_from(old.len())
                .ok()
                .and_then(|length| a.checked_add(length))
            else {
                self.fail(invalid("snapshot address overflow"));
                return;
            };
            let lo = a.max(address);
            let hi = old_end.min(end);
            if lo < hi {
                for at in lo..hi {
                    if old[(at - a) as usize] != bytes[(at - address) as usize] {
                        self.fail(CaptureError::ConflictingRead { address: at });
                        return;
                    }
                }
            }
        }
        let Some(merged_end) = keys.last().map_or(Some(end), |a| {
            u64::try_from(self.spans[a].len())
                .ok()
                .and_then(|length| a.checked_add(length))
                .map(|old_end| end.max(old_end))
        }) else {
            self.fail(invalid("snapshot address overflow"));
            return;
        };
        let Ok(len) = usize::try_from(merged_end - start) else {
            self.fail(invalid("snapshot too large"));
            return;
        };
        let existing_len = self.spans.get(&start).map_or(0, Vec::len);
        let Some(growth) = len.checked_sub(existing_len) else {
            self.fail(invalid("snapshot length underflow"));
            return;
        };
        let mut empty = Vec::new();
        let reserve_failed = if let Some(existing) = self.spans.get_mut(&start) {
            existing.try_reserve_exact(growth).is_err()
        } else {
            empty.try_reserve_exact(len).is_err()
        };
        if reserve_failed {
            self.fail(invalid("snapshot allocation failed"));
            return;
        }
        let mut merged = self.spans.remove(&start).unwrap_or(empty);
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
        finish_recording(self.log.into_inner(), entry, microcode, data_format, order)
    }
}

fn finish_recording(
    log: ReadLog,
    entry: u64,
    microcode: Microcode,
    data_format: DataFormat,
    order: u32,
) -> Result<Task> {
    if let Some(error) = log.error {
        return Err(error);
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
    fn bytes(&self, address: u64, length: usize)
        -> std::result::Result<Cow<'_, [u8]>, MemoryError>;

    fn word(&self, address: u64, length: usize) -> std::result::Result<u64, MemoryError> {
        let bytes = self.bytes(address, length)?;
        Ok(self.layout().word(&bytes))
    }

    fn command(&self, address: u64) -> std::result::Result<Command, MemoryError> {
        let layout = self.layout();
        let bytes = self.bytes(address, layout.command_stride as usize)?;
        let width = layout.command_word_bytes as usize;
        let words = bytes.get(..2 * width).ok_or_else(|| {
            memory_error(
                address,
                u64::from(layout.command_stride),
                MemoryErrorKind::Unavailable,
            )
        })?;
        let w0 = layout.word(&words[..width]) as u32;
        let w1_addr = layout.word(&words[width..]);
        Ok(Command {
            w0,
            w1: w1_addr as u32,
            w1_addr,
        })
    }

    fn matrix(&self, address: u64, format: DataFormat) -> std::result::Result<Mat4, MemoryError> {
        if self.layout().address_space == AddressSpace::Image && format == DataFormat::Float {
            return Err(memory_error(
                address,
                64,
                MemoryErrorKind::UnsupportedFormat,
            ));
        }
        let bytes = self.bytes(address, 64)?;
        let word = |off: usize, n: usize| self.layout().word(&bytes[off..off + n]) as u32;
        let mut out = [[0.0; 4]; 4];
        for (k, v) in out.iter_mut().flatten().enumerate() {
            *v = match format {
                DataFormat::Float => f32::from_bits(word(k * 4, 4)),
                DataFormat::Fixed => {
                    let (hi, lo) = match self.layout().fixed_matrix_packing {
                        FixedMatrixPacking::SplitHalfwords => (word(k * 2, 2), word(32 + k * 2, 2)),
                        FixedMatrixPacking::PackedWords => {
                            let shift = if k.is_multiple_of(2) { 16 } else { 0 };
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
        Ok(out)
    }

    fn vertex(
        &self,
        address: u64,
        format: DataFormat,
    ) -> std::result::Result<RawVertex, MemoryError> {
        if self.layout().address_space == AddressSpace::Image && format == DataFormat::Float {
            return Err(memory_error(
                address,
                24,
                MemoryErrorKind::UnsupportedFormat,
            ));
        }
        let (pos_len, st_off) = match format {
            DataFormat::Fixed => (6, 8),
            DataFormat::Float => (12, 14),
        };
        let rest_address = address
            .checked_add(st_off)
            .ok_or_else(|| memory_error(address, st_off, MemoryErrorKind::AddressOverflow))?;
        let pos = self.bytes(address, pos_len as usize)?;
        let rest = self.bytes(rest_address, 8)?;
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
        Ok(out)
    }
}

impl<R: Rdram> RecordingRdram<'_, R> {
    fn source<T>(
        &self,
        result: std::result::Result<T, MemoryError>,
    ) -> std::result::Result<T, MemoryError> {
        result.inspect_err(|error| {
            self.log.borrow_mut().fail(CaptureError::MissingSpan {
                address: error.address,
                length: error.length,
            });
        })
    }
}

impl<R: Rdram> DecodeMemory for RecordingRdram<'_, R> {
    fn layout(&self) -> MemoryLayout {
        self.layout
    }
    fn bytes(
        &self,
        address: u64,
        length: usize,
    ) -> std::result::Result<Cow<'_, [u8]>, MemoryError> {
        let length_u64 = u64::try_from(length)
            .map_err(|_| memory_error(address, u64::MAX, MemoryErrorKind::AddressOverflow))?;
        if self.log.borrow().error.is_some() {
            return Err(memory_error(
                address,
                length_u64,
                MemoryErrorKind::Unavailable,
            ));
        }
        let bytes = match self.inner.read_bytes(address, length) {
            Ok(bytes) => bytes,
            Err(error) => return self.source(Err(error)),
        };
        if bytes.len() != length {
            self.log.borrow_mut().fail(CaptureError::MissingSpan {
                address,
                length: length_u64,
            });
            return Err(memory_error(
                address,
                length_u64,
                MemoryErrorKind::Unavailable,
            ));
        }
        self.log.borrow_mut().insert(address, &bytes);
        if self.log.borrow().error.is_some() {
            return Err(memory_error(
                address,
                length_u64,
                MemoryErrorKind::Unavailable,
            ));
        }
        Ok(bytes)
    }
}

impl<R: Rdram> Rdram for RecordingRdram<'_, R> {
    fn set_segment(&mut self, seg: u32, value: u64) {
        self.inner.set_segment(seg, value);
    }
    fn resolve(&self, addr: u64) -> std::result::Result<u64, MemoryError> {
        self.inner.resolve(addr)
    }
    fn resolve_masked(&self, addr: u64) -> std::result::Result<u64, MemoryError> {
        self.inner.resolve_masked(addr)
    }
    fn read_command(&self, address: u64) -> std::result::Result<Command, MemoryError> {
        let command = self.source(self.inner.read_command(address))?;
        self.bytes(address, self.layout.command_stride as usize)?;
        Ok(command)
    }
    fn command_stride(&self) -> u64 {
        u64::from(self.layout.command_stride)
    }
    fn in_bounds(&self, address: u64, length: u64) -> bool {
        self.inner.in_bounds(address, length)
    }
    fn read_u8(&self, address: u64) -> std::result::Result<u8, MemoryError> {
        let value = self.source(self.inner.read_u8(address))?;
        self.bytes(address, 1)?;
        Ok(value)
    }
    fn read_i8(&self, address: u64) -> std::result::Result<i8, MemoryError> {
        let value = self.source(self.inner.read_i8(address))?;
        self.bytes(address, 1)?;
        Ok(value)
    }
    fn read_u16(&self, address: u64) -> std::result::Result<u16, MemoryError> {
        let value = self.source(self.inner.read_u16(address))?;
        self.bytes(address, 2)?;
        Ok(value)
    }
    fn read_i16(&self, address: u64) -> std::result::Result<i16, MemoryError> {
        let value = self.source(self.inner.read_i16(address))?;
        self.bytes(address, 2)?;
        Ok(value)
    }
    fn read_bytes(
        &self,
        address: u64,
        length: usize,
    ) -> std::result::Result<Cow<'_, [u8]>, MemoryError> {
        self.bytes(address, length)
    }
    fn read_matrix(
        &self,
        address: u64,
        format: DataFormat,
    ) -> std::result::Result<Mat4, MemoryError> {
        let matrix = self.source(self.inner.read_matrix(address, format))?;
        self.bytes(address, 64)?;
        Ok(matrix)
    }
    fn vertex_stride(&self, format: DataFormat) -> std::result::Result<u64, MemoryError> {
        self.inner.vertex_stride(format)
    }
    fn read_vertex(
        &self,
        address: u64,
        format: DataFormat,
    ) -> std::result::Result<RawVertex, MemoryError> {
        let vertex = self.source(self.inner.read_vertex(address, format))?;
        self.vertex(address, format)?;
        Ok(vertex)
    }
    fn is_rdram_image(&self) -> bool {
        self.layout.address_space == AddressSpace::Image
    }
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
    fn find(
        &self,
        address: u64,
        length: u64,
    ) -> std::result::Result<Option<Cow<'_, [u8]>>, MemoryError> {
        if length == 0 {
            return Ok(Some(Cow::Borrowed(&[])));
        }
        let requested_end = address
            .checked_add(length)
            .ok_or_else(|| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
        let i = self
            .task
            .spans
            .partition_point(|span| span.address <= address)
            .checked_sub(1);
        let Some(i) = i else {
            return Ok(None);
        };

        let mut cursor = address;
        for span in &self.task.spans[i..] {
            if span.address > cursor {
                return Ok(None);
            }
            let span_len = u64::try_from(span.bytes.len())
                .map_err(|_| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
            let span_end = span
                .address
                .checked_add(span_len)
                .ok_or_else(|| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
            if span_end <= cursor {
                continue;
            }
            cursor = span_end.min(requested_end);
            if cursor == requested_end {
                break;
            }
        }
        if cursor != requested_end {
            return Ok(None);
        }

        let first = &self.task.spans[i];
        let first_offset = address
            .checked_sub(first.address)
            .ok_or_else(|| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
        let first_len = u64::try_from(first.bytes.len())
            .map_err(|_| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
        let first_available = first_len
            .checked_sub(first_offset)
            .ok_or_else(|| memory_error(address, length, MemoryErrorKind::Unavailable))?;
        if first_available >= length {
            let offset = usize::try_from(first_offset)
                .map_err(|_| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
            let count = usize::try_from(length)
                .map_err(|_| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
            let end = offset
                .checked_add(count)
                .ok_or_else(|| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
            let bytes = first
                .bytes
                .get(offset..end)
                .ok_or_else(|| memory_error(address, length, MemoryErrorKind::Unavailable))?;
            return Ok(Some(Cow::Borrowed(bytes)));
        }

        let output_len = usize::try_from(length)
            .map_err(|_| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(output_len)
            .map_err(|_| memory_error(address, length, MemoryErrorKind::Unavailable))?;
        cursor = address;
        let mut remaining = length;
        for span in &self.task.spans[i..] {
            let offset = cursor
                .checked_sub(span.address)
                .ok_or_else(|| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
            let span_len = u64::try_from(span.bytes.len())
                .map_err(|_| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
            let available = span_len
                .checked_sub(offset)
                .ok_or_else(|| memory_error(address, length, MemoryErrorKind::Unavailable))?;
            let count = remaining.min(available);
            let offset = usize::try_from(offset)
                .map_err(|_| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
            let count_usize = usize::try_from(count)
                .map_err(|_| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
            let Some(end) = offset.checked_add(count_usize) else {
                return Err(memory_error(
                    address,
                    length,
                    MemoryErrorKind::AddressOverflow,
                ));
            };
            let Some(bytes) = span.bytes.get(offset..end) else {
                return Err(memory_error(address, length, MemoryErrorKind::Unavailable));
            };
            output.extend_from_slice(bytes);
            cursor = cursor
                .checked_add(count)
                .ok_or_else(|| memory_error(address, length, MemoryErrorKind::AddressOverflow))?;
            remaining -= count;
            if remaining == 0 {
                return Ok(Some(Cow::Owned(output)));
            }
        }
        Err(memory_error(address, length, MemoryErrorKind::Unavailable))
    }
}

impl DecodeMemory for ReplayRdram<'_> {
    fn layout(&self) -> MemoryLayout {
        self.task.source.memory
    }
    fn bytes(
        &self,
        address: u64,
        length: usize,
    ) -> std::result::Result<Cow<'_, [u8]>, MemoryError> {
        let length = u64::try_from(length)
            .map_err(|_| memory_error(address, u64::MAX, MemoryErrorKind::AddressOverflow))?;
        match self.find(address, length)? {
            Some(bytes) => Ok(bytes),
            None => {
                self.fail(CaptureError::MissingSpan { address, length });
                Err(memory_error(address, length, MemoryErrorKind::Unavailable))
            }
        }
    }
}
impl Rdram for ReplayRdram<'_> {
    fn capture_layout(&self) -> Option<SourceLayout> {
        Some(SourceLayout {
            memory: self.task.source.memory,
            segments: self.segments,
        })
    }

    fn set_segment(&mut self, seg: u32, value: u64) {
        self.segments[(seg & 15) as usize] = match self.layout().address_space {
            AddressSpace::Image => value as u32 as u64,
            AddressSpace::Host => value,
        };
    }
    fn resolve(&self, addr: u64) -> std::result::Result<u64, MemoryError> {
        match self.layout().address_space {
            AddressSpace::Image => {
                let address = u32::try_from(addr)
                    .map_err(|_| memory_error(addr, 0, MemoryErrorKind::AddressOverflow))?;
                let base = self.segments[((address >> 24) & 15) as usize] as u32;
                Ok(base.wrapping_add(address & 0x00ff_ffff) as u64)
            }
            AddressSpace::Host => {
                let base = self.segments[((addr >> 24) & 15) as usize];
                if base == 0 {
                    Ok(addr)
                } else {
                    base.checked_add(addr & 0x00ff_ffff)
                        .ok_or_else(|| memory_error(addr, 0, MemoryErrorKind::AddressOverflow))
                }
            }
        }
    }
    fn resolve_masked(&self, addr: u64) -> std::result::Result<u64, MemoryError> {
        let resolved = self.resolve(addr)?;
        match self.layout().address_space {
            AddressSpace::Image => Ok(resolved & 0x00ff_fff8),
            AddressSpace::Host => Ok(resolved),
        }
    }
    fn read_command(&self, address: u64) -> std::result::Result<Command, MemoryError> {
        self.command(address)
    }
    fn command_stride(&self) -> u64 {
        u64::from(self.layout().command_stride)
    }
    fn in_bounds(&self, address: u64, length: u64) -> bool {
        matches!(self.find(address, length), Ok(Some(_)))
    }
    fn read_u8(&self, address: u64) -> std::result::Result<u8, MemoryError> {
        Ok(self.word(address, 1)? as u8)
    }
    fn read_i8(&self, address: u64) -> std::result::Result<i8, MemoryError> {
        Ok(self.word(address, 1)? as i8)
    }
    fn read_u16(&self, address: u64) -> std::result::Result<u16, MemoryError> {
        Ok(self.word(address, 2)? as u16)
    }
    fn read_i16(&self, address: u64) -> std::result::Result<i16, MemoryError> {
        Ok(self.word(address, 2)? as i16)
    }
    fn read_bytes(
        &self,
        address: u64,
        length: usize,
    ) -> std::result::Result<Cow<'_, [u8]>, MemoryError> {
        self.bytes(address, length)
    }
    fn read_matrix(
        &self,
        address: u64,
        format: DataFormat,
    ) -> std::result::Result<Mat4, MemoryError> {
        self.matrix(address, format)
    }
    fn vertex_stride(&self, format: DataFormat) -> std::result::Result<u64, MemoryError> {
        if self.layout().address_space == AddressSpace::Image && format == DataFormat::Float {
            Err(memory_error(0, 0, MemoryErrorKind::UnsupportedFormat))
        } else {
            Ok(match format {
                DataFormat::Fixed => 16,
                DataFormat::Float => 24,
            })
        }
    }
    fn read_vertex(
        &self,
        address: u64,
        format: DataFormat,
    ) -> std::result::Result<RawVertex, MemoryError> {
        if self.layout().address_space == AddressSpace::Image && format == DataFormat::Float {
            return self.vertex(address, format);
        }
        let stride = match format {
            DataFormat::Fixed => 16,
            DataFormat::Float => 24,
        };
        address
            .checked_add(stride)
            .ok_or_else(|| memory_error(address, stride, MemoryErrorKind::AddressOverflow))?;
        let position_length = match format {
            DataFormat::Fixed => 6,
            DataFormat::Float => 12,
        };
        let rest_offset = match format {
            DataFormat::Fixed => 8,
            DataFormat::Float => 14,
        };
        let rest_address = address
            .checked_add(rest_offset)
            .ok_or_else(|| memory_error(address, stride, MemoryErrorKind::AddressOverflow))?;
        let position = self
            .find(address, position_length)
            .map_err(|error| memory_error(address, stride, error.kind))?;
        let remainder = self
            .find(rest_address, 8)
            .map_err(|error| memory_error(address, stride, error.kind))?;
        if position.is_none() || remainder.is_none() {
            self.fail(CaptureError::MissingSpan {
                address,
                length: stride,
            });
            return Err(memory_error(address, stride, MemoryErrorKind::Unavailable));
        }
        self.vertex(address, format)
            .map_err(|error| memory_error(address, stride, error.kind))
    }
    fn is_rdram_image(&self) -> bool {
        self.layout().address_space == AddressSpace::Image
    }
}

#[cfg(test)]
mod tests;
