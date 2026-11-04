// Browser devices cannot synchronously map guest write-back buffers.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use super::ImageLayout;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Writer {
    pub address: u64,
    pub layout: ImageLayout,
    pub depth: bool,
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

    pub fn encode(self, rgba: [u8; 4], pixel: usize, literal: Option<u32>) -> Vec<u8> {
        if !self.supported() {
            return vec![0; self.pixel_bytes()];
        }
        if let Some(word) = literal {
            return if self.pixel_bytes() == 2 {
                ((word >> (16 * (1 - pixel % 2))) as u16)
                    .to_ne_bytes()
                    .to_vec()
            } else {
                vec![(word >> (8 * (3 - pixel % 4))) as u8]
            };
        }
        if self.depth {
            super::pack_depth(f32::from_ne_bytes(rgba))
                .to_ne_bytes()
                .to_vec()
        } else if self.pixel_bytes() == 1 {
            vec![rgba[(pixel % self.layout.width as usize) & 1]]
        } else {
            const MAGIC: [u8; 16] = [0, 6, 1, 7, 4, 2, 5, 3, 3, 5, 2, 4, 7, 1, 6, 0];
            let x = pixel % self.layout.width as usize;
            let y = pixel / self.layout.width as usize;
            pack_color(rgba, MAGIC[(y & 3) * 4 + (x & 3)])
                .to_ne_bytes()
                .to_vec()
        }
    }
}

fn pack_color(rgba: [u8; 4], dither: u8) -> u16 {
    // rt64's write-back takes the alpha bit from coverage's three low bits.
    let channel = |i: usize| u16::from(rgba[i].saturating_add(dither) >> 3);
    (channel(0) << 11) | (channel(1) << 6) | (channel(2) << 1) | u16::from((rgba[3] & 4) != 0)
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
            assert_eq!(pack_color(rgba, dither), expected);
        }
    }

    #[test]
    fn rgba16_dither_follows_rt64_magic_square_by_row_and_column() {
        let writer = image(0, 5, 2, false);
        let dither = |x: usize, y: usize| {
            (1..8u8)
                .filter(|&threshold| {
                    let word = writer.encode([8 - threshold, 0, 0, 255], y * 5 + x, None);
                    u16::from_ne_bytes(word.try_into().unwrap()) >> 11 == 1
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
            assert_eq!(writer.encode([11, 27, 91, 255], 0, None), vec![11]);
            assert_eq!(writer.encode([11, 27, 91, 255], 1, None), vec![27]);
        }
    }

    #[test]
    fn literal_fill_preserves_words_and_parity() {
        let writer = image(0, 3, 2, false);
        assert_eq!(
            writer.encode([0; 4], 0, Some(0x1234_abcd)),
            0x1234u16.to_ne_bytes()
        );
        assert_eq!(
            writer.encode([0; 4], 3, Some(0x1234_abcd)),
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
    pipelines: std::collections::HashMap<(bool, bool, bool, bool, bool), wgpu::RenderPipeline>,
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
        self.pipeline_for(device, textured, (cull, test, write, decal, any_depth))
    }

    /// Build every pipeline variant up front: a cold first use compiles one mid-frame, and enough of
    /// those in one task trip a guest's RCP watchdog.
    pub(in crate::render) fn warm(
        &mut self,
        device: &wgpu::Device,
        textured: &super::super::TexturedPipeline,
    ) {
        for bits in 0..32u8 {
            let flag = |bit: u8| bits & (1 << bit) != 0;
            // A depth-writing decal always has a depth attachment; no draw reaches the other case.
            if flag(3) && flag(2) && !flag(4) {
                continue;
            }
            self.pipeline_for(
                device,
                textured,
                (flag(0), flag(1), flag(2), flag(3), flag(4)),
            );
        }
    }

    fn pipeline_for(
        &mut self,
        device: &wgpu::Device,
        textured: &super::super::TexturedPipeline,
        key: (bool, bool, bool, bool, bool),
    ) -> &wgpu::RenderPipeline {
        let (cull, test, write, decal, any_depth) = key;
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
            device.create_render_pipeline(&descriptor)
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

struct Snapshot {
    coverage: Readback,
    color: Option<(Writer, Readback)>,
    depth: Option<(Writer, Readback)>,
    literals: Vec<Option<u32>>,
    generation: u64,
}

#[derive(Default)]
pub(crate) struct Coherence {
    pub owners: Ownership,
    generation: u64,
    first_generation: u64,
    snapshots: Vec<Snapshot>,
    pending: Vec<(u64, Writer, u64, Vec<u8>)>,
    /// Edits reported once by `release_changed`, queued until an accepted task uploads them.
    pub imports: Vec<crate::GuestWrite>,
    pub imported: bool,
}

impl Coherence {
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
            let bytes = (pixel..pixel + width as usize)
                .flat_map(|p| writer.encode([0; 4], p, Some(word)))
                .collect();
            self.pending.push((
                self.generation,
                writer,
                writer.address + (pixel * writer.pixel_bytes()) as u64,
                bytes,
            ));
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn snapshot(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        coverage: &wgpu::Texture,
        color: Option<(Writer, &wgpu::Texture)>,
        depth: Option<(Writer, &wgpu::Texture)>,
        literals: Vec<Option<u32>>,
    ) {
        let generation = self.generation;
        self.generation += literals.len() as u64 * 2;
        self.snapshots.push(Snapshot {
            coverage: Readback::new(device, encoder, coverage, 8),
            color: color
                .map(|(writer, texture)| (writer, Readback::new(device, encoder, texture, 4))),
            depth: depth
                .map(|(writer, texture)| (writer, Readback::new(device, encoder, texture, 4))),
            literals,
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
            .any(|(_, _, address, bytes)| overlaps(*address, bytes.len() as u64))
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

    #[cfg(not(target_arch = "wasm32"))]
    pub fn resolve(&mut self, device: &wgpu::Device) {
        for snapshot in self.snapshots.drain(..) {
            let stamps = snapshot.coverage.read(device);
            for (channel, target) in [(0, snapshot.color), (1, snapshot.depth)] {
                let Some((writer, readback)) = target else {
                    continue;
                };
                let data = readback.read(device);
                let mut start = 0;
                while start < stamps.len() / 8 {
                    let stamp = |pixel: usize| {
                        u32::from_ne_bytes(
                            stamps[pixel * 8 + channel * 4..pixel * 8 + channel * 4 + 4]
                                .try_into()
                                .unwrap(),
                        )
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
                    let bytes = (start..end)
                        .flat_map(|pixel| {
                            writer.encode(
                                data[pixel * 4..pixel * 4 + 4].try_into().unwrap(),
                                pixel,
                                if writer.depth {
                                    None
                                } else {
                                    snapshot.literals[slot as usize - 1]
                                },
                            )
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
                    self.pending.push((
                        generation,
                        writer,
                        writer.address + writer.byte_offset(start),
                        bytes,
                    ));
                    start = end;
                }
            }
        }
        self.pending
            .sort_by_key(|(generation, _, _, _)| *generation);
        for (generation, writer, address, bytes) in self.pending.drain(..) {
            self.owners.write(writer, generation, address, bytes);
        }
        self.owners.coalesce(self.first_generation);
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
                    let word = u16::from_ne_bytes(bytes.try_into().unwrap());
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
