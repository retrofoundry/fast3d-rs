use super::{f3dex2, GBICommandRegistry};
use crate::rsp::RSP;

pub fn setup(gbi: &mut GBICommandRegistry, rsp: &mut RSP) {
    f3dex2::setup(gbi, rsp);
}
