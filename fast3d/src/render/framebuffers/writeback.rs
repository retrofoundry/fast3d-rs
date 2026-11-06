#[cfg(not(target_arch = "wasm32"))]
use super::coherence::{CoverageDraw, CoverageGeometry};
use crate::hle::combiner::{AlphaIn, ColorIn, CycleSel};

#[derive(Clone, Debug, PartialEq)]
pub(in crate::render) struct Combiner {
    selectors: CycleSel,
    prim: [f32; 4],
    env: [f32; 4],
    blend: [f32; 4],
    fog: [f32; 4],
    lod: f32,
}

impl Combiner {
    pub fn new(
        uniform: &crate::render::CombinerUniform,
        mode: crate::hle::RenderMode,
    ) -> Option<Self> {
        if uniform.tex_enable != 0
            || mode.cycle_type != 0
            || mode.z_test
            || mode.z_write
            || mode.cvg_x_alpha
            || mode.alpha_compare != crate::hle::AlphaCompare::None
            || mode.alpha_cvg_select
            || mode.blend_class == crate::hle::BlendClass::BlendColor
            || mode.color_on_coverage
        {
            return None;
        }
        let selectors =
            crate::hle::combiner::decode_combine(uniform.combine_l, uniform.combine_h).cyc1;
        let result = Self {
            selectors,
            prim: uniform.prim,
            env: uniform.env,
            blend: uniform.blend_color,
            fog: uniform.fog_color,
            lod: uniform.lod_params[2],
        };
        result.color([1.0; 4])?;
        Some(result)
    }

