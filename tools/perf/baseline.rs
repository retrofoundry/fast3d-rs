mod shared;
use fast3d::{
    capture::{ReplayHardware, ReplayOutput, Sequence},
    ClearPolicy, DataFormat, Hardware, Microcode, PresentTarget, Rdram, RdramImage, Renderer,
    RendererConfig, ViRegisters,
};
use serde_json::{json, Value};
pub use shared::*;

pub fn config() -> RendererConfig {
    RendererConfig {
        clear_policy: ClearPolicy::PerFrame,
        resolution_multiplier: 1,
        sample_count: 1,
        present_mode: wgpu::PresentMode::Fifo,
        format: Some(wgpu::TextureFormat::Rgba8Unorm),
        power_preference: wgpu::PowerPreference::None,
    }
}

async fn renderer(
    config: RendererConfig,
    width: u32,
    height: u32,
    dual: bool,
) -> Result<Renderer, String> {
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: config.power_preference,
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_features: if dual {
                wgpu::Features::DUAL_SOURCE_BLENDING
            } else {
                wgpu::Features::empty()
            },
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(Renderer::with_device(
        device,
        queue,
        PresentTarget::Headless {
            format: config.format.unwrap_or(wgpu::TextureFormat::Rgba8Unorm),
            width,
            height,
        },
        config,
    ))
}

