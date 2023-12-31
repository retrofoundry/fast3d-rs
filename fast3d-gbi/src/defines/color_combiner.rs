use bitflags::bitflags;

pub const G_CC_SHADE: CombineParams = CombineParams::SHADE;

// TODO: Replace with the new getter on GfxCommand
pub fn get_cmd(val: usize, start_bit: u32, num_bits: u32) -> usize {
    (val >> start_bit) & ((1 << num_bits) - 1)
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Hash)]
    pub struct ColorCombinerMux: u32 {
        const COMBINED = 0x00;
        const TEXEL0 = 0x01;
        const TEXEL1 = 0x02;
        const PRIMITIVE = 0x03;
        const SHADE = 0x04;
        const ENVIRONMENT = 0x05;
        const CENTER = 0x06;
        const SCALE = 0x06;
        const COMBINED_ALPHA = 0x07;
        const TEXEL0_ALPHA = 0x08;
        const TEXEL1_ALPHA = 0x09;
        const PRIMITIVE_ALPHA = 0x0A;
        const SHADE_ALPHA = 0x0B;
        const ENVIRONMENT_ALPHA = 0x0C;
        const LOD_FRACTION = 0x0D;
        const PRIM_LOD_FRAC = 0x0E;
        const NOISE = 0x07;
        const K4 = 0x07;
        const K5 = 0x0F;
        const ONE = 0x06;
        const ZERO = 0x1F;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct AlphaCombinerMux: u32 {
        const COMBINED = 0x00;
        const TEXEL0 = 0x01;
        const TEXEL1 = 0x02;
        const PRIMITIVE = 0x03;
        const SHADE = 0x04;
        const ENVIRONMENT = 0x05;
        const LOD_FRACTION = 0x00;
        const PRIM_LOD_FRAC = 0x06;
        const ONE = 0x06;
        const ZERO = 0x07;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColorCombinePass {
    pub a: ColorCombinerMux,
    pub b: ColorCombinerMux,
    pub c: ColorCombinerMux,
    pub d: ColorCombinerMux,
}

impl ColorCombinePass {
    pub const SHADE: Self = Self {
        a: ColorCombinerMux::ZERO,
        b: ColorCombinerMux::ZERO,
        c: ColorCombinerMux::ZERO,
        d: ColorCombinerMux::SHADE,
    };

    // grab property by index
    pub fn get(&self, index: usize) -> ColorCombinerMux {
        match index {
            0 => self.a,
            1 => self.b,
            2 => self.c,
            3 => self.d,
            _ => panic!("Invalid index"),
        }
    }

    pub fn uses_texture0(&self) -> bool {
        self.a == ColorCombinerMux::TEXEL0
            || self.a == ColorCombinerMux::TEXEL0_ALPHA
            || self.b == ColorCombinerMux::TEXEL0
            || self.b == ColorCombinerMux::TEXEL0_ALPHA
            || self.c == ColorCombinerMux::TEXEL0
            || self.c == ColorCombinerMux::TEXEL0_ALPHA
            || self.d == ColorCombinerMux::TEXEL0
            || self.d == ColorCombinerMux::TEXEL0_ALPHA
    }

    pub fn uses_texture1(&self) -> bool {
        self.a == ColorCombinerMux::TEXEL1
            || self.a == ColorCombinerMux::TEXEL1_ALPHA
            || self.b == ColorCombinerMux::TEXEL1
            || self.b == ColorCombinerMux::TEXEL1_ALPHA
            || self.c == ColorCombinerMux::TEXEL1
            || self.c == ColorCombinerMux::TEXEL1_ALPHA
            || self.d == ColorCombinerMux::TEXEL1
            || self.d == ColorCombinerMux::TEXEL1_ALPHA
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AlphaCombinePass {
    pub a: AlphaCombinerMux,
    pub b: AlphaCombinerMux,
    pub c: AlphaCombinerMux,
    pub d: AlphaCombinerMux,
}

impl AlphaCombinePass {
    pub const SHADE: Self = Self {
        a: AlphaCombinerMux::ZERO,
        b: AlphaCombinerMux::ZERO,
        c: AlphaCombinerMux::ZERO,
        d: AlphaCombinerMux::SHADE,
    };

    // grab property by index
    pub fn get(&self, index: usize) -> AlphaCombinerMux {
        match index {
            0 => self.a,
            1 => self.b,
            2 => self.c,
            3 => self.d,
            _ => panic!("Invalid index"),
        }
    }

    pub fn uses_texture0(&self) -> bool {
        self.a == AlphaCombinerMux::TEXEL0
            || self.b == AlphaCombinerMux::TEXEL0
            || self.c == AlphaCombinerMux::TEXEL0
            || self.d == AlphaCombinerMux::TEXEL0
    }

    pub fn uses_texture1(&self) -> bool {
        self.a == AlphaCombinerMux::TEXEL1
            || self.b == AlphaCombinerMux::TEXEL1
            || self.c == AlphaCombinerMux::TEXEL1
            || self.d == AlphaCombinerMux::TEXEL1
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CombineParams {
    pub c0: ColorCombinePass,
    pub a0: AlphaCombinePass,
    pub c1: ColorCombinePass,
    pub a1: AlphaCombinePass,
}

impl CombineParams {
    pub const ZERO: Self = Self {
        c0: ColorCombinePass {
            a: ColorCombinerMux::COMBINED,
            b: ColorCombinerMux::COMBINED,
            c: ColorCombinerMux::COMBINED,
            d: ColorCombinerMux::COMBINED,
        },
        a0: AlphaCombinePass {
            a: AlphaCombinerMux::COMBINED,
            b: AlphaCombinerMux::COMBINED,
            c: AlphaCombinerMux::COMBINED,
            d: AlphaCombinerMux::COMBINED,
        },
        c1: ColorCombinePass {
            a: ColorCombinerMux::COMBINED,
            b: ColorCombinerMux::COMBINED,
            c: ColorCombinerMux::COMBINED,
            d: ColorCombinerMux::COMBINED,
        },
        a1: AlphaCombinePass {
            a: AlphaCombinerMux::COMBINED,
            b: AlphaCombinerMux::COMBINED,
            c: AlphaCombinerMux::COMBINED,
            d: AlphaCombinerMux::COMBINED,
        },
    };

    pub const SHADE: Self = Self {
        c0: ColorCombinePass {
            a: ColorCombinerMux::COMBINED,
            b: ColorCombinerMux::COMBINED,
            c: ColorCombinerMux::COMBINED,
            d: ColorCombinerMux::SHADE,
        },
        a0: AlphaCombinePass {
            a: AlphaCombinerMux::COMBINED,
            b: AlphaCombinerMux::COMBINED,
            c: AlphaCombinerMux::COMBINED,
            d: AlphaCombinerMux::SHADE,
        },
        c1: ColorCombinePass {
            a: ColorCombinerMux::COMBINED,
            b: ColorCombinerMux::COMBINED,
            c: ColorCombinerMux::COMBINED,
            d: ColorCombinerMux::SHADE,
        },
        a1: AlphaCombinePass {
            a: AlphaCombinerMux::COMBINED,
            b: AlphaCombinerMux::COMBINED,
            c: AlphaCombinerMux::COMBINED,
            d: AlphaCombinerMux::SHADE,
        },
    };

    pub fn decode(w0: usize, w1: usize) -> Self {
        let a0 = (get_cmd(w0, 20, 4) & 0xF) as u32;
        let b0 = (get_cmd(w1, 28, 4) & 0xF) as u32;
        let c0 = (get_cmd(w0, 15, 5) & 0x1F) as u32;
        let d0 = (get_cmd(w1, 15, 3) & 0x7) as u32;

        let aa0 = (get_cmd(w0, 12, 3) & 0x7) as u32;
        let ab0 = (get_cmd(w1, 12, 3) & 0x7) as u32;
        let ac0 = (get_cmd(w0, 9, 3) & 0x7) as u32;
        let ad0 = (get_cmd(w1, 9, 3) & 0x7) as u32;

        let a1 = (get_cmd(w0, 5, 4) & 0xF) as u32;
        let b1 = (get_cmd(w1, 24, 4) & 0xF) as u32;
        let c1 = (get_cmd(w0, 0, 5) & 0x1F) as u32;
        let d1 = (get_cmd(w1, 6, 3) & 0x7) as u32;

        let aa1 = (get_cmd(w1, 21, 3) & 0x7) as u32;
        let ab1 = (get_cmd(w1, 3, 3) & 0x7) as u32;
        let ac1 = (get_cmd(w1, 18, 3) & 0x7) as u32;
        let ad1 = (get_cmd(w1, 0, 3) & 0x7) as u32;

        Self {
            c0: ColorCombinePass {
                a: ColorCombinerMux::from_bits_truncate(a0),
                b: ColorCombinerMux::from_bits_truncate(b0),
                c: ColorCombinerMux::from_bits_truncate(c0),
                d: ColorCombinerMux::from_bits_truncate(d0),
            },
            a0: AlphaCombinePass {
                a: AlphaCombinerMux::from_bits_truncate(aa0),
                b: AlphaCombinerMux::from_bits_truncate(ab0),
                c: AlphaCombinerMux::from_bits_truncate(ac0),
                d: AlphaCombinerMux::from_bits_truncate(ad0),
            },
            c1: ColorCombinePass {
                a: ColorCombinerMux::from_bits_truncate(a1),
                b: ColorCombinerMux::from_bits_truncate(b1),
                c: ColorCombinerMux::from_bits_truncate(c1),
                d: ColorCombinerMux::from_bits_truncate(d1),
            },
            a1: AlphaCombinePass {
                a: AlphaCombinerMux::from_bits_truncate(aa1),
                b: AlphaCombinerMux::from_bits_truncate(ab1),
                c: AlphaCombinerMux::from_bits_truncate(ac1),
                d: AlphaCombinerMux::from_bits_truncate(ad1),
            },
        }
    }

    pub fn get_cc(&self, index: usize) -> ColorCombinePass {
        match index {
            0 => self.c0,
            1 => self.c1,
            _ => panic!("Invalid index"),
        }
    }

    pub fn get_ac(&self, index: usize) -> AlphaCombinePass {
        match index {
            0 => self.a0,
            1 => self.a1,
            _ => panic!("Invalid index"),
        }
    }

    pub fn cc_ac_same(&self, index: usize) -> bool {
        match index {
            0 => {
                self.c0.a.bits() == self.a0.a.bits()
                    && self.c0.b.bits() == self.a0.b.bits()
                    && self.c0.c.bits() == self.a0.c.bits()
                    && self.c0.d.bits() == self.a0.d.bits()
            }
            1 => {
                self.c1.a.bits() == self.a1.a.bits()
                    && self.c1.b.bits() == self.a1.b.bits()
                    && self.c1.c.bits() == self.a1.c.bits()
                    && self.c1.d.bits() == self.a1.d.bits()
            }
            _ => panic!("Invalid index"),
        }
    }

    pub fn uses_texture0(&self) -> bool {
        self.c0.uses_texture0()
            || self.c1.uses_texture0()
            || self.a0.uses_texture0()
            || self.a1.uses_texture0()
    }

    pub fn uses_texture1(&self) -> bool {
        self.c0.uses_texture1()
            || self.c1.uses_texture1()
            || self.a0.uses_texture1()
            || self.a1.uses_texture1()
    }
}
