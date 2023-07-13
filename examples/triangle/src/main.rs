use crate::gbi::{
    gsDPFillRectangle, gsDPFullSync, gsDPPipeSync, gsDPPipelineMode, gsDPSetAlphaCompare,
    gsDPSetColorDither, gsDPSetColorImage, gsDPSetCombineKey, gsDPSetCombineMode, gsDPSetCycleType,
    gsDPSetFillColor, gsDPSetRenderMode, gsDPSetScissor, gsDPSetTextureConvert,
    gsDPSetTextureDetail, gsDPSetTextureFilter, gsDPSetTextureLOD, gsDPSetTextureLUT,
    gsDPSetTexturePersp, gsSP1Triangle, gsSPClearGeometryMode, gsSPDisplayList, gsSPEndDisplayList,
    gsSPMatrix, gsSPSetGeometryMode, gsSPTexture, gsSPVertex, gsSPViewport, G_IM_SIZ_16b,
    GPACK_RGBA5551, G_AC_NONE, G_CC_SHADE, G_CD_DISABLE, G_CK_NONE, G_CULL_BOTH, G_CYC_1CYCLE,
    G_CYC_FILL, G_FOG, G_IM_FMT_RGBA, G_LIGHTING, G_LOD, G_MAXZ, G_MTX_LOAD, G_MTX_MODELVIEW,
    G_MTX_NOPUSH, G_MTX_PROJECTION, G_PM_NPRIMITIVE, G_RM_AA_OPA_SURF, G_RM_AA_OPA_SURF2,
    G_RM_OPA_SURF, G_RM_OPA_SURF2, G_SC_NON_INTERLACE, G_SHADE, G_SHADING_SMOOTH, G_TC_FILT,
    G_TD_CLAMP, G_TEXTURE_GEN, G_TEXTURE_GEN_LINEAR, G_TF_BILERP, G_TL_TILE, G_TP_PERSP, G_TT_NONE,
};
use crate::gu::{guOrtho, guRotate};
use f3dwgpu::WgpuRenderer;
use fast3d::gbi::defines::{Color_t, Gfx, Mtx, Viewport, Vtx, Vtx_t};
use fast3d::rdp::OutputDimensions;
use fast3d::{RCPOutputCollector, RCP};
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

mod gbi;
mod gu;

static mut theta: f32 = 0.0;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

const SCREEN_HT: u32 = 240;
const SCREEN_WD: u32 = 320;

struct Example<'a> {
    rcp: RCP,
    rcp_output_collector: RCPOutputCollector,
    renderer: WgpuRenderer<'a>,
}

impl<'a> Example<'a> {
    pub fn new(device: &wgpu::Device, screen_size: [u32; 2]) -> Self {
        Self {
            rcp: RCP::default(),
            rcp_output_collector: RCPOutputCollector::default(),
            renderer: WgpuRenderer::new(device, screen_size),
        }
    }

    pub fn start_frame(&mut self, size: PhysicalSize<u32>) {
        let dimensions = OutputDimensions {
            width: size.width,
            height: size.height,
            aspect_ratio: size.width as f32 / size.height as f32,
        };
        self.rcp.rdp.output_dimensions = dimensions;

        self.renderer.update_frame_count();
    }

    pub fn end_frame(&mut self) {
        self.rcp_output_collector.clear_draw_calls();
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        depth_texture_view: &wgpu::TextureView,
        surface_format: wgpu::TextureFormat,
        commands: *mut Gfx,
    ) {
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Game Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: true,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_texture_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: true,
                }),
                stencil_ops: None,
            }),
        });

        // Run the RCP
        self.rcp
            .run(&mut self.rcp_output_collector, commands as usize);

        // Process the RCP output
        self.renderer.process_rcp_output(
            device,
            queue,
            surface_format,
            &mut self.rcp_output_collector,
        );

        // Draw the RCP output
        self.renderer.draw(&mut rpass);
    }
}

