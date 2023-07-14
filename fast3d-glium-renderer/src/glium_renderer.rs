use std::borrow::Cow;

use crate::opengl_program::ShaderVersion;
use fast3d::output::{ShaderConfig, ShaderId};
use fast3d::{
    output::{
        gfx::{BlendComponent, BlendFactor, BlendOperation, BlendState, CompareFunction, Face},
        models::{OutputFogParams, OutputSampler, OutputStencil, OutputTexture, OutputUniforms},
    },
    RenderData,
};
use fast3d_gbi::defines::WrapMode;
use glam::Vec4Swizzles;
use glium::buffer::{Buffer, BufferAny, BufferMode, BufferType};
use glium::{
    draw_parameters::{DepthClamp, PolygonOffset},
    implement_uniform_block, implement_vertex,
    index::{NoIndices, PrimitiveType},
    program::ProgramCreationInput,
    texture::{RawImage2d, Texture2d},
    uniforms::{
        MagnifySamplerFilter, MinifySamplerFilter, SamplerBehavior, SamplerWrapFunction,
        UniformValue, Uniforms,
    },
    vertex::{AttributeType, VertexBufferAny},
    BackfaceCullingMode, BlendingFunction, DepthTest, Display, DrawParameters, Frame,
    LinearBlendingFactor, Program, Surface, VertexBuffer,
};

use super::opengl_program::OpenGLProgram;

struct TextureData {
    texture: Texture2d,
    sampler: Option<SamplerBehavior>,
}

