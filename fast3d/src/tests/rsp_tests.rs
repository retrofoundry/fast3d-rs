use crate::tests::common;

use crate::hle::mem::RdramImage;
use crate::hle::rsp::{Rsp, Scene};

fn vtx_bytes(x: i16, y: i16, z: i16, r: u8, g: u8, b: u8, a: u8) -> [u8; 16] {
    let mut buf = [0u8; 16];
    buf[0..2].copy_from_slice(&x.to_be_bytes());
    buf[2..4].copy_from_slice(&y.to_be_bytes());
    buf[4..6].copy_from_slice(&z.to_be_bytes());
    buf[12] = r;
    buf[13] = g;
    buf[14] = b;
    buf[15] = a;
    buf
}

#[test]
fn set_vertex_with_identity_mvp_preserves_position_and_color() {
    let bytes = vtx_bytes(7, 11, 0, 0xAA, 0xBB, 0xCC, 0xDD);
    let rdram = RdramImage::new(&bytes);
    let mut rsp = Rsp::default();
    let mut scene = Scene::default();
    rsp.set_vertex(
        &rdram,
        0,
        1,
        0,
        &crate::hle::rdp::Rdp::default(),
        &mut scene,
    );
    rsp.finish(&mut scene);
    assert_eq!(scene.raw_pos.len(), 1);
    // Full-screen default viewport maps x,y through unchanged; z -> vp_trans.z = 511/1024.
    assert_eq!(common::ref_pos(&scene, 0), [7.0, 11.0, 511.0 / 1024.0, 1.0]);
    let c = scene.cn[0];
    assert_eq!(
        [
            (c & 0xff) as u8,
            ((c >> 8) & 0xff) as u8,
            ((c >> 16) & 0xff) as u8,
            ((c >> 24) & 0xff) as u8
        ],
        [0xAA, 0xBB, 0xCC, 0xDD]
    );
}

#[test]
fn viewport_maps_scene_into_its_subregion() {
    // A viewport covering the top-left quarter of the 320x240 framebuffer: half-extent (80,60)
    // centered at (80,60) -> Vp fixed-point (x4) scale=trans={320,240,511}.
    //   clip center (0,0) -> NDC center of that quarter (-0.5, +0.5)  [pins X + proves load-bearing]
    //   clip top    (0,1) -> top edge of that quarter   (-0.5, +1.0)  [pins the -w Y direction]
    // Under the old viewport-ignored path both landed at clip.xy ((0,0)/(0,1)) instead.
    let mut buf = Vec::new();
    let vp: [i16; 8] = [320, 240, 511, 0, 320, 240, 511, 0];
    for v in vp {
        buf.extend_from_slice(&v.to_be_bytes());
    }
    buf.extend_from_slice(&vtx_bytes(0, 0, 0, 255, 255, 255, 255));
    buf.extend_from_slice(&vtx_bytes(0, 1, 0, 255, 255, 255, 255));
    let rdram = RdramImage::new(&buf);
    let mut rsp = Rsp::default();
    rsp.set_viewport(&rdram, 0);
    let mut scene = Scene::default();
    rsp.set_vertex(
        &rdram,
        16,
        2,
        0,
        &crate::hle::rdp::Rdp::default(),
        &mut scene,
    );
    rsp.finish(&mut scene);
    let p0 = common::ref_pos(&scene, 0);
    assert!(
        (p0[0] - (-0.5)).abs() < 1e-6 && (p0[1] - 0.5).abs() < 1e-6,
        "center -> {p0:?}"
    );
    let p1 = common::ref_pos(&scene, 1);
    assert!(
        (p1[0] - (-0.5)).abs() < 1e-6 && (p1[1] - 1.0).abs() < 1e-6,
        "top -> {p1:?}"
    );
}

