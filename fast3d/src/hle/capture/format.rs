use super::*;
use crate::{ClearPolicy, RendererConfig};

const MAGIC: &[u8; 8] = b"F3DCAP\0\0";
const VERSION: u32 = 1;
const LITTLE_ENDIAN: u32 = 0x0403_0201;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Provenance {
    pub decomp_revision: String,
    pub source_symbols: String,
    pub command_vector: String,
    pub synthetic_data: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub serial: u64,
    pub dither_seed: u32,
    pub config: RendererConfig,
    pub width: u32,
    pub height: u32,
    pub vi: Option<ViRegisters>,
    pub dual_source_blending: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixture {
    pub frame: Frame,
    pub tasks: Vec<Task>,
    pub provenance: Provenance,
}

impl Fixture {
    pub fn validate(&self) -> Result<()> {
        if self.frame.width == 0
            || self.frame.height == 0
            || self.frame.width > 16384
            || self.frame.height > 16384
        {
            return Err(invalid("output extent must be between 1 and 16384"));
        }
        if self.tasks.is_empty() {
            return Err(invalid("frame contains no tasks"));
        }
        format_id(self.frame.config.format)?;
        for (order, task) in self.tasks.iter().enumerate() {
            if task.order as usize != order {
                return Err(invalid("task order is not contiguous"));
            }
            task.validate()?;
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut w = Writer(Vec::new());
        w.0.extend_from_slice(MAGIC);
        w.u32(VERSION);
        w.u32(LITTLE_ENDIAN);
        w.u64(0);
        w.count(self.tasks.len())?;
        w.u32(0);
        let f = &self.frame;
        w.u64(f.serial);
        w.u32(f.dither_seed);
        w.u32(f.width);
        w.u32(f.height);
        w.u32(f.config.resolution_multiplier);
        w.u32(f.config.sample_count);
        w.u32(match f.config.present_mode {
            wgpu::PresentMode::AutoVsync => 0,
            wgpu::PresentMode::AutoNoVsync => 1,
            wgpu::PresentMode::Fifo => 2,
            wgpu::PresentMode::FifoRelaxed => 3,
            wgpu::PresentMode::Immediate => 4,
            wgpu::PresentMode::Mailbox => 5,
        });
        w.u32(format_id(f.config.format)?);
        w.u32(match f.config.clear_policy {
            ClearPolicy::PerFrame => 0,
            ClearPolicy::Persist => 1,
        });
        w.u32(match f.config.power_preference {
            wgpu::PowerPreference::None => 0,
            wgpu::PowerPreference::LowPower => 1,
            wgpu::PowerPreference::HighPerformance => 2,
        });
        w.u32(f.dual_source_blending as u32);
        w.u32(f.vi.is_some() as u32);
        let vi = f.vi.unwrap_or_default();
        for v in [
            vi.status,
            vi.origin,
            vi.width,
            vi.x_scale,
            vi.y_scale,
            vi.h_start,
            vi.v_start,
            vi.v_current,
        ] {
            w.u32(v);
        }
        for s in [
            &self.provenance.decomp_revision,
            &self.provenance.source_symbols,
            &self.provenance.command_vector,
            &self.provenance.synthetic_data,
        ] {
            w.string(s)?;
        }
        for task in &self.tasks {
            w.u32(task.order);
            w.u32(match task.microcode {
                Microcode::F3dex2 => 0,
                Microcode::F3d => 1,
            });
            w.u32(match task.data_format {
                DataFormat::Fixed => 0,
                DataFormat::Float => 1,
            });
            w.u32(0);
            w.u64(task.entry);
            let m = task.source.memory;
            w.0.extend_from_slice(&[
                match m.address_space {
                    AddressSpace::Image => 0,
                    AddressSpace::Host => 1,
                },
                match m.byte_order {
                    ByteOrder::Big => 0,
                    ByteOrder::Little => 1,
                },
                m.command_word_bytes,
                m.command_stride,
                match m.fixed_matrix_packing {
                    FixedMatrixPacking::SplitHalfwords => 0,
                    FixedMatrixPacking::PackedWords => 1,
                },
                0,
                0,
                0,
            ]);
            for base in task.source.segments {
                w.u64(base);
            }
            w.count(task.spans.len())?;
            w.u32(0);
            let payload_len: u64 = task.spans.iter().map(|s| s.bytes.len() as u64).sum();
            w.u64(payload_len);
            let mut offset = 0;
            for span in &task.spans {
                w.u64(span.address);
                w.u64(span.bytes.len() as u64);
                w.u64(offset);
                offset += span.bytes.len() as u64;
            }
            for span in &task.spans {
                w.0.extend_from_slice(&span.bytes);
            }
        }
        let length = w.0.len() as u64;
        w.0[16..24].copy_from_slice(&length.to_le_bytes());
        Ok(w.0)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader(bytes);
        if r.take(8)? != MAGIC || r.u32()? != VERSION || r.u32()? != LITTLE_ENDIAN {
            return Err(invalid("magic, version or container byte order"));
        }
        if r.u64()? != bytes.len() as u64 {
            return Err(invalid("container length"));
        }
        let task_count = r.u32()?;
        r.zero()?;
        let serial = r.u64()?;
        let dither_seed = r.u32()?;
        let width = r.u32()?;
        let height = r.u32()?;
        let resolution_multiplier = r.u32()?;
        let sample_count = r.u32()?;
        let present_mode = match r.u32()? {
            0 => wgpu::PresentMode::AutoVsync,
            1 => wgpu::PresentMode::AutoNoVsync,
            2 => wgpu::PresentMode::Fifo,
            3 => wgpu::PresentMode::FifoRelaxed,
            4 => wgpu::PresentMode::Immediate,
            5 => wgpu::PresentMode::Mailbox,
            _ => return Err(invalid("present mode")),
        };
        let format = match r.u32()? {
            0 => None,
            1 => Some(wgpu::TextureFormat::Rgba8Unorm),
            2 => Some(wgpu::TextureFormat::Bgra8Unorm),
            3 => Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            4 => Some(wgpu::TextureFormat::Bgra8UnormSrgb),
            _ => return Err(invalid("output format")),
        };
        let clear_policy = match r.u32()? {
            0 => ClearPolicy::PerFrame,
            1 => ClearPolicy::Persist,
            _ => return Err(invalid("clear policy")),
        };
        let power_preference = match r.u32()? {
            0 => wgpu::PowerPreference::None,
            1 => wgpu::PowerPreference::LowPower,
            2 => wgpu::PowerPreference::HighPerformance,
            _ => return Err(invalid("power preference")),
        };
        let dual_source_blending = r.boolean()?;
        let has_vi = r.boolean()?;
        let vi = ViRegisters {
            status: r.u32()?,
            origin: r.u32()?,
            width: r.u32()?,
            x_scale: r.u32()?,
            y_scale: r.u32()?,
            h_start: r.u32()?,
            v_start: r.u32()?,
            v_current: r.u32()?,
        };
        if !has_vi && vi != ViRegisters::default() {
            return Err(invalid("absent VI has nonzero registers"));
        }
        let frame = Frame {
            serial,
            dither_seed,
            width,
            height,
            vi: has_vi.then_some(vi),
            dual_source_blending,
            config: RendererConfig {
                resolution_multiplier,
                sample_count,
                present_mode,
                format,
                clear_policy,
                power_preference,
            },
        };
        let provenance = Provenance {
            decomp_revision: r.string()?,
            source_symbols: r.string()?,
            command_vector: r.string()?,
            synthetic_data: r.string()?,
        };
        let mut tasks = Vec::new();
        for _ in 0..task_count {
            let order = r.u32()?;
            let microcode = match r.u32()? {
                0 => Microcode::F3dex2,
                1 => Microcode::F3d,
                _ => return Err(invalid("microcode")),
            };
            let data_format = match r.u32()? {
                0 => DataFormat::Fixed,
                1 => DataFormat::Float,
                _ => return Err(invalid("data format")),
            };
            r.zero()?;
            let entry = r.u64()?;
            let m = r.take(8)?;
            let memory = MemoryLayout {
                address_space: match m[0] {
                    0 => AddressSpace::Image,
                    1 => AddressSpace::Host,
                    _ => return Err(invalid("address space")),
                },
                byte_order: match m[1] {
                    0 => ByteOrder::Big,
                    1 => ByteOrder::Little,
                    _ => return Err(invalid("source byte order")),
                },
                command_word_bytes: m[2],
                command_stride: m[3],
                fixed_matrix_packing: match m[4] {
                    0 => FixedMatrixPacking::SplitHalfwords,
                    1 => FixedMatrixPacking::PackedWords,
                    _ => return Err(invalid("matrix packing")),
                },
            };
            if m[5..] != [0, 0, 0] {
                return Err(invalid("reserved layout bytes"));
            }
            memory.validate()?;
            let mut segments = [0; 16];
            for s in &mut segments {
                *s = r.u64()?;
            }
            let count = r.u32()? as u64;
            r.zero()?;
            let payload_len = r.u64()?;
            let directory_len = count
                .checked_mul(24)
                .ok_or_else(|| invalid("span count overflow"))?;
            let directory = r.take_u64(directory_len)?;
            let payload = r.take_u64(payload_len)?;
            let mut spans = Vec::new();
            let mut offset = 0u64;
            let mut d = Reader(directory);
            for _ in 0..count {
                let address = d.u64()?;
                let length = d.u64()?;
                if d.u64()? != offset {
                    return Err(invalid("span payload offsets must be contiguous"));
                }
                let end = offset
                    .checked_add(length)
                    .ok_or_else(|| invalid("payload offset overflow"))?;
                if end > payload_len {
                    return Err(invalid("span exceeds payload"));
                }
                let start = usize::try_from(offset)
                    .map_err(|_| invalid("payload offset exceeds platform size"))?;
                let end_usize = usize::try_from(end)
                    .map_err(|_| invalid("payload length exceeds platform size"))?;
                spans.push(MemorySpan {
                    address,
                    bytes: payload[start..end_usize].to_vec(),
                });
                offset = end;
            }
            if offset != payload_len {
                return Err(invalid("unreferenced payload bytes"));
            }
            tasks.push(Task {
                entry,
                microcode,
                data_format,
                order,
                source: SourceLayout { memory, segments },
                spans,
            });
        }
        if !r.0.is_empty() {
            return Err(invalid("trailing bytes"));
        }
        let fixture = Self {
            frame,
            tasks,
            provenance,
        };
        fixture.validate()?;
        Ok(fixture)
    }
}

fn format_id(format: Option<wgpu::TextureFormat>) -> Result<u32> {
    match format {
        None => Ok(0),
        Some(wgpu::TextureFormat::Rgba8Unorm) => Ok(1),
        Some(wgpu::TextureFormat::Bgra8Unorm) => Ok(2),
        Some(wgpu::TextureFormat::Rgba8UnormSrgb) => Ok(3),
        Some(wgpu::TextureFormat::Bgra8UnormSrgb) => Ok(4),
        _ => Err(invalid("capture supports RGBA8 and BGRA8 outputs")),
    }
}
struct Writer(Vec<u8>);
impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn count(&mut self, n: usize) -> Result<()> {
        self.u32(u32::try_from(n).map_err(|_| invalid("record length exceeds u32"))?);
        Ok(())
    }
    fn string(&mut self, s: &str) -> Result<()> {
        self.count(s.len())?;
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let (head, tail) = self
            .0
            .split_at_checked(n)
            .ok_or_else(|| invalid("truncated record"))?;
        self.0 = tail;
        Ok(head)
    }
    fn take_u64(&mut self, n: u64) -> Result<&'a [u8]> {
        self.take(usize::try_from(n).map_err(|_| invalid("record length exceeds platform size"))?)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn zero(&mut self) -> Result<()> {
        if self.u32()? != 0 {
            Err(invalid("reserved word"))
        } else {
            Ok(())
        }
    }
    fn boolean(&mut self) -> Result<bool> {
        match self.u32()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(invalid("boolean")),
        }
    }
    fn string(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| invalid("provenance is not UTF-8"))
    }
}
