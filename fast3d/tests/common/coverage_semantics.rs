pub type Point = [i64; 2];
pub type Triangle = [Point; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    A,
    B,
}

pub const SIZES: [(i64, i64); 7] = [
    (16, 4),
    (16, 8),
    (16, 12),
    (8, 8),
    (12, 16),
    (8, 16),
    (4, 16),
];
pub const SAMPLES: [Point; 8] = [
    [0, 0],
    [2, 0],
    [1, 1],
    [3, 1],
    [0, 2],
    [2, 2],
    [1, 3],
    [3, 3],
];
pub const BLACK: [u8; 4] = [0, 0, 0, 255];
pub const COLORS: [[u8; 4]; 3] = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];

pub fn edge(a: Point, b: Point, p: Point) -> i64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

pub fn contains(mut t: Triangle, p: Point) -> bool {
    let area = edge(t[0], t[1], t[2]);
    if area == 0 {
        return false;
    }
    if area < 0 {
        t.swap(1, 2);
    }
    (0..3).all(|i| {
        let a = t[i];
        let b = t[(i + 1) % 3];
        let e = edge(a, b, p);
        e > 0 || (e == 0 && (b[1] < a[1] || (b[1] == a[1] && b[0] > a[0])))
    })
}

pub fn covered(t: Triangle, x: i64, y: i64, rule: Rule) -> bool {
    let d = if rule == Rule::A { 2 } else { 0 };
    contains(t, [4 * x + d, 4 * y + d])
}

pub fn right_triangle(w: i64, h: i64) -> Triangle {
    [[0, 0], [4 * w, 0], [0, 4 * h]]
}

#[derive(Clone, Debug)]
pub struct Draw {
    pub triangle: Triangle,
    pub color: [u8; 4],
    pub cull: i8,
}

impl Draw {
    pub fn survives(&self) -> bool {
        let area = edge(self.triangle[0], self.triangle[1], self.triangle[2]);
        area != 0 && (self.cull == 0 || area.signum() == i64::from(self.cull))
    }
}

#[derive(Clone, Debug)]
pub struct Case {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub f3d: bool,
    pub modify_xy: bool,
    pub aa: bool,
    pub scissor: [u32; 4],
    pub draws: Vec<Draw>,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Flat,
    Near,
    Perspective,
    PerspectiveNegative,
    Shade,
    Rects,
    Depth,
    Lod,
    Alpha,
    Fog,
    Dither,
    SharedUv,
    SharedAlpha,
}

