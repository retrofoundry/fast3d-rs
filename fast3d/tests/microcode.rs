use fast3d::detect_microcode;

#[test]
fn production_detection_rejects_fixture_hashes() {
    for hash in [
        0xF3D2_0000_0000_0001,
        0xF3D0_0000_0000_0003,
        0xDEAD_BEEF,
        0,
        u64::MAX,
    ] {
        assert_eq!(detect_microcode(hash), None, "hash {hash:#018x}");
    }
}
