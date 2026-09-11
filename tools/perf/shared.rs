use fast3d::{capture::ReplayOutput, Hardware, Rdram, RdramImage};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn synthetic_texture() -> Vec<u8> {
    (0u8..32)
        .flat_map(|y| (0u8..32).flat_map(move |x| [8 * x, 8 * y, 8 * (x ^ y), 255]))
        .collect()
}

pub fn assemble(source: &str, index: u32, texture: &[u8]) -> Result<n64_toys_asm::Image, String> {
    n64_toys_asm::assemble_at_with_textures(
        source,
        index as f32 / 60.0,
        &[n64_toys_asm::TextureInput {
            name: "env",
            rgba8: texture,
            width: 32,
            height: 32,
        }],
        n64_toys_asm::Microcode::F3dex2,
    )
    .map_err(|diags| format!("assembler rejected source: {diags:?}"))
}

pub fn source_metadata(source: &str) -> Result<Value, String> {
    let rgba = synthetic_texture();
    let image = assemble(source, 0, &rgba)?;
    let encoded = &image.rdram[image.tex_addr as usize..image.tex_addr as usize + 2048];
    let expected: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| {
            ((u16::from(r >> 3) << 11)
                | (u16::from(g >> 3) << 6)
                | (u16::from(b >> 3) << 1)
                | u16::from(a >= 128))
            .to_be_bytes()
        })
        .collect();
    if encoded != expected {
        return Err("assembled env bytes differ from independently packed RGBA16".into());
    }
    Ok(
        json!({"synthetic":true,"generator":"env-xor-v1","formula":"[8*x,8*y,8*(x XOR y),255], x,y in 0..32","source_sha256":sha(source.as_bytes()),"rgba8_sha256":sha(&rgba),"rgba16_sha256":sha(encoded),"rgba8_bytes":rgba.len(),"rgba16_bytes":encoded.len(),"description":"synthetic substitute for the original environment image","microcode":"F3DEX2","data_format":"Fixed","logical_extent":[320,240],"presentation_extent":[800,600],"clear_policy":"PerFrame","seed":0,"frames":[0,719],"warmup":[0,119],"observed":[120,719]}),
    )
}

pub struct ImageHardware<'a>(pub &'a [u8]);
impl Hardware for ImageHardware<'_> {
    fn rdram(&self) -> impl Rdram + '_ {
        RdramImage::new(self.0)
    }
}

pub fn output_record(serial: u64, output: &ReplayOutput) -> Value {
    json!({"serial":serial,"rgba8_sha256":(!output.rgba8.is_empty()).then(|| sha(&output.rgba8)),"width":output.width,"height":output.height,"summaries":output.summaries.iter().map(|s| format!("{s:?}")).collect::<Vec<_>>(),"diagnostics":output.diagnostics.iter().map(|s| format!("{s:?}")).collect::<Vec<_>>(),"tasks":output.summaries.len()})
}

pub async fn read_pixels(
    renderer: &fast3d::Renderer,
    texture: &wgpu::Texture,
) -> Result<Vec<u8>, String> {
    let row = texture.width() * 4;
    let stride = row.div_ceil(256) * 256;
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("toy-readback"),
        size: u64::from(stride) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = renderer
        .device()
        .create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(texture.height()),
            },
        },
        texture.size(),
    );
    renderer.queue().submit([encoder.finish()]);
    let (send, receive) = futures_channel::oneshot::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = send.send(result);
        });
    #[cfg(not(target_arch = "wasm32"))]
    renderer
        .device()
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| e.to_string())?;
    receive
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let mapped = buffer.slice(..).get_mapped_range();
    let mut pixels = Vec::with_capacity((row * texture.height()) as usize);
    for y in 0..texture.height() as usize {
        pixels.extend_from_slice(&mapped[y * stride as usize..y * stride as usize + row as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(pixels)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn sequence_input(input: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if input != "authored" {
        return Ok(std::fs::read(input)?);
    }
    let fixture = fast3d::capture::Fixture::from_bytes(include_bytes!(
        "../../fast3d/tests/fixtures/host64-fill.f3dcap"
    ))?;
    let sequence = fast3d::capture::Sequence {
        frames: (1..=6)
            .map(|serial| {
                let mut frame = fixture.clone();
                frame.frame.serial = serial;
                frame.frame.dither_seed = 0;
                frame.frame.config.clear_policy = fast3d::ClearPolicy::Persist;
                frame
            })
            .collect(),
        warmup_frames: 2,
        presentations: vec![6],
    };
    Ok(sequence.to_bytes()?)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn authored_sequence_matches_parent_declarations() {
        let sequence =
            fast3d::capture::Sequence::from_bytes(&sequence_input("authored").unwrap()).unwrap();
        let original = fast3d::capture::Fixture::from_bytes(include_bytes!(
            "../../fast3d/tests/fixtures/host64-fill.f3dcap"
        ))
        .unwrap();
        assert_eq!(sequence.frames.len(), 6);
        assert_eq!(sequence.warmup_frames, 2);
        assert_eq!(sequence.presentations, [6]);
        for (index, fixture) in sequence.frames.iter().enumerate() {
            assert_eq!(fixture.frame.serial, index as u64 + 1);
            assert_eq!(fixture.frame.dither_seed, 0);
            assert_eq!(
                fixture.frame.config.clear_policy,
                fast3d::ClearPolicy::Persist
            );
            assert_eq!((fixture.frame.width, fixture.frame.height), (64, 48));
            assert_eq!(fixture.tasks, original.tasks);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct EmissionClock(Option<std::time::Instant>);
#[cfg(not(target_arch = "wasm32"))]
impl EmissionClock {
    pub fn record(&mut self, value: &mut Value) {
        let now = std::time::Instant::now();
        value["emission_interval_ms"] = json!(self
            .0
            .replace(now)
            .map(|previous| now.duration_since(previous).as_secs_f64() * 1000.0));
    }
}
