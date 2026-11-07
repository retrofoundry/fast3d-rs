use super::*;
use crate::hle::{
    framebuffer_load::Tag,
    rdp::{Rdp, TileDescriptor},
    texture_request::{prepare, TextureSource},
};
use crate::render::framebuffers::{coherence::Writer, targets::ImageDescriptor, ImageLayout};

const PIXELS: [[u8; 4]; 4] = [
    [19, 85, 141, 255],
    [67, 133, 201, 3],
    [245, 29, 99, 4],
    [111, 177, 43, 128],
];
const QUANTIZED: [[u8; 4]; 4] = [
    [16, 82, 140, 255],
    [66, 132, 206, 0],
    [247, 24, 99, 255],
    [107, 181, 41, 0],
];

fn copy(rdp: &mut Rdp, base: usize, rows: usize) {
    let source = ImageDescriptor {
        address: 0x10000,
        layout: ImageLayout {
            width: 4,
            fmt: 0,
            siz: 2,
        },
        height: rows as u32,
        generation: 1,
        depth: false,
    };
    let copy = Arc::new(FramebufferCopy::new(
        rdp.tmem_bank.next_copy_id(),
        source,
        source.address,
        8,
        8,
        rows as u32,
    ));
    rdp.tmem_bank
        .copy_load(copy, |bank| bank.write_tile(&[], base, 1, rows, 1, 8, 1));
}

fn tile(fmt: u8, siz: u8, width: u16, height: u16) -> TileDescriptor {
    TileDescriptor {
        fmt,
        siz,
        width,
        height,
        line: 1,
        cms: 2,
        cmt: 2,
        ..Default::default()
    }
}

fn request(rdp: &Rdp, tile: &TileDescriptor, tlut: u8) -> Arc<FramebufferTexture> {
    let binding = prepare(rdp, tile, tlut, &Recorder::default()).unwrap();
    let TextureSource::Framebuffer(request) = binding.source else {
        panic!("expected GPU request")
    };
    request
}

fn pixels(request: &Arc<FramebufferTexture>) -> Vec<[u8; 4]> {
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let recorder = Recorder::new(crate::profiling::Mode::Counters);
    let mut copies = Copies::default();
    copies.begin(&device, request.tags.copies.iter().cloned(), 1);
    copies.texture(&device, &queue, request, &recorder, false);
    let output = copies
        .pending
        .values()
        .flatten()
        .next()
        .unwrap()
        .job
        .texture
        .clone();
    copies.texture(&device, &queue, request, &recorder, false);
    assert_eq!(copies.pending.values().map(Vec::len).sum::<usize>(), 1);
    let mut encoder = device.create_command_encoder(&Default::default());
    for copy in &request.tags.copies {
        let source = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 4,
                height: copy.source.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let row = if copy.sequence == 1 {
            PIXELS
        } else {
            [[8, 49, 90, 255]; 4]
        };
        queue.write_texture(
            source.as_image_copy(),
            &row.concat().repeat(copy.source.height as usize),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16),
                rows_per_image: None,
            },
            source.size(),
        );
        let versions = HashMap::from([(
            Writer {
                address: copy.source.address,
                layout: copy.source.layout,
                depth: false,
            },
            source,
        )]);
        copies.copy(
            &mut encoder,
            copy,
            &HashMap::new(),
            &versions,
            &HashMap::new(),
            None,
            &recorder,
        );
    }
    assert!(copies.pending.is_empty());
    assert!(copies.failed.is_empty());
    let stride = (output.width() * 4).next_multiple_of(256);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(stride) * u64::from(output.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: None,
            },
        },
        output.size(),
    );
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.map_async(wgpu::MapMode::Read, .., move |result| {
        tx.send(result).unwrap()
    });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let mapped = buffer.get_mapped_range(..);
    mapped
        .chunks_exact(stride as usize)
        .flat_map(|row| {
            row[..output.width() as usize * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .copied()
        })
        .collect()
}

