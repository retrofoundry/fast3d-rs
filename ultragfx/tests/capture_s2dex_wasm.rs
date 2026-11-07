#![cfg(feature = "capture")]

use std::ops::ControlFlow;
use ultragfx::capture::{
    Fixture, MemoryLayout, MemorySpan, ObjBgAbi, ReplayHardware, SourceLayout, Task,
};
use ultragfx::inspect::{Emission, WalkObserver, WalkStep, WalkTermination};
use ultragfx::{DataFormat, Hardware, Microcode, ObjBgKind, Rdram};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn capture_v2_s2dex_host_replays_on_wasm32() {
    let entry = 0x1_0000_0000;
    let object_at = 0x2_0000_0000;
    let image_at = 0x3_0000_0000u64;
    let s2dex_token: u64 = 0x7_1234_5678;
    let f3dex_token: u64 = 0x8_1234_5678;
    let mut object = vec![0; 44];
    for (offset, value) in [
        (2, 32u16),
        (6, 32),
        (10, 32),
        (14, 32),
        (24, 0xfff4),
        (32, 5),
        (34, 408),
        (36, 1632),
        (38, 407),
        (40, 4),
    ] {
        object[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    object[16..24].copy_from_slice(&image_at.to_le_bytes());
    object[27] = 2;
    let commands: Vec<u8> = [
        (0xe1000000u64, 0xdead0000),
        (0xdd0007ff, s2dex_token),
        (0xff100007, 0x4_0000_0000),
        (0xed000000, 0x00020020),
        (0xef200cf0, 0),
        (0x0a000000, object_at),
        (0xe1000000, 0xdead0000),
        (0xdd0007ff, f3dex_token),
        (0xdf000000, 0),
    ]
    .into_iter()
    .flat_map(|(a, b)| [a.to_le_bytes(), b.to_le_bytes()].concat())
    .collect();
    let mut fixture = Fixture::from_bytes(include_bytes!("fixtures/host64-fill.f3dcap")).unwrap();
    fixture.frame.width = 8;
    fixture.frame.height = 8;
    fixture.tasks = vec![Task {
        entry,
        microcode: Microcode::F3dex2,
        data_format: DataFormat::Float,
        order: 0,
        bindings: vec![
            (s2dex_token, Microcode::S2dex2),
            (f3dex_token, Microcode::F3dex2),
        ],
        source: SourceLayout {
            memory: MemoryLayout::HOST64_LE,
            segments: [0; 16],
        },
        spans: vec![
            MemorySpan {
                address: entry,
                bytes: commands,
            },
            MemorySpan {
                address: object_at,
                bytes: object,
            },
            MemorySpan {
                address: image_at,
                bytes: vec![0xff; 128],
            },
        ],
    }];
    let encoded = fixture.to_bytes().unwrap();
    assert_eq!(&encoded[8..12], &3u32.to_le_bytes());
    let fixture = Fixture::from_bytes(&encoded).unwrap();
    let task = &fixture.tasks[0];
    assert_eq!(task.source.memory.obj_bg_abi, ObjBgAbi::Host64);
    let hardware = ReplayHardware::new(task, None).unwrap();
    let bg = hardware
        .rdram()
        .read_obj_bg(object_at, ObjBgKind::Copy)
        .unwrap();
    assert_eq!(bg.image_ptr, image_at);
    assert_eq!(bg.tmem_h, 408);
    struct Observer {
        rectangles: usize,
        decoders: Vec<Microcode>,
    }
    impl WalkObserver for Observer {
        fn command(&mut self, step: WalkStep<'_>) -> ControlFlow<()> {
            self.decoders.push(step.microcode);
            for emission in step.emissions {
                if let Emission::TexRect {
                    rect, dsdx, dtdy, ..
                } = emission
                {
                    assert_eq!([rect.ulx, rect.uly, rect.lrx, rect.lry], [0, 0, 28, 28]);
                    assert_eq!((*dsdx, *dtdy), (4096, 1024));
                    self.rectangles += 1;
                }
            }
            ControlFlow::Continue(())
        }
    }
    let mut observer = Observer {
        rectangles: 0,
        decoders: Vec::new(),
    };
    let result = ultragfx::inspect::walk_with_bindings(
        hardware.rdram(),
        entry,
        task.microcode,
        task.data_format,
        &mut observer,
        &task.bindings,
    );
    hardware.check().unwrap();
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(result.termination, WalkTermination::End);
    assert_eq!(observer.rectangles, 1);
    assert_eq!(
        observer.decoders,
        vec![
            Microcode::F3dex2,
            Microcode::F3dex2,
            Microcode::S2dex2,
            Microcode::S2dex2,
            Microcode::S2dex2,
            Microcode::S2dex2,
            Microcode::S2dex2,
            Microcode::S2dex2,
            Microcode::F3dex2
        ]
    );
}