impl TextureData {
    pub fn new(texture: Texture2d) -> Self {
        Self {
            texture,
            sampler: None,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
struct VertexUniforms {
    screen_size: [f32; 2],
    _padding: [f32; 2],
    projection: [[f32; 4]; 4],
}

impl VertexUniforms {
    pub fn new(screen_size: [f32; 2], projection: [[f32; 4]; 4]) -> Self {
        Self {
            screen_size,
            _padding: [0.0; 2],
            projection,
        }
    }
}

implement_uniform_block!(VertexUniforms, screen_size, projection);

#[repr(C)]
#[derive(Copy, Clone)]
struct VertexWithFogUniforms {
    screen_size: [f32; 2],
    _padding: [f32; 2],
    projection: [[f32; 4]; 4],
    fog_multiplier: f32,
    fog_offset: f32,
}

impl VertexWithFogUniforms {
    pub fn new(
        screen_size: [f32; 2],
        projection: [[f32; 4]; 4],
        fog_multiplier: f32,
        fog_offset: f32,
    ) -> Self {
        Self {
            screen_size,
            _padding: [0.0; 2],
            projection,
            fog_multiplier,
            fog_offset,
        }
    }
}

implement_uniform_block!(
    VertexWithFogUniforms,
    screen_size,
    projection,
    fog_multiplier,
    fog_offset
);

#[repr(C)]
#[derive(Copy, Clone)]
struct BlendUniforms {
    blend_color: [f32; 4],
}

implement_uniform_block!(BlendUniforms, blend_color);

#[repr(C)]
#[derive(Copy, Clone)]
struct BlendWithFogUniforms {
    blend_color: [f32; 4],
    fog_color: [f32; 3],
    _padding: f32,
}

implement_uniform_block!(BlendWithFogUniforms, blend_color, fog_color);

#[repr(C)]
#[derive(Copy, Clone)]
struct CombineUniforms {
    prim_color: [f32; 4],
    env_color: [f32; 4],
    key_center: [f32; 3],
    _padding: f32,
    key_scale: [f32; 3],
    _padding2: f32,
    prim_lod_frac: f32,
    uk4: f32,
    uk5: f32,
}

implement_uniform_block!(
    CombineUniforms,
    prim_color,
    env_color,
    key_center,
    key_scale,
    prim_lod_frac,
    uk4,
    uk5
);

#[repr(C)]
#[derive(Copy, Clone)]
struct FrameUniforms {
    frame_count: i32,
    frame_height: i32,
}

implement_uniform_block!(FrameUniforms, frame_count, frame_height);

#[repr(C)]
#[derive(Copy, Clone)]
struct Vertex {
    position: [f32; 4],
    color: [f32; 4],
}

implement_vertex!(Vertex, position location(0), color location(1));

#[repr(C)]
#[derive(Copy, Clone)]
struct VertexWithTexture {
    position: [f32; 4],
    color: [f32; 4],
    tex_coord: [f32; 2],
}

implement_vertex!(VertexWithTexture, position location(0), color location(1), tex_coord location(2));

#[derive(Default)]
struct UniformVec<'a, 'b> {
    pub uniforms: Vec<(&'a str, UniformValue<'b>)>,
}

impl Uniforms for UniformVec<'_, '_> {
    fn visit_values<'a, F: FnMut(&str, UniformValue<'a>)>(&'a self, mut func: F) {
        for uniform in &self.uniforms {
            func(uniform.0, uniform.1);
        }
    }
}

pub struct GliumRenderer<'a> {
    pub shader_cache: rustc_hash::FxHashMap<ShaderId, OpenGLProgram<Program>>,
    current_shader: Option<ShaderId>,

    textures: Vec<TextureData>,
    active_texture: usize,
    current_texture_ids: [usize; 2],

    frame_count: i32,
    current_height: i32,
    screen_size: [u32; 2],

    draw_params: DrawParameters<'a>,
}

fn blend_component_to_glium(component: BlendComponent) -> BlendingFunction {
    match component.operation {
        BlendOperation::Add => BlendingFunction::Addition {
            source: blend_factor_to_glium(component.src_factor),
            destination: blend_factor_to_glium(component.dst_factor),
        },
        BlendOperation::Subtract => BlendingFunction::Subtraction {
            source: blend_factor_to_glium(component.src_factor),
            destination: blend_factor_to_glium(component.dst_factor),
        },
        BlendOperation::ReverseSubtract => BlendingFunction::ReverseSubtraction {
            source: blend_factor_to_glium(component.src_factor),
            destination: blend_factor_to_glium(component.dst_factor),
        },
        BlendOperation::Min => BlendingFunction::Min,
        BlendOperation::Max => BlendingFunction::Max,
    }
}

fn blend_factor_to_glium(factor: BlendFactor) -> LinearBlendingFactor {
    match factor {
        BlendFactor::Zero => LinearBlendingFactor::Zero,
        BlendFactor::One => LinearBlendingFactor::One,
        BlendFactor::Src => LinearBlendingFactor::SourceColor,
        BlendFactor::OneMinusSrc => LinearBlendingFactor::OneMinusSourceColor,
        BlendFactor::SrcAlpha => LinearBlendingFactor::SourceAlpha,
        BlendFactor::OneMinusSrcAlpha => LinearBlendingFactor::OneMinusSourceAlpha,
        BlendFactor::Dst => LinearBlendingFactor::DestinationColor,
        BlendFactor::OneMinusDst => LinearBlendingFactor::OneMinusDestinationColor,
        BlendFactor::DstAlpha => LinearBlendingFactor::DestinationAlpha,
        BlendFactor::OneMinusDstAlpha => LinearBlendingFactor::OneMinusDestinationAlpha,
        BlendFactor::SrcAlphaSaturated => LinearBlendingFactor::SourceAlphaSaturate,
        BlendFactor::Constant => LinearBlendingFactor::ConstantColor,
        BlendFactor::OneMinusConstant => LinearBlendingFactor::OneMinusConstantColor,
    }
}

fn clamp_to_glium(clamp: WrapMode) -> SamplerWrapFunction {
    if clamp == WrapMode::Clamp {
        return SamplerWrapFunction::Clamp;
    } else if clamp == WrapMode::MirrorRepeat {
        return SamplerWrapFunction::Mirror;
    }

    SamplerWrapFunction::Repeat
}

impl<'a> GliumRenderer<'a> {
    pub fn new(screen_size: [u32; 2]) -> Self {
        Self {
            shader_cache: rustc_hash::FxHashMap::default(),
            current_shader: None,

            textures: Vec::new(),
            active_texture: 0,
            current_texture_ids: [0; 2],

            frame_count: 0,
            current_height: 0,
            screen_size,

            draw_params: DrawParameters {
                ..Default::default()
            },
        }
    }

    /// Starts a new frame.
    /// Should be called before any drawing is done.
    pub fn start_frame(&mut self, target: &mut Frame) {
        self.frame_count += 1;

        target.clear_color_and_depth((0.0, 0.0, 0.0, 1.0), 1.0);

        self.draw_params = DrawParameters {
            ..Default::default()
        };
    }

    /// Sets the screen size for rendering.
    ///
    /// The easiest way of doing this is to take every [`WindowEvent::Resized`]
    /// that is received and pass its [`dpi::PhysicalSize`] into this function.
    pub fn resize(&mut self, screen_size: [u32; 2]) {
        self.screen_size = screen_size;
    }

