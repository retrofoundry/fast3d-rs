#![cfg(all(feature = "capture", feature = "profiling"))]

use fast3d::{
    capture::{Fixture, MemorySpan, ReplayHardware, Sequence},
    profiling::{CpuInterpreter, Mode, Recorder, Representation, Request, Snapshot},
    Hardware, PresentTarget, Rdram, Renderer,
};
use n64_gbi::encode::{gdp_set_cycle_type_f3d, gsp_setothermode_h_f3d};

#[allow(dead_code)]
#[path = "common/tmem_semantics.rs"]
mod semantics;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn fixture(name: &str) -> Fixture {
    Fixture::from_bytes(match name {
        "tmem-layouts" => include_bytes!("fixtures/tmem-layouts.f3dcap"),
        "tmem-tlut-mutation" => include_bytes!("fixtures/tmem-tlut-mutation.f3dcap"),
        "tmem-roles-lifetime" => include_bytes!("fixtures/tmem-roles-lifetime.f3dcap"),
        _ => panic!("unknown fixture {name}"),
    })
    .unwrap()
}

fn requests(fixture: &Fixture) -> Vec<Request> {
    let mut interpreter = CpuInterpreter::default();
    interpreter.recorder = Recorder::new(Mode::Trace);
    for task in &fixture.tasks {
        let hardware = ReplayHardware::new(task, None).unwrap();
        let (_, diagnostics) = interpreter.process(
            hardware.rdram(),
            task.entry,
            task.microcode,
            task.data_format,
        );
        hardware.check().unwrap();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
    interpreter.recorder.drain().requests
}

fn commands(fixture: &Fixture) -> Vec<(u32, u32)> {
    let task = &fixture.tasks[0];
    let hardware = ReplayHardware::new(task, None).unwrap();
    let memory = hardware.rdram();
    let mut commands = Vec::new();
    for index in 0..4096 {
        let command = memory.read_command(task.entry + index * 8).unwrap();
        commands.push((command.w0, command.w1));
        if command.w0 >> 24 == 0xb8 {
            return commands;
        }
    }
    panic!("unterminated authored command list");
}

fn replace_commands(fixture: &mut Fixture, commands: Vec<(u32, u32)>) {
    let task = &mut fixture.tasks[0];
    task.entry = 0x0020_0000;
    task.spans.retain(|span| span.address != task.entry);
    task.spans.push(MemorySpan {
        address: task.entry,
        bytes: commands
            .into_iter()
            .flat_map(|(w0, w1)| [w0, w1])
            .flat_map(u32::to_be_bytes)
            .collect(),
    });
    task.spans.sort_by_key(|span| span.address);
}

fn draw_start(commands: &[(u32, u32)]) -> usize {
    commands
        .iter()
        .position(|&(w0, _)| w0 == 0xbb00_0801)
        .unwrap()
}

fn reordered(fixture: &Fixture) -> Fixture {
    let mut fixture = fixture.clone();
    let original = commands(&fixture);
    let start = draw_start(&original);
    let mut commands = original[..start].to_vec();
    for draw in [2, 0, 1] {
        commands.extend_from_slice(&original[start + draw * 5..start + (draw + 1) * 5]);
    }
    commands.extend_from_slice(&original[start + 15..]);
    replace_commands(&mut fixture, commands);
    fixture
}

fn sampling_changes(fixture: &Fixture) -> Fixture {
    let mut fixture = fixture.clone();
    let mut commands = commands(&fixture);
    let start = draw_start(&commands);
    commands.splice(
        start..start,
        [
            n64_gbi::encode::gdp_set_prim_color(0, 0, 0x157b_39ff),
            n64_gbi::encode::gdp_set_env_color(0xb51f_91ff),
            gsp_setothermode_h_f3d(12, 2, 2 << 12),
        ],
    );
    replace_commands(&mut fixture, commands);
    fixture
}

fn one_role(role: &str) -> Fixture {
    let mut fixture = fixture("tmem-roles-lifetime");
    let original = commands(&fixture);
    let start = draw_start(&original);
    let draw = usize::from(role == "detail");
    let mut commands = original[..start].to_vec();
    if role == "texture1" {
        commands.extend([
            gsp_setothermode_h_f3d(16, 1, 0),
            gdp_set_cycle_type_f3d(1),
            (0xfc88_7f10, 0x88fc_fc7e),
        ]);
    } else if role == "detail" {
        commands.push(gsp_setothermode_h_f3d(17, 2, 2 << 17));
    }
    commands.extend_from_slice(&original[start + draw * 5..start + (draw + 1) * 5]);
    commands.extend_from_slice(&original[start + 15..]);
    replace_commands(&mut fixture, commands);
    fixture
}

fn change_source(fixture: &Fixture, source: usize) -> Fixture {
    let mut changed = fixture.clone();
    let address = u64::from(
        commands(fixture)
            .into_iter()
            .filter(|&(w0, _)| w0 >> 24 == 0xfd)
            .nth(source)
            .unwrap()
            .1,
    );
    let span = changed.tasks[0]
        .spans
        .iter_mut()
        .find(|span| span.address <= address && address < span.address + span.bytes.len() as u64)
        .unwrap();
    span.bytes[(address - span.address) as usize] = 239;
    changed
}

#[derive(Clone, Copy, Debug)]
enum RecipeCase {
    LookupI8,
    LinearI8,
    LinearCi8,
}

impl RecipeCase {
    const ALL: [Self; 3] = [Self::LookupI8, Self::LinearI8, Self::LinearCi8];

    fn representation(self) -> Representation {
        match self {
            Self::LookupI8 => Representation::Lookup,
            Self::LinearI8 | Self::LinearCi8 => Representation::Linear,
        }
    }

    fn input_bytes(self) -> u64 {
        match self {
            Self::LookupI8 => 4096,
            Self::LinearI8 | Self::LinearCi8 => 4108,
        }
    }
}

fn lookup_texels(changed: bool) -> [u8; 24] {
    let last = if changed { 239 } else { 211 };
    [
        37, 91, 173, 241, 17, 43, 89, 211, 51, 119, 187, 255, 61, 97, 149, 223, 73, 109, 182, 219,
        29, 83, 157, last,
    ]
}

fn recipe_fixture(recipe: RecipeCase, changed: bool) -> Fixture {
    use n64_gbi::encode::*;

    const SOURCE: u32 = 0x0030_0000;
    const PALETTE: u32 = 0x0030_0100;
    let mut fixture = fixture("tmem-layouts");
    let mut commands = commands(&fixture)[..16].to_vec();
    let fmt = if matches!(recipe, RecipeCase::LinearCi8) {
        2
    } else {
        4
    };
    let ci = fmt == 2;
    commands.push(gsp_setothermode_h_f3d(14, 2, if ci { 2 << 14 } else { 0 }));
    if ci {
        fixture.tasks[0].spans.push(MemorySpan {
            address: PALETTE.into(),
            bytes: if changed {
                vec![0, 0x3f]
            } else {
                vec![0xf8, 1]
            },
        });
        commands.extend([
            gdp_set_texture_image(0, 2, 1, PALETTE),
            gdp_set_tile(0, 2, 0, 256, 7, 0, 0, 0, 0, 0, 0, 0),
            gdp_load_tlut(7, 0),
            gdp_pipe_sync(),
        ]);
    }
    let (source, width) = match recipe {
        RecipeCase::LookupI8 => {
            commands.extend([
                gdp_set_texture_image(fmt, 1, 8, SOURCE),
                gdp_set_tile(fmt, 1, 1, 0, 7, 0, 2, 0, 0, 2, 0, 0),
                (0xf400_0000, (7 << 24) | (28 << 12) | 8),
                gdp_set_tile(fmt, 1, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0),
            ]);
            (lookup_texels(changed).to_vec(), 8)
        }
        RecipeCase::LinearI8 | RecipeCase::LinearCi8 => {
            commands.extend([
                gdp_set_texture_image(fmt, 1, 3, SOURCE),
                gdp_set_tile(fmt, 1, 0, 0, 7, 0, 2, 0, 0, 2, 0, 0),
                gdp_load_block(7, 0, 0, 15, 0),
                gdp_set_tile(fmt, 1, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0),
            ]);
            let mut bytes = vec![
                if ci {
                    0
                } else if changed {
                    173
                } else {
                    61
                };
                9
            ];
            bytes.extend([211; 7]);
            (bytes, 3)
        }
    };
    fixture.tasks[0].spans.push(MemorySpan {
        address: SOURCE.into(),
        bytes: source,
    });
    let color = CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: 1,
    };
    let opaque = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 6,
    };
    commands.extend([
        gdp_set_tile_size(0, 0, 0, 8, 8),
        gdp_set_cycle_type_f3d(0),
        (0xb900_031d, 0x0f0a_4000),
        gdp_set_combine_lerp(color, opaque, color, opaque),
        (
            0xe400_0000 | ((16 + width) * 4) << 12 | (27 * 4),
            (16 * 4) << 12 | (24 * 4),
        ),
        (0xb400_0000, 0),
        (0xb300_0000, 0x0400_0400),
        gdp_pipe_sync(),
        (0xe900_0000, 0),
        gsp_enddl_f3d(),
    ]);
    replace_commands(&mut fixture, commands);
    fixture.validate().unwrap();
    fixture
}

