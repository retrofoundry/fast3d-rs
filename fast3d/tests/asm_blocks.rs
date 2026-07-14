#![cfg(feature = "asm")]
use fast3d::asm::asm::assemble;

fn word(rdram: &[u8], off: usize) -> (u32, u32) {
    (
        u32::from_be_bytes(rdram[off..off + 4].try_into().unwrap()),
        u32::from_be_bytes(rdram[off + 4..off + 8].try_into().unwrap()),
    )
}

#[test]
fn named_blocks_subdl_and_segment_resolve() {
    // main maps segment 6 -> the `quad` block, then calls it through the segment, then ends.
    let src = "\
Vtx { 0, 0, 0, 0, 0, 0, 255,255,255,255 }
Gfx quad[] = {
  gsSPVertex(verts, 1, 0)
  gsSPEndDisplayList()
}
Gfx main[] = {
  gsSPSegment(6, quad)
  gsSPDisplayList(seg(6, 0))
  gsSPEndDisplayList()
}
";
    let img = assemble(src).expect("assemble");
    // main is laid first (entry right after the data section); `quad` after it, 8-aligned.
    let e = img.entry_addr as usize;
    // main[0] = gsSPSegment(6, quad_addr): w0 = G_MOVEWORD|G_MW_SEGMENT|seg6 = 0xDB06_0018, w1 = quad phys addr.
    let (seg_w0, seg_w1) = word(&img.rdram, e);
    assert_eq!(seg_w0, 0xDB06_0018, "segment w0 (seg index 6 << 2 = 0x18)");
    // main[1] = gsSPDisplayList(seg(6,0)): w0 = 0xDE000000 (call, branch bit clear), w1 = 0x06000000.
    let (dl_w0, dl_w1) = word(&img.rdram, e + 8);
    assert_eq!(dl_w0, 0xDE00_0000);
    assert_eq!(dl_w1, 0x0600_0000);
    // The segment base (seg_w1) is the physical address of the `quad` block (8-aligned, past main).
    assert!(seg_w1 > img.entry_addr, "quad block laid after main");
    assert_eq!(seg_w1 % 8, 0, "block 8-aligned");
}

#[test]
fn paren_aware_seg_operand_with_offset() {
    let src = "\
Gfx main[] = {
  gsSPDisplayList(seg(6, 16))
  gsSPEndDisplayList()
}
";
    let img = assemble(src).expect("assemble");
    let (_w0, w1) = word(&img.rdram, img.entry_addr as usize);
    assert_eq!(w1, 0x0600_0010, "(6<<24)|16");
}

#[test]
fn tri2_and_popmatrix_author_through_assembler() {
    let src = "\
Vtx { 0, 0, 0, 0, 0, 0, 255,255,255,255 }
Mtx m = identity()
Gfx main[] = {
  gsSPMatrix(m, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_PUSH)
  gsSP2Triangles(0, 1, 2, 0, 0, 2, 3, 0)
  gsSPPopMatrix(2)
  gsSPEndDisplayList()
}
";
    let img = assemble(src).expect("assemble");
    let e = img.entry_addr as usize;
    // [1] = gsSP2Triangles(0,1,2,0,0,2,3,0) -> (0x06000204, 0x00000406).
    assert_eq!(word(&img.rdram, e + 8), (0x0600_0204, 0x0000_0406));
    // [2] = gsSPPopMatrix(2) -> (0xD8000000, 0x80).
    assert_eq!(word(&img.rdram, e + 16), (0xD800_0000, 0x0000_0080));
}

#[test]
fn flat_source_still_assembles_as_implicit_main() {
    // No Gfx[] block: top-level commands form the implicit `main` (entry right after data).
    let src = "\
Vtx { 0, 0, 0, 0, 0, 0, 255,255,255,255 }
gsSPVertex(verts, 1, 0)
gsSPEndDisplayList()
";
    let img = assemble(src).expect("assemble");
    // entry is the first command (a G_VTX, opcode 0x01).
    let (w0, _w1) = word(&img.rdram, img.entry_addr as usize);
    assert_eq!(w0 >> 24, 0x01, "implicit-main entry is the G_VTX");
}

#[test]
fn branchlist_authors_to_branch_word() {
    // gsSPBranchList(sub) -> w0=0xDE010000 (branch bit set), w1=sub block phys addr.
    let src = "\
Gfx sub[] = {
  gsSPEndDisplayList()
}
Gfx main[] = {
  gsSPBranchList(sub)
}
";
    let img = assemble(src).expect("assemble");
    let (w0, w1) = word(&img.rdram, img.entry_addr as usize);
    assert_eq!(w0, 0xDE01_0000, "branch bit set");
    assert!(
        w1 > img.entry_addr && w1 % 8 == 0,
        "sub block laid after main, 8-aligned"
    );
}

#[test]
fn unknown_block_symbol_is_diagnosed() {
    // `verts` in gsSPVertex is a fixed pool placeholder (ignored), NOT a resolved symbol; the
    // error here is the gsSPDisplayList target `nope`.
    let src = "\
Gfx main[] = {
  gsSPDisplayList(nope)
  gsSPEndDisplayList()
}
";
    let err = assemble(src).expect_err("should error on unknown symbol");
    assert!(
        err.iter().any(|d| d.msg.contains("unknown symbol")),
        "{err:?}"
    );
}

#[test]
fn error_in_main_with_following_block_returns_err_without_panic() {
    // A diagnosed error in main (a non-last block) must not trip the layout debug_assert when
    // emitting the following `tail` block — assemble returns Err cleanly. (See Step 6 guard.)
    let src = "\
Gfx main[] = {
  gsSPDisplayList(nope)
  gsSPEndDisplayList()
}
Gfx tail[] = {
  gsSPEndDisplayList()
}
";
    let err = assemble(src).expect_err("should error");
    assert!(
        err.iter().any(|d| d.msg.contains("unknown symbol")),
        "{err:?}"
    );
}