    /// Render the contents of a given [`RenderData`] to the screen.
    pub fn render_rcp_output(
        &mut self,
        render_data: &mut RenderData,
        display: &Display,
        frame: &mut Frame,
    ) {
        // omit the last draw call, because we know we that's an extra from the last flush
        // for draw_call in &self.rcp_output.draw_calls[..self.rcp_output.draw_calls.len() - 1] {
        for draw_call in render_data.draw_calls.iter().take(render_data.draw_calls.len() - 1) {
            assert!(!draw_call.vbo.vbo.is_empty());

            self.set_cull_mode(draw_call.cull_mode);
            self.set_depth_stencil_params(draw_call.stencil);
            self.set_blend_state(draw_call.blend_state);
            self.set_viewport(&draw_call.viewport);
            self.set_scissor(draw_call.scissor);

            // select the shader program
            self.select_program(display, draw_call.shader_id, draw_call.shader_config);

            // loop through textures and bind them
            for (index, hash) in draw_call.texture_indices.iter().enumerate() {
                if let Some(hash) = hash {
                    let texture = render_data.texture_cache.get_mut(*hash).unwrap();
                    self.bind_texture(display, index, texture);
                }
            }

            // loop through samplers and bind them
            for (index, sampler) in draw_call.samplers.iter().enumerate() {
                if let Some(sampler) = sampler {
                    self.bind_sampler(index, sampler);
                }
            }

            // draw triangles
            self.draw_triangles(
                display,
                frame,
                draw_call.projection_matrix,
                &draw_call.fog,
                &draw_call.vbo.vbo,
                draw_call.vbo.num_tris,
                &draw_call.uniforms,
            );
        }
    }

    // MARK: - Helpers

    fn set_cull_mode(&mut self, cull_mode: Option<Face>) {
        self.draw_params.backface_culling = match cull_mode {
            Some(Face::Front) => BackfaceCullingMode::CullCounterClockwise,
            Some(Face::Back) => BackfaceCullingMode::CullClockwise,
            None => BackfaceCullingMode::CullingDisabled,
        }
    }

    fn set_depth_stencil_params(&mut self, params: Option<OutputStencil>) {
        self.draw_params.depth = if let Some(params) = params {
            glium::Depth {
                test: match params.depth_compare {
                    CompareFunction::Never => DepthTest::Ignore,
                    CompareFunction::Less => DepthTest::IfLess,
                    CompareFunction::Equal => DepthTest::IfEqual,
                    CompareFunction::LessEqual => DepthTest::IfLessOrEqual,
                    CompareFunction::Greater => DepthTest::IfMore,
                    CompareFunction::NotEqual => DepthTest::IfNotEqual,
                    CompareFunction::GreaterEqual => DepthTest::IfMoreOrEqual,
                    CompareFunction::Always => DepthTest::Overwrite,
                },
                write: params.depth_write_enabled,
                clamp: DepthClamp::Clamp,
                ..Default::default()
            }
        } else {
            glium::Depth {
                clamp: DepthClamp::Clamp,
                ..Default::default()
            }
        };

        self.draw_params.polygon_offset = if let Some(params) = params {
            PolygonOffset {
                factor: if params.polygon_offset { -2.0 } else { 0.0 },
                units: if params.polygon_offset { 2.0 } else { 0.0 },
                fill: true,
                ..Default::default()
            }
        } else {
            PolygonOffset {
                ..Default::default()
            }
        };
    }

    fn set_blend_state(&mut self, blend_state: Option<BlendState>) {
        self.draw_params.blend = if let Some(blend_state) = blend_state {
            glium::Blend {
                color: blend_component_to_glium(blend_state.color),
                alpha: blend_component_to_glium(blend_state.alpha),
                ..Default::default()
            }
        } else {
            glium::Blend {
                ..Default::default()
            }
        };
    }

    fn set_viewport(&mut self, viewport: &glam::Vec4) {
        self.draw_params.viewport = Some(glium::Rect {
            left: viewport.x as u32,
            bottom: viewport.y as u32,
            width: viewport.z as u32,
            height: viewport.w as u32,
        });

        self.current_height = viewport.w as i32;
    }

    fn set_scissor(&mut self, scissor: [u32; 4]) {
        self.draw_params.scissor = Some(glium::Rect {
            left: scissor[0],
            bottom: scissor[1],
            width: scissor[2],
            height: scissor[3],
        });
    }