fn recipe_color(recipe: RecipeCase, changed: bool) -> [u8; 4] {
    match recipe {
        RecipeCase::LookupI8 => unreachable!(),
        RecipeCase::LinearI8 => [if changed { 173 } else { 61 }; 4],
        RecipeCase::LinearCi8 if changed => [0, 0, 255, 255],
        RecipeCase::LinearCi8 => [255, 0, 0, 255],
    }
}

fn expected_recipe_decode(recipe: RecipeCase, changed: bool) -> Vec<u8> {
    if !matches!(recipe, RecipeCase::LookupI8) {
        return recipe_color(recipe, changed).repeat(9);
    }
    let last = if changed { 239 } else { 211 };
    let even = [
        37, 91, 173, 241, 17, 43, 89, 211, 61, 97, 149, 223, 51, 119, 187, 255, 73, 109, 182, 219,
        29, 83, 157, last,
    ];
    let odd = [
        17, 43, 89, 211, 37, 91, 173, 241, 51, 119, 187, 255, 61, 97, 149, 223, 29, 83, 157, last,
        73, 109, 182, 219,
    ];
    let mut pixels = vec![0; 4096 * 4 * 4];
    for (plane, values) in [even, even, odd, odd].into_iter().enumerate() {
        for (index, value) in values.into_iter().enumerate() {
            let offset = (plane * 4096 + index) * 4;
            pixels[offset..offset + 4].fill(value);
        }
    }
    pixels
}

