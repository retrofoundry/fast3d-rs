use super::dl_builder::DlBuilder;
use crate::hle::interpret_rdram;
use n64_gbi::encode::*;

fn walk(commands: &[(u32, u32)]) -> crate::hle::InterpResult {
    let mut dl = DlBuilder::new();
    let mut commands = commands.to_vec();
    commands.push(gsp_enddl());
    dl.list("main", &commands);
    let built = dl.finish("main");
    interpret_rdram(&built.rdram, built.entry)
}

#[test]
fn convert_key_setters_alone_are_silent() {
    let result = walk(&[
        (0xec20_1ff0, 0x03fe_0100),
        (0xeb00_0000, 0x0abc_1234),
        (0xea12_3fed, 0x5678_9abc),
    ]);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert_eq!(result.summary(false).errors, 0);
    assert_eq!(result.summary(false).warns, 0);
    assert_eq!(result.dropped_runs, 0);
    let control = walk(&[(0, 0); 3]);
    assert_eq!(result.scene, control.scene);
    assert_eq!(result.summary(false), control.summary(false));
}

#[test]
fn convert_signed_nine_bit_literals() {
    for (words, value) in [
        ((0xec20_1008, 0x0402_0100), -256),
        ((0xec3f_ffff, 0xffff_ffff), -1),
        ((0xec00_0000, 0x0000_0000), 0),
        ((0xec1f_eff7, 0xfbfd_feff), 255),
    ] {
        let result = walk(&[words]);
        assert!(result.diags.is_empty());
        assert_eq!(result.rdp.convert, [value; 6]);
    }
}

#[test]
fn convert_k2_crosses_word_boundary() {
    for (words, value) in [
        ((0xec00_0000, 0xf800_0000), 31),
        ((0xec00_0001, 0x0000_0000), 32),
        ((0xec00_0007, 0xf800_0000), 255),
        ((0xec00_0008, 0x0000_0000), -256),
        ((0xec00_000f, 0xf800_0000), -1),
    ] {
        assert_eq!(walk(&[words]).rdp.convert, [0, 0, value, 0, 0, 0]);
    }
}

#[test]
fn keyr_keygb_preserve_other_channels() {
    use crate::hle::rdp::KeyChannel;
    let red = (0xeb00_0000, 0x0abc_1234);
    let gb = (0xea12_3fed, 0x5678_9abc);
    let expected = [
        KeyChannel {
            center: 0x12,
            scale: 0x34,
            width: 0xabc,
        },
        KeyChannel {
            center: 0x56,
            scale: 0x78,
            width: 0x123,
        },
        KeyChannel {
            center: 0x9a,
            scale: 0xbc,
            width: 0xfed,
        },
    ];
    for commands in [[red, gb], [gb, red]] {
        assert_eq!(walk(&commands).rdp.key, expected);
    }
    let result = walk(&[red, gb, (0xeb00_0000, 0x0fff_ffff)]);
    assert_eq!(result.rdp.key[1..], expected[1..]);
    assert_eq!(
        result.rdp.key[0],
        KeyChannel {
            center: 255,
            scale: 255,
            width: 4095
        }
    );
    let result = walk(&[gb, red, (0xea00_0000, 0)]);
    assert_eq!(
        result.rdp.key,
        [expected[0], KeyChannel::default(), KeyChannel::default()]
    );
}

#[test]
fn key_width_is_retained() {
    for (words, expected) in [
        (
            [(0xeb00_0000, 0x0800_0000), (0xea00_1fff, 0)],
            [0x800, 1, 0xfff],
        ),
        (
            [(0xebff_ffff, 0xf123_0000), (0xeaab_cdef, 0)],
            [0x123, 0xabc, 0xdef],
        ),
    ] {
        assert_eq!(walk(&words).rdp.key.map(|channel| channel.width), expected);
    }
}

#[test]
fn convert_key_words_and_roundtrip() {
    let convert = gdp_set_convert(-256, -1, 0, 255, -256, -1);
    assert_eq!(convert, (0xec20_1ff0, 0x03fe_01ff));
    let keyr = gdp_set_key_r(0x12, 0x34, 0xabc);
    let keygb = gdp_set_key_gb(0x56, 0x78, 0x123, 0x9a, 0xbc, 0xfed);
    assert_eq!(keyr, (0xeb00_0000, 0x0abc_1234));
    assert_eq!(keygb, (0xea12_3fed, 0x5678_9abc));
    let result = walk(&[convert, keyr, keygb]);
    assert_eq!(result.rdp.convert, [-256, -1, 0, 255, -256, -1]);
    assert_eq!(
        result.rdp.key.map(|channel| channel.center),
        [0x12, 0x56, 0x9a]
    );
    assert_eq!(
        result.rdp.key.map(|channel| channel.scale),
        [0x34, 0x78, 0xbc]
    );
    assert_eq!(
        result.rdp.key.map(|channel| channel.width),
        [0xabc, 0x123, 0xfed]
    );
}

#[test]
fn convert_negative_range_uses_signed_arithmetic() {
    for slot in 0..6 {
        for pattern in 256..512u64 {
            let payload = pattern * 512u64.pow(5 - slot as u32);
            let words = (
                0xec00_0000 + (payload / 0x1_0000_0000) as u32,
                (payload % 0x1_0000_0000) as u32,
            );
            let mut expected = [0; 6];
            expected[slot] = pattern as i16 - 512;
            assert_eq!(
                walk(&[words]).rdp.convert,
                expected,
                "slot {slot}, pattern {pattern}"
            );
        }
    }
}
