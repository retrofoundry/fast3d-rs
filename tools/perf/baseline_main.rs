use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).ok_or("expected sequence/source")?;
    let input = args.get(2).ok_or("missing input")?;
    let out = Path::new(args.get(3).ok_or("missing output directory")?);
    let configuration = args.get(4).map(String::as_str).unwrap_or("correctness");
    if !["correctness", "coarse", "counters"].contains(&configuration) {
        return Err("parent mode: correctness/coarse/counters".into());
    }
    if args
        .iter()
        .skip(5)
        .any(|arg| !["--readback", "--one-frame"].contains(&arg.as_str()))
    {
        return Err("unknown parent option".into());
    }
    let options = b1_perf::Options {
        coarse: configuration == "coarse",
        readback: configuration == "correctness" || args.iter().any(|arg| arg == "--readback"),
        frames_in_flight: if args.iter().any(|arg| arg == "--one-frame") {
            1
        } else {
            2
        },
    };
    fs::create_dir_all(out)?;
    fs::write(
        out.join("setup.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "mode":configuration,"readback":options.readback,"frames_in_flight":options.frames_in_flight,
            "timing_source":"public-api-boundaries","library_counters_available":false,
            "command_trace":true
        }))?,
    )?;
    let mut emission = b1_perf::EmissionClock::default();
    let mut writer = BufWriter::new(File::create(out.join("frames.jsonl"))?);
    let mut emit = |mut row| {
        emission.record(&mut row);
        serde_json::to_writer(&mut writer, &row).unwrap();
        writeln!(writer).unwrap();
    };
    match mode.as_str() {
        "sequence" => pollster::block_on(b1_perf::sequence_with_options(
            &fast3d::capture::Sequence::from_bytes(&b1_perf::sequence_input(input)?)?,
            options,
            &mut emit,
        ))?,
        "source" => pollster::block_on(b1_perf::source_with_options(
            &fs::read_to_string(input)?,
            options,
            &mut emit,
        ))?,
        _ => return Err("expected sequence/source".into()),
    }
    Ok(())
}
