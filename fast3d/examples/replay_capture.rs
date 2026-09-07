#[cfg(not(target_arch = "wasm32"))]
mod native {
    use fast3d::capture::{Fixture, ReplayOutput, Sequence, SequenceOutput};
    use fast3d::DepthResetPolicy;
    use std::fmt::Write;
    use std::io::BufWriter;
    use std::path::{Path, PathBuf};

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    pub fn run() -> Result<()> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.first().is_some_and(|arg| arg == "--pack") {
            if args.len() < 5 {
                return Err("usage: replay_capture --pack <sequence.f3dcap> <warmup-count> <serials-comma-separated> <frame.f3dcap>...".into());
            }
            let sequence = Sequence {
                frames: args[4..]
                    .iter()
                    .map(|path| Ok(Fixture::from_bytes(&std::fs::read(path)?)?))
                    .collect::<Result<_>>()?,
                warmup_frames: args[2].to_str().ok_or("invalid warm-up count")?.parse()?,
                presentations: args[3]
                    .to_str()
                    .ok_or("invalid presentation serials")?
                    .split(',')
                    .map(str::parse)
                    .collect::<std::result::Result<_, _>>()?,
            };
            std::fs::write(&args[1], sequence.to_bytes()?)?;
            return Ok(());
        }
        if !(2..=3).contains(&args.len()) {
            return Err("usage: replay_capture <capture.f3dcap> <output-prefix> [persist|cimg|task|frame|all]".into());
        }
        let bytes = std::fs::read(&args[0])?;
        let prefix = PathBuf::from(&args[1]);
        if bytes.get(8..12) != Some(&2u32.to_le_bytes()) {
            if args.len() != 2 {
                return Err("depth controls require a reset-rooted sequence".into());
            }
            let fixture = Fixture::from_bytes(&bytes)?;
            let output = pollster::block_on(fixture.replay_headless())?;
            write_image(&prefix, &output)?;
            std::fs::write(named(&prefix, ".log"), format!("adapter: {:?}\nframe: {:?}\nprovenance: {:?}\nsummaries: {:?}\ndiagnostics: {:?}\ncommands: {:#?}\n", output.adapter_info, fixture.frame, fixture.provenance, output.summaries, output.diagnostics, output.commands))?;
            return Ok(());
        }
        let sequence = Sequence::from_bytes(&bytes)?;
        let mode = args
            .get(2)
            .map_or(Some("persist"), |arg| arg.to_str())
            .ok_or("invalid depth control")?;
        let controls = [
            ("persist", DepthResetPolicy::Never),
            ("cimg", DepthResetPolicy::ColorImageSwitch),
            ("task", DepthResetPolicy::TaskBoundary),
            ("frame", DepthResetPolicy::FrameBoundary),
        ];
        if mode != "all" && !controls.iter().any(|(name, _)| *name == mode) {
            return Err("unknown depth control; expected persist, cimg, task, frame or all".into());
        }
        let adapter = pollster::block_on(wgpu::Instance::default().request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: sequence.frames[0].frame.config.power_preference,
                ..Default::default()
            },
        ))?;
        let adapter_info = adapter.get_info();
        let features = if sequence.frames[0].frame.dual_source_blending {
            wgpu::Features::DUAL_SOURCE_BLENDING
        } else {
            wgpu::Features::empty()
        };
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_features: features,
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            }))?;
        let mut baseline = None;
        for (name, policy) in controls {
            if mode != "all" && mode != name {
                continue;
            }
            let mut output =
                pollster::block_on(sequence.replay(device.clone(), queue.clone(), policy))?;
            output.adapter_info = Some(adapter_info.clone());
            write_sequence(&prefix, name, &sequence, &output)?;
            if let Some(baseline) = &baseline {
                write_diff(&prefix, name, baseline, &output)?;
            }
            if mode == "all" && policy == DepthResetPolicy::Never {
                baseline = Some(output);
            }
        }
        Ok(())
    }

    fn named(prefix: &Path, suffix: &str) -> PathBuf {
        let mut path = prefix.as_os_str().to_owned();
        path.push(suffix);
        PathBuf::from(path)
    }

    fn png(path: &Path, width: u32, height: u32, rgba8: &[u8]) -> Result<()> {
        let mut encoder =
            png::Encoder::new(BufWriter::new(std::fs::File::create(path)?), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba8)?;
        writer.finish()?;
        Ok(())
    }

    fn write_image(prefix: &Path, output: &ReplayOutput) -> Result<()> {
        std::fs::write(named(prefix, ".rgba8"), &output.rgba8)?;
        png(
            &named(prefix, ".png"),
            output.width,
            output.height,
            &output.rgba8,
        )?;
        eprintln!("{}: {}x{}", prefix.display(), output.width, output.height);
        Ok(())
    }

    fn hash(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
    }

    fn write_sequence(
        prefix: &Path,
        name: &str,
        sequence: &Sequence,
        output: &SequenceOutput,
    ) -> Result<()> {
        let mut log = format!(
            "depth reset: {name}\nadapter: {:?}\nwarmup frames: {}\nselected serials: {:?}\n",
            output.adapter_info, sequence.warmup_frames, sequence.presentations
        );
        for (fixture, frame) in sequence.frames.iter().zip(&output.frames) {
            writeln!(
                log,
                "frame {}: {:?}\nprovenance: {:?}",
                frame.serial, fixture.frame, fixture.provenance
            )?;
            for (task, (summary, diagnostics)) in fixture
                .tasks
                .iter()
                .zip(frame.summaries.iter().zip(&frame.diagnostics))
            {
                writeln!(
                    log,
                    "task {} entry {:#x} {:?} {:?} {:?}: {summary:?}",
                    task.order, task.entry, task.microcode, task.data_format, task.source
                )?;
                for diagnostic in diagnostics {
                    writeln!(log, "  {diagnostic:?}")?;
                }
            }
            for event in &frame.commands {
                let command = event.command;
                let kind = match command.w0 >> 24 {
                    0xff => "CIMG",
                    0xfe => "ZIMG",
                    0xf6 => "FILLRECT",
                    0xf7 => "FILLCOLOR",
                    0xed => "SCISSOR",
                    _ => unreachable!(),
                };
                writeln!(
                    log,
                    "serial {} task {} PC {:#018x} {kind} {:08x} {:016x}",
                    frame.serial, event.task, command.pc, command.w0, command.w1
                )?;
            }
        }
        for presentation in &output.presentations {
            let image_prefix = named(prefix, &format!("-{name}-{:06}", presentation.serial));
            write_image(&image_prefix, &presentation.output)?;
            writeln!(
                log,
                "serial {} RGBA8 fnv1a64 {:016x}",
                presentation.serial,
                hash(&presentation.output.rgba8)
            )?;
        }
        std::fs::write(named(prefix, &format!("-{name}.log")), log)?;
        Ok(())
    }

    fn write_diff(
        prefix: &Path,
        name: &str,
        baseline: &SequenceOutput,
        variant: &SequenceOutput,
    ) -> Result<()> {
        let mut log = String::new();
        for (before, after) in baseline.presentations.iter().zip(&variant.presentations) {
            let width = before.output.width;
            let mut count = 0;
            let mut bounds = [width, before.output.height, 0, 0];
            let mut mask = vec![0; before.output.rgba8.len()];
            for (i, ((before, after), mask)) in before
                .output
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .zip(after.output.rgba8.as_chunks::<4>().0)
                .zip(mask.as_chunks_mut::<4>().0.iter_mut())
                .enumerate()
            {
                if before != after {
                    count += 1;
                    let x = i as u32 % width;
                    let y = i as u32 / width;
                    bounds = [
                        bounds[0].min(x),
                        bounds[1].min(y),
                        bounds[2].max(x),
                        bounds[3].max(y),
                    ];
                    *mask = [255, 255, 255, 255];
                }
            }
            writeln!(log, "serial {} changed_pixels {count} inclusive_bounds {:?} persist_fnv1a64 {:016x} {name}_fnv1a64 {:016x}", before.serial, (count != 0).then_some(bounds), hash(&before.output.rgba8), hash(&after.output.rgba8))?;
            png(
                &named(prefix, &format!("-{name}-{:06}-mask.png", before.serial)),
                width,
                before.output.height,
                &mask,
            )?;
        }
        std::fs::write(named(prefix, &format!("-{name}-diff.log")), log)?;
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::run()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
