use std::sync::Arc;

use crate::render::framebuffers::targets::ImageDescriptor;

/// An ordered snapshot of rendered pixels read by a texture load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FramebufferCopy {
    pub(crate) sequence: u64,
    byte_offset: u32,
    pub(crate) source: ImageDescriptor,
    pub(crate) spans: Vec<CopySpan>,
    pub(crate) size: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CopySpan {
    pub offset: u32,
    pub origin: [u32; 2],
    pub width: u32,
    pub buffer_offset: u32,
}

impl FramebufferCopy {
    pub(crate) fn new(
        sequence: u64,
        source: ImageDescriptor,
        address: u64,
        row_bytes: u32,
        stride: u32,
        rows: u32,
    ) -> Self {
        let byte_offset = ((address - source.address) & 1) as u32;
        let mut spans = Vec::new();
        let mut size = 0;
        for row in 0..rows {
            let mut pixel = (address - source.address + u64::from(row * stride)) / 2;
            let parity = ((address - source.address + u64::from(row * stride)) & 1) as u32;
            let mut remaining = (row_bytes + parity).div_ceil(2);
            let mut offset = row * stride + byte_offset - parity;
            while remaining != 0 {
                let x = pixel as u32 % source.layout.width;
                let width = remaining.min(source.layout.width - x);
                spans.push(CopySpan {
                    offset,
                    origin: [x, (pixel / u64::from(source.layout.width)) as u32],
                    width,
                    buffer_offset: size,
                });
                size += (width * 4).next_multiple_of(256);
                offset += width * 2;
                pixel += u64::from(width);
                remaining -= width;
            }
        }
        spans.sort_by_key(|span| span.offset);
        Self {
            sequence,
            byte_offset,
            source,
            spans,
            size,
        }
    }

    pub(crate) fn pointer(&self, offset: u32) -> u32 {
        let offset = offset + self.byte_offset;
        let span = &self.spans[self.spans.partition_point(|span| span.offset <= offset) - 1];
        (span.buffer_offset / 4 + (offset - span.offset) / 2) * 2 + (offset & 1)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tag {
    pub copy: u16,
    pub pointer: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tags {
    pub bytes: Box<[Tag; 4096]>,
    pub copies: Vec<Arc<FramebufferCopy>>,
}

impl Default for Tags {
    fn default() -> Self {
        Self {
            bytes: Box::new([Tag::default(); 4096]),
            copies: Vec::new(),
        }
    }
}

/// A texture request whose TMEM includes rendered framebuffer pixels.
#[derive(Debug)]
pub struct FramebufferTexture {
    pub(crate) encoded: super::texture_request::EncodedTextureRequest,
    pub(crate) tags: Tags,
    pub(crate) materialize: bool,
}

impl FramebufferTexture {
    pub(crate) fn image_route(&self) -> bool {
        let recipe = self.encoded.recipe();
        if self.materialize
            || recipe.representation as u32 != 0
            || !matches!((recipe.fmt, recipe.siz, recipe.tlut), (0 | 3, 2, 0 | 1))
        {
            return false;
        }
        (0..recipe.output[1]).all(|y| {
            (0..recipe.output[0]).all(|x| {
                let relative = (y & 255) * u32::from(recipe.line) * 8 + x * 2;
                let address = |byte| {
                    ((u32::from(recipe.base) * 8 + ((relative + byte) ^ ((y & 1) * 4))) & 4095)
                        as usize
                };
                let hi = self.tags.bytes[address(0)];
                let lo = self.tags.bytes[address(1)];
                hi.copy != 0
                    && hi.copy == lo.copy
                    && hi.pointer & 1 == 0
                    && lo.pointer == hi.pointer + 1
            })
        })
    }

    pub(crate) fn new(encoded: super::texture_request::EncodedTextureRequest, tags: Tags) -> Self {
        let recipe = encoded.recipe();
        let exact_format = matches!(
            (recipe.fmt, recipe.siz, recipe.tlut),
            (0 | 3, 2, 0 | 1) | (2, 1, 2 | 3)
        );
        let palette_tags = recipe.tlut >= 2 && tags.bytes[2048..].iter().any(|tag| tag.copy != 0);
        let complete_pairs = tags.bytes.as_chunks::<2>().0.iter().all(|pair| {
            let [hi, lo] = [pair[0], pair[1]];
            (hi.copy == 0 && lo.copy == 0)
                || (hi.copy != 0
                    && hi.copy == lo.copy
                    && hi.pointer & 1 == 0
                    && lo.pointer == hi.pointer + 1)
        });
        Self {
            materialize: !exact_format || palette_tags || !complete_pairs,
            encoded,
            tags,
        }
    }
}

impl PartialEq for FramebufferTexture {
    fn eq(&self, other: &Self) -> bool {
        if self.materialize != other.materialize || self.tags != other.tags {
            return false;
        }
        if self.encoded.recipe().tlut < 2 {
            return self.encoded == other.encoded;
        }
        // Framebuffer indices can select palette entries absent from the CPU placeholders.
        let (
            super::texture_request::EncodedInput::Tmem(a),
            super::texture_request::EncodedInput::Tmem(b),
        ) = (self.encoded.input(), other.encoded.input());
        self.encoded.recipe() == other.encoded.recipe() && a == b
    }
}
