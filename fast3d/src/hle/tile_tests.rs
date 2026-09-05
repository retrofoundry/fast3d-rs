use super::combiner::build_material;
use super::rdp::{Rdp, TileDescriptor};
use super::rsp::Rsp;
use super::tile_sampling::TileSampling;

fn tile() -> TileDescriptor {
    TileDescriptor {
        fmt: 4,
        siz: 1,
        width: 8,
        height: 4,
        lrs: 28,
        lrt: 12,
        line: 1,
        masks: 3,
        maskt: 2,
        ..Default::default()
    }
}

fn material(tile: TileDescriptor) -> super::Material {
    let mut rdp = Rdp {
        combine_l: 0x00ff_ffff,
        combine_h: 0xfffc_f279,
        tmem: vec![1; 4096],
        load_via_tile: true,
        ..Default::default()
    };
    rdp.tiles[0] = tile;
    let bytes: Vec<_> = (0..4096)
        .map(|i| ((i * 37 + i / 8 * 11) & 255) as u8)
        .collect();
    rdp.tmem_bank.write_block(&bytes, 0, 0, 0, 512, 1);
    let mut rsp = Rsp::default();
    rsp.texture_state.on = true;
    let mut diags = Vec::new();
    let mat = build_material(&rdp, &rsp, &mut diags, 0).unwrap();
    assert!(diags.is_empty(), "{diags:?}");
    mat
}

fn reference_coordinate(sampling: TileSampling, uv: [f32; 2]) -> [f32; 2] {
    std::array::from_fn(|axis| {
        let shift = sampling.shift_mask[axis];
        let scale = if shift <= 10 {
            2.0_f32.powi(-(shift as i32))
        } else {
            2.0_f32.powi(16 - shift as i32)
        };
        (uv[axis] * scale * 128.0).round_ties_even() / 128.0 - sampling.bounds[axis] as f32 / 4.0
    })
}

fn reference_address(sampling: TileSampling, tap: [i32; 2]) -> [i32; 2] {
    std::array::from_fn(|axis| {
        let mode = sampling.modes[axis];
        let mask = sampling.shift_mask[axis + 2];
        let mut n = tap[axis];
        if mode & 2 != 0 || mask == 0 {
            n = n.clamp(0, sampling.modes[axis + 2] as i32 - 1);
        }
        if mask != 0 {
            let period = 1 << mask;
            n = if mode & 1 != 0 {
                let phase = n.rem_euclid(2 * period);
                if phase < period {
                    phase
                } else {
                    2 * period - 1 - phase
                }
            } else {
                n.rem_euclid(period)
            };
        }
        n
    })
}

#[test]
fn tile_origin_is_applied_after_shift() {
    let mat = material(TileDescriptor {
        uls: 5,
        ult: 9,
        lrs: 33,
        lrt: 21,
        shifts: 1,
        shiftt: 15,
        ..tile()
    });
    assert_eq!(mat.sampling.bounds, [5, 9, 33, 21]);
    assert_eq!(reference_coordinate(mat.sampling, [9.0, 3.0]), [3.25, 3.75]);
    assert_eq!(
        reference_coordinate(mat.sampling, [9.0 + 1.0 / 128.0, 3.0 + 1.0 / 512.0]),
        [3.25, 3.75]
    );
    assert_eq!(
        reference_coordinate(mat.sampling, [-0.0234375, -0.005859375]),
        [-1.265625, -2.265625]
    );
    assert_eq!(reference_address(mat.sampling, [3, 3]), [3, 3]);
}

#[test]
fn tile_shift_all_16_values() {
    let expected = [
        1024.0, 512.0, 256.0, 128.0, 64.0, 32.0, 16.0, 8.0, 4.0, 2.0, 1.0, 32768.0, 16384.0,
        8192.0, 4096.0, 2048.0,
    ];
    for (shift, expected) in expected.into_iter().enumerate() {
        let mat = material(TileDescriptor {
            shifts: shift as u8,
            shiftt: shift as u8,
            ..tile()
        });
        assert_eq!(
            reference_coordinate(mat.sampling, [1024.0; 2]),
            [expected; 2],
            "shift {shift}"
        );
        assert_eq!(
            reference_address(mat.sampling, [expected as i32; 2]),
            [expected as i32 % 8, expected as i32 % 4]
        );
    }
}

#[test]
fn tile_mask_zero_clamps() {
    let mat = material(TileDescriptor {
        masks: 0,
        maskt: 0,
        ..tile()
    });
    for (tap, expected) in [([-9, -1], [0, 0]), ([8, 4], [7, 3]), ([3, 2], [3, 2])] {
        assert_eq!(reference_address(mat.sampling, tap), expected);
    }
}