    fn select_program(
        &mut self,
        display: &Display,
        shader_id: ShaderId,
        shader_config: ShaderConfig,
    ) {
        // check if the shader is already loaded
        if self.current_shader == Some(shader_id) {
            return;
        }

        // unload the current shader
        if self.current_shader.is_some() {
            self.current_shader = None;
        }

        // check if the shader is in the cache
        if self.shader_cache.contains_key(&shader_id) {
            self.current_shader = Some(shader_id);
            return;
        }

        // create the shader and add it to the cache
        let mut program = OpenGLProgram::new(shader_config);
        program.init();
        program.preprocess(&ShaderVersion::GLSL410); // 410 is latest version supported by macOS

        let source = ProgramCreationInput::SourceCode {
            vertex_shader: &program.preprocessed_vertex,
            fragment_shader: &program.preprocessed_frag,
            geometry_shader: None,
            tessellation_control_shader: None,
            tessellation_evaluation_shader: None,
            transform_feedback_varyings: None,
            outputs_srgb: true, // workaround to avoid glium doing gamma correction
            uses_point_size: false,
        };

        program.compiled_program = Some(Program::new(display, source).unwrap());

        self.current_shader = Some(shader_id);
        self.shader_cache.insert(shader_id, program);
    }

    fn bind_texture(&mut self, display: &Display, tile: usize, texture: &mut OutputTexture) {
        // check if we've already uploaded this texture to the GPU
        if let Some(texture_id) = texture.device_id {
            // trace!("Texture found in GPU cache");
            self.active_texture = tile;
            self.current_texture_ids[tile] = texture_id as usize;

            return;
        }

        // Create the texture
        let raw_texture =
            RawImage2d::from_raw_rgba(texture.data.clone(), (texture.width, texture.height));
        let native_texture = Texture2d::new(display, raw_texture).unwrap();

        self.active_texture = tile;
        self.current_texture_ids[tile] = self.textures.len();
        texture.device_id = Some(self.textures.len() as u32);

        self.textures.push(TextureData::new(native_texture));
    }