impl Case {
    pub fn expected(&self, rule: Rule) -> Vec<u8> {
        let mut pixels = BLACK.repeat((self.width * self.height) as usize);
        if self.kind == Kind::Rects {
            for (region, color) in [
                ([8, 8, 40, 40], COLORS[0]),
                ([56, 8, 88, 40], COLORS[1]),
                ([104, 8, 136, 40], COLORS[2]),
                ([153, 9, 185, 41], COLORS[0]),
            ] {
                for y in region[1]..region[3] {
                    for x in region[0]..region[2] {
                        let i = ((y * self.width + x) * 4) as usize;
                        pixels[i..i + 4].copy_from_slice(&color);
                    }
                }
            }
        }
        for y in self.scissor[1]..self.scissor[3] {
            for x in self.scissor[0]..self.scissor[2] {
                for (draw_index, draw) in self.draws.iter().enumerate() {
                    if self.kind == Kind::Depth
                        && draw_index == 2
                        && covered(self.draws[1].triangle, x.into(), y.into(), rule)
                    {
                        continue;
                    }
                    let t = draw.triangle;
                    let clipped = if self.kind == Kind::Near {
                        let m = |a: Point, b: Point| [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
                        [
                            [m(t[0], t[1]), t[1], t[2]],
                            [m(t[0], t[1]), t[2], m(t[2], t[0])],
                        ]
                    } else {
                        [t, t]
                    };
                    if draw.survives()
                        && clipped
                            .into_iter()
                            .any(|t| covered(t, x.into(), y.into(), rule))
                    {
                        let i = ((y * self.width + x) * 4) as usize;
                        let d = if rule == Rule::A { 2 } else { 0 };
                        let p = [4 * i64::from(x) + d, 4 * i64::from(y) + d];
                        let l = edge(t[2], t[0], p) as f64 / edge(t[0], t[1], t[2]) as f64;
                        if self.kind == Kind::Alpha && 255.0 * l < 128.0 {
                            continue;
                        }
                        if self.kind == Kind::Dither && !dither_keep(x, y, self.width) {
                            continue;
                        }
                        let color = match self.kind {
                            Kind::SharedAlpha => [
                                draw.color[0] / 2 + draw.color[0] % 2,
                                draw.color[1] / 2 + draw.color[1] % 2,
                                0,
                                128,
                            ],
                            Kind::SharedUv => {
                                let p = [4 * i64::from(x), 4 * i64::from(y)];
                                let weights = [
                                    edge(t[1], t[2], p),
                                    edge(t[2], t[0], p),
                                    edge(t[0], t[1], p),
                                ];
                                let coords = if draw_index == 0 {
                                    [[0, 0], [16, 0], [0, 16]]
                                } else {
                                    [[16, 0], [31, 31], [0, 16]]
                                };
                                let uv: [i64; 2] = std::array::from_fn(|axis| {
                                    let n: i64 = weights
                                        .iter()
                                        .zip(coords)
                                        .map(|(w, st)| w * st[axis])
                                        .sum();
                                    let value = n as f64 / edge(t[0], t[1], t[2]) as f64;
                                    ((value * 128.0).round() / 128.0).floor() as i64
                                });
                                COLORS[((uv[0].clamp(0, 31) / 4 + 2 * (uv[1].clamp(0, 31) / 4)) % 3)
                                    as usize]
                            }
                            Kind::Alpha => [255, 0, 0, (255.0 * l).round() as u8],
                            Kind::Fog => [
                                (255.0 - 128.0 * l).round() as u8,
                                (128.0 * l).round() as u8,
                                0,
                                255,
                            ],
                            Kind::Dither => [255, 0, 0, 128],
                            Kind::Lod => {
                                let u = |x: f64| {
                                    let l = (x - 32.0) / 160.0;
                                    (5.0 * (1.0 - l) + 101.0 * l / 4.0) / (1.0 - l + l / 4.0)
                                };
                                let d = if rule == Rule::A { 0.5 } else { 0.0 };
                                let left = f64::from(x & !1) + d;
                                let level = usize::from(u(left + 1.0) - u(left) >= 2.0);
                                let s = ((u(f64::from(x)) * 128.0).round() / 128.0).floor() as i64;
                                COLORS
                                    [((s.rem_euclid(32) / 4 + level as i64).rem_euclid(3)) as usize]
                            }
                            Kind::Perspective | Kind::PerspectiveNegative => {
                                let mut uv =
                                    perspective_uv(t, [4 * i64::from(x), 4 * i64::from(y)]);
                                if self.kind == Kind::PerspectiveNegative {
                                    uv = uv.map(|v| 31.0 - v);
                                }
                                let s = ((uv[0] * 128.0).round() / 128.0).floor() as i64;
                                let t = ((uv[1] * 128.0).round() / 128.0).floor() as i64;
                                COLORS[((s.div_euclid(4) + 2 * t.div_euclid(4)).rem_euclid(3))
                                    as usize]
                            }
                            Kind::Shade => {
                                let d = if rule == Rule::A { 2 } else { 0 };
                                let p = [4 * i64::from(x) + d, 4 * i64::from(y) + d];
                                let area = edge(t[0], t[1], t[2]) as f64;
                                [
                                    (255.0 * edge(t[2], t[0], p) as f64 / area).round() as u8,
                                    (255.0 * edge(t[0], t[1], p) as f64 / area).round() as u8,
                                    0,
                                    255,
                                ]
                            }
                            _ => draw.color,
                        };
                        pixels[i..i + 4].copy_from_slice(&color);
                    }
                }
            }
        }
        pixels
    }
}

pub fn perspective_uv(t: Triangle, p: Point) -> [f64; 2] {
    let weights = [
        edge(t[1], t[2], p) as f64,
        edge(t[2], t[0], p) as f64 / 2.0,
        edge(t[0], t[1], p) as f64 / 4.0,
    ];
    let sum = weights.iter().sum::<f64>();
    [
        (weights[0] * 1.25 + weights[1] * 25.25 + weights[2] * 1.25) / sum,
        (weights[0] * 2.75 + weights[1] * 2.75 + weights[2] * 26.75) / sum,
    ]
}

pub fn dither_keep(x: u32, y: u32, width: u32) -> bool {
    let mut a = 0u32;
    let mut b = x + width * y;
    let mut sum = 0u32;
    for _ in 0..16 {
        sum = sum.wrapping_add(0x9e37_79b9);
        a = a.wrapping_add(
            (b << 4).wrapping_add(0xa341_316c)
                ^ b.wrapping_add(sum)
                ^ (b >> 5).wrapping_add(0xc801_3ea4),
        );
        b = b.wrapping_add(
            (a << 4).wrapping_add(0xad90_777d)
                ^ a.wrapping_add(sum)
                ^ (a >> 5).wrapping_add(0x7e95_761e),
        );
    }
    let high = a.wrapping_mul(1664525).wrapping_add(1013904223) >> 24;
    255 * (2 * high + 1) <= 65536
}

pub fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, width, height, f3d, exact, offset, modify_xy, aa) in [
        ("slopes-f3d", 320, 240, true, false, [0, 0], false, false),
        (
            "slopes-f3dex2",
            320,
            240,
            false,
            false,
            [0, 0],
            false,
            false,
        ),
        ("ties-f3d", 256, 256, true, true, [0, 0], false, false),
        ("ties-f3dex2", 256, 256, false, true, [0, 0], false, false),
        ("ties-aa", 256, 256, false, true, [0, 0], false, true),
        ("subpixel-x", 256, 256, false, true, [1, 0], true, false),
        ("subpixel-y", 256, 256, false, true, [0, 1], true, false),
        ("subpixel-xy", 256, 256, false, true, [1, 3], true, false),
        ("subpixel-half", 256, 256, false, true, [2, 2], true, false),
        ("subpixel-yx", 256, 256, false, true, [3, 1], true, false),
        ("subpixel-f3d", 256, 256, true, true, [1, 3], false, false),
        (
            "extent-192x128",
            192,
            128,
            false,
            true,
            [0, 0],
            false,
            false,
        ),
        (
            "extent-193x132",
            193,
            132,
            false,
            true,
            [0, 0],
            false,
            false,
        ),
        (
            "retained-height",
            256,
            128,
            false,
            true,
            [0, 0],
            false,
            false,
        ),
    ] {
        let mut draws = Vec::new();
        for (column, (w, h)) in SIZES.into_iter().enumerate() {
            if exact && w == 16 && h == 12 {
                continue;
            }
            for reflection in 0..4 {
                for reverse in 0..2 {
                    let mut triangle = right_triangle(w, h).map(|[x, y]| {
                        [
                            (if reflection & 1 == 0 { x } else { 4 * w - x })
                                + 4 * (8 + column as i64 * 24)
                                + offset[0],
                            (if reflection & 2 == 0 { y } else { 4 * h - y })
                                + 4 * (8 + reflection * 44 + reverse * 20)
                                + offset[1],
                        ]
                    });
                    if reverse == 1 {
                        triangle.swap(1, 2);
                    }
                    draws.push(Draw {
                        triangle,
                        color: COLORS[reverse as usize],
                        cull: 0,
                    });
                }
            }
        }
        cases.push(Case {
            name: format!("coverage-{name}"),
            width,
            height,
            f3d,
            modify_xy,
            aa,
            scissor: [0, 0, width, height],
            draws,
            kind: Kind::Flat,
        });
    }
    // Each cell has both input windings; culling must remove whole primitives.
    for (name, cull) in [("cull-back", -1), ("cull-front", 1)] {
        let mut case = cases[3].clone();
        case.name = format!("coverage-{name}");
        for draw in &mut case.draws {
            draw.cull = cull;
        }
        cases.push(case);
        let mut case = cases[2].clone();
        case.name = format!("coverage-{name}-f3d");
        for draw in &mut case.draws {
            draw.cull = cull;
        }
        cases.push(case);
    }
    for diagonal in 0..2 {
        for reverse in 0..2 {
            let mut draws = Vec::new();
            for (cell, offset) in [[0, 0], [1, 3], [2, 2], [3, 1]].into_iter().enumerate() {
                let p = [[0, 0], [64, 0], [64, 64], [0, 64]]
                    .map(|[x, y]| [x + 32 + cell as i64 * 96 + offset[0], y + 64 + offset[1]]);
                let triangles = if diagonal == 0 {
                    [[p[0], p[1], p[2]], [p[0], p[2], p[3]]]
                } else {
                    [[p[0], p[1], p[3]], [p[1], p[2], p[3]]]
                };
                let mut pair: Vec<_> = triangles
                    .into_iter()
                    .enumerate()
                    .map(|(i, triangle)| Draw {
                        triangle,
                        color: COLORS[i],
                        cull: 0,
                    })
                    .collect();
                if reverse == 1 {
                    pair.reverse();
                }
                draws.extend(pair);
            }
            cases.push(Case {
                name: format!("coverage-shared-{diagonal}-{reverse}"),
                width: 256,
                height: 256,
                f3d: false,
                modify_xy: true,
                aa: false,
                scissor: [0, 0, 256, 256],
                draws,
                kind: Kind::Flat,
            });
        }
    }
    // Fractional intercepts keep clipped edges off both sample lattices.
    let draws = [
        ([[-16, 24], [48, 8], [24, 72]], [1, 1]),
        ([[232, 8], [280, 40], [208, 72]], [1, 1]),
        ([[64, -16], [112, 48], [40, 24]], [2, 1]),
        ([[96, 216], [152, 280], [56, 240]], [3, 1]),
        ([[0, 0], [0, 0], [0, 0]], [0, 0]),
        ([[120, 100], [140, 120], [160, 140]], [0, 0]),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (t, [dx, dy]))| Draw {
        triangle: t.map(|[x, y]| [4 * x + dx, 4 * y + dy]),
        color: COLORS[i % 3],
        cull: 0,
    })
    .collect();
    cases.push(Case {
        name: "coverage-clip-scissor".into(),
        width: 256,
        height: 256,
        f3d: false,
        modify_xy: false,
        aa: false,
        scissor: [12, 20, 244, 236],
        draws,
        kind: Kind::Flat,
    });
    for (name, kind) in [
        ("near-plane", Kind::Near),
        ("uv-perspective", Kind::Perspective),
        ("uv-perspective-negative", Kind::PerspectiveNegative),
        ("shade", Kind::Shade),
        ("rect-controls", Kind::Rects),
        ("lod-perspective", Kind::Lod),
        ("alpha-threshold", Kind::Alpha),
        ("fog", Kind::Fog),
        ("dither", Kind::Dither),
    ] {
        cases.push(Case {
            name: format!("coverage-{name}"),
            width: 256,
            height: 256,
            f3d: false,
            modify_xy: false,
            aa: false,
            scissor: if kind == Kind::Rects {
                [0, 0, 256, 256]
            } else {
                [12, 20, 244, 236]
            },
            kind,
            draws: vec![Draw {
                triangle: if kind == Kind::Rects {
                    [[128, 320], [768, 320], [128, 960]]
                } else if kind == Kind::Near {
                    [[130, 128], [768, 128], [128, 768]]
                } else {
                    [[128, 128], [768, 128], [128, 768]]
                },
                color: COLORS[0],
                cull: 0,
            }],
        });
    }
    cases.push(Case {
        name: "coverage-decal-depth".into(),
        width: 256,
        height: 256,
        f3d: false,
        modify_xy: false,
        aa: false,
        scissor: [0, 0, 256, 256],
        kind: Kind::Depth,
        draws: vec![
            Draw {
                triangle: [[128, 128], [768, 128], [128, 768]],
                color: COLORS[2],
                cull: 0,
            },
            Draw {
                triangle: [[256, 256], [448, 256], [256, 448]],
                color: COLORS[1],
                cull: 0,
            },
            Draw {
                triangle: [[128, 128], [768, 128], [128, 768]],
                color: COLORS[0],
                cull: 0,
            },
        ],
    });
    let mut transparent: Vec<_> = cases
        .iter()
        .filter(|c| c.name.starts_with("coverage-shared-"))
        .cloned()
        .collect();
    for case in &mut transparent {
        case.name.push_str("-alpha");
        case.kind = Kind::SharedAlpha;
    }
    cases.extend(transparent);
    cases.push(Case {
        name: "coverage-shared-uv".into(),
        width: 256,
        height: 256,
        f3d: false,
        modify_xy: false,
        aa: false,
        scissor: [0, 0, 256, 256],
        kind: Kind::SharedUv,
        draws: vec![
            Draw {
                triangle: [[128, 128], [768, 128], [128, 768]],
                color: COLORS[0],
                cull: 0,
            },
            Draw {
                triangle: [[768, 128], [768, 768], [128, 768]],
                color: COLORS[1],
                cull: 0,
            },
        ],
    });
    cases
}

