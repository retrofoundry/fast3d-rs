use crate::hle::consts::rsp_f3dex2::{G_ENDDL, G_MOVEMEM, G_MV_LIGHT};
use crate::hle::mem::RdramImage;
use crate::hle::rsp::Rsp;

#[test]
fn sp_lookat_emit_decode_sets_lookat_axes() {
    // Eye on +Z looking at origin, up +Y -> Right (S) = +X, Up' (T) = +Y (see gu_look_at_reflect).
    let (rdram, entry_addr) = crate::tests::fixtures::fixture("lookat--positive-z");

    // Decode (w0, w1) pairs from entry_addr until G_ENDDL.
    let mut off = entry_addr as usize;
    let mut cmds: Vec<(u32, u32)> = Vec::new();
    loop {
        let w0 = u32::from_be_bytes(rdram.to_vec()[off..off + 4].try_into().unwrap());
        let w1 = u32::from_be_bytes(rdram.to_vec()[off + 4..off + 8].try_into().unwrap());
        cmds.push((w0, w1));
        off += 8;
        if (w0 >> 24) as u8 == G_ENDDL {
            break;
        }
    }

    // Drive the two MOVEMEM(G_MV_LIGHT) words through the real byte_off/24 slot routing.
    let rd = RdramImage::new(rdram);
    let mut rsp = Rsp::default();
    let mut saw_lookat_movemem = false;
    for (w0, w1) in cmds {
        if (w0 >> 24) as u8 == G_MOVEMEM && (w0 & 0xFF) == G_MV_LIGHT as u32 {
            let byte_off = ((w0 >> 8) & 0xFF) * 8; // p0(8,8) * 8
            let light_idx = byte_off / 24;
            assert!(light_idx < 2, "lookat MOVEMEM slot must be 0 (S) or 1 (T)");
            rsp.set_lookat(&rd, light_idx, w1 as u64);
            saw_lookat_movemem = true;
        }
    }
    assert!(
        saw_lookat_movemem,
        "gsSPLookAt must emit G_MOVEMEM(G_MV_LIGHT) words"
    );

    // S = +X, T = +Y after the /127 s8 decode.
    assert_eq!(rsp.lookat_axes[0], [1.0, 0.0, 0.0], "S axis = +X");
    assert_eq!(rsp.lookat_axes[1], [0.0, 1.0, 0.0], "T axis = +Y");
}
