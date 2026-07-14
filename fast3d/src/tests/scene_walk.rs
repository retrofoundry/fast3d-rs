use crate::tests::common;

use std::fs;
use std::path::PathBuf;

fn scenes_dir() -> PathBuf {
    // fast3d/ crate root -> tests/scenes
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scenes")
}

#[test]
fn every_curated_scene_assembles_and_renders_clean() {
    // 32x32 white RGBA8 — >= every scene's declared texture dims (2x2 or 32x32).
    let white = vec![255u8; 32 * 32 * 4];
    let mut checked = 0;
    for entry in fs::read_dir(scenes_dir()).expect("tests/scenes must exist") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("n64") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let source = fs::read_to_string(&path).unwrap();
        let img = crate::asm::assemble_with_texture(&source, &white, 32, 32)
            .unwrap_or_else(|d| panic!("{name}: assemble failed: {d:?}"));
        let r = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
        assert!(r.diags.is_empty(), "{name}: interp diags: {:?}", r.diags);
        // 2D scenes (those that open a framebuffer pair via gsDPSetColorImage) may have no
        // flat triangle indices and no 3D vertex buffer — skip the 3D-only assertions.
        let is_2d = !r.scene.framebuffer_pairs.is_empty();
        if !is_2d {
            assert!(
                !r.scene.materials.is_empty(),
                "{name}: materials empty (would not render)"
            );
            assert!(!r.scene.indices.is_empty(), "{name}: drew no triangles");
            // Every 3D curated scene must land on-screen after the perspective divide (NDC x,y
            // in [-1,1]) with w>0 (in front of the camera) and depth in [0,1].
            for i in 0..r.scene.raw_pos.len() {
                let p = common::ref_pos(&r.scene, i);
                let w = p[3];
                assert!(w > 0.0, "{name}: vertex behind camera (w={w})");
                let ndc_x = p[0] / w;
                let ndc_y = p[1] / w;
                let depth = p[2] / w;
                assert!(
                    (-1.001..=1.001).contains(&ndc_x) && (-1.001..=1.001).contains(&ndc_y),
                    "{name}: vertex off-screen (NDC [{ndc_x}, {ndc_y}]) — wrong viewport/camera"
                );
                assert!(
                    (-0.001..=1.001).contains(&depth),
                    "{name}: depth {depth} out of [0,1]"
                );
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 32, "expected 32 curated scenes, found {checked}");
}