#[test]
fn draw_tri_maps_cache_slots_to_global_indices() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&vtx_bytes(0, 0, 0, 255, 0, 0, 255));
    bytes.extend_from_slice(&vtx_bytes(1, 0, 0, 0, 255, 0, 255));
    bytes.extend_from_slice(&vtx_bytes(0, 1, 0, 0, 0, 255, 255));
    let rdram = RdramImage::new(&bytes);
    let mut rsp = Rsp::default();
    let mut scene = Scene::default();
    rsp.set_vertex(
        &rdram,
        0,
        3,
        0,
        &crate::hle::rdp::Rdp::default(),
        &mut scene,
    );
    rsp.draw_tri(0, 1, 2, 0, 0, [0; 4], &mut scene, None);
    assert_eq!(scene.indices, vec![0, 1, 2]);
}

#[test]
fn phase4_modify_vertex_rgba_patches_bytes_and_clears_lighting_and_fog() {
    let bytes = vtx_bytes(0, 0, 0, 0xAA, 0xBB, 0xCC, 0xDD);
    let rdram = RdramImage::new(&bytes);
    let mut rsp = Rsp::default();
    let mut scene = Scene::default();
    rsp.set_vertex(
        &rdram,
        0,
        1,
        0,
        &crate::hle::rdp::Rdp::default(),
        &mut scene,
    );
    scene.light_index[0] = 7;
    scene.light_count[0] = 3;
    scene.fog[0] = 1;

    rsp.modify_vertex(0, 0x10, 0x1122_3344, &mut scene).unwrap();

    assert_eq!(scene.cn[0], 0x4433_2211);
    assert_eq!(scene.light_index[0], 0);
    assert_eq!(scene.light_count[0], 0);
    assert_eq!(scene.fog[0], 0);
}

#[test]
fn phase4_modify_vertex_st_uses_final_texel_space_unit_scale() {
    let bytes = vtx_bytes(0, 0, 0, 255, 255, 255, 255);
    let rdram = RdramImage::new(&bytes);
    let mut rsp = Rsp::default();
    let mut scene = Scene::default();
    rsp.set_vertex(
        &rdram,
        0,
        1,
        0,
        &crate::hle::rdp::Rdp::default(),
        &mut scene,
    );
    scene.texgen_mode[0] = 2;
    scene.lookat_index[0] = 9;

    rsp.modify_vertex(0, 0x14, 0xFFE0_0020, &mut scene).unwrap();
    rsp.finish(&mut scene);

    assert_eq!(scene.raw_st[0], [-1.0, 1.0]);
    let ti = scene.texcoord_index[0] as usize;
    assert_eq!(scene.texcoord_table[ti], [1.0, 1.0]);
    assert_eq!(scene.texgen_scale_table[ti], [0.0, 0.0]);
    assert_eq!(scene.texgen_mode[0], 0);
    assert_eq!(scene.lookat_index[0], 0);
}

#[test]
fn phase4_modify_vertex_screen_fields_accumulate_flags_and_values() {
    let bytes = vtx_bytes(0, 0, 0, 255, 255, 255, 255);
    let rdram = RdramImage::new(&bytes);
    let mut rsp = Rsp::default();
    let mut scene = Scene::default();
    rsp.set_vertex(
        &rdram,
        0,
        1,
        0,
        &crate::hle::rdp::Rdp::default(),
        &mut scene,
    );

    rsp.modify_vertex(0, 0x18, 0x0080_0040, &mut scene).unwrap();
    rsp.modify_vertex(0, 0x1C, 0x0000_8000, &mut scene).unwrap();

    assert_eq!(scene.modify_flags[0], 3);
    assert_eq!(scene.modify_screen[0], [32.0, 16.0, 0.5, 0.0]);
}

