use std::sync::Arc;

use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

use fast3d::rdp::{OutputDimensions, SCREEN_HEIGHT, SCREEN_WIDTH};
use fast3d::{RenderData, RCP};
use fast3d_gbi::defines::color_combiner::CombineParams;
use fast3d_gbi::defines::f3dex2::{GeometryModes, MatrixMode, MatrixOperation};
use fast3d_gbi::defines::{
    AlphaCompare, ColorDither, ComponentSize, CycleType,
    GeometryModes as SharedGeometryModes, GfxCommand, ImageFormat, Matrix, PipelineMode,
    ScissorMode, TextureConvert, TextureDetail, TextureFilter, TextureLOD, TextureLUT,
    TextureTile, Vertex, Viewport, G_MAXZ,
};
use fast3d_gbi::dma::{gsSPDisplayList, gsSPMatrix, gsSPViewport};
use fast3d_gbi::gbi::{
    gsDPFullSync, gsDPPipeSync, gsSPEndDisplayList, GPACK_RGBA5551, G_RM_AA_OPA_SURF,
    G_RM_AA_OPA_SURF2, G_RM_OPA_SURF, G_RM_OPA_SURF2,
};
use fast3d_gbi::gu::{guOrtho, guRotate};
use fast3d_gbi::rdp::{gsDPFillRectangle, gsDPSetColorImage, gsDPSetFillColor, gsDPSetScissor};
use fast3d_gbi::rsp::{
    gsDPPipelineMode, gsDPSetAlphaCompare, gsDPSetColorDither, gsDPSetCombineKey,
    gsDPSetCombineMode, gsDPSetCycleType, gsDPSetRenderMode, gsDPSetTextureConvert,
    gsDPSetTextureDetail, gsDPSetTextureFilter, gsDPSetTextureLOD, gsDPSetTextureLUT,
    gsDPSetTexturePersp, gsSP1Triangle, gsSPClearGeometryMode, gsSPSetGeometryMode, gsSPTexture,
    gsSPVertex,
};
use fast3d_wgpu::WgpuRenderer;

mod vertices;
use vertices::create_vertices;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

struct State {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    size: winit::dpi::PhysicalSize<u32>,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,

    // fast3d specific
    rcp: RCP,
    render_data: RenderData,
    renderer: WgpuRenderer<'static>,
    depth_texture: wgpu::TextureView,
    vertex_data: Vec<Vertex>,
    viewport: Viewport,
    frame_count: f32,
    rsp_color_fb: [u16; (SCREEN_WIDTH * SCREEN_HEIGHT) as usize],
}

impl State {
    async fn new(window: Arc<Window>) -> State {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .unwrap();

        let size = window.inner_size();
        let cap = surface.get_capabilities(&adapter);
        let surface_format = cap.formats[0];

        // Create depth texture
        let depth_texture = Self::create_depth_texture(size.width, size.height, &device);

        // Initialize fast3d
        let mut rcp = RCP::default();
        let dimensions = OutputDimensions {
            width: size.width,
            height: size.height,
            aspect_ratio: size.width as f32 / size.height as f32,
        };
        rcp.rdp.output_dimensions = dimensions;

        let viewport = Viewport::new(
            [
                SCREEN_WIDTH as i16 * 2,
                SCREEN_HEIGHT as i16 * 2,
                G_MAXZ as i16 / 2,
                0,
            ],
            [
                SCREEN_WIDTH as i16 * 2,
                SCREEN_HEIGHT as i16 * 2,
                G_MAXZ as i16 / 2,
                0,
            ],
        );

        let renderer = WgpuRenderer::new(&device, [size.width, size.height]);

        let state = State {
            window,
            device,
            queue,
            size,
            surface,
            surface_format,
            rcp,
            render_data: RenderData::default(),
            renderer,
            depth_texture,
            vertex_data: create_vertices(),
            viewport,
            frame_count: 0.0,
            rsp_color_fb: [0; (SCREEN_WIDTH * SCREEN_HEIGHT) as usize],
        };

        state.configure_surface();
        state
    }

