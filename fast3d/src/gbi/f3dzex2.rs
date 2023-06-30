use super::{f3dex2::F3DEX2, GBICommandRegistry, GBIMicrocode};
use crate::rsp::RSP;

pub struct F3DZEX2;

impl GBIMicrocode for F3DZEX2 {
    fn setup(gbi: &mut GBICommandRegistry, rsp: &mut RSP) {
        F3DEX2::setup(gbi, rsp);
    }
}
