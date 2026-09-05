use crate::capture::{Fixture, Provenance};
use crate::hle::combiner::{AlphaIn, ColorIn};
use crate::hle::{gbi::GbiUcode, AlphaCompare, BlendClass, ZMode};
use crate::{DataFormat, RdramImage};
use n64_gbi::encode::{mtx_to_bytes, Vp, VtxColored};

pub(super) const REGION: [usize; 4] = [144, 104, 32, 32];

// Independent integer calculation of RT64 initRand(0, x + 320*y), one LCG step,
// and alpha 128/255 >= (high_byte + 0.5)/256. Bit zero is the leftmost pixel.
pub(super) const HALF_MASK: [u32; 32] = [
    0x35b9_7211,
    0x233f_3be7,
    0x68e5_9086,
    0x8261_1345,
    0xb090_25af,
    0x962e_8abd,
    0x7ec0_e9b8,
    0xa7ed_f096,
    0x48bc_11dd,
    0xd46e_849c,
    0x475d_befe,
    0x7855_2df3,
    0xbb6c_3b33,
    0x7939_48ca,
    0x7440_e890,
    0xb007_6430,
    0xa1bd_9ff1,
    0xbf53_a9be,
    0x5990_b232,
    0x240f_fce2,
    0xfaa5_906b,
    0xd478_1f84,
    0x0546_3688,
    0x1cdd_bc9a,
    0xb5d3_854b,
    0x5c9f_1047,
    0x12fe_a9f1,
    0x775c_c03e,
    0xe85c_c407,
    0xebbe_6908,
    0x3bf1_e71c,
    0x5030_7bd6,
];

fn commands(bytes: &mut [u8], address: usize, words: &[(u32, u32)]) {
    for (i, &(w0, w1)) in words.iter().enumerate() {
        bytes[address + i * 8..address + i * 8 + 4].copy_from_slice(&w0.to_be_bytes());
        bytes[address + i * 8 + 4..address + i * 8 + 8].copy_from_slice(&w1.to_be_bytes());
    }
}