#[derive(Clone, Debug)]
pub struct RdpEdges {
    pub yh: i64,
    pub ym: i64,
    pub yl: i64,
    pub xh: i64,
    pub xm: i64,
    pub xl: i64,
    pub dh: i64,
    pub dm: i64,
    pub dl: i64,
    pub major_left: bool,
}

impl RdpEdges {
    pub fn exact(mut t: Triangle) -> Self {
        t.sort_by_key(|p| (p[1], p[0]));
        let [h, m, l] = t;
        assert_ne!(edge(h, m, l), 0);
        let slope = |a: Point, b: Point| {
            if a[1] == b[1] {
                return 0;
            }
            let n = (b[0] - a[0]) * 65536;
            assert_eq!(n % (b[1] - a[1]), 0, "nonrepresentable slope");
            let d = n / (b[1] - a[1]);
            assert_eq!(d & 7, 0, "quarter step loses bits");
            d
        };
        let dh = slope(h, l);
        let dm = slope(h, m);
        let dl = slope(m, l);
        Self {
            yh: h[1],
            ym: m[1],
            yl: l[1],
            xh: h[0] * 16384 - dh / 4 * (h[1] & 3),
            xm: h[0] * 16384 - dm / 4 * (h[1] & 3),
            xl: m[0] * 16384,
            dh,
            dm,
            dl,
            major_left: edge(h, m, l) > 0,
        }
    }

