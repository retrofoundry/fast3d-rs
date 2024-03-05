mod resources;

use fast3d::rdp::{OutputDimensions, SCREEN_HEIGHT, SCREEN_WIDTH};
use fast3d::{RenderData, RCP};
use fast3d_gbi::defines::f3dex2::{GeometryModes, MatrixMode, MatrixOperation};
use fast3d_gbi::defines::{
    AlphaCompare, ColorDither, CombineParams, ComponentSize, CycleType,
    GeometryModes as SharedGeometryModes, GfxCommand, ImageFormat, Matrix, PipelineMode,
    ScissorMode, TextureConvert, TextureDetail, TextureFilter, TextureLOD, TextureLUT,
    TextureShift, TextureTile, Vertex, Viewport, WrapMode, G_MAXZ,
};
use fast3d_gbi::dma::{gsSPDisplayList, gsSPMatrix, gsSPViewport};
use fast3d_gbi::gbi::{
    gsDPFullSync, gsDPPipeSync, gsSPEndDisplayList, GPACK_RGBA5551, G_RM_AA_OPA_SURF,
    G_RM_AA_OPA_SURF2, G_RM_OPA_SURF, G_RM_OPA_SURF2,
};
use fast3d_gbi::gu::{guOrtho, guRotate};
use fast3d_gbi::rdp::{gsDPFillRectangle, gsDPSetColorImage, gsDPSetFillColor, gsDPSetScissor};
use fast3d_gbi::rsp::{
    gsDPLoadTextureBlock, gsDPPipelineMode, gsDPSetAlphaCompare, gsDPSetColorDither,
    gsDPSetCombineKey, gsDPSetCombineMode, gsDPSetCycleType, gsDPSetRenderMode,
    gsDPSetTextureConvert, gsDPSetTextureDetail, gsDPSetTextureFilter, gsDPSetTextureLOD,
    gsDPSetTextureLUT, gsDPSetTexturePersp, gsSP1Triangle, gsSPClearGeometryMode,
    gsSPSetGeometryMode, gsSPTexture, gsSPVertex,
};
use fast3d_wgpu::WgpuRenderer;
use resources::{BRICK_TEX, SHADE_VTX, TEX_VTX};
use std::iter;
use wgpu::StoreOp;
use winit::event::KeyEvent;
use winit::keyboard::{Key, NamedKey};

enum RenderMode {
    Shade,
    Texture,
}

struct Example<'a> {
    rcp: RCP,
    render_data: RenderData,
    renderer: WgpuRenderer<'a>,

    depth_view: wgpu::TextureView,
    surface_format: wgpu::TextureFormat,

    viewport: Viewport,
    frame_count: f32,
    rsp_color_fb: [u16; (SCREEN_WIDTH * SCREEN_HEIGHT) as usize],

    render_mode: RenderMode,
}

impl<'a> Example<'a> {
    const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

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
            format: Self::DEPTH_FORMAT,
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
            gsDPSetFillColor(GPACK_RGBA5551(64, 64, 255, 1) << 16 | GPACK_RGBA5551(64, 64, 255, 1)),
            gsDPFillRectangle(20, 20, SCREEN_WIDTH as u32 - 20, SCREEN_HEIGHT as u32 - 20),
            gsSPEndDisplayList(),
        ]
    }

    fn generate_shaded_triangle_dl(
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

    fn generate_tex_triangle_dl(
        projection_mtx_ptr: *mut Matrix,
        modelview_mtx_ptr: *mut Matrix,
        triangle_vertices: &[Vertex],
    ) -> Vec<GfxCommand> {
        let mut commands = vec![
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
            gsSPClearGeometryMode(
                SharedGeometryModes::SHADE.bits() | GeometryModes::SHADING_SMOOTH.bits(),
            ),
            gsSPTexture(0x8000, 0x8000, 0, TextureTile::RENDERTILE, true),
            gsDPSetCombineMode(CombineParams::DECAL_RGB),
            gsDPSetTextureFilter(TextureFilter::Bilerp.raw_gbi_value()),
        ];

        commands.extend(gsDPLoadTextureBlock(
            BRICK_TEX.as_ptr() as usize,
            ImageFormat::Rgba,
            ComponentSize::Bits16,
            32,
            32,
            0,
            WrapMode::MirrorRepeat,
            WrapMode::MirrorRepeat,
            5,
            5,
            TextureShift::NOLOD,
            TextureShift::NOLOD,
        ));

        commands.extend(vec![
            gsSPVertex(triangle_vertices, 4, 0),
            gsSP1Triangle(0, 1, 2, 0),
            gsSP1Triangle(0, 2, 3, 0),
            gsSPTexture(0, 0, 0, TextureTile::RENDERTILE, false),
            gsSPEndDisplayList(),
        ]);

        commands
    }
}

impl crate::framework::Example for Example<'static> {
    fn init(
        config: &wgpu::SurfaceConfiguration,
        _adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
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
            render_data: RenderData::default(),
            renderer: WgpuRenderer::new(device, [config.width, config.height]),

            depth_view: Self::create_depth_texture(config, device),
            surface_format: config.format,

            viewport,
            frame_count: 0.0,
            rsp_color_fb: [0; (SCREEN_WIDTH * SCREEN_HEIGHT) as usize],
            render_mode: RenderMode::Shade,
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

    fn update(&mut self, event: winit::event::WindowEvent) {
        if let winit::event::WindowEvent::KeyboardInput {
            event:
                KeyEvent {
                    logical_key: Key::Named(NamedKey::Space),
                    ..
                },
            ..
        } = event
        {
            self.render_mode = match self.render_mode {
                RenderMode::Shade => RenderMode::Texture,
                RenderMode::Texture => RenderMode::Shade,
            };
        }
    }

    fn render(&mut self, view: &wgpu::TextureView, device: &wgpu::Device, queue: &wgpu::Queue) {
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

        let triangle_dl = match self.render_mode {
            RenderMode::Shade => {
                Self::generate_shaded_triangle_dl(projection_mtx_ptr, modelview_mtx_ptr, &SHADE_VTX)
            }
            RenderMode::Texture => {
                Self::generate_tex_triangle_dl(projection_mtx_ptr, modelview_mtx_ptr, &TEX_VTX)
            }
        };

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

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Game Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: StoreOp::Discard,
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
                device,
                queue,
                self.surface_format,
                &mut self.render_data,
            );

            // Draw the RCP output
            self.renderer.draw(&mut pass);
        }

        // Clear the draw calls
        self.render_data.clear_draw_calls();

        queue.submit(iter::once(encoder.finish()));
    }
}

pub fn main() {
    crate::framework::run::<Example>("triangle");
}
