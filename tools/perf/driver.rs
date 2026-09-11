pub mod cache;
use fast3d::{
    capture::MeasuredFrame,
    profiling::{now_ms, CpuInterpreter, Mode, Recorder, Snapshot},
    DataFormat, Microcode, RdramImage,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

mod shared;
pub use shared::*;

pub fn frame_record(frame: &MeasuredFrame) -> Value {
    let mut record = output_record(frame.serial, &frame.output);
    record["observed"] = json!(frame.observed);
    record["profile"] = json!(frame.profile);
    record["adapter_ms"] = json!(frame.adapter_ms);
    record["wait_ms"] = json!(frame.wait_ms);
    record["readback_ms"] = json!(frame.readback_ms);
    record["readback_profile"] = json!(frame.readback_profile);
    record
}

pub struct TraceCollector {
    pub simulator: cache::Simulator,
    pub cases: BTreeMap<String, Vec<fast3d::profiling::Request>>,
    seen: std::collections::HashSet<Vec<u8>>,
}
impl TraceCollector {
    pub fn new(budget: usize) -> Self {
        Self {
            simulator: cache::Simulator::new(budget, 2),
            cases: BTreeMap::new(),
            seen: Default::default(),
        }
    }
    pub fn frame(&mut self, serial: u64, observed: bool, snapshot: &Snapshot) -> Value {
        for request in &snapshot.requests {
            if request.rejection.is_some() {
                continue;
            }
            let cases = self.cases.entry(request.class()).or_default();
            let prepared = request.executor().prepare().expect("admitted trace input");
            if self.seen.insert(request.canonical(&prepared)) {
                cases.push(request.clone());
            }
        }
        json!(cache::trace_frame(
            &mut self.simulator,
            serial,
            observed,
            &snapshot.requests
        ))
    }
}

pub fn source_cpu(
    source: &str,
    mut emit: impl FnMut(Value),
    mut trace: impl FnMut(u64, bool, &Snapshot),
) -> Result<(), String> {
    let texture = synthetic_texture();
    let mut interpreter = CpuInterpreter::default();
    interpreter.recorder = Recorder::new(Mode::Trace);
    for index in 0..720 {
        let start = now_ms();
        let image = assemble(source, index, &texture)?;
        let assembly_ms = now_ms() - start;
        let (summary, diags) = interpreter.process(
            RdramImage::new(&image.rdram),
            image.entry_addr.into(),
            Microcode::F3dex2,
            DataFormat::Fixed,
        );
        if !diags.is_empty() {
            return Err(format!("source input blocked at frame {index}: {diags:?}"));
        }
        let profile = interpreter.recorder.drain();
        trace(index.into(), index >= 120, &profile);
        emit(
            json!({"serial":index,"observed":index>=120,"execution":"cpu-diagnostic-no-render","assembly_ms":assembly_ms,"assembled_sha256":sha(&image.rdram),"summary":format!("{summary:?}"),"profile":profile}),
        );
    }
    Ok(())
}

pub async fn source_gpu(
    source: &str,
    mode: Mode,
    readback: bool,
    frames_in_flight: usize,
    mut emit: impl FnMut(Value),
) -> Result<Value, String> {
    use fast3d::{ClearPolicy, PresentTarget, Renderer, RendererConfig};
    if !(1..=2).contains(&frames_in_flight) {
        return Err("frames_in_flight must be 1 or 2".into());
    }
    let texture = synthetic_texture();
    let input = source_metadata(source)?;
    let admission = assemble(source, 0, &texture)?;
    let mut interpreter = CpuInterpreter::default();
    let (_, diags) = interpreter.process(
        RdramImage::new(&admission.rdram),
        admission.entry_addr.into(),
        Microcode::F3dex2,
        DataFormat::Fixed,
    );
    if !diags.is_empty() {
        return Err(format!("assembler bridge rejected: {diags:?}"));
    }
    let start = now_ms();
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|e| e.to_string())?;
    let info = adapter.get_info();
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
    let device_ms = now_ms() - start;
    let recorder = Recorder::new(mode);
    let start = now_ms();
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("toy-output"),
        size: wgpu::Extent3d {
            width: 800,
            height: 600,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    recorder.texture("output", 800 * 600 * 4, 0);
    let view = target.create_view(&Default::default());
    let mut renderer = Renderer::with_device_profiled(
        device,
        queue,
        PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 800,
            height: 600,
        },
        RendererConfig {
            clear_policy: ClearPolicy::PerFrame,
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            power_preference: wgpu::PowerPreference::None,
        },
        recorder.clone(),
    );
    let renderer_ms = now_ms() - start;
    let setup = recorder.drain();
    let mut pending = std::collections::VecDeque::new();
    for index in 0..720 {
        let start = now_ms();
        if pending.len() >= frames_in_flight {
            wait(renderer.device(), pending.pop_front().unwrap()).await?;
        }
        let wait_ms = now_ms() - start;
        let start = now_ms();
        let image = assemble(source, index, &texture)?;
        let assembly_ms = now_ms() - start;
        let hardware = ImageHardware(&image.rdram);
        renderer.begin_frame();
        let mut diagnostics = Vec::new();
        let summary = renderer.process_dl(
            &hardware,
            image.entry_addr.into(),
            Microcode::F3dex2,
            &mut diagnostics,
        );
        if !diagnostics.is_empty() {
            return Err(format!("source rejected at {index}: {diagnostics:?}"));
        }
        renderer.present_last_to(&view);
        let (send, receive) = futures_channel::oneshot::channel();
        renderer.queue().on_submitted_work_done(move || {
            let _ = send.send(());
        });
        pending.push_back(receive);
        let profile = recorder.drain();
        let start = now_ms();
        let pixels = if readback {
            read_pixels(&renderer, &target).await?
        } else {
            Vec::new()
        };
        let readback_ms = if readback { now_ms() - start } else { 0.0 };
        emit(
            json!({"serial":index,"observed":index>=120,"assembly_ms":assembly_ms,"wait_ms":wait_ms,"readback_ms":readback_ms,"rgba8_sha256":readback.then(||sha(&pixels)),"summary":format!("{summary:?}"),"diagnostics":format!("{diagnostics:?}"),"profile":profile}),
        );
    }
    let start = now_ms();
    for completion in pending {
        wait(renderer.device(), completion).await?;
    }
    Ok(
        json!({"input":input,"adapter":format!("{info:?}"),"features":format!("{:?}",renderer.device().features()),"limits":format!("{:?}",renderer.device().limits()),"device_ms":device_ms,"renderer_ms":renderer_ms,"setup":setup,"final_wait_ms":now_ms()-start,"frames_in_flight":frames_in_flight,"mode":mode,"readback":readback,"clock":fast3d::profiling::clock_probe()}),
    )
}