#[test]
fn framebuffer_decode_complete_pairs_keep_rgba_and_ia_precision() {
    let mut rdp = Rdp::default();
    copy(&mut rdp, 0, 1);
    let rgba = request(&rdp, &tile(0, 2, 4, 1), 0);
    assert!(!rgba.materialize);
    assert_eq!(pixels(&rgba), PIXELS);
    let ia = request(&rdp, &tile(3, 2, 4, 1), 0);
    assert!(!ia.materialize);
    assert_eq!(
        pixels(&ia),
        [
            [19, 19, 19, 89],
            [67, 67, 67, 108],
            [245, 245, 245, 70],
            [111, 111, 111, 98]
        ]
    );
}

#[test]
fn framebuffer_decode_split_and_mismatched_pairs_materialize_the_whole_request() {
    for (pointer, low, first) in [
        (0, None, [16, 66, 132, 255]),
        (0, Some(3), [16, 66, 206, 0]),
        (1, Some(2), [165, 107, 16, 0]),
    ] {
        let mut rdp = Rdp::default();
        copy(&mut rdp, 0, 1);
        let mut tags = rdp.tmem_bank.framebuffer_tags.clone().unwrap();
        rdp.tmem_bank
            .write_tile(&[0, 0x21, 0, 0, 0, 0, 0, 0], 0, 1, 1, 1, 8, 1);
        tags.bytes[0].pointer = pointer;
        tags.bytes[1] = low.map_or(Tag::default(), |pointer| Tag { copy: 1, pointer });
        rdp.tmem_bank.framebuffer_tags = Some(tags);
        let request = request(&rdp, &tile(0, 2, 4, 1), 0);
        assert!(request.materialize);
        assert_eq!(
            pixels(&request),
            [first, QUANTIZED[1], QUANTIZED[2], QUANTIZED[3]]
        );
    }
}

#[test]
fn framebuffer_decode_multiple_copies_and_cross_copy_pair() {
    let mut rdp = Rdp::default();
    copy(&mut rdp, 0, 1);
    copy(&mut rdp, 1, 1);
    let exact = request(&rdp, &tile(0, 2, 8, 1), 0);
    assert!(!exact.materialize);
    assert_eq!(
        pixels(&exact),
        [PIXELS.to_vec(), vec![[8, 49, 90, 255]; 4]].concat()
    );
    rdp.tmem_bank.framebuffer_tags.as_mut().unwrap().bytes[1] = Tag {
        copy: 2,
        pointer: 1,
    };
    let materialized = request(&rdp, &tile(0, 2, 8, 1), 0);
    assert!(materialized.materialize);
    assert_eq!(
        pixels(&materialized),
        [
            vec![[16, 82, 90, 255]],
            QUANTIZED[1..].to_vec(),
            vec![[8, 49, 90, 255]; 4]
        ]
        .concat()
    );
}

#[test]
fn framebuffer_decode_stale_crystal_ia8_i4_ci4_and_overwritten_tags() {
    let mut rdp = Rdp::default();
    copy(&mut rdp, 0, 4);
    rdp.tmem_bank.write_block(&[0x56; 16], 0, 0, 0, 2, 1);
    for (fmt, siz, expected) in [
        (
            3,
            1,
            [
                vec![[85, 85, 85, 102]; 8],
                [
                    [17, 17, 17, 34],
                    [170, 170, 170, 51],
                    [68; 4],
                    [51, 51, 51, 34],
                ]
                .repeat(2),
            ]
            .concat(),
        ),
        (
            4,
            0,
            [
                [[85; 4], [102; 4], [85; 4], [102; 4]].repeat(2),
                [[17; 4], [34; 4], [170; 4], [51; 4]].repeat(2),
            ]
            .concat(),
        ),
        (
            2,
            0,
            [
                [[0x55; 4], [0x56; 4], [0x55; 4], [0x56; 4]].repeat(2),
                [[0x51; 4], [0x52; 4], [0x5a; 4], [0x53; 4]].repeat(2),
            ]
            .concat(),
        ),
    ] {
        let mut tile = tile(fmt, siz, 4, 4);
        tile.palette = 5;
        let request = request(&rdp, &tile, 0);
        assert!(request.materialize);
        assert_eq!(pixels(&request), expected, "{fmt}/{siz}");
        assert!(request.tags.bytes[..16].iter().all(|tag| tag.copy == 0));
    }
    rdp.tmem_bank.write_block(&[0x56; 32], 0, 0, 0, 4, 1);
    assert!(matches!(
        prepare(&rdp, &tile(3, 1, 4, 4), 0, &Recorder::default())
            .unwrap()
            .source,
        TextureSource::Encoded(_)
    ));
}

