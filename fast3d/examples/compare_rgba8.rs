#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::error::Error;
    use std::ffi::OsStr;
    use std::io::BufWriter;
    use std::path::PathBuf;
    use std::str::FromStr;

    #[derive(Clone, Copy)]
    struct DitherRegion {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    }

    impl DitherRegion {
        fn parse(
            value: &OsStr,
            image_width: u32,
            image_height: u32,
        ) -> Result<Self, Box<dyn Error>> {
            let [x, y, width, height] = parse_quad(value, "dither region")?;
            if width == 0
                || height == 0
                || u64::from(x) + u64::from(width) > u64::from(image_width)
                || u64::from(y) + u64::from(height) > u64::from(image_height)
            {
                return Err("dither region must be nonempty and inside the image".into());
            }
            Ok(Self {
                x,
                y,
                width,
                height,
            })
        }

        fn contains(self, x: u32, y: u32) -> bool {
            x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
        }

        fn pixels(self) -> usize {
            self.width as usize * self.height as usize
        }
    }

    fn parse_quad<T: FromStr>(value: &OsStr, label: &str) -> Result<[T; 4], Box<dyn Error>> {
        value
            .to_str()
            .and_then(|text| {
                text.split(',')
                    .map(str::parse)
                    .collect::<Result<Vec<_>, _>>()
                    .ok()
            })
            .and_then(|values| values.try_into().ok())
            .ok_or_else(|| format!("invalid {label}: expected four comma-separated values").into())
    }

    struct DitherStats {
        survivors: usize,
        max_row_discards: u32,
        max_column_discards: u32,
    }

    fn dither_stats(
        image: &[u8],
        width: u32,
        region: DitherRegion,
        background: [u8; 4],
        ignore_alpha: bool,
    ) -> DitherStats {
        let mut stats = DitherStats {
            survivors: 0,
            max_row_discards: 0,
            max_column_discards: 0,
        };
        let mut column_discards = vec![0u32; region.width as usize];
        for y in region.y..region.y + region.height {
            let mut row_discards = 0;
            for (column, x) in (region.x..region.x + region.width).enumerate() {
                let offset = (y as usize * width as usize + x as usize) * 4;
                let channels = if ignore_alpha { 3 } else { 4 };
                if image[offset..offset + channels] == background[..channels] {
                    row_discards += 1;
                    column_discards[column] += 1;
                } else {
                    stats.survivors += 1;
                }
            }
            stats.max_row_discards = stats.max_row_discards.max(row_discards);
        }
        stats.max_column_discards = column_discards.into_iter().max().unwrap();
        stats
    }

    fn check_dither(
        left: &[u8],
        right: &[u8],
        width: u32,
        region: DitherRegion,
        background: [u8; 4],
        ignore_alpha: bool,
    ) -> Result<(), Box<dyn Error>> {
        let reference = dither_stats(left, width, region, background, ignore_alpha);
        let replay = dither_stats(right, width, region, background, ignore_alpha);
        let pixels = region.pixels();
        let channels = if ignore_alpha { 3 } else { 4 };
        println!(
            "dither region (x,y,width,height): {},{},{},{}; discarded {}: {:?}",
            region.x,
            region.y,
            region.width,
            region.height,
            if ignore_alpha { "RGB" } else { "RGBA" },
            &background[..channels]
        );
        let mut failures = Vec::new();
        for (name, stats) in [("reference", &reference), ("replay", &replay)] {
            println!(
                "{name} dither survivors: {} / {pixels} ({:.6}); max discarded row: {:.6}, column: {:.6}",
                stats.survivors,
                stats.survivors as f64 / pixels as f64,
                f64::from(stats.max_row_discards) / f64::from(region.width),
                f64::from(stats.max_column_discards) / f64::from(region.height)
            );
            if u64::from(stats.max_row_discards) * 5 > u64::from(region.width) * 4 {
                failures.push(format!("{name} dither row exceeds 80% discarded pixels"));
            }
            if u64::from(stats.max_column_discards) * 5 > u64::from(region.height) * 4 {
                failures.push(format!("{name} dither column exceeds 80% discarded pixels"));
            }
        }
        let survivor_difference = reference.survivors.abs_diff(replay.survivors);
        println!(
            "dither survivor fraction difference: {:.6} (maximum 0.05)",
            survivor_difference as f64 / pixels as f64
        );
        if survivor_difference as u128 * 20 > pixels as u128 {
            failures.push("dither survivor fractions differ by more than 0.05".to_owned());
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; ").into())
        }
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct Differences {
        max_channels: [u8; 4],
        pixels: usize,
        bounds: Option<(u32, u32, u32, u32)>,
        mask: Vec<u8>,
    }

    fn compare(
        left: &[u8],
        right: &[u8],
        width: u32,
        threshold: u8,
        dither_region: Option<DitherRegion>,
        ignore_alpha: bool,
    ) -> Differences {
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
            let x = (index % width as usize) as u32;
            let y = (index / width as usize) as u32;
            if dither_region.is_some_and(|region| region.contains(x, y)) {
                continue;
            }
            let mut over_threshold = false;
            for channel in 0..if ignore_alpha { 3 } else { 4 } {
                let difference = a[channel].abs_diff(b[channel]);
                result.max_channels[channel] = result.max_channels[channel].max(difference);
                over_threshold |= difference > threshold;
            }
            if over_threshold {
                result.pixels += 1;
                result.mask[index] = 255;
                result.bounds = Some(match result.bounds {
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                    None => (x, y, x, y),
                });
            }
        }
        result
    }

    pub fn run() -> Result<(), Box<dyn Error>> {
        run_with_args(std::env::args_os().skip(1))
    }

    fn run_with_args(
        mut args: impl Iterator<Item = std::ffi::OsString>,
    ) -> Result<(), Box<dyn Error>> {
        let (Some(left), Some(right), Some(width), Some(height)) =
            (args.next(), args.next(), args.next(), args.next())
        else {
            return Err("usage: compare_rgba8 <reference.rgba8> <replay.rgba8> <width> <height> [--threshold 8] [--ignore-alpha] [--max-diff-pixels N] [--diff-mask output.png] [--dither-region x,y,width,height] [--dither-background r,g,b,a]\nDither regions use statistical comparison; exact matches to the background RGBA (default 0,0,0,255) are discarded samples. Select a region covered by the dithered primitive whose surviving color differs from the background. Survivor fractions must differ by at most 0.05, and no row or column in either image may be more than 80% discarded. The threshold, pixel budget, and diff mask apply only outside the region.".into());
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
        let mut ignore_alpha = false;
        let mut budget = None;
        let mut dither_region = None;
        let mut dither_background = None;
        let mut mask_path = PathBuf::from(&right).with_extension("diff.png");
        while let Some(option) = args.next() {
            if option == "--ignore-alpha" {
                ignore_alpha = true;
                continue;
            }
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
                Some("--dither-region") => {
                    dither_region = Some(DitherRegion::parse(&value, width, height)?);
                }
                Some("--dither-background") => {
                    dither_background = Some(parse_quad(&value, "dither background")?);
                }
                _ => return Err(format!("unknown option: {}", option.to_string_lossy()).into()),
            }
        }
        if dither_background.is_some() && dither_region.is_none() {
            return Err("--dither-background requires --dither-region".into());
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
        let differences = compare(&left, &right, width, threshold, dither_region, ignore_alpha);
        let channels = if ignore_alpha { 3 } else { 4 };
        let maxima = &differences.max_channels[..channels];
        println!(
            "max channel diff: {} / 255 ({}: {:?})",
            maxima.iter().max().unwrap(),
            if ignore_alpha { "RGB" } else { "RGBA" },
            maxima
        );
        println!(
            "pixels over {threshold} / 255: {} / {}",
            differences.pixels,
            expected / 4 - dither_region.map_or(0, DitherRegion::pixels)
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
        if let Some(region) = dither_region {
            check_dither(
                &left,
                &right,
                width,
                region,
                dither_background.unwrap_or([0, 0, 0, 255]),
                ignore_alpha,
            )?;
        }
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
        use std::ffi::OsString;
        use std::sync::atomic::{AtomicUsize, Ordering};

        const BLACK: [u8; 4] = [0, 0, 0, 255];
        const RED: [u8; 4] = [255, 0, 0, 255];

        struct Fixture {
            directory: PathBuf,
            width: u32,
            height: u32,
        }

        impl Fixture {
            fn new(width: u32, height: u32, left: &[[u8; 4]], right: &[[u8; 4]]) -> Self {
                static NEXT: AtomicUsize = AtomicUsize::new(0);
                let directory = std::env::temp_dir().join(format!(
                    "fast3d-compare-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                std::fs::create_dir(&directory).unwrap();
                std::fs::write(directory.join("left.rgba8"), left.as_flattened()).unwrap();
                std::fs::write(directory.join("right.rgba8"), right.as_flattened()).unwrap();
                Self {
                    directory,
                    width,
                    height,
                }
            }

            fn run(&self, options: &[&str]) -> Result<(), Box<dyn Error>> {
                let mut args = vec![
                    self.directory.join("left.rgba8").into_os_string(),
                    self.directory.join("right.rgba8").into_os_string(),
                    OsString::from(self.width.to_string()),
                    OsString::from(self.height.to_string()),
                    OsString::from("--diff-mask"),
                    self.directory.join("diff.png").into_os_string(),
                ];
                args.extend(options.iter().map(OsString::from));
                run_with_args(args.into_iter())
            }
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.directory).unwrap();
            }
        }

        fn checkerboard(width: usize, height: usize) -> Vec<[u8; 4]> {
            (0..width * height)
                .map(|i| {
                    if (i % width + i / width) % 2 == 0 {
                        RED
                    } else {
                        BLACK
                    }
                })
                .collect()
        }

        #[test]
        fn dither_accepts_different_patterns_with_equal_survivor_rates() {
            let left = checkerboard(10, 10);
            let right: Vec<_> = left
                .iter()
                .map(|&p| if p == RED { BLACK } else { RED })
                .collect();
            let fixture = Fixture::new(10, 10, &left, &right);
            fixture
                .run(&["--dither-region", "0,0,10,10", "--max-diff-pixels", "0"])
                .unwrap();
            assert!(fixture.run(&["--max-diff-pixels", "0"]).is_err());
        }

        #[test]
        fn dither_survivor_tolerance_includes_five_percentage_points() {
            let left = checkerboard(10, 10);
            let mut right = left.clone();
            for index in [1, 3, 5, 7, 9] {
                right[index] = RED;
            }
            Fixture::new(10, 10, &left, &right)
                .run(&["--dither-region", "0,0,10,10"])
                .unwrap();
            right[10] = RED;
            let error = Fixture::new(10, 10, &left, &right)
                .run(&["--dither-region", "0,0,10,10"])
                .unwrap_err();
            assert!(error.to_string().contains("survivor"), "{error}");
        }

        #[test]
        fn dither_checks_discard_dispersion_in_both_images() {
            let dispersed = checkerboard(10, 10);
            for columns in [false, true] {
                let striped: Vec<_> = (0..100)
                    .map(|i| {
                        if (if columns { i % 10 } else { i / 10 }) < 5 {
                            BLACK
                        } else {
                            RED
                        }
                    })
                    .collect();
                for (left, right) in [(&dispersed, &striped), (&striped, &dispersed)] {
                    let error = Fixture::new(10, 10, left, right)
                        .run(&["--dither-region", "0,0,10,10"])
                        .unwrap_err();
                    let axis = if columns { "column" } else { "row" };
                    assert!(error.to_string().contains(axis), "{error}");
                }
            }
        }

        #[test]
        fn dither_dispersion_allows_exactly_eighty_percent_discarded() {
            let pixels: Vec<_> = (0..25)
                .map(|i| if i % 5 == i / 5 { RED } else { BLACK })
                .collect();
            Fixture::new(5, 5, &pixels, &pixels)
                .run(&["--dither-region", "0,0,5,5"])
                .unwrap();
        }

        #[test]
        fn dither_region_preserves_threshold_and_budget_outside_it() {
            let left = checkerboard(4, 4);
            let mut right = left.clone();
            for index in [5, 6, 9, 10] {
                right[index] = if left[index] == RED { BLACK } else { RED };
            }
            right[0][3] = 246;
            right[15][0] = 247;
            let fixture = Fixture::new(4, 4, &left, &right);
            fixture
                .run(&[
                    "--dither-region",
                    "1,1,2,2",
                    "--threshold",
                    "8",
                    "--max-diff-pixels",
                    "1",
                ])
                .unwrap();
            let error = fixture
                .run(&[
                    "--dither-region",
                    "1,1,2,2",
                    "--threshold",
                    "8",
                    "--max-diff-pixels",
                    "0",
                ])
                .unwrap_err();
            assert!(error.to_string().contains("1 differing pixels"), "{error}");
        }

        #[test]
        fn dither_background_uses_all_rgba_channels() {
            let left: Vec<_> = checkerboard(10, 10)
                .iter()
                .map(|&p| if p == BLACK { [10, 20, 30, 255] } else { RED })
                .collect();
            let mut right = left.clone();
            for index in [1, 3, 5, 7, 9, 10] {
                right[index][3] = 254;
            }
            let fixture = Fixture::new(10, 10, &left, &right);
            fixture.run(&["--dither-region", "0,0,10,10"]).unwrap();
            let error = fixture
                .run(&[
                    "--dither-region",
                    "0,0,10,10",
                    "--dither-background",
                    "10,20,30,255",
                ])
                .unwrap_err();
            assert!(error.to_string().contains("survivor"), "{error}");
        }

        #[test]
        fn dither_region_rejects_empty_out_of_bounds_and_malformed_rectangles() {
            let pixels = checkerboard(4, 4);
            let fixture = Fixture::new(4, 4, &pixels, &pixels);
            for region in [
                "0,0,0,1",
                "0,0,1,0",
                "4,0,1,1",
                "0,4,1,1",
                "1,1,4,4",
                "4294967295,0,2,1",
                "0,0,4",
                "0,0,4,4,1",
                "-1,0,1,1",
            ] {
                let error = fixture.run(&["--dither-region", region]).unwrap_err();
                assert!(
                    error.to_string().contains("dither region"),
                    "{region}: {error}"
                );
            }
            fixture.run(&["--dither-region", "0,0,4,4"]).unwrap();
        }

        #[test]
        fn dither_background_rejects_invalid_channels_or_missing_region() {
            let pixels = checkerboard(4, 4);
            let fixture = Fixture::new(4, 4, &pixels, &pixels);
            for background in ["0,0,0", "0,0,0,255,0", "0,0,0,256", "-1,0,0,255"] {
                let error = fixture
                    .run(&[
                        "--dither-region",
                        "0,0,4,4",
                        "--dither-background",
                        background,
                    ])
                    .unwrap_err();
                assert!(error.to_string().contains("dither background"), "{error}");
            }
            assert!(fixture.run(&["--dither-background", "0,0,0,255"]).is_err());
        }

        #[test]
        fn threshold_is_exclusive_and_counts_pixels_once_including_alpha() {
            let left = [0; 24];
            let right = [
                8, 0, 0, 0, 9, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 11, 0, 0, 0, 0, 0, 0, 255, 0,
            ];
            assert_eq!(
                compare(&left, &right, 3, 8, None, false),
                Differences {
                    max_channels: [9, 10, 255, 11],
                    pixels: 3,
                    bounds: Some((0, 0, 2, 1)),
                    mask: vec![0, 255, 0, 255, 0, 255],
                }
            );
        }

        #[test]
        fn ignore_alpha_excludes_alpha_from_maxima_budget_bounds_and_mask() {
            let left = [[0; 4]; 3];
            let right = [[0, 0, 0, 255], [9, 0, 0, 0], [0, 0, 8, 255]];
            assert_eq!(
                compare(left.as_flattened(), right.as_flattened(), 3, 8, None, true),
                Differences {
                    max_channels: [9, 0, 8, 0],
                    pixels: 1,
                    bounds: Some((1, 0, 1, 0)),
                    mask: vec![0, 255, 0],
                }
            );
            let fixture = Fixture::new(3, 1, &left, &right);
            fixture
                .run(&["--max-diff-pixels", "1", "--ignore-alpha"])
                .unwrap();
            assert!(fixture
                .run(&["--ignore-alpha", "--max-diff-pixels", "0"])
                .is_err());
            assert!(fixture.run(&["--max-diff-pixels", "1"]).is_err());
        }

        #[test]
        fn ignore_alpha_matches_dither_background_by_rgb() {
            let left = checkerboard(10, 10);
            let right: Vec<_> = left.iter().map(|p| [p[0], p[1], p[2], 128]).collect();
            let fixture = Fixture::new(10, 10, &left, &right);
            fixture
                .run(&["--ignore-alpha", "--dither-region", "0,0,10,10"])
                .unwrap();
            assert!(fixture.run(&["--dither-region", "0,0,10,10"]).is_err());
        }

        #[test]
        fn equal_images_have_no_bounds() {
            let diff = compare(&[15; 8], &[15; 8], 2, 0, None, false);
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
