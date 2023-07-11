use crate::gbi::defines::{rsp_geometry, AlphaCompare, CycleType, OtherModeHLayout, TextureFilter};
use crate::rsp::RSPConstants;
use crate::{
    output::gfx::{BlendFactor, Face},
    rdp::{BlendParamB, BlendParamPMColor, OtherModeLayoutL},
};

pub fn get_cmd(val: usize, start_bit: u32, num_bits: u32) -> usize {
    (val >> start_bit) & ((1 << num_bits) - 1)
}

pub fn geometry_mode_uses_lighting(geometry_mode: u32) -> bool {
    geometry_mode & rsp_geometry::g::LIGHTING > 0
}

pub fn geometry_mode_uses_fog(geometry_mode: u32) -> bool {
    geometry_mode & rsp_geometry::g::FOG > 0
}

pub fn other_mode_l_uses_texture_edge(other_mode_l: u32) -> bool {
    other_mode_l >> (OtherModeLayoutL::CVG_X_ALPHA as u32) & 0x01 == 0x01
}

pub fn other_mode_l_uses_alpha(other_mode_l: u32) -> bool {
    other_mode_l & ((BlendParamB::G_BL_A_MEM as u32) << (OtherModeLayoutL::B_1 as u32)) == 0
}

pub fn other_mode_l_alpha_compare_threshold(other_mode_l: u32) -> bool {
    other_mode_l & AlphaCompare::Threshold as u32 == AlphaCompare::Threshold as u32
}

pub fn other_mode_l_uses_fog(other_mode_l: u32) -> bool {
    (other_mode_l >> OtherModeLayoutL::P_1 as u32) == BlendParamPMColor::G_BL_CLR_FOG as u32
}

pub fn other_mode_l_alpha_compare_dither(other_mode_l: u32) -> bool {
    other_mode_l & AlphaCompare::Dither as u32 == AlphaCompare::Dither as u32
}

pub fn get_cycle_type_from_other_mode_h(mode_h: u32) -> CycleType {
    (((mode_h >> OtherModeHLayout::CYCLE_TYPE.bits()) & 0x03) as u8)
        .try_into()
        .unwrap()
}

pub fn get_texture_filter_from_other_mode_h(mode_h: u32) -> TextureFilter {
    (((mode_h >> OtherModeHLayout::TEXT_FILT.bits()) & 0x3) as u8)
        .try_into()
        .unwrap()
}

pub fn translate_cull_mode(geometry_mode: u32, rsp_constants: &RSPConstants) -> Option<Face> {
    let cull_front = (geometry_mode & rsp_constants.G_CULL_FRONT) != 0;
    let cull_back = (geometry_mode & rsp_constants.G_CULL_BACK) != 0;

    if cull_front && cull_back {
        panic!("Culling both front and back faces is not supported");
    } else if cull_front {
        Some(Face::Front)
    } else if cull_back {
        Some(Face::Back)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub trait I32MathExt {
        fn ushr(self, n: u32) -> u32;
    }

    impl I32MathExt for i32 {
        fn ushr(self, n: u32) -> u32 {
            ((self >> n) & ((1 << (32 - n)) - 1)) as u32
        }
    }

    #[test]
    fn test_get_cmd() {
        let word: usize = 84939284;
        let a = get_cmd(word, 16, 8) / 2;
        let b = get_cmd(word, 8, 8) / 2;
        let c = get_cmd(word, 0, 8) / 2;

        assert_eq!(a, 8);
        assert_eq!(b, 9);
        assert_eq!(c, 10);

        assert_eq!(a, ((((word as i32).ushr(16)) & 0xFF) / 2) as usize);
    }
}
