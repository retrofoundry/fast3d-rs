use gbi_assembler::defines::{ComponentSize, ImageFormat, WrapMode};

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct TileDescriptor {
    pub uls: u16,
    pub ult: u16,
    pub lrs: u16,
    pub lrt: u16,
    // Set by G_SETTILE
    pub format: ImageFormat,
    pub size: ComponentSize,
    /// Size of 1 line (s-axis) of texture tile (9bit precision, 0 - 511)
    pub line: u16,
    /// Address of texture tile origin (9bit precision, 0 - 511)
    pub tmem: u16,
    /// slot in tmem (usually 0 or 1)?
    pub tmem_index: u8,
    /// Position of palette for 4bit color index textures (4bit precision, 0 - 15)
    pub palette: u8,
    /// s-axis mirror, wrap, clamp flags
    pub clamp_s: WrapMode,
    /// s-axis mask (4bit precision, 0 - 15)
    pub mask_s: u8,
    /// s-coordinate shift value
    pub shift_s: u8,
    /// t-axis mirror, wrap, clamp flags
    pub clamp_t: WrapMode,
    /// t-axis mask (4bit precision, 0 - 15)
    pub mask_t: u8,
    /// t-coordinate shift value
    pub shift_t: u8,
}

impl TileDescriptor {
    pub const EMPTY: Self = Self {
        uls: 0,
        ult: 0,
        lrs: 0,
        lrt: 0,
        format: ImageFormat::Yuv,
        size: ComponentSize::Bits4,
        line: 0,
        tmem: 0,
        tmem_index: 0,
        palette: 0,
        clamp_s: WrapMode::Repeat,
        mask_s: 0,
        shift_s: 0,
        clamp_t: WrapMode::Repeat,
        mask_t: 0,
        shift_t: 0,
    };

    pub fn set_format(&mut self, format: u8) {
        match format {
            0 => self.format = ImageFormat::Rgba,
            1 => self.format = ImageFormat::Yuv,
            2 => self.format = ImageFormat::Ci,
            3 => self.format = ImageFormat::Ia,
            4 => self.format = ImageFormat::I,
            _ => panic!("Invalid format: {}", format),
        }
    }

    pub fn set_size(&mut self, size: u8) {
        match size {
            0 => self.size = ComponentSize::Bits4,
            1 => self.size = ComponentSize::Bits8,
            2 => self.size = ComponentSize::Bits16,
            3 => self.size = ComponentSize::Bits32,
            _ => panic!("Invalid size: {}", size),
        }
    }

    pub fn get_width(&self) -> u16 {
        ((self.lrs - self.uls) + 4) / 4
    }

    pub fn get_height(&self) -> u16 {
        ((self.lrt - self.ult) + 4) / 4
    }
}