#[test]
fn phase4_modify_vertex_after_draw_copies_row_without_retroactive_edit() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&vtx_bytes(0, 0, 0, 10, 20, 30, 40));
    bytes.extend_from_slice(&vtx_bytes(1, 0, 0, 50, 60, 70, 80));
    bytes.extend_from_slice(&vtx_bytes(0, 1, 0, 90, 100, 110, 120));
    let rdram = RdramImage::new(&bytes);
    let mut rsp = Rsp::default();
    let mut scene = Scene::default();
    rsp.set_vertex(
        &rdram,
        0,
        3,
        0,
        &crate::hle::rdp::Rdp::default(),
        &mut scene,
    );
    rsp.draw_tri(0, 1, 2, 0, 0, [0; 4], &mut scene, None);
    let original_cn = scene.cn[0];

    rsp.modify_vertex(0, 0x10, 0xA1B2_C3D4, &mut scene).unwrap();
    rsp.draw_tri(0, 1, 2, 0, 0, [0; 4], &mut scene, None);

    assert_eq!(scene.indices, vec![0, 1, 2, 3, 1, 2]);
    assert_eq!(scene.cn[0], original_cn);
    assert_eq!(scene.cn[3], 0xD4C3_B2A1);
    assert_eq!(scene.raw_pos.len(), 4);
    assert_eq!(scene.modify_flags.len(), 4);
    assert_eq!(scene.modify_screen.len(), 4);
}

#[test]
fn culled_triangle_does_not_make_later_modify_clone_vertex() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&vtx_bytes(0, 0, 0, 10, 20, 30, 40));
    bytes.extend_from_slice(&vtx_bytes(1, 0, 0, 50, 60, 70, 80));
    bytes.extend_from_slice(&vtx_bytes(0, 1, 0, 90, 100, 110, 120));
    let rdram = RdramImage::new(&bytes);
    let mut rsp = Rsp::default();
    let mut scene = Scene::default();
    rsp.set_vertex(
        &rdram,
        0,
        3,
        0,
        &crate::hle::rdp::Rdp::default(),
        &mut scene,
    );
    rsp.modify_geometry_mode(u32::MAX, crate::hle::consts::G_CULL_BOTH);

    rsp.draw_tri(0, 1, 2, 0, 0, [0; 4], &mut scene, None);
    rsp.modify_vertex(0, 0x10, 0xA1B2_C3D4, &mut scene).unwrap();

    assert!(scene.indices.is_empty());
    assert!(scene.draw_runs.is_empty());
    assert_eq!(scene.raw_pos.len(), 3);
    assert_eq!(scene.cn[0], 0xD4C3_B2A1);
}

fn phase4_prim_material(prim: [u8; 4]) -> crate::hle::Material {
    crate::hle::Material {
        sampling: Default::default(),
        texture: vec![255, 255, 255, 255],
        tex_w: 1,
        tex_h: 1,
        selectors: crate::hle::combiner::decode_combine(0, 0xC3),
        cycle_type: 0,
        filter_mode: 0,
        prim,
        env: [0, 0, 0, 255],
        blend_color: [0, 0, 0, 255],
        tex_enable: false,
        wrap_s: 2,
        wrap_t: 2,
        fmt: 0,
        siz: 0,
        tile_count: 1,
        tex1: None,
        prim_lod_frac: 0.0,
        prim_min_level: 0.0,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    }
}

