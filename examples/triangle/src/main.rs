use std::{future::Future, pin::Pin, task};
use winit::dpi::LogicalSize;
use f3dwgpu::WgpuRenderer;
use fast3d::{RCP, RCPOutputCollector};
use fast3d::gbi::defines::{Color_t, Gfx, Mtx, Viewport, Vtx, Vtx_t};
use fast3d::rdp::{OutputDimensions, SCREEN_HEIGHT, SCREEN_WIDTH};

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

mod gbi;
mod gu;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

fn create_vertices() -> Vec<Vtx> {
    vec![
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

    vertex_data: Vec<Vtx>,
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

    fn generate_rdp_init_dl() -> Vec<Gfx> {
        vec![
            gsDPSetCycleType(G_CYC_1CYCLE),
            gsDPPipelineMode(G_PM_NPRIMITIVE),
            gsDPSetScissor(G_SC_NON_INTERLACE, 0, 0, SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32),
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
        ]
    }

    fn generate_rsp_init_dl(viewport: &Viewport) -> Vec<Gfx> {
        vec![
            gsSPViewport(viewport),
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
        ]
    }

    fn generate_clear_color_fb_dl(rsp_color_fb: usize) -> Vec<Gfx> {
        vec![
            gsDPSetCycleType(G_CYC_FILL),
            gsDPSetColorImage(
                G_IM_FMT_RGBA,
                G_IM_SIZ_16b,
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

    fn generate_triangle_dl(projection_mtx_ptr: *mut Mtx, modelview_mtx_ptr: *mut Mtx, triangle_vertices: &[Vtx]) -> Vec<Gfx> {
        vec![
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
        let clear_color_fb_dl = Self::generate_clear_color_fb_dl(self.rsp_color_fb.as_ptr() as usize);

        let mut projection_mtx = Mtx { m: [[0; 4]; 4] };
        let projection_mtx_ptr = &mut projection_mtx as *mut Mtx;

        let mut modelview_mtx = Mtx { m: [[0; 4]; 4] };
        let modelview_mtx_ptr = &mut modelview_mtx as *mut Mtx;

        guOrtho(
            projection_mtx_ptr,
            -(SCREEN_WIDTH as f32) / 2.0,
            (SCREEN_WIDTH as f32) / 2.0,
            -(SCREEN_HEIGHT as f32) / 2.0,
            (SCREEN_HEIGHT as f32) / 2.0,
            1.0,
            10.0,
            1.0,
        );
        guRotate(modelview_mtx_ptr, self.frame_count, 0.0, 0.0, 1.0);

        let triangle_dl = Self::generate_triangle_dl(
            projection_mtx_ptr,
            modelview_mtx_ptr,
            &self.vertex_data,
        );

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
    fast3d_example::framework::run::<Example>("triangle", Some(LogicalSize { width: 800, height: 600 }));
}
