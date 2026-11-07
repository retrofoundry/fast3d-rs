use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    error::Error,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};
use ultragfx::{
    capture::{Fixture, ReplayHardware, Sequence},
    inspect::WalkTermination,
    profiling::{CensusAttempt, CpuInterpreter, Mode, Recorder},
    Hardware,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn identity_bytes(attempt: &CensusAttempt, witness: Option<&str>) -> Result<Vec<u8>> {
    let mut sampled: Vec<(u16, Vec<u8>)> = Vec::new();
    for &address in &attempt.sampled {
        if let Some((start, bytes)) = sampled.last_mut() {
            if usize::from(*start) + bytes.len() == usize::from(address) {
                bytes.push(attempt.bank[usize::from(address)]);
                continue;
            }
        }
        sampled.push((address, vec![attempt.bank[usize::from(address)]]));
    }
    Ok(serde_json::to_vec(&json!({
        "tile": attempt.tile,
        "tlut": attempt.tlut,
        "representation": attempt.physical_representation,
        "outcome": attempt.outcome,
        "extent": attempt.extent,
        "sampled_ranges": sampled,
        "tags": attempt.framebuffer_tags.iter().filter(|(address, _, _)| {
            attempt.sampled.contains(address)
        }).collect::<Vec<_>>(),
        "witness_file": witness.map(|path| format!("../{path}")),
        "framebuffer": attempt.framebuffer,
        "framebuffer_decode": attempt.framebuffer_decode,
        "sampled_framebuffer": attempt.sampled_framebuffer,
    }))?)
}

fn blob(root: &Path, directory: &str, bytes: &[u8], extension: &str) -> Result<(String, bool)> {
    let hash = twox_hash::XxHash3_64::oneshot(bytes);
    for collision in 0u64.. {
        let suffix = if collision == 0 {
            String::new()
        } else {
            format!("-{collision}")
        };
        let relative = format!("{directory}/{hash:016x}{suffix}.{extension}");
        let path = root.join(&relative);
        match fs::read(&path) {
            Ok(previous) if previous == bytes => return Ok((relative, false)),
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::write(path, bytes)?;
                return Ok((relative, true));
            }
            Err(error) => return Err(error.into()),
        }
    }
    unreachable!()
}

fn json_file(path: impl AsRef<Path>, value: &Value) -> Result<()> {
    let mut file = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}

