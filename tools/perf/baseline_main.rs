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
    fs::create_dir_all(out)?;
    let mut writer = BufWriter::new(File::create(out.join("frames.jsonl"))?);
    let mut emit = |row| {
        serde_json::to_writer(&mut writer, &row).unwrap();
        writeln!(writer).unwrap();
    };
    match mode.as_str() {
        "sequence" => pollster::block_on(b1_perf::sequence(
            &fast3d::capture::Sequence::from_bytes(&fs::read(input)?)?,
            &mut emit,
        ))?,
        "source" => pollster::block_on(b1_perf::source(&fs::read_to_string(input)?, &mut emit))?,
        _ => return Err("expected sequence/source".into()),
    }
    Ok(())
}