#[test]
fn framebuffer_decode_rgba32_materializes_both_banks() {
    let mut rdp = Rdp::default();
    copy(&mut rdp, 0, 1);
    copy(&mut rdp, 256, 1);
    let request = request(&rdp, &tile(0, 3, 4, 1), 0);
    assert!(request.materialize);
    assert_eq!(
        pixels(&request),
        [
            [0x12, 0xa3, 0x09, 0x97],
            [0x44, 0x32, 0x09, 0x97],
            [0xf0, 0xd9, 0x09, 0x97],
            [0x6d, 0x8a, 0x09, 0x97]
        ]
    );
}

#[test]
fn framebuffer_decode_tagged_palette_lanes_for_ci_and_non_ci_tlut() {
    for (fmt, siz) in (0..=7).flat_map(|fmt| (0..=3).map(move |siz| (fmt, siz))) {
        for tlut in [2, 3] {
            let mut rdp = Rdp::default();
            copy(&mut rdp, 256, 1);
            let request = request(&rdp, &tile(fmt, siz, 1, 1), tlut);
            assert!(request.materialize);
            assert!(request.encoded.recipe().palette_lanes);
            let expected = if tlut == 2 {
                QUANTIZED
            } else {
                [
                    [0x12, 0x12, 0x12, 0xa3],
                    [0x44, 0x44, 0x44, 0x32],
                    [0xf0, 0xf0, 0xf0, 0xd9],
                    [0x6d, 0x6d, 0x6d, 0x8a],
                ]
            };
            let plane = request.encoded.recipe().output.iter().product::<u32>() as usize;
            assert_eq!(
                pixels(&request),
                expected
                    .into_iter()
                    .flat_map(|color| vec![color; plane])
                    .collect::<Vec<_>>(),
                "{fmt}/{siz}/tlut{tlut}"
            );
        }
    }
}

#[test]
fn framebuffer_decode_tagged_indices_select_unknown_palette_entries() {
    for (fmt, siz, width) in [(2, 0, 4), (2, 1, 2), (0, 2, 1), (3, 1, 2), (4, 0, 4)] {
        let mut rdp = Rdp::default();
        for index in [0x51, 0x52, 0x5a, 0x53, 0x12, 0xa3] {
            rdp.tmem_bank.write_tile(
                &[index, 0xfe].repeat(4),
                256 + usize::from(index),
                0,
                1,
                1,
                8,
                1,
            );
        }
        copy(&mut rdp, 0, 1);
        let mut tile = tile(fmt, siz, width, 1);
        tile.palette = 5;
        let request = request(&rdp, &tile, 3);
        let expected = if siz == 0 {
            vec![
                [0x51, 0x51, 0x51, 0xfe],
                [0x52, 0x52, 0x52, 0xfe],
                [0x5a, 0x5a, 0x5a, 0xfe],
                [0x53, 0x53, 0x53, 0xfe],
            ]
        } else if siz == 1 {
            vec![[0x12, 0x12, 0x12, 0xfe], [0xa3, 0xa3, 0xa3, 0xfe]]
        } else {
            vec![[0x12, 0x12, 0x12, 0xfe]]
        };
        assert_eq!(pixels(&request), expected, "{fmt}/{siz}");
    }
}

