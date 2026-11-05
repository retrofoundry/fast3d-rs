//! P3.9: `present_to` scans the selected framebuffer out into a caller-owned view.

use crate::{
    ClearPolicy, Hardware, Microcode, NopSink, PresentTarget, Rdram, RdramImage, Renderer,
    RendererConfig,
};

struct ImgHw {
    rdram: Vec<u8>,
}
impl Hardware for ImgHw {
    fn rdram(&self) -> impl Rdram + '_ {
        RdramImage::new(&self.rdram)
    }
}

fn cfg() -> RendererConfig {
    RendererConfig {
        resolution_multiplier: 1,
        sample_count: 1,
        present_mode: wgpu::PresentMode::Fifo,
        format: Some(wgpu::TextureFormat::Rgba8Unorm),
        clear_policy: ClearPolicy::PerFrame,
        power_preference: wgpu::PowerPreference::LowPower,
    }
}

#[test]
fn present_to_scans_out_the_last_rendered_framebuffer() {
    let (rdram, entry_addr) = crate::tests::fixtures::fixture("flat-color--white1");
    let hw = ImgHw {
        rdram: rdram.to_vec(),
    };

    let (device, queue, _dual) = crate::render::headless_device();
    let mut r = Renderer::with_device(
        device,
        queue,
        PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 64,
            height: 64,
        },
        cfg(),
    );

    r.begin_frame();
    r.process_dl(&hw, entry_addr, Microcode::F3dex2, &mut NopSink);

    let target = r.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("present-to-target"),
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    r.present_to(&hw, &view);

    let readback = r.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("rb"),
        size: 64 * 64 * 4,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = r
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(64 * 4),
                rows_per_image: Some(64),
            },
        },
        wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
    );
    r.queue().submit(Some(enc.finish()));

    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| tx.send(res).unwrap());
    r.device()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    let data = slice.get_mapped_range().to_vec();

    let off = (32 * 64 + 32) * 4; // center pixel
    let px = [data[off], data[off + 1], data[off + 2], data[off + 3]];
    let near = |a: u8, b: u8| (a as i16 - b as i16).abs() <= 2;
    assert!(
        near(px[0], 64) && near(px[1], 200) && near(px[2], 255) && near(px[3], 255),
        "center pixel must be the scanned-out flat-color PRIM (64,200,255,255); got {px:?}"
    );
}

#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
#[test]
fn host_scanout_selects_full_width_address_and_falls_back() {
    use n64_gbi::encode::*;
    let (device, queue, _) = crate::render::headless_device();
    let mut r = Renderer::with_device(
        device.clone(),
        queue.clone(),
        PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 64,
            height: 64,
        },
        cfg(),
    );
    let addresses = [0x1_0020_0000u64, 0x2_0020_0000];
    for (address, word) in addresses.into_iter().zip([0xf801_f801, 0x07c1_07c1]) {
        let mut dl: Vec<[u64; 2]> = [
            gdp_set_color_image(0, 2, 64, 0),
            gdp_set_scissor(0, 0, 0, 256, 256),
            gdp_set_cycle_type(3),
            gdp_set_fill_color(word),
            gdp_fill_rectangle(0, 0, 252, 252),
            gsp_enddl(),
        ]
        .into_iter()
        .map(|(a, b)| [a as u64, b as u64])
        .collect();
        dl[0][1] = address;
        unsafe {
            r.process_dl_host(
                crate::HostRam::new(&[]),
                dl.as_ptr() as u64,
                Microcode::F3dex2,
                &mut NopSink,
            );
        }
    }
    for (address, expected) in [
        (Some(addresses[0]), [255, 0, 0, 255]),
        (Some(addresses[1]), [0, 255, 0, 255]),
        (Some(0x3_0020_0000), [0, 255, 0, 255]),
        (None, [0, 255, 0, 255]),
    ] {
        let pixels = super::common::pixels_from_render(
            &device,
            &queue,
            64,
            64,
            wgpu::TextureFormat::Rgba8Unorm,
            |view| r.present_host_to(address, view),
        );
        assert!(pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == expected));
    }
}
