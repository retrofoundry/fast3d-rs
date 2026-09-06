#![cfg(feature = "capture")]

use fast3d::capture::{CaptureError, Fixture, ReplayHardware};
use fast3d::{Hardware, MemoryError, MemoryErrorKind, Rdram};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn capture_high_addresses_resolve_on_this_platform() {
    let mut fixture = Fixture::from_bytes(include_bytes!("fixtures/host64-fill.f3dcap")).unwrap();
    let task = &mut fixture.tasks[0];
    task.source.segments[7] = task.entry;
    let replay = ReplayHardware::new(task, None).unwrap();
    let mem = replay.rdram();
    let entry = mem.resolve_masked(0x0700_0000).unwrap();
    assert_eq!(entry, 0x123456000);
    assert_eq!(mem.command_stride(), 16);
    assert_eq!(mem.read_command(entry).unwrap().w0, 0xed000000);
    let cimg = mem.read_command(entry + mem.command_stride()).unwrap();
    assert_eq!(cimg.w0, 0xff10003f);
    assert_eq!(cimg.w1, 0x34567000);
    assert_eq!(cimg.w1_addr, 0x234567000);
    assert_eq!(mem.read_u16(entry + 16).unwrap(), 0x003f);
    assert_eq!(&*mem.read_bytes(entry + 17, 3).unwrap(), &[0, 0x10, 0xff]);
    replay.check().unwrap();
    assert!(!mem.in_bounds(entry - 1, 16));
    replay.check().unwrap();
    assert_eq!(
        mem.read_command(entry - 1),
        Err(MemoryError {
            address: 0x123455fff,
            length: 16,
            kind: MemoryErrorKind::Unavailable,
        })
    );
    assert!(matches!(
        replay.check(),
        Err(CaptureError::MissingSpan {
            address: 0x123455fff,
            length: 16
        })
    ));
}