fn expected_recipe_pixels(recipe: RecipeCase, changed: bool) -> Vec<u8> {
    let mut pixels = [0, 0, 0, 255].repeat(320 * 240);
    let colors: Vec<_> = if matches!(recipe, RecipeCase::LookupI8) {
        lookup_texels(changed).map(|value| [value; 4]).to_vec()
    } else {
        vec![recipe_color(recipe, changed); 9]
    };
    let width = colors.len() / 3;
    for (index, [r, g, b, _]) in colors.into_iter().enumerate() {
        let offset = ((24 + index / width) * 320 + 16 + index % width) * 4;
        pixels[offset..offset + 4].copy_from_slice(&[r, g, b, 255]);
    }
    pixels
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn hle_lookup_and_linear_recipe_census_matches_literals() {
    for recipe in RecipeCase::ALL {
        let mut identities = Vec::new();
        for changed in [false, true] {
            let requests = requests(&recipe_fixture(recipe, changed));
            assert_eq!(requests.len(), 1, "{recipe:?}");
            let request = &requests[0];
            assert_eq!(request.representation, recipe.representation());
            assert_eq!(request.role, "texture0");
            assert_eq!([request.tile.width, request.tile.height], [3, 3]);
            assert_eq!([request.tile.tmem_addr, request.tile.line], [0, 1]);
            assert_eq!(request.tile.siz, 1);
            assert_eq!(request.bank.bytes.len(), 4096);
            assert!(request.output.is_empty());
            assert!(request.rejection.is_none());
            if matches!(recipe, RecipeCase::LookupI8) {
                assert_eq!(request.extent, [4096, 4]);
                assert_eq!([request.tile.cms, request.tile.masks], [0, 3]);
                assert!(request.linear.is_none());
            } else {
                assert_eq!(request.extent, [3, 3]);
                let value = match recipe {
                    RecipeCase::LinearI8 if changed => 173,
                    RecipeCase::LinearI8 => 61,
                    RecipeCase::LinearCi8 => 0,
                    RecipeCase::LookupI8 => unreachable!(),
                };
                assert_eq!(request.linear.as_deref(), Some([value; 9].as_slice()));
                assert_eq!(&request.bank.bytes[..9], &[value; 9]);
                assert_eq!(&request.bank.bytes[9..16], &[211; 7]);
            }
            if matches!(recipe, RecipeCase::LinearCi8) {
                assert_eq!(request.tile.fmt, 2);
                assert_eq!(request.tlut, 2);
                let palette = if changed { [0, 0x3f] } else { [0xf8, 1] };
                assert_eq!(&request.bank.bytes[2048..2056], palette.repeat(4));
            } else {
                assert_eq!(request.tile.fmt, 4);
                assert_eq!(request.tlut, 0);
            }
            assert_eq!(
                request
                    .decode_prepared(&request.prepare().unwrap())
                    .unwrap(),
                expected_recipe_decode(recipe, changed),
                "{recipe:?}, changed {changed}"
            );
            identities.push(request.encoded_identity().unwrap());
        }
        assert_ne!(identities[0], identities[1], "{recipe:?}");
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixture_identity_census_matches_frozen_dispatch_counts() {
    for (name, unique, lengths) in [
        (
            "tmem-layouts",
            9,
            vec![
                344, 344, 632, 632, 128, 128, 200, 200, 128, 128, 200, 200, 344, 344, 152, 152,
                224, 224,
            ],
        ),
        (
            "tmem-roles-lifetime",
            4,
            vec![152, 152, 101, 101, 101, 86, 86, 86, 68],
        ),
        ("tmem-tlut-mutation", 3, vec![158; 5]),
    ] {
        let identities: Vec<_> = requests(&fixture(name))
            .iter()
            .map(|request| request.encoded_identity().unwrap())
            .collect();
        assert_eq!(
            identities
                .iter()
                .map(|(_, witness)| witness.len())
                .collect::<Vec<_>>(),
            lengths
        );
        assert_eq!(
            identities
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            unique
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn authored_changes_affect_only_the_independent_role() {
    for (role, source, expected_roles) in [
        ("texture1", 1, vec!["texture0", "texture1"]),
        ("lod1", 1, vec!["texture0", "lod0", "lod1"]),
        ("detail", 0, vec!["texture0", "lod0", "lod1", "detail"]),
    ] {
        let fixture = one_role(role);
        let before = requests(&fixture);
        let after = requests(&change_source(&fixture, source));
        assert_eq!(
            before
                .iter()
                .map(|request| request.role.as_str())
                .collect::<Vec<_>>(),
            expected_roles
        );
        let changed: Vec<_> = before
            .iter()
            .zip(&after)
            .filter_map(|(a, b)| {
                (a.encoded_identity().unwrap() != b.encoded_identity().unwrap())
                    .then_some(a.role.as_str())
            })
            .collect();
        assert_eq!(changed, [role]);
    }
    let fixture = fixture("tmem-roles-lifetime");
    let identities = |fixture: &Fixture| {
        requests(fixture)
            .iter()
            .map(|request| request.encoded_identity().unwrap())
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(identities(&fixture), identities(&reordered(&fixture)));
    assert_eq!(
        identities(&fixture),
        identities(&sampling_changes(&fixture))
    );
}

struct Harness {
    renderer: Renderer,
    recorder: Recorder,
    target: wgpu::Texture,
}

impl Harness {
    async fn new(fixture: &Fixture) -> Self {
        // Sequence recording requires Persist, as `CaptureSequence::begin` enforces; the stored
        // fixtures carry their own policy and clearing does not affect decode or residency counts.
        // A sequence is reset-rooted: Persist clear policy, and every frame retained from serial
        // one. The stored fixtures carry their own serial and policy, and neither affects decode
        // or residency counts.
        let mut frame = fixture.clone();
        frame.frame.config.clear_policy = fast3d::ClearPolicy::Persist;
        frame.frame.serial = 1;
        let sequence = Sequence {
            frames: vec![frame],
            warmup_frames: 0,
            presentations: vec![1],
        };
        let (device, queue, info) = match sequence.measurement_device().await {
            Ok(device) => device,
            Err(error) => {
                panic!("residency harness could not obtain a measurement device: {error:?}")
            }
        };
        #[cfg(target_arch = "wasm32")]
        assert_eq!(info.backend, wgpu::Backend::BrowserWebGpu);
        #[cfg(not(target_arch = "wasm32"))]
        let _ = info;
        let recorder = Recorder::new(Mode::Counters);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("residency-test-output"),
            size: wgpu::Extent3d {
                width: 320,
                height: 240,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let renderer = Renderer::with_device_profiled(
            device,
            queue,
            PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 320,
                height: 240,
            },
            fixture.frame.config,
            recorder.clone(),
        );
        recorder.drain();
        Self {
            renderer,
            recorder,
            target,
        }
    }

    fn process(&mut self, fixture: &Fixture, prefix: Option<u32>) {
        for task in &fixture.tasks {
            let hardware = ReplayHardware::new(task, fixture.frame.vi).unwrap();
            self.renderer.set_data_format(task.data_format);
            let mut diagnostics = Vec::new();
            let summary = if let Some(count) = prefix {
                self.renderer.process_dl_prefix(
                    &hardware,
                    task.entry,
                    task.microcode,
                    &mut diagnostics,
                    count,
                )
            } else {
                self.renderer
                    .process_dl(&hardware, task.entry, task.microcode, &mut diagnostics)
            };
            hardware.check().unwrap();
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            assert!(summary.renderable);
            assert_eq!(summary.errors, 0);
        }
    }

    async fn pixels(&mut self) -> Vec<u8> {
        self.renderer
            .present_last_to(&self.target.create_view(&Default::default()));
        let buffer = self
            .renderer
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("residency-test-readback"),
                size: 1280 * 240,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
        let mut encoder = self
            .renderer
            .device()
            .create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(1280),
                    rows_per_image: Some(240),
                },
            },
            self.target.size(),
        );
        self.renderer.queue().submit([encoder.finish()]);
        let (completion_sender, completion) = futures_channel::oneshot::channel();
        self.renderer.queue().on_submitted_work_done(move || {
            let _ = completion_sender.send(());
        });
        let (sender, receiver) = futures_channel::oneshot::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        #[cfg(not(target_arch = "wasm32"))]
        self.renderer
            .device()
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        receiver.await.unwrap().unwrap();
        completion.await.unwrap();
        let pixels = buffer.slice(..).get_mapped_range().to_vec();
        buffer.unmap();
        pixels
    }
}

fn count(snapshot: &Snapshot, counter: &str) -> u64 {
    snapshot.counters.get(counter).copied().unwrap_or(0)
}

#[cfg(not(target_arch = "wasm32"))]
fn export_snapshot(name: &str, snapshot: &Snapshot) {
    if let Some(directory) = std::env::var_os("FAST3D_RESIDENCY_OUTPUT") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            std::path::Path::new(&directory).join(format!("{name}.json")),
            serde_json::to_vec_pretty(snapshot).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn public_interpreter_counts_reload_copies_before_renderer_lookup() {
    let fixture = fixture("tmem-layouts");
    let mut interpreter = CpuInterpreter::default();
    interpreter.recorder = Recorder::new(Mode::Counters);
    for frame in 0..3 {
        for task in &fixture.tasks {
            let hardware = ReplayHardware::new(task, None).unwrap();
            let (_, diagnostics) = interpreter.process(
                hardware.rdram(),
                task.entry,
                task.microcode,
                task.data_format,
            );
            assert!(diagnostics.is_empty());
        }
        let snapshot = interpreter.recorder.drain();
        assert_eq!(
            count(&snapshot, "tmem.bank_cow_allocations"),
            9,
            "frame {frame}"
        );
        assert_eq!(
            count(&snapshot, "tmem.bank_cow_bytes"),
            36864,
            "frame {frame}"
        );
    }
}

fn assert_work(snapshot: &Snapshot, misses: u64, hits: u64, tasks: u64) {
    assert_work_with_input(snapshot, misses, hits, tasks, misses * 4096, 0);
}

fn assert_work_with_input(
    snapshot: &Snapshot,
    misses: u64,
    hits: u64,
    tasks: u64,
    input_bytes: u64,
    padding_bytes: u64,
) {
    for (name, expected) in [
        ("tmem.misses", misses),
        ("tmem.hits", hits),
        ("tmem.decode_dispatches", misses),
        ("tmem.decode_compute_passes", u64::from(misses != 0)),
        ("tmem.decode_input_upload_calls", misses),
        ("tmem.decode_input_upload_bytes", input_bytes),
        ("tmem.decode_upload_padding_bytes", padding_bytes),
        ("tmem.decode_uniform_upload_calls", misses),
        ("tmem.decode_uniform_upload_bytes", misses * 64),
        ("tmem.evictions", 0),
        ("tmem.bypasses", 0),
        ("tmem.cpu_decode_executions", 0),
        ("tmem.cpu_decode_output_bytes", 0),
        ("submissions", tasks),
    ] {
        assert_eq!(
            count(snapshot, name),
            expected,
            "{name}: {:?}",
            snapshot.counters
        );
    }
    let mut images_created = 0;
    for role in ["texture0", "texture1", "detail", "lod0", "lod1", "lod2"] {
        images_created += count(snapshot, &format!("texture.{role}.creations"));
        assert_eq!(
            count(snapshot, &format!("texture.{role}.upload_bytes")),
            0,
            "{role}"
        );
    }
    assert_eq!(images_created, misses);
    assert!(snapshot
        .decodes
        .values()
        .all(|decode| decode.executions == 0 && decode.output_bytes == 0));
}

async fn reuse_across_frames() {
    let fixture = fixture("tmem-roles-lifetime");
    let mut harness = Harness::new(&fixture).await;
    let scope = harness
        .renderer
        .device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    for (index, frame) in [
        fixture.clone(),
        fixture.clone(),
        reordered(&fixture),
        sampling_changes(&fixture),
    ]
    .iter()
    .enumerate()
    {
        harness.renderer.begin_frame();
        harness.process(frame, None);
        let profile = harness.recorder.drain();
        assert_work(
            &profile,
            if index == 0 { 4 } else { 0 },
            if index == 0 { 2 } else { 6 },
            1,
        );
        assert_eq!(
            count(&profile, "texture.lod0.creations"),
            u64::from(index == 0)
        );
        assert_eq!(
            count(&profile, "texture.lod1.creations"),
            if index == 0 { 3 } else { 0 }
        );
        semantics::assert_pixels("tmem-roles-lifetime", &harness.pixels().await);
        #[cfg(not(target_arch = "wasm32"))]
        export_snapshot(
            [
                "roles-cold",
                "roles-warm",
                "roles-reordered",
                "roles-sampling-only",
            ][index],
            &profile,
        );
        harness.recorder.drain();
    }
    assert!(scope.pop().await.is_none());
}

async fn pending_work_and_guest_lifetime() {
    let mut harness = Harness::new(&fixture("tmem-roles-lifetime")).await;
    let scope = harness
        .renderer
        .device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    harness.renderer.begin_frame();
    {
        let guest = fixture("tmem-roles-lifetime");
        harness.process(&guest, None);
        harness.process(&guest, None);
    }
    assert_work(&harness.recorder.drain(), 4, 8, 2);
    semantics::assert_pixels("tmem-roles-lifetime", &harness.pixels().await);
    harness.recorder.drain();
    semantics::assert_pixels("tmem-roles-lifetime", &harness.pixels().await);
    assert_work(&harness.recorder.drain(), 0, 0, 1);
    let fixture = fixture("tmem-roles-lifetime");
    let start = draw_start(&commands(&fixture));
    for count in [start as u32 + 5, start as u32 + 10, u32::MAX] {
        harness.process(&fixture, Some(count));
        assert_work(
            &harness.recorder.drain(),
            0,
            if count == u32::MAX {
                6
            } else {
                u64::from(count - start as u32) / 5 * 2
            },
            1,
        );
    }
    semantics::assert_pixels("tmem-roles-lifetime", &harness.pixels().await);
    assert!(scope.pop().await.is_none());
}

async fn reconfigure_and_new_device() {
    let fixture = fixture("tmem-roles-lifetime");
    let mut harness = Harness::new(&fixture).await;
    harness.renderer.begin_frame();
    harness.process(&fixture, None);
    assert_work(&harness.recorder.drain(), 4, 2, 1);
    harness.renderer.reconfigure(fixture.frame.config);
    harness.recorder.drain();
    harness.renderer.begin_frame();
    harness.process(&fixture, None);
    assert_work(&harness.recorder.drain(), 4, 2, 1);
    semantics::assert_pixels("tmem-roles-lifetime", &harness.pixels().await);
    drop(harness);
    let mut new_epoch = Harness::new(&fixture).await;
    new_epoch.renderer.begin_frame();
    new_epoch.process(&fixture, None);
    assert_work(&new_epoch.recorder.drain(), 4, 2, 1);
    semantics::assert_pixels("tmem-roles-lifetime", &new_epoch.pixels().await);
}

async fn independent_role_changes() {
    for (role, source, resident, before_pixel, after_pixel, draw) in [
        ("texture1", 1, 2, 91, 239, 0),
        ("lod1", 1, 2, 37, 37, 0),
        ("detail", 0, 3, 37, 239, 1),
    ] {
        let fixture = one_role(role);
        let mut harness = Harness::new(&fixture).await;
        for (index, frame) in [
            fixture.clone(),
            change_source(&fixture, source),
            fixture.clone(),
        ]
        .iter()
        .enumerate()
        {
            harness.renderer.begin_frame();
            harness.process(frame, None);
            let profile = harness.recorder.drain();
            let misses = if index == 0 {
                resident
            } else {
                u64::from(index == 1)
            };
            assert_work(&profile, misses, resident - misses, 1);
            if index == 1 {
                assert_eq!(count(&profile, &format!("texture.{role}.creations")), 1);
                assert_eq!(count(&profile, "tmem.staging_reuses"), 1);
            }
            let pixels = harness.pixels().await;
            let value = if index == 1 {
                after_pixel
            } else {
                before_pixel
            };
            let mut expected = [0, 0, 0, 255].repeat(320 * 240);
            for y in 112..128 {
                for x in 136 + draw * 16..152 + draw * 16 {
                    expected[(y * 320 + x) * 4..(y * 320 + x + 1) * 4]
                        .copy_from_slice(&[value, value, value, 255]);
                }
            }
            for (pixel, (actual, expected)) in pixels
                .as_chunks::<4>()
                .0
                .iter()
                .zip(expected.as_chunks::<4>().0)
                .enumerate()
            {
                assert_eq!(
                    actual,
                    expected,
                    "{role}, frame {index}, pixel ({},{})",
                    pixel % 320,
                    pixel / 320
                );
            }
            harness.recorder.drain();
        }
    }
}

async fn realistic_reloads() {
    let fixture = fixture("tmem-layouts");
    let mut harness = Harness::new(&fixture).await;
    let mut aggregate = Snapshot::default();
    for frame in 0..3 {
        harness.renderer.begin_frame();
        harness.process(&fixture, None);
        let profile = harness.recorder.drain();
        assert_work(
            &profile,
            if frame == 0 { 9 } else { 0 },
            if frame == 0 { 9 } else { 18 },
            1,
        );
        for (name, expected) in [
            ("tmem.hashes_computed", 18),
            ("tmem.bytes_hashed", 4704),
            ("tmem.witness_comparisons", if frame == 0 { 9 } else { 18 }),
            ("tmem.encoded_comparisons", 9),
            (
                "tmem.bytes_compared",
                if frame == 0 { 39216 } else { 41568 },
            ),
            ("tmem.bank_cow_allocations", 9),
            ("tmem.bank_cow_bytes", 36864),
            ("tmem.linear_reconstruction_bytes", 0),
        ] {
            assert_eq!(count(&profile, name), expected, "{name}, frame {frame}");
        }
        assert_eq!(profile.gauges["tmem.resident_entries"], 9);
        assert_eq!(profile.gauges["tmem.resident_gpu_bytes"], 1728);
        assert_eq!(profile.gauges["tmem.resident_cpu_bytes"], 2352);
        semantics::assert_pixels("tmem-layouts", &harness.pixels().await);
        #[cfg(not(target_arch = "wasm32"))]
        export_snapshot(
            ["reloads-cold", "reloads-warm-1", "reloads-warm-2"][frame],
            &profile,
        );
        for (name, value) in profile.counters {
            *aggregate.counters.entry(name).or_default() += value;
        }
        for (name, value) in profile.gauges {
            if name.ends_with(".peak") {
                let peak = aggregate.gauges.entry(name).or_default();
                *peak = (*peak).max(value);
            } else {
                aggregate.gauges.insert(name, value);
            }
        }
        for (name, counts) in profile.decodes {
            let total = aggregate.decodes.entry(name).or_default();
            total.requests += counts.requests;
            total.executions += counts.executions;
            total.rejected += counts.rejected;
            total.output_bytes += counts.output_bytes;
        }
        harness.recorder.drain();
    }
    let contract: serde_json::Value =
        serde_json::from_str(include_str!("../../tools/tmem/dispatch-expectations.json")).unwrap();
    for (name, expected) in contract["t4"]["display_list_reloads_three_frames"]
        .as_object()
        .unwrap()
    {
        let actual = match name.as_str() {
            "moved_rdp_fixture_bank_cow_allocations" | "moved_rdp_fixture_bank_cow_bytes" => {
                continue
            }
            "public_facade_bank_cow_allocations" => count(&aggregate, "tmem.bank_cow_allocations"),
            "public_facade_bank_cow_bytes" => count(&aggregate, "tmem.bank_cow_bytes"),
            "distinct_images" => aggregate.gauges["tmem.resident_entries"],
            "resident_gpu_bytes" | "resident_cpu_bytes" => {
                aggregate.gauges[&format!("tmem.{name}")]
            }
            "encoded_bytes_compared" => count(&aggregate, "tmem.encoded_comparisons") * 4096,
            "witness_bytes_compared" => {
                count(&aggregate, "tmem.bytes_compared")
                    - count(&aggregate, "tmem.encoded_comparisons") * 4096
            }
            _ => count(&aggregate, &format!("tmem.{name}")),
        };
        assert_eq!(
            actual,
            expected.as_u64().unwrap(),
            "reload aggregate: {name}"
        );
    }
    #[cfg(not(target_arch = "wasm32"))]
    export_snapshot("reloads-three-frames", &aggregate);
}

async fn palette_reloads() {
    let fixture = fixture("tmem-tlut-mutation");
    let mut harness = Harness::new(&fixture).await;
    for frame in 0..2 {
        harness.renderer.begin_frame();
        harness.process(&fixture, None);
        assert_work(
            &harness.recorder.drain(),
            if frame == 0 { 3 } else { 0 },
            if frame == 0 { 2 } else { 5 },
            1,
        );
        semantics::assert_pixels("tmem-tlut-mutation", &harness.pixels().await);
        harness.recorder.drain();
    }
}

async fn lookup_and_linear_residency() {
    for recipe in RecipeCase::ALL {
        let mut harness = Harness::new(&recipe_fixture(recipe, false)).await;
        let scope = harness
            .renderer
            .device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        for (frame, changed) in [false, false, true, false].into_iter().enumerate() {
            harness.renderer.begin_frame();
            harness.process(&recipe_fixture(recipe, changed), None);
            let profile = harness.recorder.drain();
            let misses = u64::from(frame == 0 || changed);
            let padding = if matches!(recipe, RecipeCase::LookupI8) {
                0
            } else {
                3
            };
            assert_work_with_input(
                &profile,
                misses,
                1 - misses,
                1,
                misses * recipe.input_bytes(),
                misses * padding,
            );
            assert_eq!(
                count(&profile, "tmem.staging_reuses"),
                u64::from(changed),
                "{recipe:?}, frame {frame}"
            );
            let entries = if frame < 2 { 1 } else { 2 };
            let image_bytes = if matches!(recipe, RecipeCase::LookupI8) {
                65536
            } else {
                36
            };
            assert_eq!(profile.gauges["tmem.resident_entries"], entries);
            assert_eq!(
                profile.gauges["tmem.resident_gpu_bytes"],
                entries * image_bytes
            );
            let pixels = harness.pixels().await;
            let expected = expected_recipe_pixels(recipe, changed);
            assert_eq!(pixels.len(), expected.len());
            for (index, (actual, expected)) in pixels
                .as_chunks::<4>()
                .0
                .iter()
                .zip(expected.as_chunks::<4>().0.iter())
                .enumerate()
            {
                assert_eq!(
                    actual,
                    expected,
                    "{recipe:?}, frame {frame}, pixel ({},{})",
                    index % 320,
                    index / 320
                );
            }
            harness.recorder.drain();
        }
        assert!(scope.pop().await.is_none());
    }
}

macro_rules! gpu_test {
    ($name:ident, $body:ident) => {
        #[cfg(not(target_arch = "wasm32"))]
        #[test]
        fn $name() {
            pollster::block_on($body());
        }
        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen_test::wasm_bindgen_test]
        async fn $name() {
            $body().await;
        }
    };
}

gpu_test!(
    cold_warm_reordered_and_sampling_only_roles,
    reuse_across_frames
);
gpu_test!(
    pending_tasks_prefixes_and_retained_scanout,
    pending_work_and_guest_lifetime
);
gpu_test!(
    reconfigure_and_device_epoch_are_cold,
    reconfigure_and_new_device
);
gpu_test!(
    only_changed_independent_roles_decode,
    independent_role_changes
);
gpu_test!(three_frame_display_list_reload_counts, realistic_reloads);
gpu_test!(only_relevant_palette_reloads_decode, palette_reloads);
gpu_test!(
    lookup_and_linear_cold_warm_changed_input_counts_and_pixels,
    lookup_and_linear_residency
);
