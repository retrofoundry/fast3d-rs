#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    OffsetRect,
    OffsetTriangle,
    ExactRect,
    ShortcutRect,
}

pub const CASES: [Case; 4] = [
    Case::OffsetRect,
    Case::OffsetTriangle,
    Case::ExactRect,
    Case::ShortcutRect,
];

pub const PRODUCER: [[u8; 4]; 8] = [
    [19, 85, 141, 255],
    [67, 133, 201, 255],
    [245, 29, 99, 255],
    [111, 177, 43, 255],
    [33, 66, 99, 255],
    [132, 165, 198, 255],
    [222, 189, 156, 255],
    [8, 49, 90, 255],
];

pub const UNPACKED: [[u8; 4]; 8] = [
    [16, 82, 140, 255],
    [66, 132, 206, 255],
    [247, 24, 99, 255],
    [107, 181, 41, 255],
    [33, 66, 99, 255],
    [132, 165, 198, 255],
    [222, 189, 156, 255],
    [8, 49, 90, 255],
];
pub const REPLACEMENT: [u8; 4] = [203, 53, 107, 255];

impl Case {
    pub fn fixture_bytes(self) -> &'static [u8] {
        match self {
            Self::OffsetRect => include_bytes!("../fixtures/fbload-offset-rect.f3dcap"),
            Self::OffsetTriangle => include_bytes!("../fixtures/fbload-offset-triangle.f3dcap"),
            Self::ExactRect => include_bytes!("../fixtures/fbload-exact-rect.f3dcap"),
            Self::ShortcutRect => include_bytes!("../fixtures/fbload-shortcut-rect.f3dcap"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::OffsetRect => "fbload-offset-rect",
            Self::OffsetTriangle => "fbload-offset-triangle",
            Self::ExactRect => "fbload-exact-rect",
            Self::ShortcutRect => "fbload-shortcut-rect",
        }
    }

    pub fn offset(self) -> bool {
        matches!(self, Self::OffsetRect | Self::OffsetTriangle)
    }
}

pub fn oracle_expected(x: u32, y: u32) -> [u8; 4] {
    expected(x, y, UNPACKED, UNPACKED)
}

pub fn fast3d_expected(case: Case, x: u32, y: u32) -> [u8; 4] {
    if case == Case::ShortcutRect {
        if (88..120).contains(&y) {
            [0, 0, 0, 255]
        } else {
            expected(x, y, PRODUCER, [REPLACEMENT; 8])
        }
    } else if (232..264).contains(&x) && (40..72).contains(&y) {
        [0, 0, 255, 255]
    } else {
        [0, 0, 0, 255]
    }
}

fn expected(x: u32, y: u32, before: [[u8; 4]; 8], after: [[u8; 4]; 8]) -> [u8; 4] {
    for (left, colors) in [(40, before), (136, after)] {
        if (left..left + 64).contains(&x) {
            if (40..72).contains(&y) {
                return colors[((y - 40) / 16 * 4 + (x - left) / 16) as usize];
            }
            if (88..120).contains(&y) {
                return [255; 4];
            }
        }
    }
    if (232..264).contains(&x) && (40..72).contains(&y) {
        [0, 0, 255, 255]
    } else {
        [0, 0, 0, 255]
    }
}