fn main() {
    // Setup logging
    env_logger::init();

    // Set up window and GPU
    let event_loop = EventLoop::new();

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..Default::default()
    });

    let (window, size, surface) = {
        let version = env!("CARGO_PKG_VERSION");

        let window = Window::new(&event_loop).unwrap();
        window.set_inner_size(LogicalSize {
            width: 800,
            height: 600,
        });
        window.set_title(&format!("fast3d-wgpu {version}"));
        window.set_resizable(false);
        let size = window.inner_size();

        let surface = unsafe { instance.create_surface(&window) }.unwrap();

        (window, size, surface)
    };

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))
    .unwrap();

    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
            .unwrap();

    // Set up swap chain
    let surface_desc = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: wgpu::TextureFormat::Bgra8Unorm,
        width: size.width,
        height: size.height,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![wgpu::TextureFormat::Bgra8Unorm],
    };

    surface.configure(&device, &surface_desc);

    // Setup depth texture
    let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
        size: wgpu::Extent3d {
            width: surface_desc.width,
            height: surface_desc.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        label: None,
        view_formats: &[],
    });

    let depth_texture_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

    // Setup example app
    let mut example = Example::new(&device, [size.width, size.height]);

    let rdp_init_dl: Vec<Gfx> = vec![
        gsDPSetCycleType(G_CYC_1CYCLE),
        gsDPPipelineMode(G_PM_NPRIMITIVE),
        gsDPSetScissor(G_SC_NON_INTERLACE, 0, 0, SCREEN_WD, SCREEN_HT),
        gsDPSetTextureLOD(G_TL_TILE),
        gsDPSetTextureLUT(G_TT_NONE),
        gsDPSetTextureDetail(G_TD_CLAMP),
        gsDPSetTexturePersp(G_TP_PERSP),
        gsDPSetTextureFilter(G_TF_BILERP),
        gsDPSetTextureConvert(G_TC_FILT),
        gsDPSetCombineMode(G_CC_SHADE),
        gsDPSetCombineKey(G_CK_NONE),
        gsDPSetAlphaCompare(G_AC_NONE),
        gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
        gsDPSetColorDither(G_CD_DISABLE),
        gsDPPipeSync(),
        gsSPEndDisplayList(),
    ];

    let viewport = Viewport::new(
        [
            SCREEN_WD as i16 * 2,
            SCREEN_HT as i16 * 2,
            G_MAXZ as i16 / 2,
            0,
        ],
        [
            SCREEN_WD as i16 * 2,
            SCREEN_HT as i16 * 2,
            G_MAXZ as i16 / 2,
            0,
        ],
    );

    let rsp_init_dl: Vec<Gfx> = vec![
        gsSPViewport(&viewport),
        gsSPClearGeometryMode(
            G_SHADE
                | G_SHADING_SMOOTH
                | G_CULL_BOTH
                | G_FOG
                | G_LIGHTING
                | G_TEXTURE_GEN
                | G_TEXTURE_GEN_LINEAR
                | G_LOD,
        ),
        gsSPTexture(0, 0, 0, 0, false),
        gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH),
        gsSPEndDisplayList(),
    ];

    let rsp_color_fb: [u16; (SCREEN_WD * SCREEN_HT) as usize] =
        [0; (SCREEN_WD * SCREEN_HT) as usize];
    let clear_color_fb_dl: Vec<Gfx> = vec![
        gsDPSetCycleType(G_CYC_FILL),
        gsDPSetColorImage(
            G_IM_FMT_RGBA,
            G_IM_SIZ_16b,
            SCREEN_WD,
            &rsp_color_fb as *const u16 as usize,
        ),
        gsDPSetFillColor(GPACK_RGBA5551(64, 64, 64, 1) << 16 | GPACK_RGBA5551(64, 64, 64, 1)),
        gsDPFillRectangle(0, 0, SCREEN_WD - 1, SCREEN_HT - 1),
        gsDPPipeSync(),
        gsDPSetFillColor(GPACK_RGBA5551(64, 64, 255, 1) << 16 | GPACK_RGBA5551(64, 64, 255, 1)),
        gsDPFillRectangle(20, 20, SCREEN_WD - 20, SCREEN_HT - 20),
        gsSPEndDisplayList(),
    ];

    // Fixed point projection matrix
    let mut projection_mtx = Mtx { m: [[0; 4]; 4] };
    let projection_mtx_ptr = &mut projection_mtx as *mut Mtx;

    let mut modelview_mtx = Mtx { m: [[0; 4]; 4] };
    let modelview_mtx_ptr = &mut modelview_mtx as *mut Mtx;

    let triangle_vertices: Vec<Vtx> = vec![
        Vtx {
            vertex: Vtx_t::new(
                [-64, 64, -5],
                [0, 0],
                Color_t {
                    r: 0,
                    g: 0xFF,
                    b: 0,
                    a: 0xFF,
                },
            ),
        },
        Vtx {
            vertex: Vtx_t::new(
                [64, 64, -5],
                [0, 0],
                Color_t {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 0xFF,
                },
            ),
        },
        Vtx {
            vertex: Vtx_t::new(
                [64, -64, -5],
                [0, 0],
                Color_t {
                    r: 0,
                    g: 0,
                    b: 0xFF,
                    a: 0xFF,
                },
            ),
        },
        Vtx {
            vertex: Vtx_t::new(
                [-64, -64, -5],
                [0, 0],
                Color_t {
                    r: 0xFF,
                    g: 0,
                    b: 0,
                    a: 0xFF,
                },
            ),
        },
    ];

    let triangle_dl: Vec<Gfx> = vec![
        gsSPMatrix(
            projection_mtx_ptr,
            G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH,
        ),
        gsSPMatrix(
            modelview_mtx_ptr,
            G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH,
        ),
        gsDPPipeSync(),
        gsDPSetCycleType(G_CYC_1CYCLE),
        gsDPSetRenderMode(G_RM_AA_OPA_SURF, G_RM_AA_OPA_SURF2),
        gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH),
        gsSPVertex(&triangle_vertices, 4, 0),
        gsSP1Triangle(0, 1, 2, 0),
        gsSP1Triangle(0, 2, 3, 0),
        gsSPEndDisplayList(),
    ];

    // aggregate the different dl's into one
    let commands_dl = [
        gsSPDisplayList(&rdp_init_dl),
        gsSPDisplayList(&rsp_init_dl),
        gsSPDisplayList(&clear_color_fb_dl),
        gsSPDisplayList(&triangle_dl),
        gsDPFullSync(),
        gsSPEndDisplayList(),
    ];

    // Event loop
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            Event::MainEventsCleared => window.request_redraw(),
            Event::RedrawEventsCleared => unsafe {
                // Set RDP output dimensions
                let size = window.inner_size();
                example.start_frame(size);

                guOrtho(
                    projection_mtx_ptr,
                    -(SCREEN_WD as f32) / 2.0,
                    (SCREEN_WD as f32) / 2.0,
                    -(SCREEN_HT as f32) / 2.0,
                    (SCREEN_HT as f32) / 2.0,
                    1.0,
                    10.0,
                    1.0,
                );
                guRotate(modelview_mtx_ptr, theta, 0.0, 0.0, 1.0);

                let frame = match surface.get_current_texture() {
                    Ok(frame) => frame,
                    Err(e) => {
                        eprintln!("dropped frame: {e:?}");
                        return;
                    }
                };

                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());

                let mut encoder: wgpu::CommandEncoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

                let draw_commands_ptr = commands_dl.as_ptr() as *mut Gfx;
                example.render(
                    &device,
                    &queue,
                    &mut encoder,
                    &view,
                    &depth_texture_view,
                    surface_desc.format,
                    draw_commands_ptr,
                );

                example.end_frame();
                queue.submit(Some(encoder.finish()));
                frame.present();

                unsafe {
                    theta += 1.0;
                }
            },
            _ => (),
        }
    });
}
