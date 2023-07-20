use log::trace;
use pigment64::{ImageType, NativeImage};

pub fn translate_tile_rgba16(tmem: &[u8], tile_width: u32, tile_height: u32) -> Vec<u8> {
    let image = NativeImage::read(tmem, ImageType::Rgba16, tile_width, tile_height).unwrap();
    trace!("Decoding RGBA16 image");
    let mut decoded: Vec<u8> = Vec::new();
    image.decode(&mut decoded, None).unwrap();
    trace!("Decoded RGBA16 image");

    decoded
}

pub fn translate_tile_rgba32(tmem: &[u8], tile_width: u32, tile_height: u32) -> Vec<u8> {
    let image = NativeImage::read(tmem, ImageType::Rgba32, tile_width, tile_height).unwrap();
    trace!("Decoding RGBA32 image");
    let mut decoded: Vec<u8> = Vec::new();
    image.decode(&mut decoded, None).unwrap();
    trace!("Decoded RGBA32 image");

    decoded
}

pub fn translate_tile_ia4(tmem: &[u8], tile_width: u32, tile_height: u32) -> Vec<u8> {
    let image = NativeImage::read(tmem, ImageType::Ia4, tile_width, tile_height).unwrap();
    trace!("Decoding IA4 image");
    let mut decoded: Vec<u8> = Vec::new();
    image.decode(&mut decoded, None).unwrap();
    trace!("Decoded IA4 image");

    decoded
}

pub fn translate_tile_ia8(tmem: &[u8], tile_width: u32, tile_height: u32) -> Vec<u8> {
    let image = NativeImage::read(tmem, ImageType::Ia8, tile_width, tile_height).unwrap();
    trace!("Decoding IA8 image");
    let mut decoded: Vec<u8> = Vec::new();
    image.decode(&mut decoded, None).unwrap();
    trace!("Decoded IA8 image");

    decoded
}

pub fn translate_tile_ia16(tmem: &[u8], tile_width: u32, tile_height: u32) -> Vec<u8> {
    let image = NativeImage::read(tmem, ImageType::Ia16, tile_width, tile_height).unwrap();
    trace!("Decoding IA16 image");
    let mut decoded: Vec<u8> = Vec::new();
    image.decode(&mut decoded, None).unwrap();
    trace!("Decoded IA16 image");

    decoded
}

pub fn translate_tile_i4(tmem: &[u8], tile_width: u32, tile_height: u32) -> Vec<u8> {
    let image = NativeImage::read(tmem, ImageType::I4, tile_width, tile_height).unwrap();
    trace!("Decoding I4 image");
    let mut decoded: Vec<u8> = Vec::new();
    image.decode(&mut decoded, None).unwrap();
    trace!("Decoded I4 image");

    decoded
}

pub fn translate_tile_i8(tmem: &[u8], tile_width: u32, tile_height: u32) -> Vec<u8> {
    let image = NativeImage::read(tmem, ImageType::I8, tile_width, tile_height).unwrap();
    trace!("Decoding I8 image");
    let mut decoded: Vec<u8> = Vec::new();
    image.decode(&mut decoded, None).unwrap();
    trace!("Decoded I8 image");

    decoded
}

pub fn translate_tile_ci4(
    tmem: &[u8],
    palette: &[u8],
    tile_width: u32,
    tile_height: u32,
) -> Vec<u8> {
    let image = NativeImage::read(tmem, ImageType::I8, tile_width, tile_height).unwrap();
    trace!("Decoding CI4 image");
    let mut decoded: Vec<u8> = Vec::new();
    image.decode(&mut decoded, Some(palette)).unwrap();
    trace!("Decoded CI4 image");

    decoded
}

pub fn translate_tile_ci8(
    tmem: &[u8],
    palette: &[u8],
    tile_width: u32,
    tile_height: u32,
) -> Vec<u8> {
    let image = NativeImage::read(tmem, ImageType::I8, tile_width, tile_height).unwrap();
    trace!("Decoding CI8 image");
    let mut decoded: Vec<u8> = Vec::new();
    image.decode(&mut decoded, Some(palette)).unwrap();
    trace!("Decoded CI8 image");

    decoded
}

pub struct TextureState {
    pub on: bool,
    /// Index of parameter-setting tile descriptor (3bit precision, 0 - 7)
    pub tile: u8,
    pub level: u8,
    pub scale_s: u16,
    pub scale_t: u16,
}

impl TextureState {
    pub const EMPTY: Self = Self {
        on: false,
        tile: 0,
        level: 0,
        scale_s: 0,
        scale_t: 0,
    };

    pub fn new(on: bool, tile: u8, level: u8, scale_s: u16, scale_t: u16) -> Self {
        Self {
            on,
            tile,
            level,
            scale_s,
            scale_t,
        }
    }
}

pub struct TextureImageState {
    pub format: u8,
    pub size: u8,
    pub width: u16,
    pub address: usize,
}

impl TextureImageState {
    pub const EMPTY: Self = Self {
        format: 0,
        size: 0,
        width: 0,
        address: 0,
    };

    pub fn new(format: u8, size: u8, width: u16, address: usize) -> Self {
        Self {
            format,
            size,
            width,
            address,
        }
    }
}
