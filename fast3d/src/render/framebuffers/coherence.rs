// Browser devices cannot synchronously map guest write-back buffers.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use super::ImageLayout;
use crate::FramebufferEncoding;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Writer {
    pub address: u64,
    pub layout: ImageLayout,
    pub depth: bool,
}

impl FramebufferEncoding {
    fn encode_u16(self, word: u16) -> [u8; 2] {
        match self {
            Self::Native => word.to_ne_bytes(),
            Self::BigEndian => word.to_be_bytes(),
        }
    }

    fn decode_u16(self, bytes: [u8; 2]) -> u16 {
        match self {
            Self::Native => u16::from_ne_bytes(bytes),
            Self::BigEndian => u16::from_be_bytes(bytes),
        }
    }
}

impl Writer {
    pub fn supported(self) -> bool {
        self.depth || matches!((self.layout.fmt, self.layout.siz), (0, 2) | (2..=4, 1))
    }

    pub fn pixel_bytes(self) -> usize {
        ((4usize << self.layout.siz) / 8).max(1)
    }
    fn byte_offset(self, pixel: usize) -> u64 {
        let width = self.layout.width as usize;
        (pixel / width) as u64 * self.layout.row_bytes().unwrap()
            + (pixel % width) as u64 * (4u64 << self.layout.siz) / 8
    }

    pub fn encode(
        self,
        rgba: [u8; 4],
        coverage: u8,
        pixel: usize,
        literal: Option<u32>,
        encoding: FramebufferEncoding,
    ) -> [u8; 2] {
        if !self.supported() {
            return [0; 2];
        }
        if let Some(word) = literal {
            return if self.pixel_bytes() == 2 {
                encoding.encode_u16((word >> (16 * (1 - pixel % 2))) as u16)
            } else {
                [(word >> (8 * (3 - pixel % 4))) as u8, 0]
            };
        }
        if self.depth {
            encoding.encode_u16(super::pack_depth(f32::from_ne_bytes(rgba)))
        } else if self.pixel_bytes() == 1 {
            [rgba[(pixel % self.layout.width as usize) & 1], 0]
        } else {
            const MAGIC: [u8; 16] = [0, 6, 1, 7, 4, 2, 5, 3, 3, 5, 2, 4, 7, 1, 6, 0];
            let x = pixel % self.layout.width as usize;
            let y = pixel / self.layout.width as usize;
            encoding.encode_u16(pack_color(rgba, coverage, MAGIC[(y & 3) * 4 + (x & 3)]))
        }
    }
}

fn pack_color(rgba: [u8; 4], coverage: u8, dither: u8) -> u16 {
    debug_assert!(coverage <= 7);
    let channel = |i: usize| u16::from(rgba[i].saturating_add(dither) >> 3);
    (channel(0) << 11) | (channel(1) << 6) | (channel(2) << 1) | u16::from(coverage >> 2)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::render) struct CoverageDraw {
    pub(in crate::render) literal: Option<u32>,
    pub(in crate::render) combiner: Option<super::writeback::Combiner>,
    pub(in crate::render) cull: bool,
    pub(in crate::render) mode: Option<crate::hle::RenderMode>,
    pub(in crate::render) geometry: CoverageGeometry,
    pub(in crate::render) scissor: (u32, u32, u32, u32),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::render) enum CoverageGeometry {
    #[default]
    Unknown,
    Triangles(Vec<[u32; 3]>),
    Rectangle([f32; 4]),
}

impl CoverageDraw {
    fn possible_bounds(&self, vertices: &[Option<[f32; 2]>]) -> Option<(u32, u32, u32, u32)> {
        let bounds = match &self.geometry {
            CoverageGeometry::Unknown => return None,
            CoverageGeometry::Rectangle(bounds) => *bounds,
            CoverageGeometry::Triangles(triangles) => {
                let mut bounds = [
                    f32::INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                ];
                for &index in triangles.iter().flatten() {
                    let Some(Some([x, y])) = vertices.get(index as usize).copied() else {
                        return Some(self.scissor);
                    };
                    bounds = [
                        bounds[0].min(x),
                        bounds[1].min(y),
                        bounds[2].max(x),
                        bounds[3].max(y),
                    ];
                }
                [
                    bounds[0] - 0.125,
                    bounds[1] - 0.125,
                    bounds[2] + 0.125,
                    bounds[3] + 0.125,
                ]
            }
        };
        let (x, y, w, h) = self.scissor;
        let left = (bounds[0].floor().max(0.0) as u32).max(x);
        let top = (bounds[1].floor().max(0.0) as u32).max(y);
        let right = (bounds[2].ceil().max(0.0) as u32).min(x + w);
        let bottom = (bounds[3].ceil().max(0.0) as u32).min(y + h);
        Some((
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        ))
    }

    fn coverage_mode(&self) -> CoverageMode {
        let Some(mode) = self.mode else {
            return CoverageMode::Unknown;
        };
        if mode.cycle_type >= 2 {
            return CoverageMode::Unknown;
        }
        if mode.cvg_dst == 2 || (mode.cvg_dst == 3 && !mode.image_read) {
            return CoverageMode::Full;
        }
        if mode.cvg_x_alpha
            || (mode.z_test && mode.z_mode == crate::hle::ZMode::Inter)
            || mode.cvg_dst == 3
            || (mode.cvg_dst == 1 && mode.image_read)
        {
            return CoverageMode::Unknown;
        }
        CoverageMode::Interior
    }

