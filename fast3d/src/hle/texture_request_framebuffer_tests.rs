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
            prepare(&rdp, &tile, 3, &Default::default()),
            Err(crate::DiagKind::UnsupportedFramebufferAccess {
                reason: crate::FramebufferAccess::Reinterpretation,
                ..
            })
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
