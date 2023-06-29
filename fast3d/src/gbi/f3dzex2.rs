use crate::rsp::RSP;
use super::{f3dex2::F3DEX2, GBIDefinition, GBI};

pub struct F3DZEX2;

impl GBIDefinition for F3DZEX2 {
    fn setup(gbi: &mut GBI, rsp: &mut RSP) {
        F3DEX2::setup(gbi, rsp);
    }
}
