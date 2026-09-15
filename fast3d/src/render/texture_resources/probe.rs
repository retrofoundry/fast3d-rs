use std::{collections::BTreeMap, sync::Arc};

use crate::{
    hle::texture_request::EncodedTextureRequest,
    profiling::{Bank, Mode, Recorder, Representation, Request, Snapshot},
    render::texture_residency::{Limits, Residency},
};

use super::{Image, TextureResources};

fn request(value: u8) -> Result<EncodedTextureRequest, String> {
    let mut request = Request {
        representation: Representation::Tile,
        role: "pressure".into(),
        tile: Default::default(),
        tlut: 0,
        extent: [1, 1],
        bank: Bank {
            bytes: vec![value; 4096],
            sources: Vec::new(),
            blocks: Vec::new(),
        },
        linear: None,
        reachable_bytes: 0,
        read_operations: 0,
        palette_bytes: 0,
        output: Vec::new(),
        rejection: None,
    };
    request.tile.fmt = 4;
    request.tile.siz = 1;
    request.tile.width = 1;
    request.tile.height = 1;
    request
        .encoded_request()
        .map_err(|error| format!("{error:?}"))
}

fn resources(device: &wgpu::Device) -> TextureResources {
    let mut resources = TextureResources::new(device);
    resources.cache = Residency::new(Limits {
        entries: 1,
        gpu_bytes: 4,
        cpu_bytes: 8192,
    });
    resources
}

fn submit(
    resources: &mut TextureResources,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    recorder: &Recorder,
) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("residency-pressure"),
    });
    resources.encode(&mut encoder, recorder);
    queue.submit([encoder.finish()]);
    recorder.count("submissions", 1);
    resources.submitted(queue, recorder);
}

fn snapshot(
    resources: &TextureResources,
    recorder: &Recorder,
    output: &mut BTreeMap<String, Snapshot>,
    name: &str,
) {
    resources.profile(recorder);
    output.insert(name.into(), recorder.drain());
}

async fn read_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    images: &[(&Image, u8)],
) -> Result<(), String> {
    let stride = u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("residency-pressure-readback"),
        size: images.len() as u64 * stride,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("residency-pressure-readback"),
    });
    for (index, (image, _)) in images.iter().enumerate() {
        let texture = &image.value.texture;
        if texture.width() != 1 || texture.height() != 1 {
            return Err("pressure readback requires one-pixel images".into());
        }
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: index as u64 * stride,
                    bytes_per_row: Some(stride as u32),
                    rows_per_image: None,
                },
            },
            texture.size(),
        );
    }
    queue.submit([encoder.finish()]);
    let (completed, completion) = futures_channel::oneshot::channel();
    queue.on_submitted_work_done(move || {
        let _ = completed.send(());
    });
    let (mapped, mapping) = futures_channel::oneshot::channel();
    buffer.map_async(wgpu::MapMode::Read, .., move |result| {
        let _ = mapped.send(result);
    });
    #[cfg(not(target_arch = "wasm32"))]
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| error.to_string())?;
    completion.await.map_err(|error| error.to_string())?;
    mapping
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    let bytes = buffer.get_mapped_range(..);
    for (index, (_, expected)) in images.iter().enumerate() {
        let offset = index * stride as usize;
        let actual = &bytes[offset..offset + 4];
        if actual != [*expected; 4] {
            return Err(format!(
                "pressure image {index}: got {actual:?}, expected {:?}",
                [*expected; 4]
            ));
        }
    }
    drop(bytes);
    buffer.unmap();
    Ok(())
}

pub(crate) async fn run(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<BTreeMap<String, Snapshot>, String> {
    let request_a = request(17)?;
    let request_b = request(91)?;
    let recorder = Recorder::new(Mode::Counters);
    let mut output = BTreeMap::new();
    let mut resident = resources(device);

    let a = resident.resolve(device, queue, &request_a, "pressure", &recorder);
    let b = resident.resolve(device, queue, &request_b, "pressure", &recorder);
    let repeated_b = resident.resolve(device, queue, &request_b, "pressure", &recorder);
    if !a.admitted || b.admitted || !Arc::ptr_eq(&b, &repeated_b) {
        return Err("pressure admission or same-batch reuse failed".into());
    }
    drop(repeated_b);
    submit(&mut resident, device, queue, &recorder);
    snapshot(&resident, &recorder, &mut output, "cold_pressure");

    let pending_b = resident.resolve(device, queue, &request_b, "pressure", &recorder);
    if !Arc::ptr_eq(&b, &pending_b) {
        return Err("submitted bypass image was decoded again while retained".into());
    }
    drop(pending_b);
    submit(&mut resident, device, queue, &recorder);
    snapshot(
        &resident,
        &recorder,
        &mut output,
        "pending_across_submission",
    );
    read_pixels(device, queue, &[(&a, 17), (&b, 91)]).await?;
    resident.collect(&recorder);
    snapshot(&resident, &recorder, &mut output, "completed");
    drop(b);

    let b = resident.resolve(device, queue, &request_b, "pressure", &recorder);
    if b.admitted {
        return Err("retained A was evicted to admit B".into());
    }
    submit(&mut resident, device, queue, &recorder);
    snapshot(&resident, &recorder, &mut output, "repeat_bypass");
    read_pixels(device, queue, &[(&a, 17), (&b, 91)]).await?;
    resident.collect(&recorder);
    snapshot(&resident, &recorder, &mut output, "repeat_completed");
    drop(b);
    drop(a);

    let b = resident.resolve(device, queue, &request_b, "pressure", &recorder);
    if !b.admitted {
        return Err("completed unpinned A prevented admission of B".into());
    }
    submit(&mut resident, device, queue, &recorder);
    read_pixels(device, queue, &[(&b, 91)]).await?;
    resident.collect(&recorder);
    snapshot(&resident, &recorder, &mut output, "evict_a");
    drop(b);

    let a = resident.resolve(device, queue, &request_a, "pressure", &recorder);
    if !a.admitted {
        return Err("completed unpinned B prevented admission of A".into());
    }
    submit(&mut resident, device, queue, &recorder);
    read_pixels(device, queue, &[(&a, 17)]).await?;
    resident.collect(&recorder);
    snapshot(&resident, &recorder, &mut output, "evict_b");

    drop(resident);
    let mut resident = resources(device);
    let new_a = resident.resolve(device, queue, &request_a, "pressure", &recorder);
    if Arc::ptr_eq(&a, &new_a) {
        return Err("new residency epoch reused an old image".into());
    }
    submit(&mut resident, device, queue, &recorder);
    read_pixels(device, queue, &[(&a, 17), (&new_a, 17)]).await?;
    resident.collect(&recorder);
    snapshot(&resident, &recorder, &mut output, "new_epoch");
    Ok(output)
}