async fn wait(
    device: &wgpu::Device,
    completion: futures_channel::oneshot::Receiver<()>,
) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| e.to_string())?;
    #[cfg(target_arch = "wasm32")]
    let _ = device;
    completion.await.map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
mod browser;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_driver_matches_assembler_bytes() {
        let scene =
            std::env::var("B1_SCENE").expect("set B1_SCENE to the unmodified chrome-icosphere.n64");
        let source = std::fs::read_to_string(scene).unwrap();
        let texture = synthetic_texture();
        let meta = source_metadata(&source).unwrap();
        assert_eq!(meta["rgba8_sha256"], sha(&texture));
        assert_eq!(texture.len(), 4096);
        assert_eq!(&texture[..8], &[0, 0, 0, 255, 8, 0, 8, 255]);
        for index in [0, 119, 120, 719] {
            let bridge = assemble(&source, index, &texture).unwrap();
            let direct = n64_toys_asm::assemble_at_with_textures(
                &source,
                index as f32 / 60.0,
                &[n64_toys_asm::TextureInput {
                    name: "env",
                    rgba8: &texture,
                    width: 32,
                    height: 32,
                }],
                n64_toys_asm::Microcode::F3dex2,
            )
            .unwrap();
            assert_eq!(bridge.rdram, direct.rdram);
            assert_eq!(bridge.entry_addr, direct.entry_addr);
            assert_eq!(
                meta["rgba16_sha256"],
                sha(&bridge.rdram[bridge.tex_addr as usize..bridge.tex_addr as usize + 2048])
            );
        }
    }
}