fn output(
    renderer: &Renderer,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("baseline-correctness-output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
struct ViHardware(Option<ViRegisters>);
impl Hardware for ViHardware {
    fn rdram(&self) -> impl Rdram + '_ {
        RdramImage::new(&[])
    }
    fn vi(&self) -> Option<ViRegisters> {
        self.0
    }
}

pub async fn sequence(sequence: &Sequence, emit: impl FnMut(Value)) -> Result<(), String> {
    sequence_with_options(sequence, Options::correctness(), emit).await
}

pub async fn sequence_with_options(
    sequence: &Sequence,
    options: Options,
    mut emit: impl FnMut(Value),
) -> Result<(), String> {
    sequence.validate().map_err(|e| e.to_string())?;
    let first = &sequence.frames[0].frame;
    if first.dither_seed != 0 {
        return Err(format!(
            "parent adapter requires dither_seed=0; frame {} declares dither_seed={}",
            first.serial, first.dither_seed
        ));
    }
    let mut renderer = renderer(
        RendererConfig {
            clear_policy: ClearPolicy::Persist,
            ..first.config
        },
        first.width,
        first.height,
        first.dual_source_blending,
    )
    .await?;
    let texture = output(
        &renderer,
        first.width,
        first.height,
        first
            .config
            .format
            .unwrap_or(wgpu::TextureFormat::Rgba8Unorm),
    );
    let view = texture.create_view(&Default::default());
    let mut pending = std::collections::VecDeque::new();
    for fixture in &sequence.frames {
        let start = now_ms();
        if pending.len() >= options.frames_in_flight {
            wait(renderer.device(), pending.pop_front().unwrap()).await?;
        }
        let wait_ms = now_ms() - start;
        let mut timing = Timings::default();
        let mut adapter_ms = 0.0;
        let limits = renderer.device().limits();
        let row = (fixture.frame.width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        if renderer
            .device()
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING)
            != fixture.frame.dual_source_blending
            || fixture.frame.width > limits.max_texture_dimension_2d
            || fixture.frame.height > limits.max_texture_dimension_2d
            || u64::from(row) * u64::from(fixture.frame.height) > limits.max_buffer_size
        {
            return Err("capture device declaration or output limits mismatch".into());
        }
        timing.measure("begin_frame", options.coarse, || renderer.begin_frame());
        let mut summaries = Vec::new();
        let mut diagnostics = Vec::new();
        for task in &fixture.tasks {
            let start = now_ms();
            let hardware =
                ReplayHardware::new(task, fixture.frame.vi).map_err(|e| e.to_string())?;
            renderer.set_data_format(task.data_format);
            let mut diags = Vec::new();
            adapter_ms += now_ms() - start;
            summaries.push(timing.measure("process_dl", options.coarse, || {
                renderer.process_dl(&hardware, task.entry, task.microcode, &mut diags)
            }));
            let start = now_ms();
            hardware.check().map_err(|e| e.to_string())?;
            diagnostics.push(diags);
            adapter_ms += now_ms() - start;
        }
        timing.measure("presentation", options.coarse, || {
            renderer.present_to(&ViHardware(fixture.frame.vi), &view)
        });
        pending.push_back(completion(&renderer));
        let start = now_ms();
        let mut pixels = if options.readback {
            read_pixels(&renderer, &texture).await?
        } else {
            Vec::new()
        };
        let readback_ms = if options.readback {
            now_ms() - start
        } else {
            0.0
        };
        if matches!(
            texture.format(),
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        }
        let frame = ReplayOutput {
            width: first.width,
            height: first.height,
            rgba8: pixels,
            summaries,
            diagnostics,
            adapter_info: None,
            commands: Vec::new(),
        };
        let mut row = output_record(fixture.frame.serial, &frame);
        row["observed"] = json!(fixture.frame.serial > u64::from(sequence.warmup_frames));
        row["profile"] = json!({"timings":timing.0});
        row["timing_source"] = json!("public-api-boundaries");
        row["wait_ms"] = json!(wait_ms);
        row["adapter_ms"] = json!(adapter_ms);
        row["readback_ms"] = json!(readback_ms);
        emit(row);
    }
    for item in pending {
        wait(renderer.device(), item).await?;
    }
    Ok(())
}

pub async fn source(source: &str, emit: impl FnMut(Value)) -> Result<(), String> {
    source_with_options(source, Options::correctness(), emit).await
}

pub async fn source_with_options(
    source: &str,
    options: Options,
    mut emit: impl FnMut(Value),
) -> Result<(), String> {
    source_metadata(source)?;
    let rgba = synthetic_texture();
    let mut renderer = renderer(config(), 800, 600, false).await?;
    let texture = output(&renderer, 800, 600, wgpu::TextureFormat::Rgba8Unorm);
    let view = texture.create_view(&Default::default());
    let mut pending = std::collections::VecDeque::new();
    for index in 0..720 {
        let start = now_ms();
        if pending.len() >= options.frames_in_flight {
            wait(renderer.device(), pending.pop_front().unwrap()).await?;
        }
        let wait_ms = now_ms() - start;
        let start = now_ms();
        let image = assemble(source, index, &rgba)?;
        let assembly_ms = now_ms() - start;
        let mut timing = Timings::default();
        timing.measure("begin_frame", options.coarse, || renderer.begin_frame());
        renderer.set_data_format(DataFormat::Fixed);
        let mut diags = Vec::new();
        let summary = timing.measure("process_dl", options.coarse, || {
            renderer.process_dl(
                &ImageHardware(&image.rdram),
                image.entry_addr.into(),
                Microcode::F3dex2,
                &mut diags,
            )
        });
        if !diags.is_empty() {
            return Err(format!("source input rejected: {diags:?}"));
        }
        timing.measure("presentation", options.coarse, || {
            renderer.present_last_to(&view)
        });
        pending.push_back(completion(&renderer));
        let start = now_ms();
        let pixels = if options.readback {
            read_pixels(&renderer, &texture).await?
        } else {
            Vec::new()
        };
        let readback_ms = if options.readback {
            now_ms() - start
        } else {
            0.0
        };
        emit(
            json!({"serial":index,"observed":index>=120,"profile":{"timings":timing.0},"timing_source":"public-api-boundaries","assembly_ms":assembly_ms,"wait_ms":wait_ms,"readback_ms":readback_ms,"rgba8_sha256":options.readback.then(||sha(&pixels)),"summary":format!("{summary:?}"),"diagnostics":format!("{diags:?}")}),
        );
    }
    for item in pending {
        wait(renderer.device(), item).await?;
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use serde_json::json;
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    pub async fn source(
        source: String,
        _mode: String,
        readback: bool,
        _one_frame: bool,
    ) -> Result<String, JsValue> {
        if !readback {
            return Err(JsValue::from_str(
                "parent driver is for correctness readback",
            ));
        }
        let mut frames = Vec::new();
        super::source(&source, |row| frames.push(row))
            .await
            .map_err(|e| JsValue::from_str(&e))?;
        Ok(json!({"frames":frames}).to_string())
    }
    #[wasm_bindgen]
    pub async fn sequence(
        bytes: Vec<u8>,
        _mode: String,
        readback: bool,
        _one_frame: bool,
    ) -> Result<String, JsValue> {
        if !readback {
            return Err(JsValue::from_str(
                "parent driver is for correctness readback",
            ));
        }
        let sequence = fast3d::capture::Sequence::from_bytes(&bytes)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let mut frames = Vec::new();
        super::sequence(&sequence, |row| frames.push(row))
            .await
            .map_err(|e| JsValue::from_str(&e))?;
        Ok(json!({"frames":frames}).to_string())
    }
    #[wasm_bindgen]
    pub fn preflight(_cases: String) -> Result<String, JsValue> {
        Err(JsValue::from_str(
            "preflight requires the B1 profiling library",
        ))
    }
}

#[derive(Clone, Copy)]
pub struct Options {
    pub coarse: bool,
    pub readback: bool,
    pub frames_in_flight: usize,
}
impl Options {
    fn correctness() -> Self {
        Self {
            coarse: false,
            readback: true,
            frames_in_flight: 2,
        }
    }
}
fn now_ms() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_secs_f64()
            * 1000.0
    }
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().unwrap().performance().unwrap().now()
    }
}
#[derive(Default)]
struct Timings(std::collections::BTreeMap<&'static str, Value>);
impl Timings {
    fn measure<T>(&mut self, name: &'static str, enabled: bool, call: impl FnOnce() -> T) -> T {
        if !enabled {
            return call();
        }
        let start = now_ms();
        let value = call();
        let elapsed = now_ms() - start;
        let row = self
            .0
            .entry(name)
            .or_insert(json!({"calls":0,"inclusive_ms":0.0,"exclusive_ms":0.0}));
        row["calls"] = json!(row["calls"].as_u64().unwrap() + 1);
        for field in ["inclusive_ms", "exclusive_ms"] {
            row[field] = json!(row[field].as_f64().unwrap() + elapsed);
        }
        value
    }
}
fn completion(renderer: &Renderer) -> futures_channel::oneshot::Receiver<()> {
    let (send, receive) = futures_channel::oneshot::channel();
    renderer.queue().on_submitted_work_done(move || {
        let _ = send.send(());
    });
    receive
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn nonzero_seed_is_rejected_before_gpu_work() {
        let mut sequence = Sequence::from_bytes(&sequence_input("authored").unwrap()).unwrap();
        for fixture in &mut sequence.frames {
            fixture.frame.dither_seed = 17;
        }
        for coarse in [false, true] {
            let error = pollster::block_on(sequence_with_options(
                &sequence,
                Options {
                    coarse,
                    readback: false,
                    frames_in_flight: 2,
                },
                |_| panic!("rejected input must not emit a frame"),
            ))
            .unwrap_err();
            assert!(error.contains("frame 1 declares dither_seed=17"), "{error}");
            assert!(error.contains("requires dither_seed=0"), "{error}");
        }
    }
}
