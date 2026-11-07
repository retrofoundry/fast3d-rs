use crate::hle::combiner::build_material;
use crate::hle::rdp::{Rdp, TileDescriptor};
use crate::hle::rsp::Rsp;
use crate::hle::Scene;
use crate::tests::common::{pixel, render_to_pixels, scene_from_fixture};

fn state(bank: &[u8; 4096], tile: TileDescriptor, other_mode_h: u32) -> Rdp {
    let mut rdp = Rdp {
        combine_l: 0x00ff_ffff,
        combine_h: 0xfffc_f279,
        other_mode_h,
        load_via_tile: true,
        texture_loaded: true,
        ..Default::default()
    };
    rdp.tiles[0] = tile;
    rdp.tmem_bank.write_tile(bank, 0, 0, 1, 512, 4096, 1);
    rdp
}

fn scene(rdp: &Rdp, uv: [f32; 2]) -> Scene {
    let mut scene = scene_from_fixture("framebuffer-extent--white1");
    let mut rsp = Rsp::default();
    rsp.set_texture(0, 0, true, 65535, 65535);
    let mut diags = Vec::new();
    scene.materials = vec![build_material(rdp, &rsp, &mut diags, 0).unwrap()];
    assert!(diags.is_empty(), "{diags:?}");
    scene.raw_st.fill(uv);
    scene.texcoord_table.fill([1.0; 2]);
    scene
}

fn tlut_tile(format: u8, size: u8) -> TileDescriptor {
    TileDescriptor {
        fmt: format,
        siz: size,
        width: 8,
        height: 4,
        lrs: 28,
        lrt: 12,
        masks: 3,
        maskt: 2,
        ..Default::default()
    }
}

fn palette_bank() -> [u8; 4096] {
    let mut bank = [0; 4096];
    bank[2048..2056].copy_from_slice(&[0x10, 0xff, 0x30, 0xff, 0x50, 0xff, 0x70, 0xff]);
    bank
}

fn assert_probes(cases: impl IntoIterator<Item = (Scene, [u8; 4])>) {
    let (device, queue) = super::headless_device_forced_fallback();
    let mut renderer =
        super::SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240, false);
    for (scene, expected) in cases {
        let pixels = render_to_pixels(&device, &queue, &mut renderer, &scene, 320, 240);
        assert_eq!(
            pixel(&pixels, 320, 100, 80),
            expected,
            "uv={:?}, sampling={:?}, filter={}",
            scene.raw_st[0],
            scene.materials[0].sampling,
            scene.materials[0].filter_mode,
        );
    }
}

#[test]
fn tmem_sampling_retains_palette_planes_and_nearest_filter_state() {
    for sample_type in [0, 1] {
        let rdp = state(
            &palette_bank(),
            tlut_tile(2, 1),
            (3 << 14) | (sample_type << 13) | (1 << 11),
        );
        let scene = scene(&rdp, [0.5, 0.0]);
        let sampling = scene.materials[0].sampling;
        assert_eq!(sampling.palette[2], 4);
        assert_eq!(sampling.palette[3], (sample_type << 3) | 2);
        assert_eq!(sampling.allocation_extent(), [8, 16]);
        let decoded = scene.materials[0].texture.decode();
        let plane_bytes = (sampling.image[0] * sampling.palette[2] * 4) as usize;
        // Angrylion rdp/tex.c:275,301 rounds the 5-bit lower-triangle blend before adding the base tap.
        let half_s: [u8; 4] = std::array::from_fn(|channel| {
            let first = i32::from(decoded[channel]);
            let second = i32::from(decoded[plane_bytes + channel]);
            (first + ((16 * (second - first) + 16) >> 5)) as u8
        });
        assert_eq!(half_s, [0x20, 0x20, 0x20, 0xff]);
    }
}

#[test]
fn tmem_sampling_gpu_palette_lanes_follow_the_filter_triangle() {
    let mut cases = Vec::new();
    for sample_type in [0, 1] {
        let rdp = state(
            &palette_bank(),
            tlut_tile(2, 1),
            (3 << 14) | (sample_type << 13) | (1 << 11),
        );
        for (uv, intensity) in [
            ([0.0, 0.0], 0x10),
            ([0.5, 0.0], 0x20),
            ([0.75, 0.75], 0x28),
            ([0.5, 0.5], 0x40),
            ([0.75, 0.25], 0x48),
        ] {
            cases.push((scene(&rdp, uv), [intensity, intensity, intensity, 255]));
        }
    }
    assert_probes(cases);
}

#[test]
fn tmem_sampling_gpu_clamp_clears_palette_filter_fractions() {
    let mut cases = Vec::new();
    for sample_type in [0, 1] {
        let mut tile = tlut_tile(2, 1);
        tile.cms = 2;
        tile.cmt = 2;
        let rdp = state(
            &palette_bank(),
            tile,
            (3 << 14) | (sample_type << 13) | (1 << 11),
        );
        for (uv, intensity) in [
            ([-0.5, 0.0], 0x10),
            ([7.5, 0.0], 0x10),
            ([0.75, 3.75], 0x28),
        ] {
            cases.push((scene(&rdp, uv), [intensity, intensity, intensity, 255]));
        }
    }
    assert_probes(cases);
}

#[test]
fn tmem_sampling_gpu_t_neighbor_is_added_after_the_eight_bit_wrap() {
    let mut bank = [0; 4096];
    bank[0] = 0x10;
    bank[12] = 0x50;
    bank[2044] = 0x30;
    bank[2048] = 0x90;
    let rdp = state(
        &bank,
        TileDescriptor {
            fmt: 4,
            siz: 1,
            width: 4,
            height: 1024,
            lrs: 12,
            lrt: 4092,
            line: 1,
            masks: 2,
            maskt: 10,
            ..Default::default()
        },
        (1 << 13) | (1 << 11),
    );
    assert_probes(
        [
            (255.5, 0x60),
            (256.0, 0x10),
            (256.5, 0x30),
            (511.5, 0x60),
            (1023.5, 0x20),
        ]
        .map(|(t, intensity)| (scene(&rdp, [0.0, t]), [intensity; 4])),
    );
}

#[test]
fn tmem_sampling_gpu_yuv_tlut_uses_byte_indices_and_split_fractions() {
    let mut cases = Vec::new();
    for size in 0..4 {
        let mut bank = [0; 4096];
        for index in 0..8 {
            bank[index] = if size == 0 { index << 4 } else { index } as u8;
            for lane in 0..4 {
                bank[2048 + index * 8 + lane * 2] = (0x10 + index * 8 + lane * 0x20) as u8;
                bank[2049 + index * 8 + lane * 2] = 255;
            }
        }
        for sample_type in [0, 1] {
            let rdp = state(
                &bank,
                tlut_tile(1, size),
                (3 << 14) | (sample_type << 13) | (1 << 11),
            );
            let expected = if sample_type == 0 {
                [
                    0x181820ff, 0x303028ff, 0x343428ff, 0x2c2c30ff, 0x2c2c48ff, 0x4c4c50ff,
                ]
            } else {
                [
                    0x1c1c28ff, 0x3c3c30ff, 0x52524cff, 0x525254ff, 0x3a3a64ff, 0x626264ff,
                ]
            };
            for (uv, rgba) in [
                [0.5, 0.0],
                [1.5, 0.0],
                [0.75, 0.75],
                [1.75, 0.75],
                [0.75, 0.25],
                [1.75, 0.25],
            ]
            .into_iter()
            .zip(expected)
            {
                cases.push((scene(&rdp, uv), u32::to_be_bytes(rgba)));
            }
        }
    }
    assert_probes(cases);
}
