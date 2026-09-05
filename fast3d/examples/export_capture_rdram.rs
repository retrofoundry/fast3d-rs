#[cfg(not(target_arch = "wasm32"))]
mod native {
    use fast3d::capture::{Fixture, MemoryLayout};
    use fast3d::Microcode;
    use std::error::Error;
    use std::fmt::Write;
    use std::path::PathBuf;

    const RDRAM_BYTES: usize = 8 * 1024 * 1024;

    fn export(fixture: &Fixture) -> Result<(Vec<u8>, String), Box<dyn Error>> {
        fixture.validate()?;
        for task in &fixture.tasks {
            if task.source.memory != MemoryLayout::IMAGE {
                return Err("rt64 requires IMAGE layout (big-endian, 8-byte commands); HOST64 captures cannot be exported".into());
            }
        }
        let mut rdram = vec![0; RDRAM_BYTES];
        let mut occupied = vec![false; RDRAM_BYTES];
        let mut tasks = String::new();
        let mut final_color = None;
        for task in &fixture.tasks {
            if task.entry % 8 != 0 || task.entry >= RDRAM_BYTES as u64 {
                return Err(format!(
                    "task {} entry must be an aligned physical RDRAM address",
                    task.order
                )
                .into());
            }
            if task
                .source
                .segments
                .iter()
                .any(|&base| base >= RDRAM_BYTES as u64)
            {
                return Err(
                    format!("task {} segment base is outside 8 MiB RDRAM", task.order).into(),
                );
            }
            for span in &task.spans {
                let end = span
                    .address
                    .checked_add(span.bytes.len() as u64)
                    .filter(|&end| end <= RDRAM_BYTES as u64)
                    .ok_or("captured span is outside 8 MiB RDRAM")?;
                for (address, &byte) in (span.address as usize..end as usize).zip(&span.bytes) {
                    if occupied[address] && rdram[address] != byte {
                        return Err(format!("tasks contain conflicting bytes at {address:#x}; one RDRAM image cannot represent changing task snapshots").into());
                    }
                    occupied[address] = true;
                    rdram[address] = byte;
                }
            }
            final_color = Some(task.final_color_image()?);
            if !tasks.is_empty() {
                tasks.push_str(",\n");
            }
            let microcode = match task.microcode {
                Microcode::F3d => "f3d",
                Microcode::F3dex2 => "f3dex2",
            };
            write!(
                tasks,
                "    {{\"entry\": {}, \"microcode\": \"{}\", \"segments\": {:?}}}",
                task.entry, microcode, task.source.segments
            )?;
        }
        let color = final_color.ok_or("fixture contains no tasks")?;
        if color.fmt != 0 || !matches!(color.siz, 2 | 3) || color.width == 0 {
            return Err("final colour image must be explicitly set to RGBA16 or RGBA32".into());
        }
        if u32::from(color.width) != fixture.frame.width {
            return Err("frame width differs from the final colour image width; scaled/cropped scanout is not supported".into());
        }
        let bytes_per_pixel = 1u64 << (color.siz - 1);
        let color_end = color
            .addr
            .checked_add(u64::from(color.width) * u64::from(fixture.frame.height) * bytes_per_pixel)
            .ok_or("colour image address overflow")?;
        if color.addr % 8 != 0 || color_end > RDRAM_BYTES as u64 {
            return Err("colour image must be 8-byte aligned and fit in 8 MiB RDRAM".into());
        }
        if let Some(vi) = fixture.frame.vi {
            if u64::from(vi.origin) != color.addr || vi.width != u32::from(color.width) {
                return Err("recorded VI selects a different colour image; only the final target can be compared".into());
            }
        }
        let metadata = format!(
            "{{\n  \"version\": 1,\n  \"width\": {},\n  \"height\": {},\n  \"tasks\": [\n{}\n  ],\n  \"color_image\": {{\"address\": {}, \"width\": {}, \"format\": {}, \"size\": {}}}\n}}\n",
            fixture.frame.width, fixture.frame.height, tasks, color.addr, color.width, color.fmt, color.siz
        );
        Ok((rdram, metadata))
    }

