use glam::Vec4Swizzles;
use std::{borrow::Cow, collections::HashMap};

use wgpu::{BindGroup, RenderPipeline, ShaderModule};

use wgpu::util::{align_to, DeviceExt};

use crate::defines::{
    FragmentBlendUniforms, FragmentBlendWithFogUniforms, FragmentCombineUniforms,
    FragmentFrameUniforms, PipelineId, ShaderEntry, TextureData, VertexUniforms,
    VertexWithFogUniforms,
};
use crate::wgpu_program::ShaderVersion;
use fast3d::{
    gbi::defines::g,
    output::{
        gfx::{BlendFactor, BlendOperation, BlendState, CompareFunction, Face},
        models::{OutputFogParams, OutputSampler, OutputStencil, OutputTexture, OutputUniforms},
        ShaderConfig, ShaderId,
    },
    rdp::OutputDimensions,
};

use super::wgpu_program::WgpuProgram;

pub struct WgpuGraphicsDevice<'a> {
    depth_texture: wgpu::TextureView,

    shader_cache: rustc_hash::FxHashMap<ShaderId, ShaderEntry<'a>>,
    current_shader: Option<ShaderId>,

    pipeline_cache: rustc_hash::FxHashMap<PipelineId, RenderPipeline>,
    current_pipeline: Option<PipelineId>,

    textures: Vec<TextureData>,
    active_texture: usize,
    current_texture_ids: [usize; 2],

    frame_count: i32,
    current_height: i32,
}

impl<'a> WgpuGraphicsDevice<'a> {
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

    fn create_textures_bind_group(
        &self,
        program: &ShaderEntry,
        device: &wgpu::Device,
    ) -> Option<BindGroup> {
        let mut bind_group_entries = Vec::new();

        for i in 0..2 {
            let texture_index = format!("USE_TEXTURE{}", i);
            if program.program.get_define_bool(&texture_index) {
                bind_group_entries.push(wgpu::BindGroupEntry {
                    binding: i * 2,
                    resource: wgpu::BindingResource::TextureView(
                        // &texture_data[i as usize].texture_view,
                        &self.textures[self.current_texture_ids[i as usize]].texture_view,
                    ),
                });

                bind_group_entries.push(wgpu::BindGroupEntry {
                    binding: (i * 2 + 1),
                    resource: wgpu::BindingResource::Sampler(
                        &self.textures[self.current_texture_ids[i as usize]].sampler,
                    ),
                });
            }
        }

        if !bind_group_entries.is_empty() {
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &program.texture_bind_group_layout.as_ref().unwrap(),
                entries: &bind_group_entries,
                label: Some("Textures/Samplers Group"),
            });

            Some(bind_group)
        } else {
            None
        }
    }
}

impl<'a> WgpuGraphicsDevice<'a> {
    pub fn new(config: &wgpu::SurfaceConfiguration, device: &wgpu::Device) -> Self {
        // Create the depth texture
        let depth_texture = Self::create_depth_texture(config, device);

        Self {
            depth_texture,

            shader_cache: rustc_hash::FxHashMap::default(),
            current_shader: None,

            pipeline_cache: rustc_hash::FxHashMap::default(),
            current_pipeline: None,

            textures: Vec::new(),
            active_texture: 0,
            current_texture_ids: [0; 2],

            frame_count: 0,
            current_height: 0,
        }
    }

    pub fn resize(&mut self, config: &wgpu::SurfaceConfiguration, device: &wgpu::Device) {
        self.depth_texture = Self::create_depth_texture(config, device);
    }

    pub fn update_frame_count(&mut self) {
        self.frame_count += 1;
    }

    pub fn update_current_height(&mut self, height: i32) {
        self.current_height = height;
    }