    fn create_depth_texture(
        width: u32,
        height: u32,
        device: &wgpu::Device,
    ) -> wgpu::TextureView {
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width,
                height,
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

        depth_texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    fn configure_surface(&self) {
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.surface_format,
            view_formats: vec![self.surface_format.add_srgb_suffix()],
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            width: self.size.width,
            height: self.size.height,
            desired_maximum_frame_latency: 2,
            present_mode: wgpu::PresentMode::AutoVsync,
        };
        self.surface.configure(&self.device, &surface_config);
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.size = new_size;
        self.depth_texture =
            Self::create_depth_texture(new_size.width, new_size.height, &self.device);

        let dimensions = OutputDimensions {
            width: new_size.width,
            height: new_size.height,
            aspect_ratio: new_size.width as f32 / new_size.height as f32,
        };
        self.rcp.rdp.output_dimensions = dimensions;

        self.configure_surface();
    }

    fn generate_rdp_init_dl() -> Vec<GfxCommand> {
        vec![
            gsDPSetCycleType(CycleType::default().raw_gbi_value()),
            gsDPPipelineMode(PipelineMode::default().raw_gbi_value()),
            gsDPSetScissor(
                ScissorMode::NonInterlace as u32,
                0,
                0,
                SCREEN_WIDTH as u32,
                SCREEN_HEIGHT as u32,
            ),
            gsDPSetTextureLOD(TextureLOD::default().raw_gbi_value()),
            gsDPSetTextureLUT(TextureLUT::default().raw_gbi_value()),
            gsDPSetTextureDetail(TextureDetail::default().raw_gbi_value()),
            gsDPSetTexturePersp(true),
            gsDPSetTextureFilter(TextureFilter::Bilerp.raw_gbi_value()),
            gsDPSetTextureConvert(TextureConvert::Filt.raw_gbi_value()),
            gsDPSetCombineMode(CombineParams::SHADE),
            gsDPSetCombineKey(false),
            gsDPSetAlphaCompare(AlphaCompare::None.raw_gbi_value()),
            gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
            gsDPSetColorDither(ColorDither::Disable.raw_gbi_value()),
            gsDPPipeSync(),
            gsSPEndDisplayList(),
        ]
    }

    fn generate_rsp_init_dl(viewport: &Viewport) -> Vec<GfxCommand> {
        vec![
            gsSPViewport(viewport),
            gsSPClearGeometryMode(
                SharedGeometryModes::SHADE.bits()
                    | GeometryModes::SHADING_SMOOTH.bits()
                    | SharedGeometryModes::FOG.bits()
                    | SharedGeometryModes::LIGHTING.bits()
                    | SharedGeometryModes::TEXTURE_GEN.bits()
                    | SharedGeometryModes::TEXTURE_GEN_LINEAR.bits()
                    | SharedGeometryModes::LOD.bits(),
            ),
            gsSPTexture(0, 0, 0, TextureTile::RENDERTILE, false),
            gsSPSetGeometryMode(
                SharedGeometryModes::SHADE.bits() | GeometryModes::SHADING_SMOOTH.bits(),
            ),
            gsSPEndDisplayList(),
        ]
    }

    fn generate_clear_color_fb_dl(rsp_color_fb: usize) -> Vec<GfxCommand> {
        vec![
            gsDPSetCycleType(CycleType::Fill.raw_gbi_value()),
            gsDPSetColorImage(
                ImageFormat::Rgba,
                ComponentSize::Bits16,
                SCREEN_WIDTH as u32,
                rsp_color_fb,
            ),
            gsDPSetFillColor(GPACK_RGBA5551(64, 64, 64, 1) << 16 | GPACK_RGBA5551(64, 64, 64, 1)),
            gsDPFillRectangle(0, 0, SCREEN_WIDTH as u32 - 1, SCREEN_HEIGHT as u32 - 1),
            gsDPPipeSync(),
            gsDPSetFillColor(
                GPACK_RGBA5551(64, 64, 255, 1) << 16 | GPACK_RGBA5551(64, 64, 255, 1),
            ),
            gsDPFillRectangle(20, 20, SCREEN_WIDTH as u32 - 20, SCREEN_HEIGHT as u32 - 20),
            gsSPEndDisplayList(),
        ]
    }

