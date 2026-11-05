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