    pub fn bind_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        tile: usize,
        texture: &mut OutputTexture,
        sampler: &OutputSampler,
    ) {
        // check if we've already uploaded this texture to the GPU
        if let Some(texture_id) = texture.device_id {
            self.active_texture = tile;
            self.current_texture_ids[tile] = texture_id as usize;

            return;
        }

        // Create device texture
        let texture_extent = wgpu::Extent3d {
            width: texture.width,
            height: texture.height,
            depth_or_array_layers: 1,
        };

        let device_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: texture_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Write data to the device texture
        let bytes_per_pixel = 4;
        let bytes_per_row = bytes_per_pixel * texture.width;

        queue.write_texture(
            device_texture.as_image_copy(),
            &texture.data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: None,
            },
            texture_extent,
        );

        // Create the texture
        let texture_view = device_texture.create_view(&wgpu::TextureViewDescriptor::default());

        self.active_texture = tile;
        self.current_texture_ids[tile] = self.textures.len();
        texture.device_id = Some(self.textures.len() as u32);

        // Create the sampler
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: None,
            address_mode_u: gfx_cm_to_wgpu(sampler.clamp_s),
            address_mode_v: gfx_cm_to_wgpu(sampler.clamp_t),
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: if sampler.linear_filter {
                wgpu::FilterMode::Linear
            } else {
                wgpu::FilterMode::Nearest
            },
            min_filter: if sampler.linear_filter {
                wgpu::FilterMode::Linear
            } else {
                wgpu::FilterMode::Nearest
            },
            ..Default::default()
        });

        self.textures.push(TextureData::new(texture_view, sampler));
    }

    pub fn select_program(
        &mut self,
        device: &wgpu::Device,
        shader_id: ShaderId,
        shader_config: ShaderConfig,
    ) {
        // check if the shader is already loaded
        if self.current_shader == Some(shader_id) {
            return;
        }

        // unload the current shader
        if self.current_shader != None {
            self.current_shader = None;
        }

        // check if the shader is in the cache
        if self.shader_cache.contains_key(&shader_id) {
            self.current_shader = Some(shader_id);
            return;
        }

        // create the shader and add it to the cache
        let mut program = WgpuProgram::new(shader_config);
        program.init();
        program.preprocess(&ShaderVersion::GLSL440);

        program.compiled_vertex_program =
            Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Glsl {
                    shader: Cow::Borrowed(&program.preprocessed_vertex),
                    stage: naga::ShaderStage::Vertex,
                    defines: program.defines.clone(),
                },
            }));

        program.compiled_fragment_program =
            Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Glsl {
                    shader: Cow::Borrowed(&program.preprocessed_frag),
                    stage: naga::ShaderStage::Fragment,
                    defines: program.defines.clone(),
                },
            }));

        self.current_shader = Some(shader_id);

        // create the shader entry
        let shader_entry = ShaderEntry::new(program, device);
        self.shader_cache.insert(shader_id, shader_entry);
    }

    pub fn update_uniforms(
        &mut self,
        queue: &wgpu::Queue,
        projection_matrix: glam::Mat4,
        fog: &OutputFogParams,
        uniforms: &OutputUniforms,
    ) {
        // Grab current program
        let program = self
            .shader_cache
            .get_mut(&self.current_shader.unwrap())
            .unwrap();

        // Update the vertex uniforms
        if program.program.get_define_bool("USE_FOG") {
            let uniform = VertexWithFogUniforms::new(
                projection_matrix.to_cols_array_2d(),
                fog.multiplier as f32,
                fog.offset as f32,
            );

            queue.write_buffer(&program.vertex_uniform_buf, 0, bytemuck::bytes_of(&uniform));
        } else {
            let uniform = VertexUniforms {
                projection_matrix: projection_matrix.to_cols_array_2d(),
            };

            queue.write_buffer(&program.vertex_uniform_buf, 0, bytemuck::bytes_of(&uniform));
        }

        // Update the blend uniforms
        if program.program.get_define_bool("USE_FOG") {
            let uniform = FragmentBlendWithFogUniforms::new(
                uniforms.blend.blend_color.to_array(),
                uniforms.blend.fog_color.xyz().to_array(),
            );

            queue.write_buffer(&program.blend_uniform_buf, 0, bytemuck::bytes_of(&uniform));
        } else {
            let uniform = FragmentBlendUniforms {
                blend_color: uniforms.blend.blend_color.to_array(),
            };

            queue.write_buffer(&program.blend_uniform_buf, 0, bytemuck::bytes_of(&uniform));
        }

        // Update the combine uniforms
        let uniform = FragmentCombineUniforms::new(
            uniforms.combine.prim_color.to_array(),
            uniforms.combine.env_color.to_array(),
            uniforms.combine.key_center.to_array(),
            uniforms.combine.key_scale.to_array(),
            uniforms.combine.prim_lod.x,
            uniforms.combine.convert_k4,
            uniforms.combine.convert_k5,
        );

        queue.write_buffer(
            &program.combine_uniform_buf,
            0,
            bytemuck::bytes_of(&uniform),
        );

        // Update the frame uniforms
        if let Some(frame_uniform_buf) = &program.frame_uniform_buf {
            let uniform = FragmentFrameUniforms {
                count: self.frame_count as u32,
                height: self.current_height as u32,
            };

            queue.write_buffer(frame_uniform_buf, 0, bytemuck::bytes_of(&uniform));
        }
    }

    pub fn configure_pipeline(
        &mut self,
        device: &wgpu::Device,
        surface_texture_format: wgpu::TextureFormat,
        pipeline_id: PipelineId,
        blend_state: Option<BlendState>,
        cull_mode: Option<Face>,
        depth_stencil: Option<OutputStencil>,
    ) {
        // Grab current program
        let program = self
            .shader_cache
            .get_mut(&self.current_shader.unwrap())
            .unwrap();

        // Check if we have a cached pipeline
        if self.pipeline_cache.contains_key(&pipeline_id) {
            return;
        }

        // Create the pipeline layout
        let mut bind_group_layout_entries = vec![
            &program.vertex_bind_group_layout,
            &program.fragment_uniform_bind_group_layout,
        ];

        if program.program.get_define_bool("USE_TEXTURE0")
            || program.program.get_define_bool("USE_TEXTURE0")
        {
            bind_group_layout_entries.push(&program.texture_bind_group_layout.as_ref().unwrap());
        }

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pipeline Layout"),
            bind_group_layouts: &bind_group_layout_entries,
            push_constant_ranges: &[],
        });

        // Create color target state
        let color_target_states = wgpu::ColorTargetState {
            format: surface_texture_format,
            blend: blend_state_to_wgpu(blend_state),
            write_mask: wgpu::ColorWrites::ALL,
        };

        // Depth stencil state
        let depth_stencil = depth_stencil.map(|ds| wgpu::DepthStencilState {
            format: Self::DEPTH_FORMAT,
            depth_write_enabled: ds.depth_write_enabled,
            depth_compare: compare_function_to_wgpu(ds.depth_compare),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState {
                constant: 0,
                slope_scale: if ds.polygon_offset { -2.0 } else { 0.0 },
                clamp: 0.0,
            },
        });

        // Create pipeline descriptor
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: program.program.compiled_vertex_program.as_ref().unwrap(),
                entry_point: "main",
                buffers: &[program.vertex_buf_layout.clone()],
            },
            fragment: Some(wgpu::FragmentState {
                module: program.program.compiled_fragment_program.as_ref().unwrap(),
                entry_point: "main",
                targets: &[Some(color_target_states)],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: face_to_wgpu(cull_mode),
                ..Default::default()
            },
            depth_stencil,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        self.pipeline_cache.insert(pipeline_id, pipeline);
    }

    pub fn draw_triangles(
        &mut self,
        draw_call_index: usize,
        view: &wgpu::TextureView,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        pipeline_id: PipelineId,
        output_size: &OutputDimensions,
        viewport: &glam::Vec4,
        scissor: [u32; 4],
        buf_vbo: &[u8],
        num_tris: usize,
    ) {
        // Grab current program
        let program = self
            .shader_cache
            .get(&self.current_shader.unwrap())
            .unwrap();

        // Render the triangles
        encoder.push_debug_group(&format!("draw triangle pass: {}", draw_call_index));

        {
            // Create the texture bind groups
            let textures_bind_group = self.create_textures_bind_group(program, device);

            // Copy the vertex data to the buffer
            let staging_vertex_buffer =
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Vertex Staging Buffer"),
                    contents: buf_vbo,
                    usage: wgpu::BufferUsages::COPY_SRC,
                });

            encoder.copy_buffer_to_buffer(
                &staging_vertex_buffer,
                0,
                &program.vertex_buf,
                0,
                buf_vbo.len() as u64,
            );

            // Create the render pass
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(&format!("Game Render Pass: {}", draw_call_index)),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: true,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_texture,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: true,
                    }),
                    stencil_ops: None,
                }),
            });

            pass.push_debug_group("Prepare data for draw.");

            // When we manage to use one rpass for all draw calls, we can add this check
            // if pipeline_id != self.current_pipeline {
            //     self.current_pipeline = pipeline_id;
            let pipeline = self.pipeline_cache.get(&pipeline_id).unwrap();
            pass.set_pipeline(pipeline);
            // }

            pass.set_bind_group(0, &program.vertex_bind_group, &[]);
            pass.set_bind_group(1, &program.fragment_uniform_bind_group, &[]);
            if let Some(texture_bind_group) = &textures_bind_group {
                pass.set_bind_group(2, texture_bind_group, &[]);
            }
            pass.set_vertex_buffer(0, program.vertex_buf.slice(..));

            let wgpu_y = output_size.height as f32 - viewport.y - viewport.w;
            pass.set_viewport(viewport.x, wgpu_y, viewport.z, viewport.w, 0.0, 1.0);

            let wgpu_y = output_size.height - scissor[1] - scissor[3];
            pass.set_scissor_rect(scissor[0], wgpu_y, scissor[2], scissor[3]);

            pass.pop_debug_group();
            pass.insert_debug_marker("Draw!");
            pass.draw(0..(num_tris * 3) as u32, 0..1);
        }

        encoder.pop_debug_group();
    }
}