    fn bind_sampler(&mut self, tile: usize, sampler: &OutputSampler) {
        if let Some(texture_data) = self.textures.get_mut(self.current_texture_ids[tile]) {
            let wrap_s = clamp_to_glium(sampler.clamp_s);
            let wrap_t = clamp_to_glium(sampler.clamp_t);

            let native_sampler = SamplerBehavior {
                minify_filter: if sampler.linear_filter {
                    MinifySamplerFilter::Linear
                } else {
                    MinifySamplerFilter::Nearest
                },
                magnify_filter: if sampler.linear_filter {
                    MagnifySamplerFilter::Linear
                } else {
                    MagnifySamplerFilter::Nearest
                },
                wrap_function: (wrap_s, wrap_t, SamplerWrapFunction::Repeat),
                ..Default::default()
            };

            texture_data.sampler = Some(native_sampler);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_triangles(
        &self,
        display: &Display,
        target: &mut Frame,
        projection_matrix: glam::Mat4,
        fog: &OutputFogParams,
        vbo: &[u8],
        num_tris: usize,
        uniforms: &OutputUniforms,
    ) {
        // Grab current program
        let program = self
            .shader_cache
            .get(&self.current_shader.unwrap())
            .unwrap();

        // Setup vertex buffer
        let mut vertex_format_data = vec![
            (
                Cow::Borrowed("aVtxPos"),
                0,
                -1,
                AttributeType::F32F32F32F32,
                false,
            ),
            (
                Cow::Borrowed("aVtxColor"),
                4 * ::std::mem::size_of::<f32>(),
                -1,
                AttributeType::F32F32F32F32,
                false,
            ),
        ];

        if program.get_define_bool("USE_TEXTURE0") || program.get_define_bool("USE_TEXTURE1") {
            vertex_format_data.push((
                Cow::Borrowed("aTexCoord"),
                8 * ::std::mem::size_of::<f32>(),
                -1,
                AttributeType::F32F32,
                false,
            ));
        }

        let vertex_buffer = if program.get_define_bool("USE_TEXTURE0")
            || program.get_define_bool("USE_TEXTURE1")
        {
            let vertex_array = unsafe {
                std::slice::from_raw_parts(vbo.as_ptr() as *const VertexWithTexture, num_tris * 3)
            };
            let buffer = VertexBuffer::new(display, vertex_array).unwrap();
            VertexBufferAny::from(buffer)
        } else {
            let vertex_array =
                unsafe { std::slice::from_raw_parts(vbo.as_ptr() as *const Vertex, num_tris * 3) };
            let buffer = VertexBuffer::new(display, vertex_array).unwrap();
            VertexBufferAny::from(buffer)
        };

        // Setup uniforms

        let vtx_uniform_buf = if program.get_define_bool("USE_FOG") {
            let data = VertexWithFogUniforms::new(
                [self.screen_size[0] as f32, self.screen_size[1] as f32],
                projection_matrix.to_cols_array_2d(),
                fog.multiplier as f32,
                fog.offset as f32,
            );

            let buffer = Buffer::new(
                display,
                &data,
                BufferType::UniformBuffer,
                BufferMode::Default,
            )
            .unwrap();
            BufferAny::from(buffer)
        } else {
            let data = VertexUniforms::new(
                [self.screen_size[0] as f32, self.screen_size[1] as f32],
                projection_matrix.to_cols_array_2d(),
            );

            let buffer = Buffer::new(
                display,
                &data,
                BufferType::UniformBuffer,
                BufferMode::Default,
            )
            .unwrap();
            BufferAny::from(buffer)
        };

        let blend_uniform_buf = if program.get_define_bool("USE_FOG") {
            let data = BlendWithFogUniforms {
                blend_color: uniforms.blend.blend_color.to_array(),
                fog_color: uniforms.blend.fog_color.xyz().to_array(),
                _padding: 0.0,
            };

            let buffer = Buffer::new(
                display,
                &data,
                BufferType::UniformBuffer,
                BufferMode::Default,
            )
            .unwrap();
            BufferAny::from(buffer)
        } else {
            let data = BlendUniforms {
                blend_color: uniforms.blend.blend_color.to_array(),
            };

            let buffer = Buffer::new(
                display,
                &data,
                BufferType::UniformBuffer,
                BufferMode::Default,
            )
            .unwrap();
            BufferAny::from(buffer)
        };

        let combine_uniform_buf = {
            let data = CombineUniforms {
                prim_color: uniforms.combine.prim_color.to_array(),
                env_color: uniforms.combine.env_color.to_array(),
                _padding: 0.0,
                key_center: uniforms.combine.key_center.to_array(),
                key_scale: uniforms.combine.key_scale.to_array(),
                _padding2: 0.0,
                prim_lod_frac: uniforms.combine.prim_lod.x,
                uk4: uniforms.combine.convert_k4,
                uk5: uniforms.combine.convert_k5,
            };

            let buffer = Buffer::new(
                display,
                &data,
                BufferType::UniformBuffer,
                BufferMode::Default,
            )
            .unwrap();
            BufferAny::from(buffer)
        };

        let frame_uniform_buf = if program.get_define_bool("USE_ALPHA")
            && program.get_define_bool("ALPHA_COMPARE_DITHER")
        {
            let data = FrameUniforms {
                frame_count: self.frame_count,
                frame_height: self.current_height,
            };

            Some(
                Buffer::new(
                    display,
                    &data,
                    BufferType::UniformBuffer,
                    BufferMode::Default,
                )
                .unwrap(),
            )
        } else {
            None
        };

        // Setup uniforms
        let mut shader_uniforms = vec![
            (
                "Uniforms",
                UniformValue::Block(vtx_uniform_buf.as_slice_any(), |_block| Ok(())),
            ),
            (
                "BlendUniforms",
                UniformValue::Block(blend_uniform_buf.as_slice_any(), |_block| Ok(())),
            ),
            (
                "CombineUniforms",
                UniformValue::Block(combine_uniform_buf.as_slice_any(), |_block| Ok(())),
            ),
        ];

        if program.get_define_bool("USE_TEXTURE0") {
            let texture = self.textures.get(self.current_texture_ids[0]).unwrap();
            shader_uniforms.push((
                "uTex0",
                UniformValue::Texture2d(&texture.texture, texture.sampler),
            ));
        }

        if program.get_define_bool("USE_TEXTURE1") {
            let texture = self.textures.get(self.current_texture_ids[1]).unwrap();
            shader_uniforms.push((
                "uTex1",
                UniformValue::Texture2d(&texture.texture, texture.sampler),
            ));
        }

        if program.get_define_bool("USE_ALPHA") && program.get_define_bool("ALPHA_COMPARE_DITHER") {
            let frame_uniform_buf = frame_uniform_buf.as_ref();
            shader_uniforms.push((
                "FrameUniforms",
                UniformValue::Block(frame_uniform_buf.unwrap().as_slice_any(), |_block| Ok(())),
            ));
        }

        // Draw triangles
        target
            .draw(
                &vertex_buffer,
                NoIndices(PrimitiveType::TrianglesList),
                program.compiled_program.as_ref().unwrap(),
                &UniformVec {
                    uniforms: shader_uniforms,
                },
                &self.draw_params,
            )
            .unwrap();
    }
}
