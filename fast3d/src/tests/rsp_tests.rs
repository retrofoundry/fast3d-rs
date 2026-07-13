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
    rsp.draw_tri(0, 1, 2, 0, 0, &mut scene, None);
    assert_eq!(scene.indices, vec![0, 1, 2]);
}

#[test]
fn perspective_mvp_emits_clip_space_with_varying_w_and_depth_in_unit_range() {
    use crate::hle::rdp::Rdp;
    // Bake proj = perspective(90, 1, 1, 10) and view = lookat(eye (0,0,5) -> origin, up +Y).
    let (proj, _pn) = crate::asm::gu::gu_perspective(90.0, 1.0, 1.0, 10.0, 1.0);
    let view = crate::asm::gu::gu_look_at(0.0, 0.0, 5.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0);
    let mut rdram_bytes = Vec::new();
    rdram_bytes.extend_from_slice(&crate::asm::encode::mtx_to_bytes(proj)); // proj @ 0
    rdram_bytes.extend_from_slice(&crate::asm::encode::mtx_to_bytes(view)); // view @ 64
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
    rdram_bytes.extend_from_slice(&crate::asm::encode::mtx_to_bytes([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 2.0, 0.0, 0.0],
        [0.0, 0.0, 2.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]));
    rdram_bytes.extend_from_slice(&crate::asm::encode::mtx_to_bytes([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [10.0, 0.0, 0.0, 1.0],
    ]));
    rdram_bytes.extend_from_slice(&crate::asm::encode::mtx_to_bytes([
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
