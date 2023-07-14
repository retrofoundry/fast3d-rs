use f3dwgpu::WgpuRenderer;
use fast3d::rdp::{OutputDimensions, SCREEN_HEIGHT, SCREEN_WIDTH};
use fast3d::{RCPOutputCollector, RCP};
use gbi_assembler::defines::color_combiner::G_CC_SHADE;
use gbi_assembler::defines::{
    AlphaCompare, ColorDither, ColorVertex, ComponentSize, CycleType,
    GeometryModes as SharedGeometryModes, GfxCommand, ImageFormat, Matrix, PipelineMode,
    ScissorMode, TextureConvert, TextureDetail, TextureFilter, TextureLOD, TextureLUT, Vertex,
    Viewport, G_MAXZ,
};
use gbi_assembler::dma::{gsSPDisplayList, gsSPMatrix, gsSPViewport};
use gbi_assembler::f3dex2::{GeometryModes, MatrixMode, MatrixOperation};
use gbi_assembler::gbi::{
    gsDPFullSync, gsDPPipeSync, gsSPEndDisplayList, GPACK_RGBA5551, G_RM_AA_OPA_SURF,
    G_RM_AA_OPA_SURF2, G_RM_OPA_SURF, G_RM_OPA_SURF2,
};
use gbi_assembler::gu::{guOrtho, guRotate};
use gbi_assembler::rdp::{gsDPFillRectangle, gsDPSetColorImage, gsDPSetFillColor, gsDPSetScissor};
use gbi_assembler::rsp::{
    gsDPPipelineMode, gsDPSetAlphaCompare, gsDPSetColorDither, gsDPSetCombineKey,
    gsDPSetCombineMode, gsDPSetCycleType, gsDPSetRenderMode, gsDPSetTextureConvert,
    gsDPSetTextureDetail, gsDPSetTextureFilter, gsDPSetTextureLOD, gsDPSetTextureLUT,
    gsDPSetTexturePersp, gsSP1Triangle, gsSPClearGeometryMode, gsSPSetGeometryMode, gsSPTexture,
    gsSPVertex,
};
use pigment::color::Color;
use std::{future::Future, pin::Pin, task};
use winit::dpi::LogicalSize;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

fn create_vertices() -> Vec<Vertex> {
    vec![
        Vertex {
            color: ColorVertex::new([-64, 64, -5], [0, 0], Color::RGBA(0, 0xFF, 0, 0xFF)),
        },
        Vertex {
            color: ColorVertex::new([64, 64, -5], [0, 0], Color::RGBA(0, 0, 0, 0xFF)),
        },
        Vertex {
            color: ColorVertex::new([64, -64, -5], [0, 0], Color::RGBA(0, 0, 0xFF, 0xFF)),
        },
        Vertex {
            color: ColorVertex::new([-64, -64, -5], [0, 0], Color::RGBA(0xFF, 0, 0, 0xFF)),
        },
    ]
}

/// A wrapper for `pop_error_scope` futures that panics if an error occurs.
///
/// Given a future `inner` of an `Option<E>` for some error type `E`,
/// wait for the future to be ready, and panic if its value is `Some`.
///
/// This can be done simpler with `FutureExt`, but we don't want to add
/// a dependency just for this small case.
struct ErrorFuture<F> {
    inner: F,
}
impl<F: Future<Output = Option<wgpu::Error>>> Future for ErrorFuture<F> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> task::Poll<()> {
        let inner = unsafe { self.map_unchecked_mut(|me| &mut me.inner) };
        inner.poll(cx).map(|error| {
            if let Some(e) = error {
                panic!("Rendering {e}");
            }
        })
    }
}

struct Example<'a> {
    rcp: RCP,
    rcp_output_collector: RCPOutputCollector,
    renderer: WgpuRenderer<'a>,

    depth_texture: wgpu::TextureView,
    surface_format: wgpu::TextureFormat,

    vertex_data: Vec<Vertex>,
    viewport: Viewport,
    frame_count: f32,
    rsp_color_fb: [u16; (SCREEN_WIDTH * SCREEN_HEIGHT) as usize],
}

impl<'a> Example<'a> {
    fn create_depth_texture(
        config: &wgpu::SurfaceConfiguration,
        device: &wgpu::Device,
    ) -> wgpu::TextureView {
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
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
            gsDPSetCombineMode(G_CC_SHADE),
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
            gsSPTexture(0, 0, 0, 0, false),
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
                ImageFormat::Rgba as u32,
                ComponentSize::Bits16 as u32,
                SCREEN_WIDTH as u32,
                rsp_color_fb,
            ),
            gsDPSetFillColor(GPACK_RGBA5551(64, 64, 64, 1) << 16 | GPACK_RGBA5551(64, 64, 64, 1)),
            gsDPFillRectangle(0, 0, SCREEN_WIDTH as u32 - 1, SCREEN_HEIGHT as u32 - 1),
            gsDPPipeSync(),
            gsDPSetFillColor(GPACK_RGBA5551(64, 64, 255, 1) << 16 | GPACK_RGBA5551(64, 64, 255, 1)),
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
}

impl<'a> fast3d_example::framework::Example for Example<'static> {
    fn init(
        config: &wgpu::SurfaceConfiguration,
        _adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _content_size: winit::dpi::PhysicalSize<u32>,
    ) -> Self {
        let mut rcp = RCP::default();

        let dimensions = OutputDimensions {
            width: config.width,
            height: config.height,
            aspect_ratio: config.width as f32 / config.height as f32,
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

        Self {
            rcp,
            rcp_output_collector: RCPOutputCollector::default(),
            renderer: WgpuRenderer::new(device, [config.width, config.height]),

            depth_texture: Self::create_depth_texture(config, device),
            surface_format: config.format,

            vertex_data: create_vertices(),
            viewport,
            frame_count: 0.0,
            rsp_color_fb: [0; (SCREEN_WIDTH * SCREEN_HEIGHT) as usize],
        }
    }

    fn resize(
        &mut self,
        config: &wgpu::SurfaceConfiguration,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
    ) {
        let dimensions = OutputDimensions {
            width: config.width,
            height: config.height,
            aspect_ratio: config.width as f32 / config.height as f32,
        };
        self.rcp.rdp.output_dimensions = dimensions;
    }

    fn update(&mut self, _event: winit::event::WindowEvent) {
        //empty
    }

    fn render(
        &mut self,
        view: &wgpu::TextureView,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _spawner: &fast3d_example::framework::Spawner,
    ) {
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
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        let draw_commands_ptr = commands_dl.as_ptr();

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
                view: &self.depth_texture,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: true,
                }),
                stencil_ops: None,
            }),
        });

        // Run the RCP
        self.rcp
            .run(&mut self.rcp_output_collector, draw_commands_ptr as usize);

        // Process the RCP output
        self.renderer.process_rcp_output(
            device,
            queue,
            self.surface_format,
            &mut self.rcp_output_collector,
        );

        // Draw the RCP output
        self.renderer.draw(&mut rpass);

        self.rcp_output_collector.clear_draw_calls();
        drop(rpass);
        queue.submit(Some(encoder.finish()));
    }
}

fn main() {
    fast3d_example::framework::run::<Example>(
        "triangle",
        Some(LogicalSize {
            width: 800,
            height: 600,
        }),
    );
}
