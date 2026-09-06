use super::dl_builder::DlBuilder;
use crate::hle::interpret_rdram;
use n64_gbi::encode::*;

#[test]
fn primdepth_words_and_roundtrip() {
    for (z, dz, expected) in [
        (0, 0, (0xee00_0000, 0)),
        (0x1234, 0xabcd, (0xee00_0000, 0x1234_abcd)),
        (0xffff, 0x8000, (0xee00_0000, 0xffff_8000)),
        (0, 0xffff, (0xee00_0000, 0x0000_ffff)),
    ] {
        assert_eq!(gdp_set_prim_depth(z, dz), expected);
        let mut dl = DlBuilder::new();
        dl.list("main", &[expected, gsp_enddl()]);
        let built = dl.finish("main");
        let result = interpret_rdram(&built.rdram, built.entry);
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        assert_eq!(result.rdp.prim_depth.z, z as u16);
        assert_eq!(result.rdp.prim_depth.dz, dz as u16);
    }
}

#[test]
fn primdepth_setter_alone_is_silent() {
    let mut dl = DlBuilder::new();
    dl.list("baseline", &[(0, 0), (0, 0), gsp_enddl()]);
    dl.list(
        "setters",
        &[
            (0xee00_0000, 0x1234_abcd),
            (0xee00_0000, 0xffff_8000),
            gsp_enddl(),
        ],
    );
    let baseline = dl.address("baseline");
    let built = dl.finish("setters");
    let baseline = interpret_rdram(&built.rdram, baseline);
    let result = interpret_rdram(&built.rdram, built.entry);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert_eq!(result.summary(false), baseline.summary(false));
    assert_eq!(result.scene, baseline.scene);
}