fn memory() -> Vec<u8> {
    let mut bytes = vec![0; 0x1000];
    bytes[..64].copy_from_slice(&mtx_to_bytes([
        [1.0 / 128.0, 0.0, 0.0, 0.0],
        [0.0, 1.0 / 128.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]));
    bytes[64..80].copy_from_slice(
        &Vp {
            vscale: [512, 512, 511, 0],
            vtrans: [640, 480, 511, 0],
        }
        .to_bytes(),
    );
    for (i, (x, y)) in [(-16, -16), (16, -16), (16, 16), (-16, 16)]
        .into_iter()
        .enumerate()
    {
        bytes[0x100 + i * 16..0x110 + i * 16].copy_from_slice(
            &VtxColored {
                x,
                y,
                z: 0,
                flag: 0,
                s: 0,
                t: 0,
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            }
            .to_bytes(),
        );
    }
    commands(
        &mut bytes,
        0x300,
        &[
            // src/game/mario_misc.c: make_gfx_mario_alpha(alpha=128).
            (0xb900_0002, 0x0000_0003),
            (0xfb00_0000, 0xffff_ff80),
            (0xb800_0000, 0),
        ],
    );
    commands(
        &mut bytes,
        0x400,
        &[
            (0x0103_0040, 0),
            (0x0380_0010, 64),
            (0xba00_1402, 0),
            (0xba00_1301, 0x0008_0000),
            (0xb900_031d, 0x0050_49d8),
            (0xb700_0000, 4),
            (0x0600_0000, 0x300),
            (0xe700_0000, 0),
            // actors/mario/model.inc.c: mario_butt, G_CC_SHADEFADEA.
            (0xfcff_ffff, 0xfffe_7b3d),
            (0x0430_0040, 0x100),
            (0xbf00_0000, 0x0000_0a14),
            (0xbf00_0000, 0x0000_141e),
            (0xb800_0000, 0),
        ],
    );
    bytes
}

fn provenance() -> Provenance {
    Provenance {
        decomp_revision: "sm64 1372ae1bb7cbedc03df366393188f4f05dcfc422".into(),
        source_symbols: "src/game/mario_misc.c: make_gfx_mario_alpha(alpha=128); actors/mario/model.inc.c: mario_butt; src/game/rendering_graph_node.c: transparent layer render mode".into(),
        command_vector: "Literal F3D G_AC_DITHER, white environment color with alpha 128, and SHADEFADEA commands; one-cycle AA_ZB_XLU_SURF; synthetic addresses and framebuffer setup".into(),
        synthetic_data: "Synthetic unlit red vertices form a 32x32 square at (144,104) in a 320x240 opaque black framebuffer; synthetic matrices, viewport and shader shade inputs; frame serial and dither seed zero. This models transparent Mario state, not captured Mario geometry or a console RNG phase".into(),
    }
}

pub(super) fn fixture() -> Fixture {
    super::capture_fixture::make(memory(), 0x400, 320, 240, provenance())
}

#[test]
fn transparent_mario_commands_preserve_environment_alpha_for_dither() {
    let bytes = memory();
    let result = crate::hle::interpret(
        RdramImage::new(&bytes),
        0x400,
        GbiUcode::F3d,
        DataFormat::Fixed,
    );
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    let scene = &result.scene;
    assert_eq!(scene.draw_runs.len(), 1);
    assert_eq!(scene.draw_runs[0].index_count, 6);
    assert_eq!(scene.raw_pos.len(), 4);
    assert_eq!(scene.cn, [0xff00_00ff; 4]);
    assert_eq!(scene.light_count, [0; 4]);
    let material = &scene.materials[scene.draw_runs[0].material_index as usize];
    assert_eq!(material.env, [255, 255, 255, 128]);
    assert_eq!(material.cycle_type, 0);
    assert!(!material.tex_enable);
    for cycle in [&material.selectors.cyc0, &material.selectors.cyc1] {
        assert_eq!(cycle.cd, ColorIn::Shade);
        assert_eq!(cycle.ad, AlphaIn::Environment);
        assert_eq!(cycle.cc, ColorIn::Zero);
        assert_eq!(cycle.ac, AlphaIn::Zero);
    }
    let mode = &scene.render_modes[scene.draw_runs[0].render_mode_index as usize];
    assert_eq!(mode.alpha_compare, AlphaCompare::Dither);
    assert!(!mode.cvg_x_alpha);
    assert_eq!(mode.fallback_class, BlendClass::AlphaOver);
    assert_eq!(mode.z_mode, ZMode::Xlu);
    let uniform = crate::render::CombinerUniform::from_run(material, mode, [0; 4]);
    let words: &[u32] = bytemuck::cast_slice(bytemuck::bytes_of(&uniform));
    assert_eq!(words[6], 3, "dither must reach the fragment alpha flags");
}

#[test]
fn fixture_sm64_transparent_mario() {
    let fixture = fixture();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let output = pollster::block_on(fixture.replay(device, queue)).unwrap();
    assert!(
        output.diagnostics.iter().all(Vec::is_empty),
        "{:?}",
        output.diagnostics
    );
    assert_eq!(output.rgba8.len(), 320 * 240 * 4);
    let [left, top, width, height] = REGION;
    let mut survivors = 0;
    for (i, pixel) in output.rgba8.as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (i % 320, i / 320);
        survivors += usize::from(pixel[0] != 0);
        let visible = (left..left + width).contains(&x)
            && (top..top + height).contains(&y)
            && HALF_MASK[y - top] & (1 << (x - left)) != 0;
        let expected = if visible {
            [128, 0, 0, 128]
        } else {
            [0, 0, 0, 255]
        };
        assert_eq!(*pixel, expected, "({x},{y})");
    }
    assert_eq!(survivors, 515);
    super::sm64_semantics::Case::TransparentMario.assert_pixels(&output.rgba8);
}

#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_sm64_transparent_mario_fixture() {
    super::capture_fixture::write(
        memory(),
        0x400,
        320,
        240,
        "sm64-transparent-mario.f3dcap",
        provenance(),
    );
}