fn gfx_cm_to_wgpu(val: u32) -> wgpu::AddressMode {
    if val & g::tx::CLAMP as u32 != 0 {
        return wgpu::AddressMode::ClampToEdge;
    }

    if val & g::tx::MIRROR as u32 != 0 {
        return wgpu::AddressMode::MirrorRepeat;
    }

    wgpu::AddressMode::Repeat
}

fn face_to_wgpu(face: Option<Face>) -> Option<wgpu::Face> {
    face.map(|face| match face {
        Face::Front => wgpu::Face::Front,
        Face::Back => wgpu::Face::Back,
    })
}

fn compare_function_to_wgpu(func: CompareFunction) -> wgpu::CompareFunction {
    match func {
        CompareFunction::Never => wgpu::CompareFunction::Never,
        CompareFunction::Less => wgpu::CompareFunction::Less,
        CompareFunction::Equal => wgpu::CompareFunction::Equal,
        CompareFunction::LessEqual => wgpu::CompareFunction::LessEqual,
        CompareFunction::Greater => wgpu::CompareFunction::Greater,
        CompareFunction::NotEqual => wgpu::CompareFunction::NotEqual,
        CompareFunction::GreaterEqual => wgpu::CompareFunction::GreaterEqual,
        CompareFunction::Always => wgpu::CompareFunction::Always,
    }
}

