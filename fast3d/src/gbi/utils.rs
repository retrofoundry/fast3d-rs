use crate::gbi::defines::{
    AlphaCompare, BlendAlpha2, BlendColor, CycleType, GeometryModes, OtherModeHLayout, RenderMode,
    RenderModeFlags, TextureFilter, ZMode,
};
use crate::output::gfx::Face;
use crate::rsp::RSPConstants;

pub fn get_cmd(val: usize, start_bit: u32, num_bits: u32) -> usize {
    (val >> start_bit) & ((1 << num_bits) - 1)
}

pub fn get_render_mode_from_other_mode_l(other_mode_l: u32) -> RenderMode {
    RenderMode::try_from(other_mode_l).unwrap()
}

pub fn other_mode_l_uses_texture_edge(other_mode_l: u32) -> bool {
    let render_mode = get_render_mode_from_other_mode_l(other_mode_l);
    render_mode.flags.contains(RenderModeFlags::CVG_X_ALPHA)
}

pub fn other_mode_l_uses_alpha(other_mode_l: u32) -> bool {
    let render_mode = get_render_mode_from_other_mode_l(other_mode_l);
    render_mode.blend_cycle1.alpha2 == BlendAlpha2::OneMinusAlpha
    // TODO: Do we need to check which cycle we're in?
    // render_mode.blend_cycle2.color2 == BlendColor::Memory && render_mode.blend_cycle2.alpha2 == BlendAlpha2::OneMinusAlpha
}

pub fn other_mode_l_alpha_compare_threshold(other_mode_l: u32) -> bool {
    other_mode_l & AlphaCompare::Threshold as u32 == AlphaCompare::Threshold as u32
}

pub fn other_mode_l_uses_fog(other_mode_l: u32) -> bool {
    let render_mode = get_render_mode_from_other_mode_l(other_mode_l);
    render_mode.blend_cycle1.color1 == BlendColor::Fog
}

pub fn get_zmode_from_other_mode_l(other_mode_l: u32) -> ZMode {
    get_render_mode_from_other_mode_l(other_mode_l).z_mode
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

pub fn translate_cull_mode(
    geometry_mode: GeometryModes,
    rsp_constants: &RSPConstants,
) -> Option<Face> {
    let cull_front = geometry_mode.contains(GeometryModes::from_bits_truncate(
        rsp_constants.geomode_cull_front_val,
    ));
    let cull_back = geometry_mode.contains(GeometryModes::from_bits_truncate(
        rsp_constants.geomode_cull_back_val,
    ));

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