    pub fn words(&self) -> [u32; 8] {
        [
            0x0800_0000 | (u32::from(self.major_left) << 23) | self.yl as u32,
            ((self.ym as u32 & 0x3fff) << 16) | (self.yh as u32 & 0x3fff),
            self.xl as u32,
            self.dl as u32,
            self.xh as u32,
            self.dh as u32,
            self.xm as u32,
            self.dm as u32,
        ]
    }

    pub fn endpoints(&self, k: i64) -> Option<(i64, i64)> {
        if k < self.yh || k >= self.yl {
            return None;
        }
        let start = self.yh & !3;
        let major = (self.xh & !1) + (k - start) * ((self.dh >> 2) & !1);
        let minor = if k >= self.ym {
            (self.xl & !1) + (k - self.ym) * ((self.dl >> 2) & !1)
        } else {
            (self.xm & !1) + (k - start) * ((self.dm >> 2) & !1)
        };
        Some(if self.major_left {
            (major, minor)
        } else {
            (minor, major)
        })
    }

    pub fn mask(&self, x: i64, y: i64) -> u8 {
        let mut mask = 0;
        for subrow in 0..4 {
            let Some((left, right)) = self.endpoints(y * 4 + subrow) else {
                continue;
            };
            if left >> 14 > right >> 14 {
                continue;
            }
            let endpoint = |v: i64| ((v >> 13) & !1) | i64::from((v >> 1) & 0x1fff != 0);
            let l = endpoint(left);
            let r = endpoint(right);
            let mut nibble = 0xa >> (subrow & 1);
            if x < l >> 3 || x > r >> 3 {
                continue;
            }
            if x == l >> 3 {
                nibble &= 0xf >> (((l & 7) + 1) >> 1);
            }
            if x == r >> 3 {
                nibble &= 0xf0 >> (((r & 7) + 1) >> 1);
            }
            mask |= (nibble << ((subrow - 2) & 4)) as u8;
        }
        mask
    }
}
