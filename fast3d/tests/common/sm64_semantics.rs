#[derive(Clone, Copy, Debug)]
pub enum Case {
    Host64Fill,
    CombinerSelector,
    PowerMeterPoint,
    CastleTrilerp,
    TransparentMario,
}

pub const CASES: [Case; 5] = [
    Case::Host64Fill,
    Case::CombinerSelector,
    Case::PowerMeterPoint,
    Case::CastleTrilerp,
    Case::TransparentMario,
];

impl Case {
    pub fn filename(self) -> &'static str {
        match self {
            Self::Host64Fill => "host64-fill.f3dcap",
            Self::CombinerSelector => "combiner-env-alpha.f3dcap",
            Self::PowerMeterPoint => "sm64-power-meter-point.f3dcap",
            Self::CastleTrilerp => "sm64-castle-trilerp.f3dcap",
            Self::TransparentMario => "sm64-transparent-mario.f3dcap",
        }
    }

    pub fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Host64Fill => (64, 48),
            _ => (320, 240),
        }
    }

    pub fn assert_pixels(self, rgba8: &[u8]) {
        let (width, height) = self.dimensions();
        assert_eq!(rgba8.len(), (width * height * 4) as usize);
        let mut survivors = 0;
        for (i, pixel) in rgba8.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (i % width as usize, i / width as usize);
            let black = [0, 0, 0, 255];
            let square = (144..176).contains(&x) && (104..136).contains(&y);
            let (expected, tolerance) = match self {
                Self::Host64Fill => ([255, 0, 0, 255], 0),
                Self::CombinerSelector if square => ([50, 25, 10, 255], 1),
                Self::PowerMeterPoint if (88..152).contains(&y) && (128..192).contains(&x) => (
                    match x {
                        128..=144 => [255, 0, 0, 255],
                        145..=159 => [0, 255, 0, 255],
                        160..=175 => [0, 0, 255, 255],
                        _ => [255, 255, 0, 255],
                    },
                    2,
                ),
                Self::CastleTrilerp if square => (
                    if x >= 165 {
                        [64, 128, 128, 255]
                    } else {
                        [
                            [96, 32, 32, 255],
                            [128, 64, 64, 255],
                            [64, 128, 128, 255],
                            [32, 96, 96, 255],
                        ][(x - 144) % 4]
                    },
                    2,
                ),
                Self::TransparentMario if square && *pixel != black => {
                    survivors += 1;
                    ([128, 0, 0, 128], 1)
                }
                _ => (black, 0),
            };
            assert!(
                pixel
                    .iter()
                    .zip(expected)
                    .all(|(&a, b)| a.abs_diff(b) <= tolerance),
                "{self:?} ({x},{y}): {pixel:?}, expected {expected:?}"
            );
        }
        if matches!(self, Self::TransparentMario) {
            assert!(
                (450..=575).contains(&survivors),
                "alpha 128/255 across 1024 pixels: {survivors} survivors"
            );
        }
    }
}
