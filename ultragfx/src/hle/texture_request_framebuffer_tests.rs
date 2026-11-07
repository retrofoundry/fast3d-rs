use std::sync::Arc;

use super::{prepare, TextureSource};
use crate::hle::framebuffer_load::{FramebufferCopy, Tag, Tags};
use crate::hle::rdp::{Rdp, TileDescriptor};
use crate::render::framebuffers::{targets::ImageDescriptor, ImageLayout};

fn tags() -> Tags {
    let source = ImageDescriptor {
        address: 0x10000,
        layout: ImageLayout {
            width: 4,
            fmt: 0,
            siz: 2,
        },
        height: 1,
        generation: 1,
        depth: false,
    };
    Tags {
        copies: vec![Arc::new(FramebufferCopy::new(
            1,
            source,
            source.address,
            8,
            8,
            1,
        ))],
        ..Default::default()
    }
}

#[test]
fn tlut_halfword_ignores_framebuffer_tag_on_unused_low_byte() {
    for size in [2, 3] {
        let mut rdp = Rdp::default();
        rdp.tmem_bank
            .write_tile(&[0x10, 0xff].repeat(4), 256, 0, 1, 1, 8, 1);
        let mut tags = tags();
        tags.bytes[1] = Tag {
            copy: 1,
            pointer: 1,
        };
        rdp.tmem_bank.framebuffer_tags = Some(tags);
        let tile = TileDescriptor {
            fmt: 0,
            siz: size,
            width: 1,
            height: 1,
            ..Default::default()
        };
        let binding = prepare(&rdp, &tile, 3, &Default::default()).unwrap();
        assert!(matches!(binding.source, TextureSource::Encoded(_)));
        assert_eq!(binding.source.decode().as_ref(), [0x10, 0x10, 0x10, 0xff]);

        rdp.tmem_bank.framebuffer_tags.as_mut().unwrap().bytes[0] = Tag {
            copy: 1,
            pointer: 0,
        };
        assert!(matches!(
            prepare(&rdp, &tile, 3, &Default::default()).unwrap().source,
            TextureSource::Framebuffer(_)
        ));
    }
}

#[test]
fn framebuffer_ci8_keeps_lanes_when_palette_or_index_values_are_unknown() {
    for tagged_index in [false, true] {
        let mut rdp = Rdp::default();
        let mut tags = tags();
        if tagged_index {
            tags.bytes[0] = Tag {
                copy: 1,
                pointer: 0,
            };
            rdp.tmem_bank.write_tile(
                &[0x10, 0xff, 0x30, 0xff, 0x50, 0xff, 0x70, 0xff],
                257,
                0,
                1,
                1,
                8,
                1,
            );
        } else {
            for offset in 0..8 {
                tags.bytes[2048 + offset] = Tag {
                    copy: 1,
                    pointer: offset as u32,
                };
            }
        }
        rdp.tmem_bank.framebuffer_tags = Some(tags);
        let tile = TileDescriptor {
            fmt: 2,
            siz: 1,
            width: 1,
            height: 1,
            ..Default::default()
        };
        let binding = prepare(&rdp, &tile, 3, &Default::default()).unwrap();
        assert_eq!(binding.sampling.palette[2], 1);
        assert_eq!(binding.sampling.allocation_extent(), [1, 4]);
        let TextureSource::Framebuffer(request) = binding.source else {
            panic!("expected framebuffer request")
        };
        assert_eq!(request.encoded.recipe().allocation_extent(), [1, 4]);
        if tagged_index {
            assert_eq!(
                request.tags.bytes[0],
                Tag {
                    copy: 1,
                    pointer: 0
                }
            );
        } else {
            for lane in 0..4 {
                assert_eq!(
                    request.tags.bytes[2048 + lane * 2],
                    Tag {
                        copy: 1,
                        pointer: lane as u32 * 2
                    }
                );
            }
        }
    }
}

#[test]
fn framebuffer_ci8_unknown_index_keeps_all_palette_entries_in_equality() {
    let mut rdp = Rdp::default();
    let mut tags = tags();
    tags.bytes[0] = Tag {
        copy: 1,
        pointer: 0,
    };
    rdp.tmem_bank.framebuffer_tags = Some(tags);
    rdp.tmem_bank
        .write_tile(&[0x10, 0xff].repeat(4), 257, 0, 1, 1, 8, 1);
    let tile = TileDescriptor {
        fmt: 2,
        siz: 1,
        width: 1,
        height: 1,
        ..Default::default()
    };
    let first = prepare(&rdp, &tile, 3, &Default::default()).unwrap();
    rdp.tmem_bank
        .write_tile(&[0x20, 0xff].repeat(4), 257, 0, 1, 1, 8, 1);
    let second = prepare(&rdp, &tile, 3, &Default::default()).unwrap();
    assert!(matches!(first.source, TextureSource::Framebuffer(_)));
    assert!(matches!(second.source, TextureSource::Framebuffer(_)));
    assert!(first.source != second.source);
    assert!(!first.source.matches(&second.source, &Default::default()));
}

