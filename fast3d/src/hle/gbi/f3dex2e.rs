//! F3DEX2E (GBI_FLOATS) install: composes the F3DEX2 base, then patches.
use crate::hle::interp::Handler;
use crate::hle::mem::Rdram;

/// Install the F3DEX2E table = F3DEX2 base + single-word overrides.
pub(crate) fn install_f3dex2e<M: Rdram>(table: &mut [Handler<M>; 256]) {
    super::f3dex2::install_f3dex2(table);
    install_f3dex2e_overrides(table);
}

/// F3DEX2E's single-word opcode deltas. CURRENTLY EMPTY (proven by the table-parity
/// test). F3DEX2E differs from F3DEX2 only by data format (Float, Task 4) and by the
/// multi-word rect commands (TEXRECT/FILLRECT), which the 2D slice will special-case
/// INLINE in the walk loop gated on the ucode — a `Handler` cannot advance `pc`, so
/// they are NOT table slots. This slot is for future SINGLE-word deltas only.
fn install_f3dex2e_overrides<M: Rdram>(_table: &mut [Handler<M>; 256]) {}
