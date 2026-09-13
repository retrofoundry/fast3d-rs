#[path = "common/coverage_semantics.rs"]
mod coverage_semantics;
use coverage_semantics::*;

#[test]
fn coverage_row_tables_match_edges() {
    let a: [&[usize]; 7] = [
        &[14, 10, 6, 2],
        &[15, 13, 11, 9, 7, 5, 3, 1],
        &[15, 14, 13, 11, 10, 9, 7, 6, 5, 3, 2, 1],
        &[7, 6, 5, 4, 3, 2, 1, 0],
        &[12, 11, 10, 9, 9, 8, 7, 6, 6, 5, 4, 3, 3, 2, 1, 0],
        &[8, 7, 7, 6, 6, 5, 5, 4, 4, 3, 3, 2, 2, 1, 1, 0],
        &[4, 4, 3, 3, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1, 0, 0],
    ];
    let b: [&[usize]; 7] = [
        &[16, 12, 8, 4],
        &[16, 14, 12, 10, 8, 6, 4, 2],
        &[16, 15, 14, 12, 11, 10, 8, 7, 6, 4, 3, 2],
        &[8, 7, 6, 5, 4, 3, 2, 1],
        &[12, 12, 11, 10, 9, 9, 8, 7, 6, 6, 5, 4, 3, 3, 2, 1],
        &[8, 8, 7, 7, 6, 6, 5, 5, 4, 4, 3, 3, 2, 2, 1, 1],
        &[4, 4, 4, 4, 3, 3, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1],
    ];
    for (i, (w, h)) in SIZES.into_iter().enumerate() {
        for (rule, rows) in [(Rule::A, a[i]), (Rule::B, b[i])] {
            for y in -1..=h {
                for x in -1..=w {
                    assert_eq!(
                        covered(right_triangle(w, h), x, y, rule),
                        y >= 0 && y < h && x >= 0 && x < rows[y.max(0).min(h - 1) as usize] as i64,
                        "{w}x{h} ({x},{y}) {rule:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn coverage_reflections_windings_fractional_xy() {
    for case in cases() {
        assert_eq!(
            case.expected(Rule::A).len(),
            (case.width * case.height * 4) as usize
        );
        for draw in case.draws {
            let mut reversed = draw.triangle;
            reversed.swap(1, 2);
            for y in 0..case.height as i64 {
                for x in 0..case.width as i64 {
                    for rule in [Rule::A, Rule::B] {
                        assert_eq!(
                            covered(draw.triangle, x, y, rule),
                            covered(reversed, x, y, rule)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn coverage_rdp_sample_lattice_and_all_ties() {
    for case in cases()
        .into_iter()
        .filter(|c| c.name.contains("ties") || c.modify_xy && c.name.contains("subpixel"))
    {
        assert_eq!((case.width, case.height), (256, 256));
        for draw in case.draws {
            let rdp = RdpEdges::exact(draw.triangle);
            assert_eq!(rdp.words()[0] >> 24, 8);
            for y in 0..256 {
                for x in 0..256 {
                    let mask = rdp.mask(x, y);
                    assert_eq!(
                        mask >> 7 != 0,
                        covered(draw.triangle, x, y, Rule::B),
                        "{} ({x},{y}) {rdp:?}",
                        case.name
                    );
                    let count = SAMPLES
                        .into_iter()
                        .filter(|p| contains(draw.triangle, [4 * x + p[0], 4 * y + p[1]]))
                        .count();
                    assert_eq!(mask.count_ones() as usize, count, "{} ({x},{y})", case.name);
                }
            }
        }
    }
    let t = [[32, 32], [96, 32], [32, 96]];
    let r = RdpEdges::exact(t);
    for (p, inside) in [([8, 12], true), ([12, 8], true), ([16, 16], false)] {
        assert_eq!(r.mask(p[0], p[1]) >> 7 != 0, inside);
    }
    let bottom = RdpEdges::exact([[32, 32], [96, 96], [32, 96]]);
    assert_eq!(bottom.mask(12, 24) >> 7, 0);
    let right = RdpEdges::exact([[32, 32], [96, 32], [96, 96]]);
    assert_eq!(right.mask(24, 12) >> 7, 0);
}

#[test]
fn coverage_rdp_partial_edge_counts() {
    let rdp = RdpEdges::exact(right_triangle(16, 8));
    assert_eq!(
        [13, 14, 15, 16].map(|x| rdp.mask(x, 0).count_ones()),
        [8, 7, 3, 0]
    );
    let other = RdpEdges::exact([[64, 0], [64, 32], [0, 32]]);
    assert_eq!(other.mask(15, 0).count_ones(), 5);
    assert_eq!(other.mask(15, 0) >> 7, 0);
}

#[test]
fn coverage_shared_edge_has_one_owner() {
    for case in cases()
        .into_iter()
        .filter(|c| c.name.contains("shared") && c.kind != Kind::SharedUv)
    {
        for pair in case.draws.as_chunks::<2>().0 {
            for rule in [Rule::A, Rule::B] {
                for y in 0..256 {
                    for x in 0..256 {
                        let count = pair
                            .iter()
                            .filter(|d| covered(d.triangle, x, y, rule))
                            .count();
                        let d = if rule == Rule::A { 2 } else { 0 };
                        let minx = pair
                            .iter()
                            .flat_map(|t| t.triangle)
                            .map(|p| p[0])
                            .min()
                            .unwrap();
                        let miny = pair
                            .iter()
                            .flat_map(|t| t.triangle)
                            .map(|p| p[1])
                            .min()
                            .unwrap();
                        assert_eq!(
                            count,
                            usize::from(
                                (minx..minx + 64).contains(&(4 * x + d))
                                    && (miny..miny + 64).contains(&(4 * y + d))
                            )
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn coverage_extents_are_vi_legal() {
    for case in cases() {
        assert!((1..=1022).contains(&case.width), "{}", case.name);
        assert!((4..=512).contains(&case.height), "{}", case.name);
        assert_eq!(case.height % 4, 0, "{}", case.name);
    }
    assert!(cases().iter().any(|case| case.width % 2 == 1));
}

#[test]
fn coverage_clip_edges_miss_both_sample_lattices() {
    let case = cases()
        .into_iter()
        .find(|c| c.name == "coverage-clip-scissor")
        .unwrap();
    for (i, draw) in case.draws.iter().enumerate().filter(|(_, d)| d.survives()) {
        for e in 0..3 {
            let a = draw.triangle[e];
            let b = draw.triangle[(e + 1) % 3];
            for d in [0, 2] {
                for y in 0..i64::from(case.height) {
                    for x in 0..i64::from(case.width) {
                        assert_ne!(
                            edge(a, b, [4 * x + d, 4 * y + d]),
                            0,
                            "draw {i} edge {e} sample ({x},{y}) offset {d}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn coverage_clip_boundary_intersections_are_fractional() {
    let case = cases()
        .into_iter()
        .find(|c| c.name == "coverage-clip-scissor")
        .unwrap();
    for (i, draw) in case.draws.iter().enumerate().filter(|(_, d)| d.survives()) {
        for e in 0..3 {
            let a = draw.triangle[e];
            let b = draw.triangle[(e + 1) % 3];
            for (axis, bounds) in [
                (0, [0, case.scissor[0], case.scissor[2], case.width]),
                (1, [0, case.scissor[1], case.scissor[3], case.height]),
            ] {
                let delta = b[axis] - a[axis];
                if delta == 0 {
                    continue;
                }
                for shift in [0, 2] {
                    for bound in bounds {
                        let coordinate = 4 * i64::from(bound) - shift;
                        if !(a[axis].min(b[axis])..=a[axis].max(b[axis])).contains(&coordinate) {
                            continue;
                        }
                        let n = a[1 - axis] * delta
                            + (coordinate - a[axis]) * (b[1 - axis] - a[1 - axis]);
                        for translation in [0, shift] {
                            assert_ne!(
                                (n + translation * delta) % (4 * delta),
                                0,
                                "draw {i} edge {e} axis {axis} bound {bound} shift {shift} translation {translation}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn coverage_near_intersection_misses_both_sample_lattices() {
    let case = cases().into_iter().find(|c| c.kind == Kind::Near).unwrap();
    let t = case.draws[0].triangle;
    let midpoint = |b: Point| {
        std::array::from_fn(|axis| {
            assert_eq!((t[0][axis] + b[axis]) % 2, 0);
            (t[0][axis] + b[axis]) / 2
        })
    };
    let a = midpoint(t[1]);
    let b = midpoint(t[2]);
    for d in [0, 2] {
        for y in 0..i64::from(case.height) {
            for x in 0..i64::from(case.width) {
                assert_ne!(edge(a, b, [4 * x + d, 4 * y + d]), 0);
            }
        }
    }
}

#[test]
fn coverage_near_clip_commutes_with_w_scaled_translation() {
    let a = [-0.5, 0.25, -0.25, 0.5];
    let b = [0.5, -0.25, 0.75, 2.0];
    let translate = |p: [f64; 4]| [p[0] + p[3] / 256.0, p[1] - p[3] / 256.0, p[2], p[3]];
    let intersect = |a: [f64; 4], b: [f64; 4]| {
        let t = -a[2] / (b[2] - a[2]);
        std::array::from_fn::<_, 4, _>(|i| a[i] + t * (b[i] - a[i]))
    };
    assert_eq!(
        translate(intersect(a, b)),
        intersect(translate(a), translate(b))
    );
    let before = intersect(a, b);
    let after = translate(before);
    assert_eq!((after[0] / after[3] - before[0] / before[3]) * 128.0, 0.5);
    assert_eq!(
        (after[1] / after[3] - before[1] / before[3]) * (-128.0),
        0.5
    );
}

#[test]
fn coverage_parent_and_b_are_distinguishable() {
    for case in cases() {
        assert_ne!(
            case.expected(Rule::A),
            case.expected(Rule::B),
            "{}",
            case.name
        );
        assert!(!case.name.is_empty());
        let _ = (case.f3d, case.aa);
    }
}

#[test]
fn coverage_uv_identity_and_lod_interior_shift() {
    for kind in [
        Kind::Perspective,
        Kind::PerspectiveNegative,
        Kind::Lod,
        Kind::SharedUv,
    ] {
        let case = cases().into_iter().find(|c| c.kind == kind).unwrap();
        let a = case.expected(Rule::A);
        let b = case.expected(Rule::B);
        let mut changes = 0;
        for y in 0..case.height as i64 {
            for x in 0..case.width as i64 {
                let covered_both = [Rule::A, Rule::B]
                    .into_iter()
                    .all(|rule| case.draws.iter().any(|d| covered(d.triangle, x, y, rule)));
                let i = ((y * i64::from(case.width) + x) * 4) as usize;
                changes += usize::from(covered_both && a[i..i + 4] != b[i..i + 4]);
            }
        }
        if matches!(kind, Kind::Perspective | Kind::PerspectiveNegative) {
            assert_eq!(changes, 0);
        } else {
            assert!(changes > 0, "{kind:?} needs an interior witness");
        }
    }
}

#[test]
fn coverage_dither_uses_destination_index() {
    let row = (0..32).fold(0u32, |bits, x| {
        bits | (u32::from(dither_keep(144 + x, 104, 320)) << x)
    });
    assert_eq!(row, 0x35b9_7211);
    let case = cases()
        .into_iter()
        .find(|c| c.kind == Kind::Dither)
        .unwrap();
    let a = case.expected(Rule::A);
    let b = case.expected(Rule::B);
    for y in 33..96 {
        for x in 33..96 {
            let i = ((y * case.width + x) * 4) as usize;
            assert_eq!(a[i..i + 4], b[i..i + 4]);
        }
    }
}