    fn generate_triangle_dl(
        projection_mtx_ptr: *mut Matrix,
        modelview_mtx_ptr: *mut Matrix,
        triangle_vertices: &[Vertex],
    ) -> Vec<GfxCommand> {
        vec![
            gsSPMatrix(
                projection_mtx_ptr,
                (MatrixMode::PROJECTION.bits()
                    | MatrixOperation::LOAD.bits()
                    | MatrixOperation::NOPUSH.bits()) as u32,
            ),
            gsSPMatrix(
                modelview_mtx_ptr,
                (MatrixMode::MODELVIEW.bits()
                    | MatrixOperation::LOAD.bits()
                    | MatrixOperation::NOPUSH.bits()) as u32,
            ),
            gsDPPipeSync(),
            gsDPSetCycleType(CycleType::OneCycle.raw_gbi_value()),
            gsDPSetRenderMode(G_RM_AA_OPA_SURF, G_RM_AA_OPA_SURF2),
            gsSPSetGeometryMode(
                SharedGeometryModes::SHADE.bits() | GeometryModes::SHADING_SMOOTH.bits(),
            ),
            gsSPVertex(triangle_vertices, 4, 0),
            gsSP1Triangle(0, 1, 2, 0),
            gsSP1Triangle(0, 3, 2, 0),
            gsSPEndDisplayList(),
        ]
    }

    fn render(&mut self) {
        let surface_texture = self
            .surface
            .get_current_texture()
            .expect("failed to acquire next swapchain texture");

        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                format: Some(self.surface_format.add_srgb_suffix()),
                ..Default::default()
            });

        self.renderer.update_frame_count();
        self.frame_count += 1.0;

        let rdp_init_dl = Self::generate_rdp_init_dl();
        let rsp_init_dl = Self::generate_rsp_init_dl(&self.viewport);
        let clear_color_fb_dl =
            Self::generate_clear_color_fb_dl(self.rsp_color_fb.as_ptr() as usize);

        let mut projection_mtx = Matrix { m: [[0; 4]; 4] };
        let projection_mtx_ptr = &mut projection_mtx as *mut Matrix;

        let mut modelview_mtx = Matrix { m: [[0; 4]; 4] };
        let modelview_mtx_ptr = &mut modelview_mtx as *mut Matrix;

        // set up projection and modelview matrices
        guOrtho(
            projection_mtx_ptr,
            -(SCREEN_WIDTH) / 2.0,
            (SCREEN_WIDTH) / 2.0,
            -(SCREEN_HEIGHT) / 2.0,
            (SCREEN_HEIGHT) / 2.0,
            1.0,
            10.0,
            1.0,
        );
        guRotate(modelview_mtx_ptr, self.frame_count, 0.0, 1.0, 0.0);

        let triangle_dl =
            Self::generate_triangle_dl(projection_mtx_ptr, modelview_mtx_ptr, &self.vertex_data);

        let commands_dl = [
            gsSPDisplayList(&rdp_init_dl),
            gsSPDisplayList(&rsp_init_dl),
            gsSPDisplayList(&clear_color_fb_dl),
            gsSPDisplayList(&triangle_dl),
            gsDPFullSync(),
            gsSPEndDisplayList(),
        ];

        let mut encoder: wgpu::CommandEncoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        let draw_commands_ptr = commands_dl.as_ptr();

        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Game Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &texture_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_texture,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        // Run the RCP
        self.rcp
            .process_dl(draw_commands_ptr as usize, &mut self.render_data);

        // Process the RCP output
        self.renderer.process_rcp_output(
            &self.device,
            &self.queue,
            self.surface_format,
            &mut self.render_data,
        );

        // Draw the RCP output
        self.renderer.draw(&mut rpass);

        // Clear the draw calls
        self.render_data.clear_draw_calls();

        drop(rpass);
        self.queue.submit([encoder.finish()]);
        surface_texture.present();
    }
}

fn main() {
    env_logger::init();

    let event_loop = EventLoop::new().unwrap();
    let window = Arc::new(
        winit::window::WindowBuilder::new()
            .with_title("fast3d-rs Triangle Example")
            .with_inner_size(winit::dpi::LogicalSize::new(800, 600))
            .build(&event_loop)
            .unwrap(),
    );

    let mut state = pollster::block_on(State::new(window.clone()));

    event_loop
        .run(move |event, target| {
            target.set_control_flow(ControlFlow::Poll);

            match event {
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::CloseRequested => {
                        println!("The close button was pressed; stopping");
                        target.exit();
                    }
                    WindowEvent::RedrawRequested => {
                        state.render();
                        state.window.request_redraw();
                    }
                    WindowEvent::Resized(size) => {
                        if size.width > 0 && size.height > 0 {
                            state.resize(size);
                        }
                    }
                    _ => (),
                },
                Event::AboutToWait => {
                    state.window.request_redraw();
                }
                _ => (),
            }
        })
        .unwrap();
}