#[test]
fn phase4_modify_vertex_screen_override_renders_requested_pixel_and_depth() {
    const W: u32 = 64;
    const H: u32 = 64;
    let mut bytes = Vec::new();
    for _ in 0..6 {
        bytes.extend_from_slice(&vtx_bytes(0, 0, 0, 255, 255, 255, 255));
    }
    let rdram = RdramImage::new(&bytes);
    let mut rsp = Rsp::default();
    let mut scene = Scene::default();
    rsp.set_vertex(
        &rdram,
        0,
        6,
        0,
        &crate::hle::rdp::Rdp::default(),
        &mut scene,
    );

    let packed_xy = |x_px: i16, y_px: i16| {
        let x = x_px.wrapping_mul(4) as u16 as u32;
        let y = y_px.wrapping_mul(4) as u16 as u32;
        (x << 16) | y
    };
    // Oversized background triangle covers the whole framebuffer at depth 0.75.
    for (slot, [x, y]) in [[-320, -240], [960, -240], [-320, 720]]
        .into_iter()
        .enumerate()
    {
        rsp.modify_vertex(slot as u32, 0x18, packed_xy(x, y), &mut scene)
            .unwrap();
        rsp.modify_vertex(slot as u32, 0x1C, 0x0000_C000, &mut scene)
            .unwrap();
    }
    // Foreground triangle surrounds N64 screen pixel (160,120), which maps to target pixel (32,32).
    for (slot, [x, y]) in [[120, 80], [200, 80], [160, 160]].into_iter().enumerate() {
        let slot = (slot + 3) as u32;
        rsp.modify_vertex(slot, 0x18, packed_xy(x, y), &mut scene)
            .unwrap();
        rsp.modify_vertex(slot, 0x1C, 0x0000_8000, &mut scene)
            .unwrap();
    }
    rsp.draw_tri(0, 1, 2, 0, 0, [0; 4], &mut scene, None);
    rsp.draw_tri(3, 4, 5, 1, 0, [0; 4], &mut scene, None);
    rsp.finish(&mut scene);

    assert_eq!(scene.modify_flags, vec![3; 6]);
    assert_eq!(scene.modify_screen[3], [120.0, 80.0, 0.5, 0.0]);
    scene.materials = vec![
        phase4_prim_material([255, 0, 0, 255]),
        phase4_prim_material([0, 255, 0, 255]),
    ];
    scene.render_modes = vec![crate::hle::decode_render_mode(
        crate::hle::consts::rdp::G_RM_AA_ZB_OPA_SURF,
        0,
        0,
    )];

    let (device, queue, dual) = crate::render::headless_device();
    let mut renderer =
        crate::render::SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, W, H, dual);
    let pixels = common::render_to_pixels(&device, &queue, &mut renderer, &scene, W, H);
    assert_eq!(
        common::pixel(&pixels, W, 32, 32),
        [0, 255, 0, 255],
        "the Z=0.5 overridden triangle must win over the Z=0.75 background at its requested pixel"
    );
    assert_eq!(
        common::pixel(&pixels, W, 4, 4),
        [255, 0, 0, 255],
        "a pixel outside the overridden foreground triangle must retain the background"
    );
}

#[test]
fn perspective_mvp_emits_clip_space_with_varying_w_and_depth_in_unit_range() {
    use crate::hle::rdp::Rdp;
    // Bake proj = perspective(90, 1, 1, 10) and view = lookat(eye (0,0,5) -> origin, up +Y).
    let (proj, _pn) = n64_gbi::gu::gu_perspective(90.0, 1.0, 1.0, 10.0, 1.0);
    let view = n64_gbi::gu::gu_look_at(0.0, 0.0, 5.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0);
    let mut rdram_bytes = Vec::new();
    rdram_bytes.extend_from_slice(&n64_gbi::encode::mtx_to_bytes(proj)); // proj @ 0
    rdram_bytes.extend_from_slice(&n64_gbi::encode::mtx_to_bytes(view)); // view @ 64
    rdram_bytes.extend_from_slice(&vtx_bytes(0, 0, 0, 255, 255, 255, 255)); // origin @ 128
    rdram_bytes.extend_from_slice(&vtx_bytes(0, 0, 3, 255, 255, 255, 255)); // nearer @ 144
    let rdram = RdramImage::new(&rdram_bytes);

    let mut rsp = Rsp::default();
    // params: PROJECTION|LOAD = 0b110 = 0x06 ; MODELVIEW|LOAD = 0b010 = 0x02.
    rsp.matrix(&rdram, 0, 0x06);
    rsp.matrix(&rdram, 64, 0x02);
    let mut scene = Scene::default();
    rsp.set_vertex(&rdram, 128, 2, 0, &Rdp::default(), &mut scene);
    rsp.finish(&mut scene);

    let p_far = common::ref_pos(&scene, 0); // origin -> eye z = -5
    let p_near = common::ref_pos(&scene, 1); // (0,0,3) -> eye z = -2
    let w_far = p_far[3];
    let w_near = p_near[3];
    // w = -z_eye (perspective): in front of camera, and the farther vertex has the larger w.
    assert!(
        w_far > 0.0 && w_near > 0.0,
        "both in front: {w_far}, {w_near}"
    );
    assert!((w_far - 5.0).abs() < 1e-3, "w_far ~ 5, got {w_far}");
    assert!((w_near - 2.0).abs() < 1e-3, "w_near ~ 2, got {w_near}");
    // Post-divide NDC x,y centered (vertex on the view axis).
    assert!((p_far[0] / w_far).abs() < 1e-4 && (p_far[1] / w_far).abs() < 1e-4);
    // Depth = position.z / w is in [0,1], and the nearer vertex has the smaller depth (Less wins).
    let d_far = p_far[2] / w_far;
    let d_near = p_near[2] / w_near;
    assert!((0.0..=1.0).contains(&d_far), "d_far in [0,1], got {d_far}");
    assert!(
        (0.0..=1.0).contains(&d_near),
        "d_near in [0,1], got {d_near}"
    );
    assert!(
        d_near < d_far,
        "nearer vertex has smaller depth: {d_near} < {d_far}"
    );
}

