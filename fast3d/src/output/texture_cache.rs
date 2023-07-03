use std::hash::Hash;
use std::num::NonZeroUsize;

use crate::output::models::OutputTexture;

use crate::models::texture::{ImageFormat, ImageSize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureCacheId(pub TextureConfig);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureConfig {
    pub game_address: usize,
    pub format: ImageFormat,
    pub size: ImageSize,
}

pub struct TextureCache {
    cache: lru::LruCache<TextureCacheId, OutputTexture>,
}

impl TextureCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: lru::LruCache::new(NonZeroUsize::new(capacity).unwrap()),
        }
    }

    pub fn contains(
        &mut self,
        game_address: usize,
        format: ImageFormat,
        size: ImageSize,
    ) -> Option<TextureCacheId> {
        let texture_cache_id = TextureCacheId(TextureConfig {
            game_address,
            format,
            size,
        });

        if let Some(_texture) = self.cache.get(&texture_cache_id) {
            return Some(texture_cache_id);
        }

        None
    }

    pub fn get(&mut self, texture_cache_id: TextureCacheId) -> Option<&OutputTexture> {
        if let Some(texture) = self.cache.get(&texture_cache_id) {
            return Some(texture);
        }

        None
    }

    pub fn get_mut(&mut self, texture_cache_id: TextureCacheId) -> Option<&mut OutputTexture> {
        if let Some(texture) = self.cache.get_mut(&texture_cache_id) {
            return Some(texture);
        }

        None
    }

    pub fn insert(
        &mut self,
        game_address: usize,
        format: ImageFormat,
        size: ImageSize,
        width: u32,
        height: u32,
        uls: u16,
        ult: u16,
        data: Vec<u8>,
    ) -> TextureCacheId {
        let texture = OutputTexture::new(game_address, format, size, width, height, uls, ult, data);
        let tex_cache_id = TextureCacheId(TextureConfig {
            game_address,
            format,
            size,
        });

        if let Some(_evicted_item) = self.cache.push(tex_cache_id, texture) {
            // TODO: handle evicted item by removing it from the GPU
        }

        tex_cache_id
    }
}
