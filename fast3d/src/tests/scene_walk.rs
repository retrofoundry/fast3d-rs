use crate::tests::common;

#[test]
fn every_curated_scene_interprets_and_renders_clean() {
    let mut checked = 0;
    for &name in crate::tests::fixtures::SCENES {
        let (rdram, entry_addr) = crate::tests::fixtures::fixture(name);
        let r = crate::hle::interpret_rdram(rdram, entry_addr as u32);
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
    assert_eq!(checked, 33, "expected 33 curated scenes, found {checked}");
}