#[test]
fn framebuffer_requests_accept_every_validated_format() {
    for fmt in 0..=7 {
        for siz in 0..=3 {
            for tlut in 0..=3 {
                if (crate::hle::texdec::FormatInfo { fmt, siz })
                    .validate_tlut(tlut)
                    .is_err()
                {
                    continue;
                }
                let mut rdp = Rdp::default();
                let mut tags = tags();
                tags.bytes[0] = Tag {
                    copy: 1,
                    pointer: 0,
                };
                tags.bytes[1] = Tag {
                    copy: 1,
                    pointer: 1,
                };
                rdp.tmem_bank.framebuffer_tags = Some(tags);
                let tile = TileDescriptor {
                    fmt,
                    siz,
                    width: 2,
                    height: 1,
                    ..Default::default()
                };
                let binding = prepare(&rdp, &tile, tlut, &Default::default())
                    .unwrap_or_else(|error| panic!("{fmt}/{siz}/tlut{tlut}: {error:?}"));
                assert!(matches!(binding.source, TextureSource::Framebuffer(_)));
            }
        }
    }
}

#[test]
fn framebuffer_precision_requires_complete_matching_pairs_without_palette_tags() {
    for (fmt, siz, tlut) in [(0, 2, 0), (3, 2, 0), (2, 1, 2), (2, 1, 3)] {
        for mismatch in 0..5 {
            let mut rdp = Rdp::default();
            let mut tags = tags();
            tags.bytes[0] = Tag {
                copy: 1,
                pointer: 0,
            };
            tags.bytes[1] = Tag {
                copy: 1,
                pointer: 1,
            };
            match mismatch {
                1 => tags.bytes[1] = Tag::default(),
                2 => tags.bytes[1].pointer = 3,
                3 => {
                    tags.copies.push(Arc::new(FramebufferCopy::new(
                        2,
                        tags.copies[0].source,
                        0x10000,
                        8,
                        8,
                        1,
                    )));
                    tags.bytes[1].copy = 2;
                }
                4 => {
                    tags.bytes[0].pointer = 1;
                    tags.bytes[1].pointer = 2;
                }
                _ => {}
            }
            rdp.tmem_bank.framebuffer_tags = Some(tags);
            let tile = TileDescriptor {
                fmt,
                siz,
                width: 2,
                height: 1,
                ..Default::default()
            };
            let binding = prepare(&rdp, &tile, tlut, &Default::default()).unwrap();
            let TextureSource::Framebuffer(request) = binding.source else {
                panic!()
            };
            assert_eq!(
                request.materialize,
                mismatch != 0,
                "{fmt}/{siz}/{tlut}/{mismatch}"
            );
            if tlut >= 2 {
                let tags = rdp.tmem_bank.framebuffer_tags.as_mut().unwrap();
                tags.bytes[2048] = Tag {
                    copy: 1,
                    pointer: 0,
                };
                tags.bytes[2049] = Tag {
                    copy: 1,
                    pointer: 1,
                };
                let binding = prepare(&rdp, &tile, tlut, &Default::default()).unwrap();
                let TextureSource::Framebuffer(request) = binding.source else {
                    panic!()
                };
                assert!(request.materialize);
            }
        }
    }
}

#[test]
fn framebuffer_materialisation_does_not_merge_copy_snapshots() {
    let mut rdp = Rdp::default();
    let copy = tags().copies[0].clone();
    rdp.tmem_bank
        .copy_load(copy.clone(), |bank| bank.write_tile(&[], 0, 1, 1, 1, 8, 1));
    let tile = TileDescriptor {
        fmt: 3,
        siz: 1,
        width: 4,
        height: 1,
        line: 1,
        ..Default::default()
    };
    let first = prepare(&rdp, &tile, 0, &Default::default()).unwrap();
    let same = prepare(&rdp, &tile, 0, &Default::default()).unwrap();
    assert_eq!(first, same);
    let mut next = (*copy).clone();
    next.sequence += 1;
    rdp.tmem_bank.copy_load(Arc::new(next), |bank| {
        bank.write_tile(&[], 0, 1, 1, 1, 8, 1)
    });
    let second = prepare(&rdp, &tile, 0, &Default::default()).unwrap();
    assert_ne!(first, second);
    assert!(!first.source.matches(&second.source, &Default::default()));
    let TextureSource::Framebuffer(retained) = first.source else {
        panic!()
    };
    assert!(retained.materialize);
    assert_eq!(retained.tags.copies[0].sequence, 1);
}

#[test]
fn framebuffer_t_wrap_lookup_keeps_tags_in_its_full_decode_domain() {
    let mut rdp = Rdp::default();
    let mut tags = tags();
    tags.bytes[2052] = Tag {
        copy: 1,
        pointer: 0,
    };
    rdp.tmem_bank.framebuffer_tags = Some(tags);
    let tile = TileDescriptor {
        fmt: 4,
        siz: 1,
        width: 1,
        height: 258,
        line: 1,
        ..Default::default()
    };
    let binding = prepare(&rdp, &tile, 0, &Default::default()).unwrap();
    let TextureSource::Framebuffer(request) = binding.source else {
        panic!()
    };
    assert_eq!(
        request.encoded.recipe().representation,
        super::Representation::Lookup4096x4
    );
    assert_eq!(request.tags.bytes[2052].copy, 1);
}