#[test]
fn soa_raw_inputs_and_per_vertex_state_indices_track_mid_dl_matrices() {
    use crate::hle::rdp::Rdp;
    // Two vertex loads under two DIFFERENT mvps must record two DISTINCT mtx_index values,
    // and the mvp_table entries they point at must equal the active mvp at each load.
    let mut rdram_bytes = Vec::new();
    // proj = scale(2) at off 0, modelA = translate(10,0,0) at 64, modelB = translate(-10,0,0) at 128
    rdram_bytes.extend_from_slice(&n64_gbi::encode::mtx_to_bytes([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 2.0, 0.0, 0.0],
        [0.0, 0.0, 2.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]));
    rdram_bytes.extend_from_slice(&n64_gbi::encode::mtx_to_bytes([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [10.0, 0.0, 0.0, 1.0],
    ]));
    rdram_bytes.extend_from_slice(&n64_gbi::encode::mtx_to_bytes([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [-10.0, 0.0, 0.0, 1.0],
    ]));
    rdram_bytes.extend_from_slice(&vtx_bytes(1, 2, 3, 255, 255, 255, 255)); // @ 192
    let rdram = RdramImage::new(&rdram_bytes);

    let mut rsp = Rsp::default();
    let mut scene = Scene::default();
    rsp.matrix(&rdram, 0, 0x06); // PROJECTION|LOAD
    rsp.matrix(&rdram, 64, 0x02); // MODELVIEW|LOAD modelA
    rsp.set_vertex(&rdram, 192, 1, 0, &Rdp::default(), &mut scene); // vertex 0 under mvp A
    rsp.matrix(&rdram, 128, 0x02); // MODELVIEW|LOAD modelB
    rsp.set_vertex(&rdram, 192, 1, 0, &Rdp::default(), &mut scene); // vertex 1 under mvp B
    rsp.finish(&mut scene); // flush state tables onto the scene

    assert_eq!(scene.raw_pos.len(), 2);
    assert_eq!(scene.raw_pos[0], [1.0, 2.0, 3.0]); // object-space, untransformed
    assert_eq!(scene.mtx_index.len(), 2);
    assert_ne!(
        scene.mtx_index[0], scene.mtx_index[1],
        "two loads, two distinct mvp indices"
    );
    // A translated +10 then *2 (proj) => clip.x = (1+10)*2 = 22 ; B => (1-10)*2 = -18.
    let ma = scene.mvp_table[scene.mtx_index[0] as usize];
    let mb = scene.mvp_table[scene.mtx_index[1] as usize];
    assert!((crate::hle::math::mul_row_vec4([1.0, 2.0, 3.0, 1.0], ma)[0] - 22.0).abs() < 1e-3);
    assert!((crate::hle::math::mul_row_vec4([1.0, 2.0, 3.0, 1.0], mb)[0] - (-18.0)).abs() < 1e-3);
}