fn blend_state_to_wgpu(state: Option<BlendState>) -> Option<wgpu::BlendState> {
    state.map(|state| wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: blend_factor_to_wgpu(state.color.src_factor),
            dst_factor: blend_factor_to_wgpu(state.color.dst_factor),
            operation: blend_op_to_wgpu(state.color.operation),
        },
        alpha: wgpu::BlendComponent {
            src_factor: blend_factor_to_wgpu(state.alpha.src_factor),
            dst_factor: blend_factor_to_wgpu(state.alpha.dst_factor),
            operation: blend_op_to_wgpu(state.alpha.operation),
        },
    })
}

fn blend_factor_to_wgpu(factor: BlendFactor) -> wgpu::BlendFactor {
    match factor {
        BlendFactor::Zero => wgpu::BlendFactor::Zero,
        BlendFactor::One => wgpu::BlendFactor::One,
        BlendFactor::Src => wgpu::BlendFactor::Src,
        BlendFactor::OneMinusSrc => wgpu::BlendFactor::OneMinusSrc,
        BlendFactor::SrcAlpha => wgpu::BlendFactor::SrcAlpha,
        BlendFactor::OneMinusSrcAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
        BlendFactor::Dst => wgpu::BlendFactor::Dst,
        BlendFactor::OneMinusDst => wgpu::BlendFactor::OneMinusDst,
        BlendFactor::DstAlpha => wgpu::BlendFactor::DstAlpha,
        BlendFactor::OneMinusDstAlpha => wgpu::BlendFactor::OneMinusDstAlpha,
        BlendFactor::SrcAlphaSaturated => wgpu::BlendFactor::SrcAlphaSaturated,
        BlendFactor::Constant => wgpu::BlendFactor::Constant,
        BlendFactor::OneMinusConstant => wgpu::BlendFactor::OneMinusConstant,
    }
}

fn blend_op_to_wgpu(op: BlendOperation) -> wgpu::BlendOperation {
    match op {
        BlendOperation::Add => wgpu::BlendOperation::Add,
        BlendOperation::Subtract => wgpu::BlendOperation::Subtract,
        BlendOperation::ReverseSubtract => wgpu::BlendOperation::ReverseSubtract,
        BlendOperation::Min => wgpu::BlendOperation::Min,
        BlendOperation::Max => wgpu::BlendOperation::Max,
    }
}
