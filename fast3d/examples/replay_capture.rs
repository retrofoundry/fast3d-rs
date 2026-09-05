#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::BufWriter;
    use std::path::PathBuf;

    let mut args = std::env::args_os().skip(1);
    let (Some(input), Some(prefix), None) = (args.next(), args.next(), args.next()) else {
        return Err("usage: replay_capture <capture.f3dcap> <output-prefix>".into());
    };
    let input = PathBuf::from(input);
    let prefix = PathBuf::from(prefix);
    let fixture = fast3d::capture::Fixture::from_bytes(&std::fs::read(&input)?)?;
    let output = pollster::block_on(fixture.replay_headless())?;
    if let Some(adapter) = &output.adapter_info {
        eprintln!(
            "adapter: {} ({:?}, {:?})",
            adapter.name, adapter.backend, adapter.device_type
        );
    }
    for (order, (summary, diagnostics)) in
        output.summaries.iter().zip(&output.diagnostics).enumerate()
    {
        eprintln!(
            "task {order}: {} commands, {} triangles, {} warnings, {} errors",
            summary.commands, summary.tris, summary.warns, summary.errors
        );
        for diagnostic in diagnostics {
            eprintln!("  {diagnostic}");
        }
    }
    let rgba_path = prefix.with_extension("rgba8");
    let png_path = prefix.with_extension("png");
    std::fs::write(&rgba_path, &output.rgba8)?;
    let mut encoder = png::Encoder::new(
        BufWriter::new(std::fs::File::create(&png_path)?),
        output.width,
        output.height,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&output.rgba8)?;
    writer.finish()?;
    eprintln!(
        "{}x{}: {} and {}",
        output.width,
        output.height,
        rgba_path.display(),
        png_path.display()
    );
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