    pub fn run() -> Result<(), Box<dyn Error>> {
        let mut args = std::env::args_os().skip(1);
        let (Some(input), Some(prefix), None) = (args.next(), args.next(), args.next()) else {
            return Err("usage: export_capture_rdram <capture.f3dcap> <output-prefix>".into());
        };
        let fixture = Fixture::from_bytes(&std::fs::read(input)?)?;
        let (rdram, metadata) = export(&fixture)?;
        let prefix = PathBuf::from(prefix);
        std::fs::write(prefix.with_extension("rdram"), rdram)?;
        std::fs::write(prefix.with_extension("json"), metadata)?;
        eprintln!(
            "exported {} task(s), {}x{}",
            fixture.tasks.len(),
            fixture.frame.width,
            fixture.frame.height
        );
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use fast3d::capture::{Frame, MemorySpan, Provenance, SourceLayout, Task};
        use fast3d::{ClearPolicy, DataFormat, RendererConfig};

        fn fixture() -> Fixture {
            Fixture {
                frame: Frame {
                    serial: 0,
                    dither_seed: 0,
                    width: 320,
                    height: 240,
                    vi: None,
                    dual_source_blending: false,
                    config: RendererConfig {
                        resolution_multiplier: 1,
                        sample_count: 1,
                        present_mode: wgpu::PresentMode::Fifo,
                        format: Some(wgpu::TextureFormat::Rgba8Unorm),
                        clear_policy: ClearPolicy::PerFrame,
                        power_preference: wgpu::PowerPreference::LowPower,
                    },
                },
                tasks: vec![Task {
                    entry: 0x100,
                    microcode: Microcode::F3d,
                    data_format: DataFormat::Fixed,
                    order: 0,
                    source: SourceLayout {
                        memory: MemoryLayout::IMAGE,
                        segments: [0; 16],
                    },
                    spans: vec![MemorySpan {
                        address: 0x100,
                        bytes: vec![
                            0xff, 0x10, 0x01, 0x3f, 0, 0x10, 0, 0, 0xb8, 0, 0, 0, 0, 0, 0, 0,
                        ],
                    }],
                }],
                provenance: Provenance::default(),
            }
        }

        #[test]
        fn export_places_big_endian_spans_and_leaves_gaps_zero() {
            let (image, _) = export(&fixture()).unwrap();
            assert_eq!(image.len(), 8 * 1024 * 1024);
            assert_eq!(&image[0x100..0x108], &[0xff, 0x10, 1, 0x3f, 0, 0x10, 0, 0]);
            assert!(image[..0x100]
                .iter()
                .chain(&image[0x110..])
                .all(|&b| b == 0));
        }

        #[test]
        fn export_rejects_host_memory_and_out_of_range_spans() {
            let mut f = fixture();
            f.tasks[0].source.memory = MemoryLayout::HOST64_LE;
            assert!(export(&f).unwrap_err().to_string().contains("HOST64"));
            f.tasks[0].source.memory = MemoryLayout::IMAGE;
            f.tasks[0].spans[0].address = 0x007f_fff8;
            assert!(export(&f).is_err());
        }

        #[test]
        fn export_rejects_conflicting_task_snapshots() {
            let mut f = fixture();
            let mut second = f.tasks[0].clone();
            second.order = 1;
            f.tasks.push(second);
            assert!(export(&f).is_ok());
            f.tasks[1].spans[0].bytes[5] = 0x20;
            assert!(export(&f).unwrap_err().to_string().contains("conflicting"));
        }

        #[test]
        fn export_rejects_missing_or_overrunning_colour_target() {
            let mut f = fixture();
            f.tasks[0].entry = 0x108;
            assert!(export(&f).is_err());
            f.tasks[0].entry = 0x100;
            f.tasks[0].spans[0].bytes[4..8].copy_from_slice(&0x007f_fff8u32.to_be_bytes());
            assert!(export(&f).is_err());
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::run()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
