#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::error::Error;
    use std::io::BufWriter;
    use std::path::PathBuf;

    #[derive(Debug, Default, PartialEq, Eq)]
    struct Differences {
        max_channels: [u8; 4],
        pixels: usize,
        bounds: Option<(u32, u32, u32, u32)>,
        mask: Vec<u8>,
    }

    fn compare(left: &[u8], right: &[u8], width: u32, threshold: u8) -> Differences {
        let mut result = Differences {
            mask: vec![0; left.len() / 4],
            ..Default::default()
        };
        for (index, (a, b)) in left
            .as_chunks::<4>()
            .0
            .iter()
            .zip(right.as_chunks::<4>().0.iter())
            .enumerate()
        {
            let mut over_threshold = false;
            for channel in 0..4 {
                let difference = a[channel].abs_diff(b[channel]);
                result.max_channels[channel] = result.max_channels[channel].max(difference);
                over_threshold |= difference > threshold;
            }
            if over_threshold {
                result.pixels += 1;
                result.mask[index] = 255;
                let x = (index % width as usize) as u32;
                let y = (index / width as usize) as u32;
                result.bounds = Some(match result.bounds {
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                    None => (x, y, x, y),
                });
            }
        }
        result
    }

    pub fn run() -> Result<(), Box<dyn Error>> {
        let mut args = std::env::args_os().skip(1);
        let (Some(left), Some(right), Some(width), Some(height)) =
            (args.next(), args.next(), args.next(), args.next())
        else {
            return Err("usage: compare_rgba8 <reference.rgba8> <replay.rgba8> <width> <height> [--threshold 8] [--max-diff-pixels N] [--diff-mask output.png]".into());
        };
        let width: u32 = width.to_str().ok_or("invalid width")?.parse()?;
        let height: u32 = height.to_str().ok_or("invalid height")?.parse()?;
        let expected = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|n| n.checked_mul(4))
            .filter(|&n| n != 0)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("dimensions must be positive and their RGBA8 size must fit in memory")?;
        let mut threshold = 8u8;
        let mut budget = None;
        let mut mask_path = PathBuf::from(&right).with_extension("diff.png");
        while let Some(option) = args.next() {
            let value = args.next().ok_or("option requires a value")?;
            match option.to_str() {
                Some("--threshold") => {
                    threshold = value.to_str().ok_or("invalid threshold")?.parse()?
                }
                Some("--max-diff-pixels") => {
                    budget = Some(
                        value
                            .to_str()
                            .ok_or("invalid pixel budget")?
                            .parse::<usize>()?,
                    )
                }
                Some("--diff-mask") => mask_path = PathBuf::from(value),
                _ => return Err(format!("unknown option: {}", option.to_string_lossy()).into()),
            }
        }
        let left = std::fs::read(left)?;
        let right = std::fs::read(right)?;
        if left.len() != expected || right.len() != expected {
            return Err(format!(
                "expected {expected} bytes per image for {width}x{height}; got {} and {}",
                left.len(),
                right.len()
            )
            .into());
        }
        let differences = compare(&left, &right, width, threshold);
        println!(
            "max channel diff: {} / 255 (RGBA: {:?})",
            differences.max_channels.iter().max().unwrap(),
            differences.max_channels
        );
        println!(
            "pixels over {threshold} / 255: {} / {}",
            differences.pixels,
            expected / 4
        );
        match differences.bounds {
            Some((x0, y0, x1, y1)) => {
                println!("difference bounds (inclusive): ({x0}, {y0})..({x1}, {y1})")
            }
            None => println!("difference bounds: none"),
        }
        let mut encoder = png::Encoder::new(
            BufWriter::new(std::fs::File::create(&mask_path)?),
            width,
            height,
        );
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&differences.mask)?;
        writer.finish()?;
        println!("diff mask: {}", mask_path.display());
        if budget.is_some_and(|maximum| differences.pixels > maximum) {
            return Err(format!(
                "{} differing pixels exceeds --max-diff-pixels {}",
                differences.pixels,
                budget.unwrap()
            )
            .into());
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn threshold_is_exclusive_and_counts_pixels_once_including_alpha() {
            let left = [0; 24];
            let right = [
                8, 0, 0, 0, 9, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 11, 0, 0, 0, 0, 0, 0, 255, 0,
            ];
            assert_eq!(
                compare(&left, &right, 3, 8),
                Differences {
                    max_channels: [9, 10, 255, 11],
                    pixels: 3,
                    bounds: Some((0, 0, 2, 1)),
                    mask: vec![0, 255, 0, 255, 0, 255],
                }
            );
        }

        #[test]
        fn equal_images_have_no_bounds() {
            let diff = compare(&[15; 8], &[15; 8], 2, 0);
            assert_eq!(diff.bounds, None);
            assert_eq!(diff.pixels, 0);
            assert_eq!(diff.max_channels, [0; 4]);
            assert_eq!(diff.mask, [0, 0]);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::run()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