#[test]
fn framebuffer_decode_materialized_t_wrap_and_lookup() {
    let mut rdp = Rdp::default();
    copy(&mut rdp, 0, 1);
    let mut wrapped = tile(4, 1, 1, 258);
    wrapped.line = 2;
    let output = pixels(&request(&rdp, &wrapped, 0));
    assert_eq!(output[0], [0x12; 4]);
    assert_eq!(output[256], [0x12; 4]);
    assert_eq!(output[1], [0; 4]);
    assert_eq!(output[257], [0; 4]);
    let mut tile = tile(4, 0, 4, 1);
    tile.cms = 0;
    tile.masks = 4;
    let request = request(&rdp, &tile, 0);
    assert_eq!(request.encoded.recipe().allocation_extent(), [4096, 4]);
    let output = pixels(&request);
    assert_eq!(output[0], [17; 4]);
    assert_eq!(output[4096], [34; 4]);
    assert_eq!(output[8192], [255; 4]);
    assert_eq!(output[12288], [0; 4]);
}

#[test]
fn framebuffer_decode_coverage_alpha_threshold() {
    let mut rdp = Rdp::default();
    copy(&mut rdp, 0, 1);
    let request = request(&rdp, &tile(4, 1, 8, 1), 0);
    assert_eq!(
        pixels(&request),
        [[0x12; 4], [0xa3; 4], [0x44; 4], [0x32; 4], [0xf0; 4], [0xd9; 4], [0x6d; 4], [0x8a; 4]]
    );
}

#[test]
fn framebuffer_decode_materialized_formats_without_tlut() {
    for (fmt, siz, width, expected) in [
        (
            3,
            0,
            4,
            vec![
                [0, 0, 0, 255],
                [36, 36, 36, 0],
                [182, 182, 182, 0],
                [36, 36, 36, 255],
            ],
        ),
        (2, 1, 4, vec![[0x12; 4], [0xa3; 4], [0x44; 4], [0x32; 4]]),
        (
            2,
            2,
            2,
            vec![[0x12, 0xa3, 0x12, 0xa3], [0x44, 0x32, 0x44, 0x32]],
        ),
        (
            2,
            3,
            2,
            vec![[0x12, 0xa3, 0x12, 0xa3], [0x44, 0x32, 0x44, 0x32]],
        ),
    ] {
        let mut rdp = Rdp::default();
        copy(&mut rdp, 0, 1);
        for tlut in [0, 1] {
            let request = request(&rdp, &tile(fmt, siz, width, 1), tlut);
            assert!(request.materialize);
            assert_eq!(pixels(&request), expected, "{fmt}/{siz}/tlut{tlut}");
        }
    }
}

#[test]
fn image_route_requires_every_sample_to_use_complete_image_pairs() {
    let mut rdp = Rdp::default();
    copy(&mut rdp, 0, 2);
    for fmt in [0, 3] {
        assert!(request(&rdp, &tile(fmt, 2, 4, 2), 0).image_route());
    }
    let ci = request(&rdp, &tile(2, 1, 8, 2), 2);
    assert!(!ci.materialize);
    assert!(!ci.image_route());
    let mut mixed = rdp.clone();
    mixed.tmem_bank.write_tile(&[0x12, 0x34], 0, 1, 1, 1, 2, 1);
    let mixed = request(&mixed, &tile(0, 2, 4, 2), 0);
    assert!(!mixed.materialize);
    assert!(!mixed.image_route());
    rdp.tmem_bank.framebuffer_tags.as_mut().unwrap().bytes[1] = Tag::default();
    assert!(!request(&rdp, &tile(0, 2, 4, 2), 0).image_route());
}