#[test]
fn tile_mask_period_differs_from_image_extent() {
    let mat = material(TileDescriptor {
        masks: 2,
        maskt: 1,
        ..tile()
    });
    assert_eq!(mat.sampling.image[..2], [8, 4]);
    for (tap, expected) in [([4, 2], [0, 0]), ([7, 3], [3, 1]), ([8, 4], [0, 0])] {
        assert_eq!(reference_address(mat.sampling, tap), expected);
    }
}

#[test]
fn tile_negative_wrap_and_mirror() {
    for (mode, expected) in [(0, [3, 0, 3, 0, 1, 3, 0, 1]), (1, [0, 0, 3, 3, 2, 0, 0, 1])] {
        let mat = material(TileDescriptor {
            cms: mode,
            cmt: mode,
            masks: 2,
            maskt: 2,
            ..tile()
        });
        for (tap, expected) in [-9, -8, -5, -4, -3, -1, 0, 1].into_iter().zip(expected) {
            assert_eq!(reference_address(mat.sampling, [tap; 2]), [expected; 2]);
        }
    }
}

#[test]
fn tile_clamp_precedes_mask_for_each_tap() {
    let mat = material(TileDescriptor {
        cms: 3,
        cmt: 2,
        masks: 2,
        maskt: 1,
        ..tile()
    });
    for (tap, expected) in [
        ([6, 2], [1, 0]),
        ([7, 3], [0, 1]),
        ([8, 4], [0, 1]),
        ([-1, -1], [0, 0]),
    ] {
        assert_eq!(reference_address(mat.sampling, tap), expected);
    }
    assert_eq!(reference_coordinate(mat.sampling, [8.0, 4.0]), [8.0, 4.0]);
}

#[test]
fn tile_large_mask_uses_bounded_tmem_lookup() {
    for siz in [0, 1, 2, 3] {
        let mat = material(TileDescriptor {
            fmt: if siz == 3 { 0 } else { 3 },
            siz,
            masks: 15,
            maskt: 15,
            tmem_addr: 511,
            line: 511,
            ..tile()
        });
        assert_eq!(mat.sampling.image[2], 1);
        assert_eq!(mat.sampling.allocation_extent(), [4096, 4]);
        assert_eq!(mat.texture.len(), 65536);
        assert_eq!(
            mat.sampling.tmem,
            [4088, 4088, if siz == 3 { 0 } else { 3 }, u32::from(siz)]
        );
        assert_eq!(reference_address(mat.sampling, [-1, 32769]), [32767, 1]);
    }
}

#[test]
fn lod_tiles_apply_independent_origin_and_shift() {
    let mut rdp = Rdp {
        combine_l: 0x00ff_ffff,
        combine_h: 0xfffc_f279,
        other_mode_h: (1 << 16) | (2 << 17),
        tmem: vec![1; 4096],
        load_via_tile: true,
        ..Default::default()
    };
    rdp.tiles[0] = TileDescriptor {
        width: 32,
        height: 32,
        lrs: 124,
        lrt: 124,
        line: 4,
        masks: 5,
        maskt: 5,
        ..tile()
    };
    rdp.tiles[1] = TileDescriptor {
        uls: 8,
        ult: 12,
        lrs: 132,
        lrt: 136,
        shifts: 1,
        shiftt: 15,
        tmem_addr: 128,
        ..rdp.tiles[0].clone()
    };
    let mut rsp = Rsp::default();
    rsp.set_texture(0, 1, true, 65535, 65535);
    let mat = build_material(&rdp, &rsp, &mut Vec::new(), 0).unwrap();
    assert!(mat.lod);
    assert_eq!(mat.mip_levels.len(), 2);
    assert_eq!(
        mat.mip_levels
            .iter()
            .map(|l| (l.w, l.h))
            .collect::<Vec<_>>(),
        [(32, 32); 2]
    );
    assert_eq!(
        reference_coordinate(mat.mip_levels[0].sampling, [13.0, 9.0]),
        [13.0, 9.0]
    );
    assert_eq!(
        reference_coordinate(mat.mip_levels[1].sampling, [13.0, 9.0]),
        [4.5, 15.0]
    );
    assert_eq!(mat.detail_tex.unwrap().sampling, mat.mip_levels[0].sampling);
    rdp.combine_h = 0xfffd_0838;
    rdp.other_mode_h |= 1 << 20;
    let mat = build_material(&rdp, &rsp, &mut Vec::new(), 0).unwrap();
    assert_eq!((mat.tex_w, mat.tex_h), (32, 32));
    assert_eq!(mat.texture, mat.mip_levels[0].texture);
    assert_eq!(mat.sampling, mat.mip_levels[0].sampling);
}