fn json_line(file: &mut BufWriter<File>, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn png(path: &Path, extent: [u32; 2], rgba: &[u8]) -> Result<()> {
    let expected = u64::from(extent[0]) * u64::from(extent[1]) * 4;
    if extent.contains(&0) || rgba.len() as u64 != expected {
        return Err(format!("invalid RGBA extent {extent:?}: {} bytes", rgba.len()).into());
    }
    let mut encoder = png::Encoder::new(BufWriter::new(File::create(path)?), extent[0], extent[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgba)?;
    writer.finish()?;
    Ok(())
}

fn increment(counts: &mut BTreeMap<String, u64>, key: &str) {
    *counts.entry(key.to_owned()).or_default() += 1;
}

fn writer_ranges(writers: &[(u16, u64)]) -> Vec<(u16, u16, u64)> {
    let mut ranges: Vec<(u16, u16, u64)> = Vec::new();
    for &(address, id) in writers {
        if let Some((start, count, previous)) = ranges.last_mut() {
            if *start + *count == address && *previous == id {
                *count += 1;
                continue;
            }
        }
        ranges.push((address, 1, id));
    }
    ranges
}

fn axes() -> Value {
    json!({
        "load_correctness": "unverified",
        "decode_correctness": "unverified",
        "gpu_execution": "unverified",
        "final_sampling": "unverified",
    })
}

fn run(capture: &Path, out: &Path) -> Result<Value> {
    fs::create_dir(out)?;
    for directory in [
        "banks",
        "tags",
        "sources",
        "witnesses",
        "identities",
        "textures",
    ] {
        fs::create_dir(out.join(directory))?;
    }
    let bytes = fs::read(capture)?;
    let capture_hash = format!("{:016x}", twox_hash::XxHash3_64::oneshot(&bytes));
    let (frames, warmup, presentations, kind) = match Sequence::from_bytes(&bytes) {
        Ok(sequence) => (
            sequence.frames,
            sequence.warmup_frames,
            sequence.presentations,
            "reset_prefix_sequence",
        ),
        Err(sequence_error) => match Fixture::from_bytes(&bytes) {
            Ok(fixture) => {
                let serial = fixture.frame.serial;
                (vec![fixture], 0, vec![serial], "single_frame_fixture")
            }
            Err(fixture_error) => {
                return Err(format!("sequence: {sequence_error}; fixture: {fixture_error}").into());
            }
        },
    };
    let prefix = json!({
        "capture": capture,
        "xxh3_64": capture_hash,
        "bytes": bytes.len(),
        "kind": kind,
        "state_origin": "default interpreter before first frame",
    });
    drop(bytes);
    json_file(
        out.join("input.json"),
        &json!({
            "schema": 2,
            "reset_prefix": prefix,
            "frames": frames.len(),
            "warmup_frames": warmup,
            "presentations": presentations,
            "frame_filter": null,
            "execution": "persistent CPU interpreter; no GPU submission",
            "oracle_axes": axes(),
            "oracle_note": "No independent load/decode oracle supplied. CPU decodes are not a hardware verdict.",
            "framebuffer_note": "Tagged bytes are placeholders; sampled addresses may depend on unknown framebuffer palette indices. No tagged-byte PNG is emitted.",
        }),
    )?;

    let mut attempts = BufWriter::new(File::create(out.join("attempts.jsonl"))?);
    let mut loads = BufWriter::new(File::create(out.join("loads.jsonl"))?);
    let mut tasks = BufWriter::new(File::create(out.join("tasks.jsonl"))?);
    let mut interpreter = CpuInterpreter::default();
    interpreter.recorder = Recorder::new(Mode::Census);
    let mut classes = BTreeMap::new();
    let mut roles = BTreeMap::new();
    let mut outcomes = BTreeMap::new();
    let mut distinct_classes = BTreeMap::new();
    let mut terminations = BTreeMap::new();
    let mut diagnostics = BTreeMap::new();
    let mut attempts_count = 0u64;
    let mut replacement_native = 0u64;
    let mut replacement_rt64_v5 = 0u64;
    let mut loads_count = 0u64;
    let mut distinct = 0u64;
    let mut banks = 0u64;
    let mut task_count = 0u64;
    let mut caps = 0u64;
    let mut errors = 0u64;
    let mut warnings = 0u64;
    let mut replay_errors = 0u64;
    let mut commands = 0u64;
    let mut prefix_complete = true;
    let mut first_incomplete_task = None;

    for (frame_index, fixture) in frames.iter().enumerate() {
        let frame = &fixture.frame;
        interpreter.begin_frame(frame.width, frame.height);
        for task in &fixture.tasks {
            task_count += 1;
            let mut context = json!({
                "capture": capture,
                "reset_prefix": "input.json",
                "frame_index": frame_index,
                "frame_serial": frame.serial,
                "task": task.order,
            });
            let mut hardware = ReplayHardware::new(task, frame.vi)?;
            hardware.set_command_tracing(false);
            let (summary, diags) = interpreter.process_with_bindings(
                hardware.rdram(),
                task.entry,
                task.microcode,
                task.data_format,
                &task.bindings,
            );
            let replay_error = hardware.check().err().map(|error| error.to_string());
            replay_errors += u64::from(replay_error.is_some());
            if summary.termination != WalkTermination::End || replay_error.is_some() {
                prefix_complete = false;
                first_incomplete_task.get_or_insert_with(|| {
                    json!({
                        "frame_index": frame_index,
                        "frame_serial": frame.serial,
                        "task": task.order,
                        "entry": task.entry,
                        "termination": format!("{:?}", summary.termination),
                        "replay_error": replay_error,
                    })
                });
            }
            context["prefix_complete"] = json!(prefix_complete);
            let snapshot = interpreter.recorder.drain();
            increment(&mut terminations, &format!("{:?}", summary.termination));
            caps += u64::from(matches!(
                summary.termination,
                WalkTermination::Cap | WalkTermination::Runaway
            ));
            errors += u64::from(summary.errors);
            warnings += u64::from(summary.warns);
            commands += u64::from(summary.commands);
            for diag in &diags {
                increment(&mut diagnostics, &format!("{:?}", diag.kind));
            }
            json_line(
                &mut tasks,
                &json!({
                    "context": context,
                    "entry": task.entry,
                    "microcode": format!("{:?}", task.microcode),
                    "data_format": format!("{:?}", task.data_format),
                    "bindings": task.bindings.iter().map(|(address, microcode)| json!({"address": address, "microcode": format!("{microcode:?}")})).collect::<Vec<_>>(),
                    "commands": summary.commands,
                    "tris": summary.tris,
                    "warnings": summary.warns,
                    "errors": summary.errors,
                    "dropped_runs": summary.dropped_runs,
                    "renderable": summary.renderable,
                    "termination": format!("{:?}", summary.termination),
                    "capped": matches!(summary.termination, WalkTermination::Cap | WalkTermination::Runaway),
                    "replay_error": replay_error,
                    "diagnostics": diags.iter().map(|diag| json!({"pc": diag.at, "kind": format!("{:?}", diag.kind), "message": diag.to_string()})).collect::<Vec<_>>(),
                    "attempts": snapshot.census_attempts.len(),
                    "loads": snapshot.census_loads.len(),
                }),
            )?;
            for load in snapshot.census_loads {
                let (source, _) = blob(out, "sources", &load.source, "bin")?;
                json_line(
                    &mut loads,
                    &json!({
                        "id": load.id,
                        "command": load.command,
                        "kind": load.kind,
                        "parameters": load.parameters,
                        "tile": load.tile,
                        "source_hash": load.source_hash,
                        "source_file": source,
                        "source_length": load.source.len(),
                        "context": context,
                    }),
                )?;
                loads_count += 1;
            }
            for attempt in snapshot.census_attempts {
                attempts_count += 1;
                replacement_native += u64::from(attempt.replacement_native);
                replacement_rt64_v5 += u64::from(attempt.replacement_rt64_v5);
                increment(&mut classes, &attempt.class);
                increment(&mut roles, &attempt.role);
                increment(&mut outcomes, &attempt.outcome);
                let (bank, new_bank) = blob(out, "banks", &attempt.bank, "bin")?;
                banks += u64::from(new_bank);
                let (tags, _) = blob(
                    out,
                    "tags",
                    &serde_json::to_vec(&attempt.framebuffer_tags)?,
                    "json",
                )?;
                let witness = attempt
                    .witness
                    .as_ref()
                    .map(|bytes| blob(out, "witnesses", bytes, "bin").map(|(path, _)| path))
                    .transpose()?;
                let (identity, new_identity) = blob(
                    out,
                    "identities",
                    &identity_bytes(&attempt, witness.as_deref())?,
                    "json",
                )?;
                let id = Path::new(&identity).file_stem().unwrap().to_str().unwrap();
                let descriptor = format!("textures/{id}.json");
                if new_identity {
                    distinct += 1;
                    increment(&mut distinct_classes, &attempt.class);
                }
                let mut record = json!({
                    "pc": attempt.pc,
                    "role": attempt.role,
                    "tile": attempt.tile,
                    "tlut": attempt.tlut,
                    "outcome": attempt.outcome,
                    "replacement_native": attempt.replacement_native,
                    "replacement_rt64_v5": attempt.replacement_rt64_v5,
                    "draw_rejection": attempt.draw_rejection,
                    "framebuffer": attempt.framebuffer,
                    "framebuffer_decode": attempt.framebuffer_decode,
                    "sampled_framebuffer": attempt.sampled_framebuffer,
                    "physical_representation": attempt.physical_representation,
                    "extent": attempt.extent,
                    "class": attempt.class,
                    "decode_error": attempt.decode_error,
                });
                record["context"] = context.clone();
                record["attempt"] = json!(attempts_count);
                record["bank_file"] = json!(bank);
                record["tags_file"] = json!(tags);
                record["identity_file"] = json!(identity);
                record["witness_file"] = json!(witness);
                record["sampled_count"] = json!(attempt.sampled.len());
                record["writer_ranges"] = json!(writer_ranges(&attempt.writers));
                record["descriptor"] = json!(descriptor);
                record["oracle_axes"] = axes();
                record["sampled_hash"] = json!(format!("{:016x}", attempt.sampled_hash));
                record["identity_scope"] = json!(if attempt.framebuffer
                    || attempt.outcome.starts_with("FramebufferShortcut")
                {
                    "candidate_footprint_only; framebuffer indices and texels unverified"
                } else {
                    "sampled_bytes_and_descriptor"
                });
                record["physical_decoded"] = json!(!attempt.physical.is_empty());
                json_line(&mut attempts, &record)?;
                if new_identity {
                    let physical = if attempt.physical.is_empty() {
                        None
                    } else {
                        let path = format!("{id}-physical.png");
                        png(
                            &out.join("textures").join(&path),
                            attempt.extent,
                            &attempt.physical,
                        )?;
                        Some(path)
                    };
                    record["context"]["reset_prefix"] = json!("../input.json");
                    record["bank_file"] = json!(format!("../{bank}"));
                    record["tags_file"] = json!(format!("../{tags}"));
                    record["identity_file"] = json!(format!("../{identity}"));
                    record["witness_file"] = json!(witness.map(|path| format!("../{path}")));
                    record.as_object_mut().unwrap().remove("descriptor");
                    record["physical_png"] = json!(physical);
                    record["load_references"] = json!({"file": "../loads.jsonl", "ids": attempt.writers.iter().map(|(_, id)| id).filter(|&&id| id != 0).collect::<std::collections::BTreeSet<_>>()});
                    record["attempts_file"] = json!("../attempts.jsonl");
                    json_file(out.join(descriptor), &record)?;
                }
            }
            attempts.flush()?;
            loads.flush()?;
            tasks.flush()?;
        }
    }
    let summary = json!({
        "capture": capture,
        "reset_prefix": "input.json",
        "frames": frames.len(),
        "tasks": task_count,
        "commands": commands,
        "attempts": attempts_count,
        "replacement_native": replacement_native,
        "replacement_rt64_v5": replacement_rt64_v5,
        "loads": loads_count,
        "classes": classes,
        "roles": roles,
        "outcomes": outcomes,
        "distinct_textures": distinct,
        "distinct_classes": distinct_classes,
        "distinct_banks": banks,
        "caps": caps,
        "terminations": terminations,
        "warnings": warnings,
        "errors": errors,
        "replay_errors": replay_errors,
        "prefix_complete": prefix_complete,
        "first_incomplete_task": first_incomplete_task,
        "diagnostics": diagnostics,
        "oracle_axes": axes(),
        "attempts_file": "attempts.jsonl",
        "loads_file": "loads.jsonl",
        "tasks_file": "tasks.jsonl",
    });
    json_file(out.join("summary.json"), &summary)?;
    Ok(summary)
}

fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let output = PathBuf::from(
        arguments
            .next()
            .ok_or("usage: tmem16_census OUTPUT_DIR CAPTURE...")?,
    );
    let captures: Vec<PathBuf> = arguments.map(PathBuf::from).collect();
    if captures.is_empty() {
        return Err("usage: tmem16_census OUTPUT_DIR CAPTURE...".into());
    }
    if output.join("summary.json").exists() {
        return Err(format!("output already contains summary.json: {}", output.display()).into());
    }
    fs::create_dir_all(&output)?;
    let mut summaries = Vec::new();
    let mut failed = false;
    for (index, capture) in captures.iter().enumerate() {
        let name = capture.file_stem().unwrap_or_default().to_string_lossy();
        let directory = format!("{index:03}-{name}");
        match run(capture, &output.join(&directory)) {
            Ok(mut summary) => {
                eprintln!(
                    "{}: {} attempts, {} distinct, {} capped tasks, prefix_complete={}",
                    capture.display(),
                    summary["attempts"],
                    summary["distinct_textures"],
                    summary["caps"],
                    summary["prefix_complete"]
                );
                for field in ["reset_prefix", "attempts_file", "loads_file", "tasks_file"] {
                    summary[field] =
                        json!(format!("{directory}/{}", summary[field].as_str().unwrap()));
                }
                summary["summary_file"] = json!(format!("{directory}/summary.json"));
                summaries.push(summary);
            }
            Err(error) => {
                failed = true;
                eprintln!("{}: {error}", capture.display());
                summaries.push(
                    json!({"capture": capture, "error": error.to_string(), "directory": directory}),
                );
            }
        }
        json_file(
            output.join("summary.json"),
            &json!({
                "workloads": summaries,
                "failed": failed,
                "prefix_complete": summaries.iter().all(|summary| summary["prefix_complete"] == true),
            }),
        )?;
    }
    if failed {
        return Err("one or more workloads failed; see summary.json".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt() -> CensusAttempt {
        CensusAttempt {
            pc: 0,
            role: "tex0".into(),
            tile: Default::default(),
            tlut: 0,
            outcome: "Tile".into(),
            replacement_native: true,
            replacement_rt64_v5: true,
            draw_rejection: None,
            framebuffer: false,
            framebuffer_decode: None,
            sampled_framebuffer: false,
            framebuffer_tags: vec![],
            bank: vec![0; 4096],
            sampled: vec![0, 1],
            sampled_hash: 0,
            writers: vec![],
            physical_representation: "Tile".into(),
            extent: [2, 1],
            physical: vec![0; 8],
            witness: None,
            class: "unaffected".into(),
            decode_error: None,
        }
    }

    #[test]
    fn identity_ignores_unsampled_bank_bytes_and_load_history() {
        let original = attempt();
        let mut changed = original.clone();
        changed.bank[4095] = 255;
        changed.writers.push((0, 27));
        changed.pc = 0x100;
        changed.role = "tex1".into();
        assert_eq!(
            identity_bytes(&original, None).unwrap(),
            identity_bytes(&changed, None).unwrap()
        );
        changed.bank[1] = 255;
        assert_ne!(
            identity_bytes(&original, None).unwrap(),
            identity_bytes(&changed, None).unwrap()
        );
    }

    #[test]
    fn identity_keeps_reached_tag_pointers() {
        let original = attempt();
        let mut changed = original.clone();
        changed.framebuffer_tags.push((0, 2, 4));
        assert_ne!(
            identity_bytes(&original, None).unwrap(),
            identity_bytes(&changed, None).unwrap()
        );
        changed = original.clone();
        changed.framebuffer_tags.push((4095, 2, 4));
        assert_eq!(
            identity_bytes(&original, None).unwrap(),
            identity_bytes(&changed, None).unwrap()
        );
        assert_ne!(
            identity_bytes(&original, Some("witnesses/a.bin")).unwrap(),
            identity_bytes(&original, Some("witnesses/a-1.bin")).unwrap(),
        );
    }

    #[test]
    fn bank_hash_collisions_do_not_merge_different_bytes() {
        let directory = std::env::temp_dir().join(format!("tmem16-census-{}", std::process::id()));
        fs::create_dir_all(directory.join("banks")).unwrap();
        let bytes = [1, 2, 3];
        let hash = twox_hash::XxHash3_64::oneshot(&bytes);
        fs::write(directory.join(format!("banks/{hash:016x}.bin")), [4, 5, 6]).unwrap();
        let (path, new) = blob(&directory, "banks", &bytes, "bin").unwrap();
        assert!(new);
        assert_eq!(fs::read(directory.join(&path)).unwrap(), bytes);
        assert_eq!(
            blob(&directory, "banks", &bytes, "bin").unwrap(),
            (path, false)
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn writer_ranges_preserve_holes_and_load_transitions() {
        assert_eq!(
            writer_ranges(&[(0, 0), (1, 0), (3, 0), (4, 7), (5, 7), (6, 8)]),
            vec![(0, 2, 0), (3, 1, 0), (4, 2, 7), (6, 1, 8)]
        );
    }
}
