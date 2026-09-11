use b1_perf::{cache, *};
use fast3d::{
    capture::{MeasurementOptions, ReplayHardware, Sequence},
    profiling::{CpuInterpreter, Mode, Recorder},
    Hardware,
};
use serde_json::json;
use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
};

fn write(writer: &mut impl Write, value: &impl serde::Serialize) {
    serde_json::to_writer(&mut *writer, value).unwrap();
    writeln!(writer).unwrap();
}
fn write_timed(
    clock: &std::cell::Cell<f64>,
    writer: &mut impl Write,
    value: &impl serde::Serialize,
) {
    let start = fast3d::profiling::now_ms();
    write(writer, value);
    clock.set(clock.get() + fast3d::profiling::now_ms() - start);
}
fn file(path: impl AsRef<Path>) -> BufWriter<File> {
    BufWriter::new(File::create(path).unwrap())
}
fn mode(value: &str) -> Mode {
    match value {
        "counters" => Mode::Counters,
        "coarse" => Mode::Coarse,
        "detailed" => Mode::Detailed,
        "trace" => Mode::Trace,
        _ => panic!("mode: counters/coarse/detailed/trace"),
    }
}

fn admission(sequence: &Sequence) -> serde_json::Value {
    let mut frames = std::collections::BTreeMap::<String, u64>::new();
    let mut tasks = std::collections::BTreeMap::<String, u64>::new();
    for fixture in &sequence.frames {
        let mut frame = fixture.frame.clone();
        frame.serial = 0;
        *frames.entry(format!("{frame:?}")).or_default() += 1;
        for task in &fixture.tasks {
            *tasks
                .entry(format!(
                    "{:?}/{:?}/{:?}",
                    task.microcode, task.data_format, task.source.memory
                ))
                .or_default() += 1;
        }
    }
    json!({"frame_configurations":frames,"task_declarations":tasks,"frames_checked":sequence.frames.len(),"reset_prefix_start":sequence.frames[0].frame.serial})
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let driver_start = fast3d::profiling::now_ms();
    let formatting_ms = std::cell::Cell::new(0.0);
    let args: Vec<_> = std::env::args().collect();
    let command = args
        .get(1)
        .ok_or("expected inspect, sequence, source-cpu, source, or preflight")?;
    let input = args
        .get(2)
        .ok_or("missing input path (use authored for preflight)")?;
    let out = Path::new(args.get(3).ok_or("missing output directory")?);
    fs::create_dir_all(out)?;
    if command == "metadata" {
        let sequence = Sequence::from_bytes(&fs::read(input)?)?;
        fs::write(
            out.join("admission.json"),
            serde_json::to_vec_pretty(&admission(&sequence))?,
        )?;
        return Ok(());
    }
    let mut frames = file(out.join("frames.jsonl"));
    let mut emission = EmissionClock::default();
    let mut traces = file(out.join("requests.jsonl"));
    let mut hits = file(out.join("hits.jsonl"));
    let cache_budget = std::env::var("B1_CACHE_BUDGET")
        .ok()
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(16 * 1024 * 1024);
    let mut collector = TraceCollector::new(cache_budget);
    let mut trace = |serial, observed, snapshot: &fast3d::profiling::Snapshot| {
        write_timed(
            &formatting_ms,
            &mut hits,
            &collector.frame(serial, observed, snapshot),
        );
        for request in &snapshot.requests {
            write_timed(
                &formatting_ms,
                &mut traces,
                &json!({"serial":serial,"observed":observed,"request":request}),
            );
        }
    };
    match command.as_str() {
        "inspect" | "sequence" | "ordinary" => {
            let start = fast3d::profiling::now_ms();
            let bytes = sequence_input(input)?;
            let sequence = Sequence::from_bytes(&bytes)?;
            let input_ms = fast3d::profiling::now_ms() - start;
            let metadata = json!({"capture":input,"sha256":sha(&bytes),"frames":sequence.frames.len(),"warmup":sequence.warmup_frames,"presentations":sequence.presentations,"config":format!("{:?}",sequence.frames[0].frame),"provenance":format!("{:?}",sequence.frames[0].provenance),"input_loading_validation_ms":input_ms,"clock":fast3d::profiling::clock_probe(),"execution":command,"all_frame_admission":admission(&sequence)});
            fs::write(
                out.join("input.json"),
                serde_json::to_vec_pretty(&metadata)?,
            )?;
            if command == "inspect" {
                let mut interpreter = CpuInterpreter::default();
                interpreter.recorder = Recorder::new(Mode::Trace);
                for fixture in &sequence.frames {
                    let mut summaries = Vec::new();
                    let mut diagnostics = Vec::new();
                    for task in &fixture.tasks {
                        let mut hardware = ReplayHardware::new(task, fixture.frame.vi)?;
                        hardware.set_command_tracing(false);
                        let (summary, diags) = interpreter.process(
                            hardware.rdram(),
                            task.entry,
                            task.microcode,
                            task.data_format,
                        );
                        hardware.check()?;
                        summaries.push(format!("{summary:?}"));
                        diagnostics.push(format!("{diags:?}"));
                    }
                    let profile = interpreter.recorder.drain();
                    let observed = fixture.frame.serial > u64::from(sequence.warmup_frames);
                    trace(fixture.frame.serial, observed, &profile);
                    write_timed(
                        &formatting_ms,
                        &mut frames,
                        &json!({"serial":fixture.frame.serial,"observed":observed,"execution":"cpu-diagnostic-no-render","tasks":fixture.tasks.len(),"summaries":summaries,"diagnostics":diagnostics,"profile":{"counters":profile.counters,"decodes":profile.decodes}}),
                    );
                }
            } else {
                let start = fast3d::profiling::now_ms();
                let (device, queue, info) = pollster::block_on(sequence.measurement_device())?;
                let device_ms = fast3d::profiling::now_ms() - start;
                let features = format!("{:?}", device.features());
                let limits = format!("{:?}", device.limits());
                if command == "ordinary" {
                    pollster::block_on(sequence.replay_streamed(
                        device,
                        queue,
                        |serial, output| {
                            write_timed(
                                &formatting_ms,
                                &mut frames,
                                &output_record(serial, &output),
                            )
                        },
                    ))?;
                } else {
                    let options = MeasurementOptions {
                        mode: mode(args.get(4).map_or("coarse", String::as_str)),
                        readback: args.iter().any(|a| a == "--readback"),
                        frames_in_flight: if args.iter().any(|a| a == "--one-frame") {
                            1
                        } else {
                            2
                        },
                        command_trace: args.iter().any(|a| a == "--parent-compatible"),
                    };
                    let setup = pollster::block_on(sequence.measure(
                        device,
                        queue,
                        options,
                        |mut frame| {
                            if options.mode == Mode::Trace {
                                trace(frame.serial, frame.observed, &frame.profile);
                            }
                            frame.profile.requests.clear();
                            let mut row = frame_record(&frame);
                            row["timing_source"] = json!("library-recorder");
                            emission.record(&mut row);
                            write_timed(&formatting_ms, &mut frames, &row);
                        },
                    ))?;
                    fs::write(
                        out.join("setup.json"),
                        serde_json::to_vec_pretty(
                            &json!({"setup":setup,"device_ms":device_ms,"adapter":format!("{info:?}"),"features":features,"limits":limits,"mode":options.mode,"readback":options.readback,"frames_in_flight":options.frames_in_flight,"command_trace":options.command_trace}),
                        )?,
                    )?;
                }
            }
        }
        "source-cpu" | "source" => {
            let source = fs::read_to_string(input)?;
            let metadata = source_metadata(&source)?;
            fs::write(
                out.join("input.json"),
                serde_json::to_vec_pretty(&metadata)?,
            )?;
            let rgba = synthetic_texture();
            fs::write(out.join("env.rgba8"), &rgba)?;
            let image = assemble(&source, 0, &rgba)?;
            fs::write(
                out.join("env.rgba16"),
                &image.rdram[image.tex_addr as usize..image.tex_addr as usize + 2048],
            )?;
            if command == "source-cpu" {
                source_cpu(
                    &source,
                    |v| {
                        let mut v = v;
                        v["profile"]["requests"] = json!([]);
                        write_timed(&formatting_ms, &mut frames, &v);
                    },
                    &mut trace,
                )?;
            } else {
                let setup = pollster::block_on(source_gpu(
                    &source,
                    mode(args.get(4).map_or("coarse", String::as_str)),
                    args.iter().any(|a| a == "--readback"),
                    if args.iter().any(|a| a == "--one-frame") {
                        1
                    } else {
                        2
                    },
                    |mut v| {
                        v["timing_source"] = json!("library-recorder");
                        emission.record(&mut v);
                        write_timed(&formatting_ms, &mut frames, &v);
                    },
                ))?;
                fs::write(out.join("setup.json"), serde_json::to_vec_pretty(&setup)?)?;
            }
        }
        "preflight" => {
            let cases: Vec<fast3d::profiling::Request> = if input == "authored" {
                fast3d::profiling::authored_requests()
                    .into_iter()
                    .flat_map(|r| (0..16).map(move |xor| r.diagnostic_variant(xor)))
                    .collect()
            } else {
                serde_json::from_slice(&fs::read(input)?)?
            };
            fs::write(
                out.join("hash-inputs.json"),
                serde_json::to_vec_pretty(&cache::verify_hash_inputs(&cases))?,
            )?;
            let mut classes = std::collections::BTreeMap::<String, Vec<_>>::new();
            for case in cases {
                if case.rejection.is_none() {
                    classes.entry(case.class()).or_default().push(case);
                }
            }
            let mut report = file(out.join("costs.jsonl"));
            for (index, cases) in classes.values().enumerate() {
                for (set, inputs) in [
                    ("hot-distinct-equal", &cases[..1]),
                    ("rotating", &cases[..]),
                ] {
                    for step in 0..cache::HashAlgorithm::ALL.len() {
                        let algorithm = cache::HashAlgorithm::ALL[(index + step) % 2];
                        write_timed(
                            &formatting_ms,
                            &mut report,
                            &cache::benchmark(inputs, set, algorithm, 1.0, 5),
                        );
                    }
                }
                report.flush()?;
            }
        }
        _ => return Err("unknown command".into()),
    }
    let cases: Vec<_> = collector.cases.into_values().flatten().collect();
    fs::write(out.join("cases.json"), serde_json::to_vec(&cases)?)?;
    frames.flush()?;
    traces.flush()?;
    hits.flush()?;
    fs::write(
        out.join("driver.json"),
        serde_json::to_vec_pretty(
            &json!({"total_elapsed_ms":fast3d::profiling::now_ms()-driver_start,"serialization_write_ms":formatting_ms.get(),"cache_budget_bytes":cache_budget,"request_sets":"all distinct canonical inputs by class; complete ordered trace retained"}),
        )?,
    )?;
    Ok(())
}
