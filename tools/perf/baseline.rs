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

pub async fn sequence(sequence: &Sequence, mut emit: impl FnMut(Value)) -> Result<(), String> {
    sequence.validate().map_err(|e| e.to_string())?;
    let first = &sequence.frames[0].frame;
    if first.dither_seed != 0 {
        return Err("parent adapter only supports the declared seed-zero inputs".into());
    }
    let mut renderer = renderer(
        first.config,
        first.width,
        first.height,
        first.dual_source_blending,
    )
    .await?;
    for fixture in &sequence.frames {
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
        renderer.begin_frame();
        let mut summaries = Vec::new();
        let mut diagnostics = Vec::new();
        for task in &fixture.tasks {
            let hardware =
                ReplayHardware::new(task, fixture.frame.vi).map_err(|e| e.to_string())?;
            renderer.set_data_format(task.data_format);
            let mut diags = Vec::new();
            summaries.push(renderer.process_dl(&hardware, task.entry, task.microcode, &mut diags));
            hardware.check().map_err(|e| e.to_string())?;
            diagnostics.push(diags);
        }
        let texture = output(
            &renderer,
            first.width,
            first.height,
            first
                .config
                .format
                .unwrap_or(wgpu::TextureFormat::Rgba8Unorm),
        );
        renderer.present_to(
            &ViHardware(fixture.frame.vi),
            &texture.create_view(&Default::default()),
        );
        let mut pixels = read_pixels(&renderer, &texture).await?;
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
        emit(output_record(fixture.frame.serial, &frame));
    }
    Ok(())
}

pub async fn source(source: &str, mut emit: impl FnMut(Value)) -> Result<(), String> {
    source_metadata(source)?;
    let rgba = synthetic_texture();
    let mut renderer = renderer(config(), 800, 600, false).await?;
    for index in 0..720 {
        let image = assemble(source, index, &rgba)?;
        renderer.begin_frame();
        renderer.set_data_format(DataFormat::Fixed);
        let mut diags = Vec::new();
        let summary = renderer.process_dl(
            &ImageHardware(&image.rdram),
            image.entry_addr.into(),
            Microcode::F3dex2,
            &mut diags,
        );
        if !diags.is_empty() {
            return Err(format!("source input rejected: {diags:?}"));
        }
        let texture = output(&renderer, 800, 600, wgpu::TextureFormat::Rgba8Unorm);
        renderer.present_last_to(&texture.create_view(&Default::default()));
        let pixels = read_pixels(&renderer, &texture).await?;
        emit(
            json!({"serial":index,"rgba8_sha256":sha(&pixels),"summary":format!("{summary:?}"),"diagnostics":format!("{diags:?}")}),
        );
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
