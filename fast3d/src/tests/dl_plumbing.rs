use crate::hle::interpret_rdram;
use n64_gbi::encode::{gsp_branchlist, gsp_displaylist, gsp_enddl, gsp_segment};

/// Write a single 8-byte command at byte offset `off`.
fn put(rdram: &mut [u8], off: usize, w0: u32, w1: u32) {
    rdram[off..off + 4].copy_from_slice(&w0.to_be_bytes());
    rdram[off + 4..off + 8].copy_from_slice(&w1.to_be_bytes());
}

#[test]
fn g_dl_call_returns_to_caller() {
    // entry @0x00:  G_DL call 0x20
    //               G_MOVEWORD seg 5 = 0xAA  (executed AFTER returning from the call)
    //               G_ENDDL  -> empty stack -> end
    // sub   @0x20:  G_MOVEWORD seg 4 = 0xBB
    //               G_ENDDL  -> pop -> return to 0x08
    let mut rdram = vec![0u8; 0x100];
    let (c0, c1) = gsp_displaylist(0x20);
    put(&mut rdram, 0x00, c0, c1);
    let (s5w0, s5w1) = gsp_segment(5, 0xAA);
    put(&mut rdram, 0x08, s5w0, s5w1);
    let (e0, e1) = gsp_enddl();
    put(&mut rdram, 0x10, e0, e1);
    let (s4w0, s4w1) = gsp_segment(4, 0xBB);
    put(&mut rdram, 0x20, s4w0, s4w1);
    put(&mut rdram, 0x28, e0, e1);

    let r = interpret_rdram(&rdram, 0x00);
    assert!(r.diags.is_empty(), "unexpected diag: {:?}", r.diags);
}

#[test]
fn g_dl_branch_does_not_return() {
    // entry @0x00:  G_DL BRANCH 0x20
    //               UNKNOWN 0xAB  (must NOT execute -- branch never returns to 0x08)
    //               G_ENDDL
    // tgt   @0x20:  G_ENDDL  -> empty stack -> end (no return to 0x08)
    let mut rdram = vec![0u8; 0x100];
    let (b0, b1) = gsp_branchlist(0x20);
    put(&mut rdram, 0x00, b0, b1);
    // Genuinely-unknown opcode at 0x08: if branch WRONGLY returns here it is diagnosed.
    put(&mut rdram, 0x08, 0xAB00_0000, 0);
    let (e0, e1) = gsp_enddl();
    put(&mut rdram, 0x10, e0, e1);
    put(&mut rdram, 0x20, e0, e1);

    let r = interpret_rdram(&rdram, 0x00);
    assert!(
        r.diags.iter().all(|d| d.at != 0x08),
        "branch wrongly returned and dispatched 0x08: {:?}",
        r.diags
    );
}

#[test]
fn nested_calls_return_in_lifo_order() {
    // entry @0x00: G_DL call A(0x20); G_ENDDL(end)
    // A     @0x20: G_DL call B(0x40); G_ENDDL(return to 0x08 -> entry end)
    // B     @0x40: G_ENDDL(return to 0x28 -> A's enddl)
    let mut rdram = vec![0u8; 0x100];
    let (e0, e1) = gsp_enddl();
    let (a0, a1) = gsp_displaylist(0x20);
    put(&mut rdram, 0x00, a0, a1);
    put(&mut rdram, 0x08, e0, e1);
    let (b0, b1) = gsp_displaylist(0x40);
    put(&mut rdram, 0x20, b0, b1);
    put(&mut rdram, 0x28, e0, e1);
    put(&mut rdram, 0x40, e0, e1);

    let r = interpret_rdram(&rdram, 0x00);
    assert!(
        r.diags.is_empty(),
        "nested call/return produced a diag: {:?}",
        r.diags
    );
}

#[test]
fn segment_masked_vs_unmasked_resolution() {
    // Assert the resolvers DIRECTLY via the public RdramImage (no DL / no Material involved).
    use crate::hle::mem::RdramImage;
    let bytes = vec![0u8; 0x40];
    let mut rd = RdramImage::new(&bytes);
    rd.set_segment(7, 0x05);
    // Unmasked (SETTIMG path): 0x05 + 3 = 0x08.
    assert_eq!(rd.from_segmented(0x0700_0003), 0x08);
    // Masked (vtx/mtx/viewport path): (0x05 + 3) & 0x00FFFFF8 = 0x08 (8 is already 8-aligned).
    // Note: the plan had a comment error saying 0x00; the correct value is 0x08 (plan errata,
    // documented in interp_tests.rs:segment_store_then_resolve_masked_and_unmasked).
    assert_eq!(rd.from_segmented_masked(0x0700_0003), 0x08);
    // Identity map for segment 0 (preserves the existing sample):
    assert_eq!(rd.from_segmented(0x0000_0040), 0x40);
    assert_eq!(rd.from_segmented_masked(0x0000_0040), 0x40);
}

#[test]
fn no_enddl_runs_past_rdram_and_diagnoses_bounds() {
    // A DL with NO terminating G_ENDDL: a single G_VTX-shaped command then the buffer ends.
    // The per-read bounds check (distinct from the CAP) must fire once pc + 8 > len.
    let mut rdram = vec![0u8; 0x10];
    put(&mut rdram, 0x00, 0x0000_0000, 0); // G_NOOP
    put(&mut rdram, 0x08, 0x0000_0000, 0); // G_NOOP
    let r = interpret_rdram(&rdram, 0x00);
    assert!(
        r.diags
            .iter()
            .any(|d| d.kind.to_string().contains("past RDRAM")),
        "expected a bounds diagnostic for the no-ENDDL run-off: {:?}",
        r.diags
    );
    // And it is NOT the runaway-cap diagnostic.
    assert!(
        r.diags
            .iter()
            .all(|d| !d.kind.to_string().contains("runaway")),
        "bounds path must not report runaway: {:?}",
        r.diags
    );
}

#[test]
fn self_branch_trips_the_runaway_guard_instead_of_hanging() {
    // entry @0x00: G_DL BRANCH 0x00  -> jumps to itself forever.
    let mut rdram = vec![0u8; 0x10];
    let (b0, b1) = gsp_branchlist(0x00);
    put(&mut rdram, 0x00, b0, b1);

    let r = interpret_rdram(&rdram, 0x00);
    assert!(
        r.diags
            .iter()
            .any(|d| d.kind.to_string().contains("runaway")),
        "self-branch must produce a runaway diagnostic: {:?}",
        r.diags
    );
}