    pub fn color(&self, shade: [f32; 4]) -> Option<[f32; 4]> {
        let color = |input, channel| {
            Some(match input {
                ColorIn::Zero | ColorIn::Combined | ColorIn::CombinedAlpha => 0.0,
                ColorIn::One | ColorIn::LodFraction => 1.0,
                ColorIn::Primitive => self.prim[channel],
                ColorIn::Environment => self.env[channel],
                ColorIn::Shade => shade[channel],
                ColorIn::PrimitiveAlpha => self.prim[3],
                ColorIn::EnvAlpha => self.env[3],
                ColorIn::ShadeAlpha => shade[3],
                ColorIn::PrimLodFrac => self.lod,
                _ => return None,
            })
        };
        let alpha = |input| {
            Some(match input {
                AlphaIn::Zero | AlphaIn::Combined => 0.0,
                AlphaIn::One | AlphaIn::LodFraction => 1.0,
                AlphaIn::Primitive => self.prim[3],
                AlphaIn::Environment => self.env[3],
                AlphaIn::Shade => shade[3],
                AlphaIn::PrimLodFrac => self.lod,
                _ => return None,
            })
        };
        let s = &self.selectors;
        let mut rgba = [0.0; 4];
        for (channel, value) in rgba[..3].iter_mut().enumerate() {
            *value = ((color(s.ca, channel)? - color(s.cb, channel)?) * color(s.cc, channel)?
                + color(s.cd, channel)?)
            .clamp(0.0, 1.0);
        }
        rgba[3] = ((alpha(s.aa)? - alpha(s.ab)?) * alpha(s.ac)? + alpha(s.ad)?).clamp(0.0, 1.0);
        Some(rgba)
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Default)]
pub(super) enum Pixel {
    #[default]
    Fallback,
    Keep,
    Write([u8; 4], u8, u32),
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
pub(super) struct Vertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

#[cfg(not(target_arch = "wasm32"))]
impl Vertex {
    pub(super) fn in_range(self) -> bool {
        (-1024.0..=1023.75).contains(&self.position[0])
            && (-2048.0..=2047.75).contains(&self.position[1])
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn destination(mode: crate::hle::RenderMode, count: u8, memory: u8) -> u8 {
    let overflow = (count + memory) & 8 != 0;
    let blend = mode.force_blend || (mode.antialias && !overflow);
    match mode.cvg_dst {
        0 if blend => (count + memory).min(7),
        0 => count - 1,
        1 => (count + memory) & 7,
        2 => 7,
        _ => memory,
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn admitted(draw: &CoverageDraw, vertices: &[Option<Vertex>]) -> bool {
    let Some(combiner) = &draw.combiner else {
        return false;
    };
    match &draw.geometry {
        CoverageGeometry::Rectangle(_) => return true,
        CoverageGeometry::Unknown => return false,
        CoverageGeometry::Triangles(_) => {}
    }
    let CoverageGeometry::Triangles(triangles) = &draw.geometry else {
        unreachable!()
    };
    for triangle in triangles {
        let [Some(a), Some(b), Some(c)] =
            triangle.map(|i| vertices.get(i as usize).copied().flatten())
        else {
            return false;
        };
        let v = [a, b, c];
        if v.iter().any(|v| !v.in_range()) {
            return false;
        }
        let s = &combiner.selectors;
        if [s.aa, s.ab, s.ac, s.ad].contains(&AlphaIn::Shade)
            && v.iter().any(|v| v.color[3] != a.color[3])
        {
            return false;
        }
        if v.iter().any(|v| !v.color.iter().all(|c| c.is_finite())) {
            return false;
        }
        let Some(alpha) = combiner.color(v[0].color).map(|c| c[3]) else {
            return false;
        };
        if v.iter()
            .any(|v| combiner.color(v.color).is_none_or(|c| c[3] != alpha))
        {
            return false;
        }
    }
    true
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn rasterize(
    draw: &CoverageDraw,
    vertices: &[Option<Vertex>],
    extent: (usize, usize),
    mut pixel: impl FnMut(usize, u8, [f32; 4], f32),
) {
    let combiner = draw.combiner.as_ref().unwrap();
    let mode = draw.mode.unwrap();
    if let CoverageGeometry::Rectangle([left, top, right, bottom]) = draw.geometry {
        let (width, height) = extent;
        let (sx, sy, sw, sh) = draw.scissor;
        let color = combiner.color([1.0; 4]).unwrap();
        for y in (top.floor().max(0.0) as usize).max(sy as usize)
            ..(bottom.ceil().max(0.0) as usize)
                .min(height)
                .min((sy + sh) as usize)
        {
            for x in (left.floor().max(0.0) as usize).max(sx as usize)
                ..(right.ceil().max(0.0) as usize)
                    .min(width)
                    .min((sx + sw) as usize)
            {
                let mut mask = 0u8;
                for sub in 0..4 {
                    for sample in 0..2 {
                        let px = x as f32 + (sub & 1) as f32 * 0.25 + sample as f32 * 0.5;
                        let py = y as f32 + sub as f32 * 0.25;
                        if px >= left && px < right && py >= top && py < bottom {
                            mask |= 1 << (7 - sub * 2 - sample);
                        }
                    }
                }
                if mask != 0 && (mode.antialias || mask & 0x80 != 0) {
                    pixel(y * width + x, mask.count_ones() as u8, color, 1.0);
                }
            }
        }
        return;
    }
    let CoverageGeometry::Triangles(triangles) = &draw.geometry else {
        unreachable!();
    };
    let (width, height) = extent;
    let (sx, sy, sw, sh) = draw.scissor;
    for triangle in triangles {
        let mut v = triangle.map(|i| vertices[i as usize].unwrap());
        let [a, b, c] = v.map(|v| v.position);
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if draw.cull && area >= 0.0 {
            continue;
        }
        v.sort_by(|a, b| {
            a.position[1]
                .total_cmp(&b.position[1])
                .then(a.position[0].total_cmp(&b.position[0]))
        });
        let q = v.map(|v| v.position.map(|x| (f64::from(x) * 4.0).round() as i64));
        let [a, b, c] = q;
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area == 0 {
            continue;
        }
        let slope = |a: [i64; 2], b: [i64; 2]| {
            let numerator = (b[0] - a[0]) << 16;
            let denominator = (b[1] - a[1]).max(1);
            (numerator.signum() * ((numerator.abs() + denominator - 1) / denominator)) & !7
        };
        let major = slope(a, c);
        let upper = slope(a, b);
        let lower = slope(b, c);
        let ybase = a[1] & !3;
        let xmajor = (a[0] << 14) - (major >> 2) * (a[1] & 3);
        let xupper = (a[0] << 14) - (upper >> 2) * (a[1] & 3);
        let top = (a[1].div_euclid(4).max(0) as usize).max(sy as usize);
        let bottom = (c[1].div_euclid(4) + 1).max(0) as usize;
        for y in top..bottom.min(height).min((sy + sh) as usize) {
            let mut edges = [(0i64, 0i64); 4];
            let mut left = i64::MAX;
            let mut right = i64::MIN;
            for (sub, edge) in edges.iter_mut().enumerate() {
                let scan = y as i64 * 4 + sub as i64;
                if scan < a[1] || scan >= c[1] {
                    continue;
                }
                let mx = xmajor + ((major >> 2) & !1) * (scan - ybase);
                let nx = if scan < b[1] {
                    xupper + ((upper >> 2) & !1) * (scan - ybase)
                } else {
                    (b[0] << 14) + ((lower >> 2) & !1) * (scan - b[1])
                };
                let quantize = |x: i64| ((x >> 13) & !1) | i64::from((x & 0x3ffe) != 0);
                let (l, r) = if upper >= major {
                    (quantize(mx), quantize(nx))
                } else {
                    (quantize(nx), quantize(mx))
                };
                *edge = (l, r);
                left = left.min(l >> 3);
                right = right.max(r >> 3);
            }
            let start = left.max(0).max(i64::from(sx)) as usize;
            let end = (right + 1).max(0).min(width as i64).min(i64::from(sx + sw)) as usize;
            for x in start..end {
                let mut mask = 0u8;
                for (sub, &(l, r)) in edges.iter().enumerate() {
                    for sample in 0..2 {
                        let sample_x = x as i64 * 8 + (sub as i64 & 1) * 2 + sample * 4;
                        if l <= sample_x && sample_x < r {
                            mask |= 1 << (7 - sub * 2 - sample as usize);
                        }
                    }
                }
                if mask == 0 || (!mode.antialias && mask & 0x80 == 0) {
                    continue;
                }
                let px = x as f32;
                let py = y as f32;
                let edge = |a: [f32; 2], b: [f32; 2]| {
                    (b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0])
                };
                let denom = (v[1].position[0] - v[0].position[0])
                    * (v[2].position[1] - v[0].position[1])
                    - (v[1].position[1] - v[0].position[1]) * (v[2].position[0] - v[0].position[0]);
                let weights = [
                    edge(v[1].position, v[2].position),
                    edge(v[2].position, v[0].position),
                    edge(v[0].position, v[1].position),
                ];
                let shade = std::array::from_fn(|i| {
                    (0..3).map(|j| weights[j] * v[j].color[i] / denom).sum()
                });
                pixel(
                    y * width + x,
                    mask.count_ones() as u8,
                    combiner.color(shade).unwrap(),
                    shade[3],
                );
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn color_known(
    mode: crate::hle::RenderMode,
    alpha: f32,
    count: u8,
    coverage: Option<u8>,
    memory_rgb: bool,
) -> bool {
    let mux = mode.blender_mux;
    let p = (mux >> 14) & 3;
    let m = (mux >> 6) & 3;
    let b = (mux >> 2) & 3;
    if p == 1 && !memory_rgb {
        return false;
    }
    let reject = (mux >> 10) & 3 == 0 && b == 0 && alpha >= 1.0;
    let blend =
        mode.force_blend || (mode.antialias && coverage.is_none_or(|c| (count + c) & 8 == 0));
    if !blend || reject {
        return true;
    }
    if m == 1 && !memory_rgb {
        return false;
    }
    if coverage.is_none() {
        return b != 1 && (mode.force_blend || (p == m && b == 0));
    }
    true
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn blend(
    mode: crate::hle::RenderMode,
    combiner: &Combiner,
    rgba: [f32; 4],
    memory: [f32; 4],
    shade_alpha: f32,
    count: u8,
    coverage: u8,
) -> [u8; 4] {
    let mux = mode.blender_mux;
    let source = |index: u16| match index & 3 {
        0 => rgba,
        1 => memory,
        2 => combiner.blend,
        _ => combiner.fog,
    };
    let p = source(mux >> 14);
    let m = source(mux >> 6);
    let a = match (mux >> 10) & 3 {
        0 => rgba[3],
        1 => combiner.fog[3],
        2 => shade_alpha,
        _ => 0.0,
    };
    let b = match (mux >> 2) & 3 {
        0 => 1.0 - a,
        1 => f32::from(coverage) / 8.0,
        2 => 1.0,
        _ => 0.0,
    };
    let blend = mode.force_blend || (mode.antialias && (count + coverage) & 8 == 0);
    let reject = (mux >> 10) & 3 == 0 && (mux >> 2) & 3 == 0 && rgba[3] >= 1.0;
    let rgb: [f32; 3] = std::array::from_fn(|i| {
        if !blend || reject {
            p[i]
        } else if mode.force_blend {
            p[i] * a + m[i] * b
        } else {
            (p[i] * a + m[i] * b) / (a + b).max(1.0 / 255.0)
        }
    });
    [rgb[0], rgb[1], rgb[2], rgba[3]].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn draw() -> CoverageDraw {
        let uniform = crate::render::CombinerUniform::fill_rect(0xffffffff, 2);
        let mode = crate::hle::RenderMode::default();
        CoverageDraw {
            combiner: Combiner::new(&uniform, mode),
            mode: Some(mode),
            geometry: CoverageGeometry::Triangles(vec![[0, 1, 2]]),
            scissor: (0, 0, 1, 1),
            ..Default::default()
        }
    }

    #[test]
    fn writeback_native_rejects_variable_alpha_and_viscvg() {
        let mut draw = draw();
        let combiner = draw.combiner.as_mut().unwrap();
        combiner.selectors.aa = AlphaIn::One;
        combiner.selectors.ab = AlphaIn::Shade;
        combiner.selectors.ac = AlphaIn::Shade;
        combiner.selectors.ad = AlphaIn::Zero;
        let vertices =
            [([0.0, 0.0], 0.0), ([1.0, 0.0], 0.0), ([0.0, 1.0], 1.0)].map(|(position, alpha)| {
                Some(Vertex {
                    position,
                    color: [1.0, 1.0, 1.0, alpha],
                })
            });
        assert!(!admitted(&draw, &vertices));
        let mode = crate::hle::RenderMode {
            blend_class: crate::hle::BlendClass::BlendColor,
            ..Default::default()
        };
        assert!(Combiner::new(
            &crate::render::CombinerUniform::fill_rect(0xffffffff, 2),
            mode
        )
        .is_none());
    }

    #[test]
    fn writeback_native_culls_and_uses_the_rdp_acceptance_sample() {
        let mut draw = draw();
        let vertices = [[0.25, 0.25], [0.75, 0.25], [0.5, 0.75]].map(|position| {
            Some(Vertex {
                position,
                color: [1.0; 4],
            })
        });
        let mut counts = Vec::new();
        assert!(admitted(&draw, &vertices));
        rasterize(&draw, &vertices, (1, 1), |_, count, _, _| {
            counts.push(count)
        });
        assert!(counts.is_empty());
        draw.mode.as_mut().unwrap().antialias = true;
        assert!(admitted(&draw, &vertices));
        rasterize(&draw, &vertices, (1, 1), |_, count, _, _| {
            counts.push(count)
        });
        assert_eq!(counts, [2]);
        counts.clear();
        draw.cull = true;
        assert!(admitted(&draw, &vertices));
        rasterize(&draw, &vertices, (1, 1), |_, count, _, _| {
            counts.push(count)
        });
        assert!(counts.is_empty());
        draw.geometry = CoverageGeometry::Triangles(vec![[2, 1, 0]]);
        assert!(admitted(&draw, &vertices));
        rasterize(&draw, &vertices, (1, 1), |_, count, _, _| {
            counts.push(count)
        });
        assert_eq!(counts, [2]);
    }

    #[test]
    fn writeback_native_shade_uses_guest_integer_coordinates() {
        let mut draw = draw();
        draw.combiner.as_mut().unwrap().selectors.cd = ColorIn::Shade;
        let vertices = [
            ([-4.0, -4.0], 0.0),
            ([12.0, -4.0], 1.0),
            ([-4.0, 12.0], 0.0),
        ]
        .map(|(position, red)| {
            Some(Vertex {
                position,
                color: [red, 0.0, 0.0, 1.0],
            })
        });
        let mut colors = Vec::new();
        assert!(admitted(&draw, &vertices));
        rasterize(&draw, &vertices, (1, 1), |_, count, color, _| {
            assert_eq!(count, 8);
            colors.push(color.map(|c| (c * 255.0).round() as u8));
        });
        assert_eq!(colors, [[64, 0, 0, 255]]);
    }

    #[test]
    fn writeback_native_constant_alpha_blend_and_coverage_decisions() {
        let mut mode = crate::hle::decode_render_mode(
            n64_gbi::consts::gbl_c1(
                n64_gbi::consts::CLR_IN,
                n64_gbi::consts::A_IN,
                n64_gbi::consts::CLR_MEM,
                n64_gbi::consts::B_1MA,
            ),
            0,
            0,
        );
        mode.antialias = true;
        let draw = draw();
        let combiner = draw.combiner.unwrap();
        let source = [64.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0, 128.0 / 255.0];
        assert_eq!(
            blend(mode, &combiner, source, [1.0, 0.0, 0.0, 1.0], 1.0, 3, 0),
            [159, 64, 96, 128]
        );
        assert_eq!(
            blend(mode, &combiner, source, [1.0, 0.0, 0.0, 1.0], 1.0, 3, 7),
            [64, 128, 192, 128]
        );
        mode.force_blend = true;
        assert_eq!(
            blend(mode, &combiner, source, [1.0, 0.0, 0.0, 1.0], 1.0, 3, 7),
            [159, 64, 96, 128]
        );
        assert_eq!(
            [1, 2, 3, 4, 5, 6, 7, 8].map(|n| destination(mode, n, 7)),
            [7; 8]
        );
    }
}