    fn stored_coverage(
        &self,
        extent: (usize, usize),
        vertices: &[Option<[f32; 2]>],
    ) -> StoredCoverage {
        match self.coverage_mode() {
            CoverageMode::Unknown => return StoredCoverage::Unknown,
            CoverageMode::Full => return StoredCoverage::Full,
            CoverageMode::Interior => {}
        }
        let CoverageGeometry::Triangles(triangles) = &self.geometry else {
            return match self.geometry {
                CoverageGeometry::Rectangle(bounds) => StoredCoverage::Rectangle(bounds),
                _ => StoredCoverage::Unknown,
            };
        };
        let (width, height) = extent;
        let mut pixels = vec![0u8; width * height];
        for triangle in triangles {
            let [a, b, c] = triangle.map(|i| vertices[i as usize]);
            let (Some(a), Some(b), Some(c)) = (a, b, c) else {
                return StoredCoverage::Unknown;
            };
            let edge = |a: [f32; 2], b: [f32; 2], p: [f32; 2]| {
                (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
            };
            let area = edge(a, b, c);
            if area == 0.0 {
                continue;
            }
            let edges = [(a, b), (b, c), (c, a)];
            // Offset edge half-planes also retain partial overlaps beyond acute triangle tips.
            let lines = edges.map(|(a, b)| {
                let dx = f64::from(b[0]) - f64::from(a[0]);
                let dy = f64::from(b[1]) - f64::from(a[1]);
                let sign = f64::from(area.signum());
                let x = -dy * sign;
                let y = dx * sign;
                let offset = |v: f64| v * if v >= 0.0 { 1.125 } else { -0.125 };
                [
                    x,
                    y,
                    (dy * f64::from(a[0]) - dx * f64::from(a[1])) * sign + offset(x) + offset(y),
                ]
            });
            let outer = [(0, 1), (1, 2), (2, 0)].map(|(i, j)| {
                let [a, b, c] = lines[i];
                let [d, e, f] = lines[j];
                let det = a * e - d * b;
                [(b * f - e * c) / det, (c * d - f * a) / det]
            });
            let bounds = if outer.iter().flatten().all(|v| v.is_finite()) {
                [
                    outer
                        .iter()
                        .map(|p| p[0])
                        .fold(f64::INFINITY, f64::min)
                        .floor()
                        - 1.0,
                    outer
                        .iter()
                        .map(|p| p[1])
                        .fold(f64::INFINITY, f64::min)
                        .floor()
                        - 1.0,
                    outer
                        .iter()
                        .map(|p| p[0])
                        .fold(f64::NEG_INFINITY, f64::max)
                        .ceil()
                        + 1.0,
                    outer
                        .iter()
                        .map(|p| p[1])
                        .fold(f64::NEG_INFINITY, f64::max)
                        .ceil()
                        + 1.0,
                ]
            } else {
                [0.0, 0.0, width as f64, height as f64]
            };
            let (sx, sy, sw, sh) = self.scissor;
            let left = (bounds[0].max(0.0) as usize).max(sx as usize);
            let top = (bounds[1].max(0.0) as usize).max(sy as usize);
            let right = (bounds[2].max(0.0) as usize)
                .min(width)
                .min((sx + sw) as usize);
            let bottom = (bounds[3].max(0.0) as usize)
                .min(height)
                .min((sy + sh) as usize);
            for y in top..bottom {
                for x in left..right {
                    let x = x as f32;
                    let y = y as f32;
                    let corners = [
                        [x - 0.125, y - 0.125],
                        [x + 1.125, y - 0.125],
                        [x - 0.125, y + 1.125],
                        [x + 1.125, y + 1.125],
                    ];
                    let distances =
                        edges.map(|(a, b)| corners.map(|p| edge(a, b, p) * area.signum()));
                    if distances.iter().any(|edge| edge.iter().all(|&d| d < 0.0)) {
                        continue;
                    }
                    let pixel = y as usize * width + x as usize;
                    // Keep a margin around HLE edges; partial samples need the native span walk.
                    if distances.iter().all(|edge| edge.iter().all(|&d| d > 0.0)) {
                        pixels[pixel] |= 1;
                    } else {
                        pixels[pixel] |= 2;
                    }
                }
            }
        }
        StoredCoverage::Pixels(pixels)
    }

    #[cfg(test)]
    fn reference_coverage(
        &self,
        pixel: usize,
        width: usize,
        vertices: &[Option<[f32; 2]>],
    ) -> Option<u8> {
        let mode = self.mode?;
        if mode.cycle_type >= 2 {
            return None;
        }
        if mode.cvg_dst == 2 || (mode.cvg_dst == 3 && !mode.image_read) {
            return Some(7);
        }
        if mode.cvg_x_alpha
            || (mode.z_test && mode.z_mode == crate::hle::ZMode::Inter)
            || mode.cvg_dst == 3
            || (mode.cvg_dst == 1 && mode.image_read)
        {
            return None;
        }
        let x = (pixel % width) as f32;
        let y = (pixel / width) as f32;
        let full = match &self.geometry {
            CoverageGeometry::Rectangle([left, top, right, bottom]) => {
                x >= *left && y >= *top && x + 1.0 <= *right && y + 1.0 <= *bottom
            }
            CoverageGeometry::Triangles(triangles) => {
                let mut full = false;
                for triangle in triangles {
                    let [a, b, c] = triangle.map(|i| vertices[i as usize]);
                    let (Some(a), Some(b), Some(c)) = (a, b, c) else {
                        return None;
                    };
                    let edge = |a: [f32; 2], b: [f32; 2], p: [f32; 2]| {
                        (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
                    };
                    let area = edge(a, b, c);
                    if area == 0.0 {
                        continue;
                    }
                    let corners = [
                        [x - 0.125, y - 0.125],
                        [x + 1.125, y - 0.125],
                        [x - 0.125, y + 1.125],
                        [x + 1.125, y + 1.125],
                    ];
                    let distances = [(a, b), (b, c), (c, a)]
                        .map(|(a, b)| corners.map(|p| edge(a, b, p) * area.signum()));
                    if distances.iter().any(|edge| edge.iter().all(|&d| d < 0.0)) {
                        continue;
                    }
                    // Keep a margin around HLE edges; partial samples need the native span walk.
                    if !distances.iter().all(|edge| edge.iter().all(|&d| d > 0.0)) {
                        return None;
                    }
                    full = true;
                }
                full
            }
            CoverageGeometry::Unknown => false,
        };
        full.then_some(7)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CoverageMode {
    Unknown,
    Full,
    Interior,
}

enum StoredCoverage {
    Unknown,
    Full,
    Rectangle([f32; 4]),
    Pixels(Vec<u8>),
}

impl StoredCoverage {
    fn get(&self, pixel: usize, width: usize) -> Option<u8> {
        let full = match self {
            Self::Unknown => false,
            Self::Full => true,
            Self::Rectangle([left, top, right, bottom]) => {
                let x = (pixel % width) as f32;
                let y = (pixel / width) as f32;
                x >= *left && y >= *top && x + 1.0 <= *right && y + 1.0 <= *bottom
            }
            Self::Pixels(pixels) => pixels[pixel] == 1,
        };
        full.then_some(7)
    }
}

pub(crate) struct OwnedRange {
    pub writer: Writer,
    pub generation: u64,
    pub bytes: Vec<u8>,
    /// The guest changed these bytes after `writer` drew them: they are plain guest memory to
    /// texture loads, kept here so the writer's views can import and restore them.
    pub foreign: bool,
}

impl OwnedRange {
    fn new(writer: Writer, generation: u64, bytes: Vec<u8>) -> Self {
        Self {
            writer,
            generation,
            bytes,
            foreign: false,
        }
    }
}

#[derive(Default)]
pub(crate) struct Ownership(pub BTreeMap<u64, OwnedRange>);

impl Ownership {
    pub fn write(&mut self, writer: Writer, generation: u64, address: u64, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        let end = address + bytes.len() as u64;
        let overlaps: Vec<_> = self
            .0
            .range(..end)
            .rev()
            .take_while(|(start, old)| **start + old.bytes.len() as u64 > address)
            .map(|(start, _)| *start)
            .collect();
        for start in overlaps {
            let old = self.0.remove(&start).unwrap();
            let old_end = start + old.bytes.len() as u64;
            if start < address {
                self.0.insert(
                    start,
                    OwnedRange::new(
                        old.writer,
                        old.generation,
                        old.bytes[..(address - start) as usize].to_vec(),
                    ),
                );
            }
            if old_end > end {
                self.0.insert(
                    end,
                    OwnedRange::new(
                        old.writer,
                        old.generation,
                        old.bytes[(end - start) as usize..].to_vec(),
                    ),
                );
            }
        }
        let mut address = address;
        let mut bytes = bytes;
        if let Some((&start, old)) = self.0.range(..address).next_back() {
            if old.writer == writer
                && old.generation == generation
                && start + old.bytes.len() as u64 == address
            {
                let mut previous = self.0.remove(&start).unwrap().bytes;
                previous.extend(bytes);
                bytes = previous;
                address = start;
            }
        }
        if self
            .0
            .get(&end)
            .is_some_and(|next| next.writer == writer && next.generation == generation)
        {
            bytes.extend(self.0.remove(&end).unwrap().bytes);
        }
        self.0
            .insert(address, OwnedRange::new(writer, generation, bytes));
    }

    /// Record guest `bytes` at `address`, inside one owned range, as foreign to its writer.
    fn mark_foreign(&mut self, address: u64, bytes: Vec<u8>) {
        let start = *self.0.range(..=address).next_back().unwrap().0;
        let old = self.0.remove(&start).unwrap();
        let end = address + bytes.len() as u64;
        let part = |bytes: &[u8], foreign| OwnedRange {
            foreign,
            ..OwnedRange::new(old.writer, old.generation, bytes.to_vec())
        };
        if start < address {
            self.0.insert(
                start,
                part(&old.bytes[..(address - start) as usize], old.foreign),
            );
        }
        if start + (old.bytes.len() as u64) > end {
            self.0
                .insert(end, part(&old.bytes[(end - start) as usize..], old.foreign));
        }
        self.0.insert(address, part(&bytes, true));
    }

    /// Merge adjacent ranges one target wrote since `first_generation`; their write order is settled.
    fn coalesce(&mut self, first_generation: u64) {
        let mut merged = BTreeMap::new();
        let mut open: Option<(u64, OwnedRange)> = None;
        for (address, range) in std::mem::take(&mut self.0) {
            match &mut open {
                Some((start, current))
                    if current.writer == range.writer
                        && current.generation >= first_generation
                        && range.generation >= first_generation
                        && *start + current.bytes.len() as u64 == address =>
                {
                    current.generation = current.generation.max(range.generation);
                    current.bytes.extend(range.bytes);
                }
                _ => {
                    if let Some((start, current)) = open.replace((address, range)) {
                        merged.insert(start, current);
                    }
                }
            }
        }
        if let Some((start, current)) = open {
            merged.insert(start, current);
        }
        self.0 = merged;
    }

    pub fn patches(&self, first_generation: u64) -> Vec<crate::GuestWrite> {
        self.0
            .iter()
            .filter(|(_, range)| {
                range.generation >= first_generation && range.writer.supported() && !range.foreign
            })
            .map(|(&address, range)| crate::GuestWrite {
                address,
                bytes: range.bytes.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writeback_coverage_is_independent_of_gpu_alpha() {
        for alpha in [0, 4, 128, 255] {
            for (coverage, expected) in [(0, 0x4430), (3, 0x4430), (4, 0x4431), (7, 0x4431)] {
                assert_eq!(
                    pack_color([64, 128, 192, alpha], coverage, 0),
                    expected,
                    "alpha={alpha}, coverage={coverage}"
                );
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn writeback_native_rejected_sample_preserves_ownership() {
        let mut coherence = Coherence::new(FramebufferEncoding::BigEndian);
        let writer = image(0, 1, 2, false);
        coherence.fill(writer, (0, 0, 1, 1), 0x12311231);
        coherence.commit_pending(u64::MAX);
        let mode = crate::hle::RenderMode::default();
        let draw = CoverageDraw {
            combiner: super::super::writeback::Combiner::new(
                &crate::render::CombinerUniform::fill_rect(0xffffffff, 2),
                mode,
            ),
            mode: Some(mode),
            geometry: CoverageGeometry::Triangles(vec![[0, 1, 2]]),
            scissor: (0, 0, 1, 1),
            ..Default::default()
        };
        let vertices = [[0.25, 0.25], [0.75, 0.25], [0.5, 0.75]].map(|position| {
            Some(super::super::writeback::Vertex {
                position,
                color: [1.0; 4],
            })
        });
        let stamps: [u8; 8] = bytemuck::cast([1u32, 0]);
        let geometry = ReplayGeometry::new(std::slice::from_ref(&draw), &vertices);
        let output = coherence.replay(writer, &[draw], (0, 0), &geometry, &stamps, &[255; 4]);
        assert!(matches!(output[0], super::super::writeback::Pixel::Keep));
        assert_eq!(coherence.owners.0[&0].bytes, [0x12, 0x31]);
        assert_eq!(coherence.stored_coverage_code(0), Some(7));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn writeback_native_full_recovers_unknown_history_before_wrap() {
        let mut coherence = Coherence::new(FramebufferEncoding::BigEndian);
        let writer = image(0, 1, 2, false);
        let mode = crate::hle::RenderMode {
            image_read: true,
            cvg_dst: 2,
            ..Default::default()
        };
        let combiner = super::super::writeback::Combiner::new(
            &crate::render::CombinerUniform::fill_rect(0xffffffff, 2),
            mode,
        );
        let full = CoverageDraw {
            combiner: combiner.clone(),
            mode: Some(mode),
            geometry: CoverageGeometry::Rectangle([0.0, 0.0, 1.0, 1.0]),
            scissor: (0, 0, 1, 1),
            ..Default::default()
        };
        let wrap = CoverageDraw {
            combiner,
            mode: Some(crate::hle::RenderMode {
                antialias: true,
                cvg_dst: 1,
                ..mode
            }),
            geometry: CoverageGeometry::Rectangle([0.25, 0.0, 0.75, 1.0]),
            scissor: (0, 0, 1, 1),
            ..Default::default()
        };
        let stamps: [u8; 8] = bytemuck::cast([1u32, 0]);
        let draws = [full, wrap];
        let geometry = ReplayGeometry::new(&draws, &[]);
        let output = coherence.replay(writer, &draws, (0, 0), &geometry, &stamps, &[255; 4]);
        assert!(matches!(
            output[0],
            super::super::writeback::Pixel::Write([255, 255, 255, 255], 3, 2)
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn writeback_native_unknown_draw_does_not_reuse_old_rgb() {
        let mut coherence = Coherence::new(FramebufferEncoding::BigEndian);
        let writer = image(0, 1, 2, false);
        coherence.fill(writer, (0, 0, 1, 1), 0xf801f801);
        coherence.commit_pending(u64::MAX);
        let mode = crate::hle::decode_render_mode(
            n64_gbi::consts::FORCE_BL
                | n64_gbi::consts::gbl_c1(
                    n64_gbi::consts::CLR_IN,
                    n64_gbi::consts::A_IN,
                    n64_gbi::consts::CLR_MEM,
                    n64_gbi::consts::B_1MA,
                ),
            0,
            0,
        );
        let uniform = crate::render::CombinerUniform::fill_rect(0x003f003f, 2);
        let mut uniform = uniform;
        uniform.prim[3] = 128.0 / 255.0;
        let unsupported = CoverageDraw {
            geometry: CoverageGeometry::Rectangle([0.0, 0.0, 1.0, 1.0]),
            scissor: (0, 0, 1, 1),
            ..Default::default()
        };
        let draw = CoverageDraw {
            combiner: super::super::writeback::Combiner::new(&uniform, mode),
            mode: Some(mode),
            geometry: CoverageGeometry::Rectangle([0.0, 0.0, 1.0, 1.0]),
            scissor: (0, 0, 1, 1),
            ..Default::default()
        };
        let stamps: [u8; 8] = bytemuck::cast([2u32, 0]);
        let draws = [unsupported, draw];
        let geometry = ReplayGeometry::new(&draws, &[]);
        let output = coherence.replay(
            writer,
            &draws,
            (0, 0),
            &geometry,
            &stamps,
            &[0, 128, 128, 128],
        );
        assert!(matches!(
            output[0],
            super::super::writeback::Pixel::Write([0, 128, 128, 128], 7, 2)
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn writeback_native_unknown_footprint_invalidates_queued_fill_bits() {
        let mut coherence = Coherence::new(FramebufferEncoding::BigEndian);
        let writer = image(0, 1, 2, false);
        coherence.fill(writer, (0, 0, 1, 1), 0x12311231);
        let draw = CoverageDraw {
            geometry: CoverageGeometry::Rectangle([0.0, 0.0, 1.0, 1.0]),
            scissor: (0, 0, 1, 1),
            ..Default::default()
        };
        let stamps: [u8; 8] = bytemuck::cast([0u32, 0]);
        let geometry = ReplayGeometry::new(std::slice::from_ref(&draw), &[]);
        coherence.replay(writer, &[draw], (1, 0), &geometry, &stamps, &[0; 4]);
        coherence.commit_pending(u64::MAX);
        assert_eq!(coherence.stored_coverage_code(0), None);
        assert_eq!(coherence.owners.0[&0].bytes, [0x12, 0x31]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn writeback_native_fallback_proof_keeps_known_pixels_and_literal_resets() {
        for destination in [1, 3] {
            for height in [1, 2] {
                for reset in [false, true] {
                    for literal in [false, true] {
                        let mut coherence = Coherence::new(FramebufferEncoding::BigEndian);
                        let writer = image(0, 2, 2, false);
                        coherence.fill(writer, (0, 0, 2, 2), 0x12311231);
                        coherence.commit_pending(u64::MAX);
                        let mode = crate::hle::RenderMode {
                            image_read: true,
                            cvg_dst: destination,
                            ..Default::default()
                        };
                        let native = CoverageDraw {
                            combiner: super::super::writeback::Combiner::new(
                                &crate::render::CombinerUniform::fill_rect(0xffffffff, 2),
                                mode,
                            ),
                            literal: literal.then_some(0x12311231),
                            mode: Some(mode),
                            geometry: CoverageGeometry::Rectangle([0.0, 0.0, 2.0, 2.0]),
                            scissor: (0, 0, 2, 2),
                            ..Default::default()
                        };
                        let mut draws = vec![CoverageDraw {
                            geometry: CoverageGeometry::Rectangle([0.0, 0.0, 2.0, height as f32]),
                            scissor: (0, 0, 2, 2),
                            ..Default::default()
                        }];
                        if reset {
                            draws.push(CoverageDraw {
                                literal: Some(0x12311231),
                                combiner: None,
                                ..native.clone()
                            });
                        }
                        draws.push(native);
                        let geometry = ReplayGeometry::new(&draws, &[]);
                        let fallback = geometry.fallback_only(&draws, 0, (2, 2));
                        let stamps: Vec<_> = [draws.len() as u32, 0]
                            .into_iter()
                            .flat_map(u32::to_ne_bytes)
                            .cycle()
                            .take(32)
                            .collect();
                        let output = coherence.replay(
                            writer,
                            &draws,
                            (1, 0),
                            &geometry,
                            &stamps,
                            &[255; 16],
                        );
                        assert_eq!(fallback, height == 2 && !reset && !literal);
                        if fallback {
                            assert!(output
                                .iter()
                                .all(|p| matches!(p, super::super::writeback::Pixel::Fallback)));
                        } else {
                            assert!(output
                                .iter()
                                .any(|p| matches!(p, super::super::writeback::Pixel::Write(..))));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn writeback_native_hidden_bits_follow_byte_aliases_and_cpu_edits() {
        let mut coherence = Coherence::new(FramebufferEncoding::BigEndian);
        let writer = image(0, 2, 2, false);
        coherence.fill(writer, (0, 0, 2, 1), 0x12311231);
        coherence.commit_pending(u64::MAX);
        coherence
            .release_changed(&crate::RdramImage::new(&[0x12, 0x30, 0x12, 0x31]))
            .unwrap();
        assert_eq!(coherence.stored_coverage_code(0), None);
        assert_eq!(coherence.stored_coverage_code(2), Some(7));
        coherence.fill(writer, (0, 0, 2, 1), 0x12311231);
        coherence.commit_pending(u64::MAX);
        coherence.fill(image(1, 1, 1, false), (0, 0, 1, 1), 0xff000000);
        coherence.commit_pending(u64::MAX);
        assert_eq!(coherence.stored_coverage_code(0), None);
        assert_eq!(coherence.stored_coverage_code(2), Some(7));
    }

    #[test]
    fn writeback_coverage_limits_known_destinations() {
        use crate::hle::decode_render_mode;
        use n64_gbi::consts::*;
        let rectangle = CoverageGeometry::Rectangle([2.25, 2.25, 8.0, 8.0]);
        for (low, pixel, expected) in [
            (0, 3 * 16 + 3, Some(7)),
            (0, 2 * 16 + 2, None),
            (CVG_DST_FULL, 2 * 16 + 2, Some(7)),
            (CVG_DST_WRAP, 3 * 16 + 3, Some(7)),
            (CVG_DST_WRAP | IM_RD, 3 * 16 + 3, None),
            (CVG_DST_SAVE, 2 * 16 + 2, Some(7)),
            (CVG_DST_SAVE | IM_RD, 3 * 16 + 3, None),
            (CVG_X_ALPHA, 3 * 16 + 3, None),
            (Z_CMP | ZMODE_INTER, 3 * 16 + 3, None),
        ] {
            let draw = CoverageDraw {
                mode: Some(decode_render_mode(low, 0, 0)),
                geometry: rectangle.clone(),
                scissor: (0, 0, 16, 16),
                ..Default::default()
            };
            assert_eq!(
                draw.stored_coverage((16, 16), &[]).get(pixel, 16),
                expected,
                "mode={low:08x}"
            );
        }
        for cycle in [2, 3] {
            let draw = CoverageDraw {
                mode: Some(decode_render_mode(CVG_DST_FULL, cycle << 20, 0)),
                geometry: rectangle.clone(),
                scissor: (0, 0, 16, 16),
                ..Default::default()
            };
            assert_eq!(
                draw.stored_coverage((16, 16), &[]).get(3 * 16 + 3, 16),
                None
            );
        }
    }

    #[test]
    fn writeback_cached_interiors_match_pixel_proofs() {
        let mut seed = 1u32;
        for case in 0..256 {
            let mut vertices: Vec<_> = (0..9)
                .map(|_| {
                    Some(std::array::from_fn(|_| {
                        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                        (seed % 384) as f32 / 8.0 - 8.0
                    }))
                })
                .collect();
            if case % 16 == 0 {
                vertices[8] = None;
            }
            let draw = CoverageDraw {
                mode: Some(crate::hle::RenderMode::default()),
                geometry: CoverageGeometry::Triangles(vec![[0, 1, 2], [3, 4, 5], [6, 7, 8]]),
                scissor: (0, 0, 32, 32),
                ..Default::default()
            };
            let stored = draw.stored_coverage((32, 32), &vertices);
            for pixel in 0..32 * 32 {
                assert_eq!(
                    stored.get(pixel, 32),
                    draw.reference_coverage(pixel, 32, &vertices),
                    "case={case}, pixel={pixel}, vertices={vertices:?}"
                );
            }
        }
    }

    #[test]
    fn writeback_partial_triangle_overlap_stays_unknown() {
        let vertices = [
            Some([0.0, 0.0]),
            Some([16.0, 0.0]),
            Some([0.0, 16.0]),
            Some([3.0, 0.0]),
        ];
        let mut draw = CoverageDraw {
            mode: Some(crate::hle::RenderMode::default()),
            geometry: CoverageGeometry::Triangles(vec![[0, 1, 2]]),
            scissor: (0, 0, 16, 16),
            ..Default::default()
        };
        assert_eq!(
            draw.stored_coverage((16, 16), &vertices)
                .get(2 * 16 + 2, 16),
            Some(7)
        );
        assert_eq!(
            draw.stored_coverage((16, 16), &vertices)
                .get(8 * 16 + 8, 16),
            None
        );
        draw.geometry = CoverageGeometry::Triangles(vec![[0, 1, 2], [0, 3, 2]]);
        assert_eq!(
            draw.stored_coverage((16, 16), &vertices)
                .get(2 * 16 + 2, 16),
            None
        );
        let vertices = [
            Some([0.0, 0.0]),
            Some([16.0, 0.0]),
            Some([0.0, 16.0]),
            Some([2.25, 0.0]),
            Some([2.25, 16.0]),
        ];
        draw.geometry = CoverageGeometry::Triangles(vec![[0, 1, 2], [3, 1, 4]]);
        assert_eq!(
            draw.stored_coverage((16, 16), &vertices)
                .get(2 * 16 + 2, 16),
            None
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn writeback_coverage_follows_prefixes_and_unstamped_edges() {
        let (device, queue, _) = crate::render::headless_device();
        let texture = |format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        let color = texture(wgpu::TextureFormat::Rgba8Unorm);
        let stamps = texture(wgpu::TextureFormat::Rg32Uint);
        let upload = |texture: &wgpu::Texture, bytes: &[u8]| {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes.len() as u32),
                    rows_per_image: Some(1),
                },
                texture.size(),
            )
        };
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 3 * 48,
            usage: wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        for (color_writer, geometry, projected) in [
            (None, CoverageGeometry::Triangles(vec![[0, 1, 2]]), false),
            (
                Some(image(0x100, 1, 1, false)),
                CoverageGeometry::Triangles(vec![[0, 1, 2]]),
                false,
            ),
            (
                Some(image(0x100, 1, 2, false)),
                CoverageGeometry::Rectangle([0.0; 4]),
                false,
            ),
            (
                Some(image(0x100, 1, 2, false)),
                CoverageGeometry::Triangles(Vec::new()),
                false,
            ),
            (
                Some(image(0x100, 1, 2, false)),
                CoverageGeometry::Triangles(vec![[0, 1, 2]]),
                true,
            ),
        ] {
            let mut coherence = Coherence::default();
            let mut encoder = device.create_command_encoder(&Default::default());
            coherence.snapshot(
                &device,
                &mut encoder,
                &stamps,
                color_writer.map(|writer| (writer, &color)),
                Some((image(0x200, 1, 2, true), &color)),
                vec![CoverageDraw {
                    geometry,
                    mode: Some(crate::hle::RenderMode::default()),
                    ..Default::default()
                }],
                Some(&vertices),
                1,
                0,
            );
            assert_eq!(coherence.snapshots[0].vertices.is_some(), projected);
        }
        upload(&color, &[64, 128, 192, 0]);
        let draw = |bounds| CoverageDraw {
            mode: Some(crate::hle::RenderMode::default()),
            geometry: CoverageGeometry::Rectangle(bounds),
            scissor: (0, 0, 4, 1),
            ..Default::default()
        };
        let full = draw([0.0, 0.0, 1.0, 1.0]);
        let partial = draw([0.25, 0.0, 0.75, 1.0]);
        let disjoint = draw([2.25, 0.0, 3.0, 1.0]);
        let mut coherence = Coherence::new(FramebufferEncoding::BigEndian);
        let snapshot = |coherence: &mut Coherence, draws, slot: u32| {
            upload(&stamps, bytemuck::cast_slice(&[slot, 0]));
            coherence.begin_task();
            let mut encoder = device.create_command_encoder(&Default::default());
            coherence.snapshot(
                &device,
                &mut encoder,
                &stamps,
                Some((image(0x100, 1, 2, false), &color)),
                None,
                draws,
                None,
                1,
                0,
            );
            queue.submit([encoder.finish()]);
            coherence.finish(&device)
        };
        assert_eq!(
            snapshot(&mut coherence, vec![full.clone()], 1)[0].bytes,
            [0x44, 0x31]
        );
        assert_eq!(
            snapshot(&mut coherence, vec![full.clone(), partial.clone()], 1)[0].bytes,
            [0x44, 0x30]
        );
        assert_eq!(
            snapshot(&mut coherence, vec![full.clone(), disjoint], 1)[0].bytes,
            [0x44, 0x31]
        );
        assert!(snapshot(&mut coherence, vec![partial], 0).is_empty());
        assert_eq!(coherence.owners.0[&0x100].bytes, [0x44, 0x31]);
        use crate::render::inputs::{DrawInputs, OperationInputs, TargetInputs};
        let mut target = TargetInputs {
            color_image_epoch: 0,
            valid: true,
            id: crate::render::workload::TargetId::Guest(0x100),
            color_image: Default::default(),
            depth_image: None,
            pc: 0,
            logical_extent: (1, 1),
            output_extent: (1, 1),
            any_depth: false,
            depth_clear: false,
            operations: vec![
                OperationInputs {
                    literal_fill: None,
                    coverage_mode: full.mode,
                    coverage_rect: Some([0.0, 0.0, 1.0, 1.0]),
                    pc: 0,
                    scissor: (0, 0, 1, 1),
                    draw: DrawInputs::Rectangle {
                        material_index: None,
                        fb_source: None,
                        mode: Default::default(),
                    },
                },
                OperationInputs {
                    literal_fill: None,
                    coverage_mode: full.mode,
                    coverage_rect: Some([0.25, 0.0, 0.75, 1.0]),
                    pc: 0,
                    scissor: (0, 0, 1, 1),
                    draw: DrawInputs::Rejected,
                },
            ],
            uniforms: Vec::new(),
            rectangles: Vec::new().into(),
            rect_indices: Vec::new(),
        };
        assert_eq!(
            snapshot(
                &mut coherence,
                target.coverage_draws(&[true, true], 2, &[]),
                1
            )[0]
            .bytes,
            [0x44, 0x31]
        );
        target.operations[1].draw = DrawInputs::Rectangle {
            material_index: None,
            fb_source: None,
            mode: Default::default(),
        };
        assert_eq!(
            snapshot(
                &mut coherence,
                target.coverage_draws(&[true, false], 2, &[]),
                1
            )[0]
            .bytes,
            [0x44, 0x31]
        );
    }

    fn image(address: u64, width: u32, siz: u8, depth: bool) -> Writer {
        Writer {
            address,
            layout: ImageLayout {
                width,
                fmt: if siz == 1 { 4 } else { 0 },
                siz,
            },
            depth,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn canonical_literal_fill_red_green_bytes_and_reader() {
        let (device, _, _) = crate::render::headless_device();
        let mut coherence = Coherence::new(FramebufferEncoding::BigEndian);
        coherence.fill(image(0, 2, 2, false), (0, 0, 2, 1), 0xf801_07c1);
        coherence.resolve(&device);
        let patches = coherence.owners.patches(0);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].bytes, [0xf8, 0x01, 0x07, 0xc1]);
        let memory = crate::RdramImage::new(&patches[0].bytes);
        assert_eq!(crate::Rdram::read_u16(&memory, 0).unwrap(), 0xf801);
        assert_eq!(crate::Rdram::read_u16(&memory, 2).unwrap(), 0x07c1);
    }

    #[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
    #[test]
    fn native_literal_fill_red_green_bytes_and_reader() {
        let (device, _, _) = crate::render::headless_device();
        let mut coherence = Coherence::default();
        let mut words = [0u16; 2];
        let address = words.as_mut_ptr() as u64;
        coherence.fill(image(address, 2, 2, false), (0, 0, 2, 1), 0xf801_07c1);
        coherence.resolve(&device);
        let patches = coherence.owners.patches(0);
        assert_eq!(patches.len(), 1);
        #[cfg(target_endian = "little")]
        assert_eq!(patches[0].bytes, [0x01, 0xf8, 0xc1, 0x07]);
        #[cfg(target_endian = "big")]
        assert_eq!(patches[0].bytes, [0xf8, 0x01, 0x07, 0xc1]);
        unsafe {
            std::ptr::copy_nonoverlapping(patches[0].bytes.as_ptr(), words.as_mut_ptr().cast(), 4);
        }
        let range = address..address + 4;
        let memory = unsafe {
            crate::hle::host_mem::HostMemory::new(crate::HostRam::with_ranges(
                &[],
                std::slice::from_ref(&range),
            ))
        };
        assert_eq!(crate::Rdram::read_u16(&memory, address).unwrap(), 0xf801);
        assert_eq!(
            crate::Rdram::read_u16(&memory, address + 2).unwrap(),
            0x07c1
        );
        assert_eq!(words, [0xf801, 0x07c1]);
    }

    #[test]
    fn canonical_cpu_edits_import_split_bytes_and_restore() {
        let mut coherence = Coherence::new(FramebufferEncoding::BigEndian);
        let writer = image(0, 2, 2, false);
        coherence
            .owners
            .write(writer, 1, 0, vec![0xf8, 0x01, 0xf8, 0x01]);
        coherence
            .release_changed(&crate::RdramImage::new(&[0xf8, 0x01, 0x07, 0xc1]))
            .unwrap();
        assert_eq!(
            coherence.import_pixels(writer, 1, None),
            BTreeMap::from([(1, [0, 255, 0, 255])])
        );
        coherence.imports.clear();
        coherence
            .release_changed(&crate::RdramImage::new(&[0xf8, 0x01, 0x07, 0x3f]))
            .unwrap();
        assert_eq!(
            coherence.imports,
            vec![crate::GuestWrite {
                address: 3,
                bytes: vec![0x3f]
            }]
        );
        assert_eq!(
            coherence.import_pixels(writer, 1, None),
            BTreeMap::from([(1, [0, 231, 255, 255])])
        );
        assert_eq!(
            coherence.import_pixels(writer, 1, Some(0)),
            BTreeMap::from([(0, [255, 0, 0, 255]), (1, [0, 231, 255, 255])])
        );
    }

    #[test]
    fn canonical_depth_encodes_and_imports_big_endian_words() {
        let writer = image(0, 1, 2, true);
        assert_eq!(
            writer.encode(
                0.75f32.to_ne_bytes(),
                0,
                0,
                None,
                FramebufferEncoding::BigEndian
            ),
            [0x40, 0x00]
        );
        let mut coherence = Coherence::new(FramebufferEncoding::BigEndian);
        coherence.owners.write(writer, 1, 0, vec![0xff, 0xfc]);
        coherence
            .release_changed(&crate::RdramImage::new(&[0x40, 0x00]))
            .unwrap();
        assert_eq!(
            f32::from_ne_bytes(coherence.import_pixels(writer, 1, None)[&0]),
            0.75
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn canonical_odd_i8_overlap_and_one_byte_tail_preserve_lanes() {
        let (device, _, _) = crate::render::headless_device();
        let mut coherence = Coherence::new(FramebufferEncoding::BigEndian);
        coherence.fill(image(0, 3, 2, true), (0, 0, 3, 1), 0x4000_6000);
        let color = image(0, 2, 2, false);
        coherence.fill(color, (0, 0, 2, 1), 0xf801_07c1);
        coherence.fill(image(1, 2, 1, false), (0, 0, 2, 1), 0x1234_0000);
        coherence.fill(image(3, 1, 1, false), (0, 0, 1, 1), 0xab00_0000);
        coherence.resolve(&device);
        let patches = coherence.owners.patches(0);
        assert_eq!(
            patches.iter().map(|p| p.address).collect::<Vec<_>>(),
            [0, 1, 3, 4]
        );
        assert_eq!(
            patches
                .iter()
                .flat_map(|p| p.bytes.iter().copied())
                .collect::<Vec<_>>(),
            [0xf8, 0x12, 0x34, 0xab, 0x40, 0x00]
        );
        assert_eq!(
            coherence.import_pixels(color, 1, Some(0)),
            BTreeMap::from([(0, [255, 0, 74, 0]), (1, [49, 148, 173, 255])])
        );
    }

    #[test]
    fn latest_writer_wins_and_preserves_unwritten_bytes() {
        let mut owners = Ownership::default();
        let depth = image(100, 4, 2, true);
        let color = image(100, 4, 2, false);
        owners.write(depth, 1, 100, vec![1; 16]);
        owners.write(color, 2, 104, vec![2; 4]);
        assert_eq!(
            owners.patches(1),
            vec![
                crate::GuestWrite {
                    address: 100,
                    bytes: vec![1; 4]
                },
                crate::GuestWrite {
                    address: 104,
                    bytes: vec![2; 4]
                },
                crate::GuestWrite {
                    address: 108,
                    bytes: vec![1; 8]
                }
            ]
        );
        assert_eq!(
            owners.patches(2),
            vec![crate::GuestWrite {
                address: 104,
                bytes: vec![2; 4]
            }]
        );
        assert!(owners.patches(3).is_empty());
    }

    #[test]
    fn i8_overwrites_only_first_half_of_rgba16() {
        let mut owners = Ownership::default();
        owners.write(image(0, 320, 2, false), 1, 0, vec![0x12; 320 * 240 * 2]);
        owners.write(image(0, 320, 1, false), 2, 0, vec![0xff; 320 * 240]);
        let patches = owners.patches(1);
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].bytes, vec![0xff; 76800]);
        assert_eq!(patches[1].address, 76800);
        assert_eq!(patches[1].bytes, vec![0x12; 76800]);
    }

    #[test]
    fn fullscreen_color_supersedes_portrait_depth() {
        let mut owners = Ownership::default();
        let color = image(0, 320, 2, false);
        owners.write(color, 1, 0, vec![1; 153600]);
        owners.write(image(4096, 64, 2, true), 2, 4096, vec![2; 64 * 112 * 2]);
        owners.write(color, 3, 0, vec![3; 153600]);
        assert_eq!(
            owners.patches(1),
            vec![crate::GuestWrite {
                address: 0,
                bytes: vec![3; 153600]
            }]
        );
    }

    #[test]
    fn rgba5551_matches_rt64_formula() {
        for (rgba, dither, expected) in [
            ([0, 0, 0, 0], 0, 0),
            ([255, 255, 255, 255], 0, 0xffff),
            ([7, 8, 15, 128], 0, 0x0042),
            ([7, 8, 15, 4], 1, 0x0845),
            ([128, 64, 32, 3], 0, 0x8208),
        ] {
            assert_eq!(pack_color(rgba, rgba[3] & 7, dither), expected);
        }
    }

    #[test]
    fn rgba16_dither_follows_rt64_magic_square_by_row_and_column() {
        let writer = image(0, 5, 2, false);
        let dither = |x: usize, y: usize| {
            (1..8u8)
                .filter(|&threshold| {
                    let word = writer.encode(
                        [8 - threshold, 0, 0, 255],
                        7,
                        y * 5 + x,
                        None,
                        FramebufferEncoding::Native,
                    );
                    u16::from_ne_bytes(word) >> 11 == 1
                })
                .count()
        };
        let pattern: Vec<Vec<_>> = (0..4)
            .map(|y| (0..4).map(|x| dither(x, y)).collect())
            .collect();
        assert_eq!(
            pattern,
            [[0, 6, 1, 7], [4, 2, 5, 3], [3, 5, 2, 4], [7, 1, 6, 0]]
        );
    }

    #[test]
    fn eight_bit_color_alternates_red_and_green() {
        for fmt in [2, 3, 4] {
            let writer = Writer {
                address: 0,
                layout: ImageLayout {
                    width: 2,
                    fmt,
                    siz: 1,
                },
                depth: false,
            };
            assert_eq!(
                writer.encode([11, 27, 91, 255], 7, 0, None, FramebufferEncoding::Native),
                [11, 0]
            );
            assert_eq!(
                writer.encode([11, 27, 91, 255], 7, 1, None, FramebufferEncoding::Native),
                [27, 0]
            );
        }
    }

    #[test]
    fn literal_fill_preserves_words_and_parity() {
        let writer = image(0, 3, 2, false);
        assert_eq!(
            writer.encode([0; 4], 0, 0, Some(0x1234_abcd), FramebufferEncoding::Native),
            0x1234u16.to_ne_bytes()
        );
        assert_eq!(
            writer.encode([0; 4], 0, 3, Some(0x1234_abcd), FramebufferEncoding::Native),
            0xabcdu16.to_ne_bytes()
        );
    }
    #[test]
    fn foreign_bytes_are_compared_and_only_changed_pixels_imported() {
        let mut coherence = Coherence::default();
        let writer = image(0, 4, 2, false);
        let bytes: Vec<_> = [0xf801u16; 4]
            .into_iter()
            .flat_map(u16::to_ne_bytes)
            .collect();
        coherence.owners.write(writer, 1, 0, bytes.clone());
        coherence
            .release_changed(&crate::RdramImage::new(&bytes))
            .unwrap();
        assert!(!coherence.imported);
        assert!(coherence.import_pixels(writer, 1, None).is_empty());
        let mut changed = bytes;
        changed[2..4].copy_from_slice(&0x07c1u16.to_ne_bytes());
        coherence
            .release_changed(&crate::RdramImage::new(&changed))
            .unwrap();
        assert!(coherence.imported);
        assert_eq!(
            coherence.import_pixels(writer, 1, None),
            BTreeMap::from([(1, [0, 255, 0, 255])])
        );
        coherence.begin_task();
        coherence
            .release_changed(&crate::RdramImage::new(&changed))
            .unwrap();
        assert!(coherence.imported);
        assert_eq!(coherence.imports.len(), 1);
        assert_eq!(
            coherence.import_pixels(writer, 1, None),
            BTreeMap::from([(1, [0, 255, 0, 255])])
        );
    }

    #[test]
    fn guest_overwritten_target_bytes_stop_being_a_load_source() {
        let mut coherence = Coherence::default();
        let coverage = image(0x100, 4, 1, false);
        coherence.owners.write(coverage, 1, 0x100, vec![0xff; 8]);
        let mut targets = super::super::targets::TargetDescriptors::default();
        targets.record(0x100, coverage.layout, 2, false).unwrap();
        targets.seed_owners(&coherence.owners);
        let fail = Err(crate::DiagKind::UnsupportedFramebufferAccess {
            address: 0x102,
            reason: crate::FramebufferAccess::Reinterpretation,
        });
        assert_eq!(targets.load_source(0x102, 4), fail);
        let mut memory = vec![0; 0x100];
        memory.extend([0xff, 0xff, 1, 2, 3, 4, 0xff, 0xff]);
        coherence
            .release_changed(&crate::RdramImage::new(&memory))
            .unwrap();
        assert!(coherence
            .owners
            .patches(0)
            .iter()
            .all(|patch| patch.address != 0x102));
        targets.seed_owners(&coherence.owners);
        assert_eq!(targets.load_source(0x102, 4), Ok(None));
        assert_eq!(
            targets.load_source(0x100, 2),
            Err(crate::DiagKind::UnsupportedFramebufferAccess {
                address: 0x100,
                reason: crate::FramebufferAccess::Reinterpretation,
            })
        );
        targets.written(0x100, false);
        assert_eq!(targets.load_source(0x102, 4), fail);
    }

    #[test]
    fn never_written_ranges_do_not_read_guest_memory() {
        let mut coherence = Coherence::default();
        coherence
            .release_changed(&crate::RdramImage::new(&[]))
            .unwrap();
        assert!(!coherence.imported);
    }

    #[test]
    fn packed_depth_import_uses_native_words_and_step_one_scale() {
        let mut coherence = Coherence::default();
        let writer = image(0, 1, 2, true);
        coherence
            .owners
            .write(writer, 1, 0, 0xfffcu16.to_ne_bytes().to_vec());
        coherence
            .release_changed(&crate::RdramImage::new(&0x4000u16.to_ne_bytes()))
            .unwrap();
        assert_eq!(
            f32::from_ne_bytes(coherence.import_pixels(writer, 1, None)[&0]),
            196608.0 / 262144.0
        );
    }

    #[test]
    fn unsupported_writer_cancels_pending_supported_bytes() {
        let mut owners = Ownership::default();
        let color = image(0, 4, 2, false);
        let unsupported = Writer {
            layout: ImageLayout {
                siz: 3,
                ..color.layout
            },
            ..color
        };
        owners.write(color, 1, 0, vec![1; 8]);
        owners.write(unsupported, 2, 0, vec![0; 4]);
        assert_eq!(
            owners.patches(1),
            vec![crate::GuestWrite {
                address: 4,
                bytes: vec![1; 4]
            }]
        );
        let four_bit = Writer {
            layout: ImageLayout {
                width: 3,
                siz: 0,
                ..color.layout
            },
            ..color
        };
        assert_eq!(four_bit.byte_offset(2), 1);
        assert_eq!(four_bit.byte_offset(3), 2);
        assert_eq!(four_bit.byte_offset(4), 2);
    }

    #[test]
    fn restored_views_include_baselined_edits_and_growth_only_restores_new_rows() {
        let mut coherence = Coherence::default();
        let writer = image(0, 4, 2, false);
        let bytes: Vec<_> = [0xf801u16; 8]
            .into_iter()
            .flat_map(u16::to_ne_bytes)
            .collect();
        coherence.owners.write(writer, 1, 0, bytes.clone());
        let mut changed = bytes;
        changed[0..2].copy_from_slice(&0x003fu16.to_ne_bytes());
        changed[14..16].copy_from_slice(&0x07c1u16.to_ne_bytes());
        coherence
            .release_changed(&crate::RdramImage::new(&changed))
            .unwrap();
        assert!(coherence
            .import_pixels(writer, 2, Some(1))
            .keys()
            .all(|&pixel| pixel >= 4));
        coherence.imports.clear();
        assert!(coherence.import_pixels(writer, 2, None).is_empty());
        let restored = coherence.import_pixels(writer, 2, Some(0));
        assert_eq!(restored.len(), 8);
        assert_eq!(restored[&7], [0, 255, 0, 255]);
        let grown = coherence.import_pixels(writer, 2, Some(1));
        assert_eq!(grown.len(), 4);
        assert!(grown.keys().all(|&pixel| pixel >= 4));
    }

    #[test]
    fn coverage_and_depth_import_shaders_validate_without_a_gpu() {
        let prelude = include_str!("../combiner_prelude.wgsl");
        for source in [
            coverage_shader(prelude),
            coverage_shader(&prelude.replace('\n', "\r\n")),
            include_str!("../depth.wgsl").to_string(),
        ] {
            let module = wgpu::naga::front::wgsl::parse_str(&source).unwrap();
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn reconfigure_keeps_coverage_pipelines_warm() {
        let (device, queue, _) = crate::render::headless_device();
        let config = crate::RendererConfig {
            clear_policy: crate::ClearPolicy::Persist,
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: None,
            power_preference: wgpu::PowerPreference::LowPower,
        };
        let mut r = crate::Renderer::with_device(
            device,
            queue,
            crate::PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 64,
                height: 64,
            },
            config,
        );
        r.set_guest_writeback(true);
        let warmed = r.inner.coverage.as_ref().map(|c| c.pipelines.len());
        assert!(warmed.is_some_and(|count| count > 0));
        r.reconfigure(config);
        assert_eq!(r.inner.coverage.as_ref().map(|c| c.pipelines.len()), warmed);
    }
}

pub(crate) struct Coverage {
    shader: wgpu::ShaderModule,
    pipelines:
        std::collections::HashMap<(bool, bool, bool, bool, bool, bool), wgpu::RenderPipeline>,
}

fn coverage_shader(prelude: &str) -> String {
    let entries = format!(
        "{}\n{}",
        include_str!("../skeleton.wgsl"),
        include_str!("../decal.wgsl")
    )
    .replace("-> vec4<f32>", "-> vec2<u32>")
    .replace("-> @location(0) vec4<f32>", "-> @location(0) vec2<u32>")
    .replace(
        "return vec4<f32>(rgb, r.alpha);",
        "return vec2<u32>(combiner.coverage_slot, combiner.coverage_slot);",
    );
    format!("{prelude}\n{entries}")
        .replace("\r\n", "\n")
        .replace(
            "struct DepthOutput {\n    @location(0) color: vec4<f32>",
            "struct DepthOutput {\n    @location(0) color: vec2<u32>",
        )
}

impl Coverage {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            shader: device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("guest-coverage"),
                source: wgpu::ShaderSource::Wgsl(
                    coverage_shader(include_str!("../combiner_prelude.wgsl")).into(),
                ),
            }),
            pipelines: Default::default(),
        }
    }

    pub(in crate::render) fn pipeline(
        &mut self,
        device: &wgpu::Device,
        textured: &super::super::TexturedPipeline,
        draw: &super::super::inputs::DrawInputs,
        any_depth: bool,
    ) -> &wgpu::RenderPipeline {
        use super::super::inputs::DrawInputs;
        let (cull, test, write, decal) = match draw {
            DrawInputs::Tris { cull, mode, .. } => (
                *cull == crate::scene::CullKind::Cull,
                mode.z_test,
                mode.z_write,
                mode.read_depth,
            ),
            DrawInputs::Rectangle { mode, .. } => {
                (false, mode.z_test, mode.z_write, mode.read_depth)
            }
            _ => (false, false, false, false),
        };
        let unclipped_depth = matches!(
            draw,
            DrawInputs::Tris {
                unclipped_depth: true,
                ..
            }
        ) && device
            .features()
            .contains(wgpu::Features::DEPTH_CLIP_CONTROL);
        self.pipeline_for(
            device,
            textured,
            (cull, test, write, decal, any_depth, unclipped_depth),
        )
    }

    /// Build every pipeline variant up front: a cold first use compiles one mid-frame, and enough of
    /// those in one task trip a guest's RCP watchdog.
    pub(in crate::render) fn warm(
        &mut self,
        device: &wgpu::Device,
        textured: &super::super::TexturedPipeline,
    ) {
        let unclipped = device
            .features()
            .contains(wgpu::Features::DEPTH_CLIP_CONTROL);
        for bits in 0..64u8 {
            let flag = |bit: u8| bits & (1 << bit) != 0;
            // A depth-writing decal always has a depth attachment; no draw reaches the other case.
            if (flag(5) && !unclipped) || (flag(3) && flag(2) && !flag(4)) {
                continue;
            }
            self.pipeline_for(
                device,
                textured,
                (flag(0), flag(1), flag(2), flag(3), flag(4), flag(5)),
            );
        }
    }

    fn pipeline_for(
        &mut self,
        device: &wgpu::Device,
        textured: &super::super::TexturedPipeline,
        key: (bool, bool, bool, bool, bool, bool),
    ) -> &wgpu::RenderPipeline {
        let (cull, test, write, decal, any_depth, unclipped_depth) = key;
        self.pipelines.entry(key).or_insert_with(|| {
            let mut groups = vec![
                Some(textured.bind_group_layout()),
                Some(textured.uniform_bind_group_layout()),
            ];
            if decal {
                groups.push(Some(textured.depth_bind_group_layout()));
            }
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("guest-coverage"),
                bind_group_layouts: &groups,
                immediate_size: 0,
            });
            let descriptor = wgpu::RenderPipelineDescriptor {
                label: Some("guest-coverage"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some("vs_main"),
                    buffers: &[super::super::OutVertex::layout()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some(match (decal, write, any_depth) {
                        (true, true, _) => "fs_decal_depth",
                        (true, false, _) => "fs_decal",
                        (false, _, true) => "fs_depth",
                        _ => "fs_main",
                    }),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rg32Uint,
                        blend: None,
                        write_mask: if write {
                            wgpu::ColorWrites::RED | wgpu::ColorWrites::GREEN
                        } else {
                            wgpu::ColorWrites::RED
                        },
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode: cull.then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: (any_depth && (!decal || write)).then_some(
                    wgpu::DepthStencilState {
                        format: super::super::DEPTH_FORMAT,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(if test && !decal {
                            wgpu::CompareFunction::Less
                        } else {
                            wgpu::CompareFunction::Always
                        }),
                        stencil: Default::default(),
                        bias: Default::default(),
                    },
                ),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            };
            super::super::depth_clip_pipeline(device, &descriptor, unclipped_depth)
        })
    }
}

pub(crate) fn coverage_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("guest-coverage"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rg32Uint,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

struct Readback {
    buffer: wgpu::Buffer,
    stride: usize,
    pixel_bytes: usize,
    width: usize,
    height: usize,
}

impl Readback {
    fn vertices(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Buffer,
    ) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("guest-writeback-vertices"),
            size: source.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(source, 0, &buffer, 0, source.size());
        Self {
            buffer,
            stride: source.size() as usize,
            pixel_bytes: 48,
            width: source.size() as usize / 48,
            height: 1,
        }
    }

    fn new(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        pixel_bytes: u32,
    ) -> Self {
        let stride =
            (texture.width() * pixel_bytes).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("guest-writeback"),
            size: u64::from(stride) * u64::from(texture.height()),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: if texture.format() == super::super::DEPTH_FORMAT {
                    wgpu::TextureAspect::DepthOnly
                } else {
                    wgpu::TextureAspect::All
                },
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(texture.height()),
                },
            },
            texture.size(),
        );
        Self {
            buffer,
            stride: stride as usize,
            pixel_bytes: pixel_bytes as usize,
            width: texture.width() as usize,
            height: texture.height() as usize,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read(self, device: &wgpu::Device) -> Vec<u8> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("guest write-back GPU wait");
        rx.recv()
            .expect("guest write-back callback")
            .expect("guest write-back mapping");
        let data = self.buffer.slice(..).get_mapped_range();
        data.chunks(self.stride)
            .take(self.height)
            .flat_map(|row| &row[..self.width * self.pixel_bytes])
            .copied()
            .collect()
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct ReplayGeometry {
    vertices: Vec<Option<super::writeback::Vertex>>,
    positions: Vec<Option<[f32; 2]>>,
    admitted: Vec<bool>,
    bounds: Vec<Option<(u32, u32, u32, u32)>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ReplayGeometry {
    fn prepare(&mut self, draws: &[CoverageDraw]) {
        self.positions.clear();
        self.positions
            .extend(self.vertices.iter().map(|v| v.map(|v| v.position)));
        self.admitted.clear();
        self.admitted.extend(
            draws
                .iter()
                .map(|draw| super::writeback::admitted(draw, &self.vertices)),
        );
        self.bounds.clear();
        self.bounds.extend(
            draws
                .iter()
                .map(|draw| draw.possible_bounds(&self.positions)),
        );
    }

    fn fallback_only(
        &self,
        draws: &[CoverageDraw],
        first_draw: usize,
        extent: (usize, usize),
    ) -> bool {
        let mut admitted = self
            .admitted
            .iter()
            .enumerate()
            .skip(first_draw)
            .filter_map(|(i, &a)| a.then_some(i));
        let Some(index) = admitted.next() else {
            return false;
        };
        if admitted.next().is_some() || index + 1 != draws.len() {
            return false;
        }
        let mode = draws[index].mode.unwrap();
        if draws[index].literal.is_some() || !mode.image_read || !matches!(mode.cvg_dst, 1 | 3) {
            return false;
        }
        for i in (first_draw..index).rev() {
            if draws[i].literal.is_some() {
                return false;
            }
            if self.bounds[i].is_some_and(|(x, y, w, h)| {
                x == 0 && y == 0 && w as usize >= extent.0 && h as usize >= extent.1
            }) {
                return true;
            }
        }
        false
    }

    #[cfg(test)]
    fn new(draws: &[CoverageDraw], vertices: &[Option<super::writeback::Vertex>]) -> Self {
        let mut geometry = Self {
            vertices: vertices.to_vec(),
            ..Default::default()
        };
        geometry.prepare(draws);
        geometry
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct ReplayScratch {
    bytes: Vec<u8>,
    valid: Vec<bool>,
    color_valid: Vec<bool>,
    colors: Vec<[f32; 4]>,
    coverage: Vec<u8>,
    output: Vec<super::writeback::Pixel>,
    unknown: Vec<u32>,
    history: Vec<u32>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct ResolveScratch {
    geometry: ReplayGeometry,
    coverage: Vec<StoredCoverage>,
    last_draw: Vec<u32>,
    needed: Vec<bool>,
}

struct Snapshot {
    coverage: Readback,
    color: Option<(Writer, Readback)>,
    depth: Option<(Writer, Readback)>,
    draws: Vec<CoverageDraw>,
    vertices: Option<Readback>,
    render_height: u32,
    first_draw: usize,
    generation: u64,
}

struct PendingWrite {
    generation: u64,
    writer: Writer,
    address: u64,
    bytes: Vec<u8>,
    coverage: Vec<u8>,
    invalidate: Option<(usize, usize)>,
}

#[derive(Default)]
pub(crate) struct Coherence {
    pub encoding: FramebufferEncoding,
    pub owners: Ownership,
    hidden: BTreeMap<u64, Vec<u8>>,
    #[cfg(not(target_arch = "wasm32"))]
    replay_scratch: ReplayScratch,
    #[cfg(not(target_arch = "wasm32"))]
    resolve_scratch: ResolveScratch,
    generation: u64,
    first_generation: u64,
    snapshots: Vec<Snapshot>,
    pending: Vec<PendingWrite>,
    /// Edits reported once by `release_changed`, uploaded when the next task begins.
    pub imports: Vec<crate::GuestWrite>,
    pub imported: bool,
}

impl Coherence {
    pub fn new(encoding: FramebufferEncoding) -> Self {
        Self {
            encoding,
            ..Default::default()
        }
    }

    pub fn begin_task(&mut self) {
        self.first_generation = self.generation + 1;
        self.snapshots.clear();
        self.pending.clear();
        self.imported = !self.imports.is_empty();
    }

    pub fn fill(&mut self, writer: Writer, bounds: (u32, u32, u32, u32), word: u32) {
        self.generation += 1;
        let (x, y, width, height) = bounds;
        for row in y..y + height {
            let pixel = (row * writer.layout.width + x) as usize;
            let bytes = if writer.supported() {
                (pixel..pixel + width as usize)
                    .flat_map(|p| {
                        writer
                            .encode([0; 4], 0, p, Some(word), self.encoding)
                            .into_iter()
                            .take(writer.pixel_bytes())
                    })
                    .collect()
            } else {
                vec![0; width as usize * writer.pixel_bytes()]
            };
            self.pending.push(PendingWrite {
                generation: self.generation,
                invalidate: None,
                writer,
                address: writer.address + (pixel * writer.pixel_bytes()) as u64,
                coverage: if !writer.depth && (writer.layout.fmt, writer.layout.siz) == (0, 2) {
                    (pixel..pixel + width as usize)
                        .map(|p| {
                            if (word >> (16 * (1 - p % 2))) & 1 != 0 {
                                7
                            } else {
                                0
                            }
                        })
                        .collect()
                } else {
                    Vec::new()
                },
                bytes,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::render) fn snapshot(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        coverage: &wgpu::Texture,
        color: Option<(Writer, &wgpu::Texture)>,
        depth: Option<(Writer, &wgpu::Texture)>,
        draws: Vec<CoverageDraw>,
        vertices: Option<&wgpu::Buffer>,
        render_height: u32,
        first_draw: usize,
    ) {
        let generation = self.generation;
        self.generation += draws.len() as u64 * 2;
        let vertices = vertices
            .filter(|_| {
                color.is_some_and(|(writer, _)| !writer.depth && (writer.layout.fmt, writer.layout.siz) == (0, 2))
                    && draws.iter().any(|draw| draw.coverage_mode() != CoverageMode::Unknown || draw.combiner.is_some())
                    && draws.iter().any(|draw| {
                        matches!(&draw.geometry, CoverageGeometry::Triangles(triangles) if !triangles.is_empty())
                    })
            })
            .map(|buffer| Readback::vertices(device, encoder, buffer));
        self.snapshots.push(Snapshot {
            coverage: Readback::new(device, encoder, coverage, 8),
            color: color
                .map(|(writer, texture)| (writer, Readback::new(device, encoder, texture, 4))),
            depth: depth
                .map(|(writer, texture)| (writer, Readback::new(device, encoder, texture, 4))),
            draws,
            vertices,
            render_height,
            first_draw,
            generation,
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn pending_overlaps(&self, writer: Writer, height: u32) -> bool {
        let start = writer.address;
        let end = start + writer.layout.row_bytes().unwrap() * u64::from(height);
        let overlaps = |address, length| address < end && start < address + length;
        self.pending
            .iter()
            .any(|write| overlaps(write.address, write.bytes.len() as u64))
            || self.snapshots.iter().any(|snapshot| {
                [&snapshot.color, &snapshot.depth]
                    .into_iter()
                    .flatten()
                    .any(|(writer, readback)| {
                        overlaps(
                            writer.address,
                            writer.layout.row_bytes().unwrap() * readback.height as u64,
                        )
                    })
            })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn finish(&mut self, device: &wgpu::Device) -> Vec<crate::GuestWrite> {
        self.resolve(device);
        self.imports.clear();
        self.owners.patches(self.first_generation)
    }

    pub(crate) fn stored_coverage_code(&self, address: u64) -> Option<u8> {
        let byte = |address| {
            let (&start, range) = self.owners.0.range(..=address).next_back()?;
            range.bytes.get((address - start) as usize).copied()
        };
        let word = self
            .encoding
            .decode_u16([byte(address)?, byte(address + 1)?]);
        let hidden = *self
            .hidden
            .get(&(address / 4096))?
            .get((address % 4096) as usize / 2)?;
        (hidden != 255).then_some(((word & 1) as u8) * 4 + (hidden & 3))
    }

    fn invalidate_hidden(&mut self, address: u64, length: usize) {
        if length == 0 {
            return;
        }
        let end = address + length as u64;
        for (&page, bits) in self.hidden.range_mut(address / 4096..=(end - 1) / 4096) {
            let start = address.saturating_sub(page * 4096) as usize / 2;
            let end = (end - page * 4096).min(4096).div_ceil(2) as usize;
            bits[start..end].fill(255);
        }
    }

    fn commit_pending(&mut self, generation: u64) {
        self.pending.sort_by_key(|write| write.generation);
        let end = self
            .pending
            .partition_point(|write| write.generation <= generation);
        let mut pending = std::mem::take(&mut self.pending);
        for write in pending.drain(..end) {
            self.invalidate_hidden(write.address, write.bytes.len());
            if let Some((rows, length)) = write.invalidate {
                let stride = write.writer.layout.row_bytes().unwrap();
                for row in 0..rows {
                    self.invalidate_hidden(write.address + row as u64 * stride, length);
                }
            }
            let mut index = 0;
            while index < write.coverage.len() {
                let address = write.address + index as u64 * 2;
                let offset = (address % 4096) as usize / 2;
                let length = (2048 - offset).min(write.coverage.len() - index);
                let coverage = &write.coverage[index..index + length];
                if coverage.iter().any(|&c| c != 255) {
                    let bits = self
                        .hidden
                        .entry(address / 4096)
                        .or_insert_with(|| vec![255; 2048]);
                    for (bits, &coverage) in bits[offset..offset + length].iter_mut().zip(coverage)
                    {
                        *bits = if coverage == 255 { 255 } else { coverage & 3 };
                    }
                }
                index += length;
            }
            self.owners
                .write(write.writer, write.generation, write.address, write.bytes);
        }
        self.pending = pending;
    }

    fn queue_invalidation(
        &mut self,
        writer: Writer,
        generation: u64,
        bounds: (u32, u32, u32, u32),
        height: usize,
    ) {
        let (x, y, w, h) = bounds;
        let width = writer.layout.width as usize;
        let left = (x as usize).min(width);
        let right = ((x + w) as usize).min(width);
        let bottom = ((y + h) as usize).min(height);
        if left < right && (y as usize) < bottom {
            self.pending.push(PendingWrite {
                generation,
                writer,
                address: writer.address + writer.byte_offset(y as usize * width + left),
                bytes: Vec::new(),
                coverage: Vec::new(),
                invalidate: Some((bottom - y as usize, (right - left) * writer.pixel_bytes())),
            });
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn replay(
        &mut self,
        writer: Writer,
        draws: &[CoverageDraw],
        order: (u64, usize),
        geometry: &ReplayGeometry,
        stamps: &[u8],
        data: &[u8],
    ) -> Vec<super::writeback::Pixel> {
        use super::writeback::Pixel;
        let (generation, first_draw) = order;
        let width = writer.layout.width as usize;
        let height = data.len() / 4 / width;
        let extent = (width, height);
        let length = width * height;
        let mut scratch = std::mem::take(&mut self.replay_scratch);
        let bytes = &mut scratch.bytes;
        bytes.resize(length * 2, 0);
        bytes.fill(0);
        let valid = &mut scratch.valid;
        valid.resize(length * 2, false);
        valid.fill(false);
        let end = writer.address + bytes.len() as u64;
        for (&address, range) in self.owners.0.range(..end) {
            let start = address.max(writer.address);
            let end = (address + range.bytes.len() as u64).min(end);
            if start >= end {
                continue;
            }
            let a = (start - writer.address) as usize;
            let b = (end - writer.address) as usize;
            bytes[a..b].copy_from_slice(
                &range.bytes[(start - address) as usize..(end - address) as usize],
            );
            valid[a..b].fill(true);
        }
        let unpack = |word: u16| {
            let channel = |shift: u32| {
                let v = ((word >> shift) & 31u16) as u8;
                f32::from((v << 3) | (v >> 2)) / 255.0
            };
            [channel(11), channel(6), channel(1), 1.0]
        };
        let color_valid = &mut scratch.color_valid;
        color_valid.clear();
        color_valid.extend(valid.as_chunks::<2>().0.iter().map(|v| v[0] && v[1]));
        let colors = &mut scratch.colors;
        colors.resize(length, [0.0; 4]);
        let coverage = &mut scratch.coverage;
        coverage.resize(length, 255);
        coverage.fill(255);
        for pixel in 0..length {
            let word = self
                .encoding
                .decode_u16([bytes[pixel * 2], bytes[pixel * 2 + 1]]);
            colors[pixel] = unpack(word);
        }
        let mut pixel = 0;
        while pixel < length {
            let address = writer.address + pixel as u64 * 2;
            let offset = (address % 4096) as usize / 2;
            let count = (2048 - offset).min(length - pixel);
            if let Some(bits) = self.hidden.get(&(address / 4096)) {
                for (i, &hidden) in bits[offset..offset + count].iter().enumerate() {
                    let p = pixel + i;
                    if color_valid[p] && hidden != 255 {
                        let word = self.encoding.decode_u16([bytes[p * 2], bytes[p * 2 + 1]]);
                        coverage[p] = ((word & 1) as u8) * 4 + hidden;
                    }
                }
            }
            pixel += count;
        }
        let mut output = std::mem::take(&mut scratch.output);
        output.clear();
        output.extend(stamps.as_chunks::<8>().0.iter().map(|stamp| {
            let slot = u32::from_ne_bytes(stamp[..4].try_into().unwrap()) as usize;
            if slot > first_draw && geometry.admitted[slot - 1] {
                Pixel::Keep
            } else {
                Pixel::Fallback
            }
        }));
        let unknown = &mut scratch.unknown;
        unknown.resize(length, 0);
        unknown.fill(0);
        let history = &mut scratch.history;
        history.resize(length, 0);
        history.fill(0);
        for (index, draw) in draws.iter().enumerate().skip(first_draw) {
            if let (Some(word), CoverageGeometry::Rectangle(bounds)) =
                (draw.literal, &draw.geometry)
            {
                let (sx, sy, sw, sh) = draw.scissor;
                for y in (bounds[1].max(0.0) as usize).max(sy as usize)
                    ..(bounds[3].max(0.0) as usize)
                        .min(height)
                        .min((sy + sh) as usize)
                {
                    for x in (bounds[0].max(0.0) as usize).max(sx as usize)
                        ..(bounds[2].max(0.0) as usize)
                            .min(width)
                            .min((sx + sw) as usize)
                    {
                        let pixel = y * width + x;
                        history[pixel] = index as u32 + 1;
                        let word = (word >> (16 * (1 - pixel % 2))) as u16;
                        colors[pixel] = unpack(word);
                        color_valid[pixel] = true;
                        coverage[pixel] = if word & 1 == 0 { 0 } else { 7 };
                        output[pixel] = Pixel::Write([0; 4], coverage[pixel], index as u32 + 1);
                    }
                }
                continue;
            }
            let accepted = geometry.admitted[index];
            if accepted {
                super::writeback::rasterize(
                    draw,
                    &geometry.vertices,
                    extent,
                    |pixel, count, rgba, shade_alpha| {
                        if unknown[pixel] > history[pixel] {
                            coverage[pixel] = 255;
                            color_valid[pixel] = false;
                        }
                        history[pixel] = index as u32 + 1;
                        let mode = draw.mode.unwrap();
                        let memory = if mode.image_read { coverage[pixel] } else { 7 };
                        let memory = (memory != 255).then_some(memory);
                        let c = memory
                            .map(|m| super::writeback::destination(mode, count, m))
                            .or_else(|| {
                                (mode.cvg_dst == 2 || (count == 8 && mode.cvg_dst == 0))
                                    .then_some(7)
                            });
                        coverage[pixel] = c.unwrap_or(255);
                        if super::writeback::color_known(
                            mode,
                            rgba[3],
                            count,
                            memory,
                            color_valid[pixel],
                        ) {
                            let color = super::writeback::blend(
                                mode,
                                draw.combiner.as_ref().unwrap(),
                                rgba,
                                colors[pixel],
                                shade_alpha,
                                count,
                                memory.unwrap_or(7),
                            );
                            colors[pixel] = color.map(|c| f32::from(c) / 255.0);
                            color_valid[pixel] = true;
                            output[pixel] = c.map_or(Pixel::Fallback, |c| {
                                Pixel::Write(color, c, index as u32 + 1)
                            });
                        } else {
                            color_valid[pixel] = false;
                            let slot = u32::from_ne_bytes(
                                stamps[pixel * 8..pixel * 8 + 4].try_into().unwrap(),
                            );
                            output[pixel] = match c {
                                Some(c) if slot == index as u32 + 1 => Pixel::Write(
                                    data[pixel * 4..pixel * 4 + 4].try_into().unwrap(),
                                    c,
                                    slot,
                                ),
                                _ => Pixel::Fallback,
                            };
                        }
                    },
                );
            }
            if !accepted {
                if let Some((x, y, w, h)) = geometry.bounds[index] {
                    for row in y as usize..((y + h) as usize).min(height) {
                        let start = row * width + (x as usize).min(width);
                        let end = row * width + ((x + w) as usize).min(width);
                        unknown[start..end].fill(index as u32 + 1);
                    }
                    self.queue_invalidation(
                        writer,
                        generation + (index as u64 + 1) * 2,
                        (x, y, w, h),
                        height,
                    );
                }
            }
        }
        for pixel in 0..length {
            if unknown[pixel] > history[pixel] {
                output[pixel] = Pixel::Fallback;
            }
        }
        self.replay_scratch = scratch;
        output
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn resolve(&mut self, device: &wgpu::Device) {
        let mut scratch = std::mem::take(&mut self.resolve_scratch);
        for snapshot in std::mem::take(&mut self.snapshots) {
            self.commit_pending(snapshot.generation);
            let width = snapshot.coverage.width;
            let height = snapshot.render_height;
            let target_height = snapshot.coverage.height;
            let mut projected_vertices = snapshot.vertices;
            let stamps = snapshot.coverage.read(device);
            let rgba16 = snapshot.color.as_ref().is_some_and(|(writer, _)| {
                !writer.depth && (writer.layout.fmt, writer.layout.siz) == (0, 2)
            });
            for (channel, target) in [(0, snapshot.color), (1, snapshot.depth)] {
                let Some((writer, readback)) = target else {
                    continue;
                };
                let data = readback.read(device);
                let native_candidate = channel == 0
                    && rgba16
                    && snapshot
                        .draws
                        .iter()
                        .skip(snapshot.first_draw)
                        .any(|draw| draw.combiner.is_some());
                let coverage_enabled = rgba16
                    && channel == 0
                    && stamps
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .enumerate()
                        .any(|(pixel, stamp)| {
                            let slot = u32::from_ne_bytes(stamp[..4].try_into().unwrap());
                            slot as usize > snapshot.first_draw
                                && data[pixel * 4 + 3] & 4 == 0
                                && snapshot.draws[slot as usize - 1].coverage_mode()
                                    != CoverageMode::Unknown
                        });
                let ResolveScratch {
                    geometry,
                    coverage,
                    last_draw,
                    needed,
                } = &mut scratch;
                geometry.vertices.clear();
                if coverage_enabled || native_candidate {
                    if let Some(readback) = projected_vertices.take() {
                        for vertex in readback.read(device).as_chunks::<48>().0.iter() {
                            let p: [f32; 4] = std::array::from_fn(|i| {
                                f32::from_ne_bytes(vertex[i * 4..i * 4 + 4].try_into().unwrap())
                            });
                            let position = (p.iter().all(|v| v.is_finite())
                                && p[3] > 0.0
                                && p[2] >= 0.0
                                && p[2] <= p[3])
                                .then(|| {
                                    [
                                        (p[0] / p[3] + 1.0) * 0.5 * width as f32,
                                        (1.0 - p[1] / p[3]) * 0.5 * height as f32,
                                    ]
                                });
                            geometry.vertices.push(position.map(|position| {
                                super::writeback::Vertex {
                                    position,
                                    color: std::array::from_fn(|i| {
                                        f32::from_ne_bytes(
                                            vertex[16 + i * 4..20 + i * 4].try_into().unwrap(),
                                        )
                                    }),
                                }
                            }));
                        }
                    }
                    geometry.prepare(&snapshot.draws);
                }
                if coverage_enabled {
                    coverage.clear();
                    coverage.resize_with(snapshot.draws.len(), || StoredCoverage::Unknown);
                    last_draw.resize(stamps.len() / 8, 0);
                    last_draw.fill(0);
                    for (index, _) in snapshot.draws.iter().enumerate() {
                        if let Some((x, y, w, h)) = geometry.bounds[index] {
                            for row in y as usize..((y + h) as usize).min(target_height) {
                                last_draw[row * width + (x as usize).min(width)
                                    ..row * width + ((x + w) as usize).min(width)]
                                    .fill(index as u32 + 1);
                            }
                        }
                    }
                    needed.resize(snapshot.draws.len(), false);
                    needed.fill(false);
                    for (pixel, stamp) in stamps.as_chunks::<8>().0.iter().enumerate() {
                        let slot = u32::from_ne_bytes(stamp[..4].try_into().unwrap());
                        if slot as usize > snapshot.first_draw
                            && last_draw[pixel] <= slot
                            && data[pixel * 4 + 3] & 4 == 0
                        {
                            let draw = &snapshot.draws[slot as usize - 1];
                            if draw.coverage_mode() != CoverageMode::Unknown
                                && !geometry.admitted[slot as usize - 1]
                            {
                                needed[slot as usize - 1] = true;
                            }
                        }
                    }
                    for (index, draw) in snapshot.draws.iter().enumerate() {
                        if needed[index] {
                            coverage[index] =
                                draw.stored_coverage((width, target_height), &geometry.positions);
                        }
                    }
                }

                let native_admitted = native_candidate
                    && geometry
                        .admitted
                        .iter()
                        .skip(snapshot.first_draw)
                        .any(|&a| a);
                let native_fallback = native_admitted
                    && geometry.fallback_only(
                        &snapshot.draws,
                        snapshot.first_draw,
                        (width, target_height),
                    );
                let native_enabled = native_admitted && !native_fallback;
                let native = if native_enabled {
                    self.replay(
                        writer,
                        &snapshot.draws,
                        (snapshot.generation, snapshot.first_draw),
                        geometry,
                        &stamps,
                        &data,
                    )
                } else {
                    if channel == 0
                        && (native_fallback
                            || !self.hidden.is_empty()
                            || snapshot.draws.iter().any(|draw| draw.literal.is_some()))
                    {
                        if !coverage_enabled && !native_candidate {
                            geometry.bounds.clear();
                            geometry.bounds.extend(
                                snapshot.draws.iter().map(|draw| draw.possible_bounds(&[])),
                            );
                        }
                        for (index, draw) in
                            snapshot.draws.iter().enumerate().skip(snapshot.first_draw)
                        {
                            if native_fallback
                                && (geometry.admitted[index]
                                    || matches!(
                                        (&draw.literal, &draw.geometry),
                                        (Some(_), CoverageGeometry::Rectangle(_))
                                    ))
                            {
                                continue;
                            }
                            if let Some((x, y, w, h)) = geometry.bounds[index] {
                                self.queue_invalidation(
                                    writer,
                                    snapshot.generation + (index as u64 + 1) * 2,
                                    (x, y, w, h),
                                    target_height,
                                );
                            }
                        }
                    }
                    Vec::new()
                };
                let mut start = 0;
                while start < stamps.len() / 8 {
                    let stamp = |pixel: usize| {
                        match native.get(pixel) {
                            Some(super::writeback::Pixel::Write(_, _, slot)) => return *slot,
                            Some(super::writeback::Pixel::Keep) => return 0,
                            _ => {}
                        }
                        let slot = u32::from_ne_bytes(
                            stamps[pixel * 8 + channel * 4..pixel * 8 + channel * 4 + 4]
                                .try_into()
                                .unwrap(),
                        );
                        if slot as usize <= snapshot.first_draw {
                            0
                        } else {
                            slot
                        }
                    };
                    let slot = stamp(start);
                    if slot == 0 {
                        start += 1;
                        continue;
                    }
                    let mut end = start + 1;
                    while end < stamps.len() / 8 && stamp(end) == slot {
                        end += 1;
                    }
                    let mut stored = Vec::new();
                    let bytes = (start..end)
                        .flat_map(|pixel| {
                            let mut rgba: [u8; 4] =
                                data[pixel * 4..pixel * 4 + 4].try_into().unwrap();
                            let draw = &snapshot.draws[slot as usize - 1];
                            let coverage = if coverage_enabled && last_draw[pixel] <= slot {
                                coverage[slot as usize - 1].get(pixel, width)
                            } else {
                                None
                            };
                            let coverage =
                                if let Some(super::writeback::Pixel::Write(color, c, _)) =
                                    native.get(pixel)
                                {
                                    rgba = *color;
                                    Some(*c)
                                } else if rgba16 && channel == 0 {
                                    draw.literal
                                        .map(|word| {
                                            if (word >> (16 * (1 - pixel % 2))) & 1 != 0 {
                                                7
                                            } else {
                                                0
                                            }
                                        })
                                        .or(coverage)
                                } else {
                                    None
                                };
                            if rgba16 && channel == 0 {
                                stored.push(coverage.unwrap_or(255));
                            }
                            let coverage = coverage.unwrap_or(rgba[3] & 7);
                            writer
                                .encode(
                                    rgba,
                                    coverage,
                                    pixel,
                                    if writer.depth { None } else { draw.literal },
                                    self.encoding,
                                )
                                .into_iter()
                                .take(writer.pixel_bytes())
                        })
                        .collect();
                    let generation = snapshot.generation + u64::from(slot) * 2 - channel as u64;
                    let bytes = if writer.supported() {
                        bytes
                    } else {
                        vec![
                            0;
                            (writer.byte_offset(end - 1) + writer.pixel_bytes() as u64
                                - writer.byte_offset(start)) as usize
                        ]
                    };
                    self.pending.push(PendingWrite {
                        generation,
                        invalidate: None,
                        writer,
                        address: writer.address + writer.byte_offset(start),
                        coverage: stored,
                        bytes,
                    });
                    start = end;
                }
                if native_enabled {
                    self.replay_scratch.output = native;
                }
            }
        }
        self.commit_pending(u64::MAX);
        self.owners.coalesce(self.first_generation);
        self.hidden.retain(|_, bits| bits.iter().any(|&c| c != 255));
        self.resolve_scratch = scratch;
    }
}

impl Coherence {
    /// Mark owned bytes the guest has changed since fast3d wrote them as foreign, and queue them as
    /// imports for the targets that cover them.
    pub fn release_changed(
        &mut self,
        memory: &impl crate::Rdram,
    ) -> Result<(), crate::MemoryError> {
        let mut changed = Vec::new();
        for (&address, owned) in &self.owners.0 {
            if !owned.writer.supported() {
                continue;
            }
            let bytes = crate::hle::interp::read_bytes_exact(memory, address, owned.bytes.len())?;
            let mut start = 0;
            while start < bytes.len() {
                if bytes[start] == owned.bytes[start] {
                    start += 1;
                    continue;
                }
                let mut end = start + 1;
                while end < bytes.len() && bytes[end] != owned.bytes[end] {
                    end += 1;
                }
                changed.push(crate::GuestWrite {
                    address: address + start as u64,
                    bytes: bytes[start..end].to_vec(),
                });
                start = end;
            }
        }
        for patch in &changed {
            self.invalidate_hidden(patch.address, patch.bytes.len());
            self.owners.mark_foreign(patch.address, patch.bytes.clone());
        }
        self.imports.extend(changed);
        self.imported = !self.imports.is_empty();
        Ok(())
    }

    pub fn import_pixels(
        &self,
        writer: Writer,
        height: u32,
        restore_from: Option<u32>,
    ) -> BTreeMap<usize, [u8; 4]> {
        let mut pixels = BTreeMap::new();
        if !writer.supported() {
            return pixels;
        }
        let size = writer.pixel_bytes();
        let end = writer.address + u64::from(writer.layout.width) * u64::from(height) * size as u64;
        let ranges: Vec<_> = if let Some(row) = restore_from {
            let first = writer.address + u64::from(row) * writer.layout.row_bytes().unwrap();
            self.owners
                .0
                .iter()
                .filter(|(_, range)| range.writer.supported())
                .map(|(&address, range)| address.max(first)..address + range.bytes.len() as u64)
                .collect()
        } else {
            self.imports
                .iter()
                .map(|patch| patch.address..patch.address + patch.bytes.len() as u64)
                .collect()
        };
        for range in ranges {
            let start = range.start.max(writer.address);
            let end = range.end.min(end);
            if start >= end {
                continue;
            }
            for pixel in (start - writer.address) as usize / size
                ..((end - writer.address) as usize).div_ceil(size)
            {
                let address = writer.address + (pixel * size) as u64;
                let bytes: Option<Vec<_>> = (address..address + size as u64)
                    .map(|byte| {
                        self.owners
                            .0
                            .range(..=byte)
                            .next_back()
                            .and_then(|(&start, range)| {
                                range
                                    .writer
                                    .supported()
                                    .then(|| range.bytes.get((byte - start) as usize).copied())
                                    .flatten()
                            })
                    })
                    .collect();
                let Some(bytes) = bytes else {
                    continue;
                };
                let rgba = if size == 1 {
                    [bytes[0]; 4]
                } else {
                    let word = self.encoding.decode_u16(bytes.try_into().unwrap());
                    if writer.depth {
                        unpack_depth(word).to_ne_bytes()
                    } else {
                        let channel = |shift: u32| {
                            let value = ((word >> shift) & 31u16) as u8;
                            (value << 3) | (value >> 2)
                        };
                        [
                            channel(11),
                            channel(6),
                            channel(1),
                            if word & 1 != 0 { 255 } else { 0 },
                        ]
                    }
                };
                pixels.insert(pixel, rgba);
            }
        }
        pixels
    }
}

fn unpack_depth(word: u16) -> f32 {
    let exponent = u32::from(word >> 13);
    let mantissa = u32::from((word & 0x1ffc) >> 2);
    let fixed = (mantissa << (6 - exponent.min(6))) + 0x40000 - (0x40000 >> exponent);
    fixed as f32 / 262144.0
}

impl super::super::SceneRenderer {
    pub(crate) fn warm_coverage(&mut self, device: &wgpu::Device) {
        self.coverage
            .get_or_insert_with(|| Coverage::new(device))
            .warm(device, &self.textured_fb);
    }

    pub(crate) fn import_existing(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self
            .coherence
            .as_ref()
            .is_none_or(|coherence| !coherence.imported)
        {
            return;
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("guest-imports"),
        });
        for (&id, target) in &self.framebuffers {
            if let super::super::workload::TargetId::Guest(address) = id {
                self.import_target(
                    device,
                    &mut encoder,
                    Writer {
                        address,
                        layout: target.layout,
                        depth: false,
                    },
                    None,
                );
            }
        }
        for &writer in self.color_versions.keys() {
            if self
                .framebuffers
                .get(&super::super::workload::TargetId::Guest(writer.address))
                .is_none_or(|target| target.layout != writer.layout)
            {
                self.import_target(device, &mut encoder, writer, None);
            }
        }
        for (&id, target) in &self.depthbuffers {
            if let super::super::workload::TargetId::Guest(address) = id {
                self.import_target(
                    device,
                    &mut encoder,
                    Writer {
                        address,
                        layout: target.layout,
                        depth: true,
                    },
                    None,
                );
            }
        }
        queue.submit([encoder.finish()]);
    }

    pub(crate) fn import_target(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        writer: Writer,
        restore_from: Option<u32>,
    ) -> bool {
        use wgpu::util::DeviceExt;
        let Some(coherence) = &self.coherence else {
            return false;
        };
        let id = super::super::workload::TargetId::Guest(writer.address);
        let texture = if writer.depth {
            &self.depthbuffers[&id].texture
        } else {
            self.framebuffers
                .get(&id)
                .filter(|target| target.layout == writer.layout)
                .map(|target| &target.color)
                .unwrap_or_else(|| &self.color_versions[&writer])
        };
        let pixels = coherence.import_pixels(writer, texture.height(), restore_from);
        if pixels.is_empty() {
            return false;
        }
        let stride = (texture.width() * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let mut data = vec![0; stride as usize * texture.height() as usize];
        for (&pixel, rgba) in &pixels {
            let offset = pixel / texture.width() as usize * stride as usize
                + pixel % texture.width() as usize * 4;
            data[offset..offset + 4].copy_from_slice(rgba);
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("guest-import"),
            contents: &data,
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        let source = writer.depth.then(|| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("guest-depth-import"),
                size: texture.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Float,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        });
        let destination = source.as_ref().unwrap_or(texture);
        let mut spans = Vec::<(u32, u32, u32)>::new();
        for pixel in pixels.keys().copied() {
            let x = pixel as u32 % texture.width();
            let y = pixel as u32 / texture.width();
            if let Some((_, _, width)) = spans
                .last_mut()
                .filter(|(start, row, width)| *row == y && *start + *width == x)
            {
                *width += 1;
            } else {
                spans.push((x, y, 1));
            }
        }
        for &(x, y, width) in &spans {
            encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: u64::from(y * stride + x * 4),
                        bytes_per_row: Some(stride),
                        rows_per_image: None,
                    },
                },
                wgpu::TexelCopyTextureInfo {
                    texture: destination,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
        }
        if let Some(source) = source {
            self.depth_pipeline.import(
                device,
                encoder,
                &source.create_view(&Default::default()),
                &self.depthbuffers[&id],
                &spans,
            );
        }
        self.profiling.count("import.pixels", pixels.len() as u64);
        true
    }
}
