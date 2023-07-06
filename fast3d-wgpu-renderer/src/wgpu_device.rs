use glam::Vec4Swizzles;
use std::borrow::Cow;
use wgpu::util::DeviceExt;

use crate::defines::{
    FragmentBlendUniforms, FragmentBlendWithFogUniforms, FragmentCombineUniforms,
    FragmentFrameUniforms, PipelineConfig, PipelineId, ShaderEntry, TextureData, VertexUniforms,
    VertexWithFogUniforms,
};
use crate::wgpu_program::ShaderVersion;
use fast3d::output::{RCPOutput};
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

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub struct WgpuDrawCall {
    pub shader_id: ShaderId,
    pub pipeline_id: PipelineId,
    pub textures: [Option<usize>; 2],

    pub vertex_buffer: wgpu::Buffer,
    pub vertex_count: usize,

    pub vertex_uniform_buf: wgpu::Buffer,
    pub vertex_uniform_bind_group: wgpu::BindGroup,
    pub blend_uniform_buf: wgpu::Buffer,
    pub combine_uniform_buf: wgpu::Buffer,
    pub frame_uniform_buf: Option<wgpu::Buffer>,
    pub fragment_uniform_bind_group: wgpu::BindGroup,

    pub viewport: glam::Vec4,
    pub scissor: [u32; 4],
}

// create a from to convert from RCPOutput to WgpuDrawCall
impl WgpuDrawCall {
    fn new(
        shader_id: ShaderId,
        pipeline_id: PipelineId,
        vertex_buffer: wgpu::Buffer,
        vertex_uniform_buf: wgpu::Buffer,
        vertex_uniform_bind_group: wgpu::BindGroup,
        blend_uniform_buf: wgpu::Buffer,
        combine_uniform_buf: wgpu::Buffer,
        frame_uniform_buf: Option<wgpu::Buffer>,
        fragment_uniform_bind_group: wgpu::BindGroup,
        viewport: glam::Vec4,
        scissor: [u32; 4],
    ) -> Self {
        Self {
            shader_id,
            pipeline_id,
            textures: [None; 2],

            vertex_buffer,
            vertex_count: 0,

            vertex_uniform_buf,
            vertex_uniform_bind_group,
            blend_uniform_buf,
            combine_uniform_buf,
            frame_uniform_buf,
            fragment_uniform_bind_group,

            viewport,
            scissor,
        }
    }
}

pub struct WgpuGraphicsDevice<'a> {
    frame_count: i32,
    current_height: i32,

    texture_cache: Vec<TextureData>,
    shader_cache: rustc_hash::FxHashMap<ShaderId, ShaderEntry<'a>>,
    pipeline_cache: rustc_hash::FxHashMap<PipelineId, wgpu::RenderPipeline>,

    texture_bind_group_layout: wgpu::BindGroupLayout,
    texture_bind_groups: rustc_hash::FxHashMap<usize, wgpu::BindGroup>,

    draw_calls: Vec<WgpuDrawCall>,

    last_pipeline_id: Option<PipelineId>,
}

impl<'a> WgpuGraphicsDevice<'a> {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        Self {
            frame_count: 0,
            current_height: 0,

            texture_cache: Vec::new(),
            shader_cache: rustc_hash::FxHashMap::default(),
            pipeline_cache: rustc_hash::FxHashMap::default(),

            texture_bind_group_layout,
            texture_bind_groups: rustc_hash::FxHashMap::default(),

            draw_calls: Vec::new(),

            last_pipeline_id: None,
        }
    }

    pub fn process_rcp_output(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        output: &mut RCPOutput,
    ) {
        self.draw_calls.clear();
        self.last_pipeline_id = None;

        // omit the last draw call, because we know we that's an extra from the last flush
        // for draw_call in &self.rcp_output.draw_calls[..self.rcp_output.draw_calls.len() - 1] {
        for draw_call in output.draw_calls.iter().take(output.draw_calls.len() - 1) {
            assert!(!draw_call.vbo.vbo.is_empty());

            // Create Vertex Buffer
            // TODO: Perhaps use a single buffer that we write to?
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: &draw_call.vbo.vbo,
                usage: wgpu::BufferUsages::VERTEX,
            });

            // Create Shader
            self.prepare_shader(device, draw_call.shader_id, draw_call.shader_config);

            // Create Pipeline
            let pipeline_config = PipelineConfig {
                shader: draw_call.shader_id,
                blend_state: draw_call.blend_state,
                cull_mode: draw_call.cull_mode,
                depth_stencil: draw_call.stencil,
            };
            let pipeline_id = PipelineId(pipeline_config);

            self.configure_pipeline(
                device,
                surface_format,
                &draw_call.shader_id,
                pipeline_id,
                draw_call.blend_state,
                draw_call.cull_mode,
                draw_call.stencil,
            );

            // Configure uniforms
            let (vertex_uniform_buf, blend_uniform_buf, combine_uniform_buf, frame_uniform_buf) =
                self.configure_uniforms(
                    device,
                    &draw_call.shader_id,
                    draw_call.projection_matrix,
                    &draw_call.fog,
                    &draw_call.uniforms,
                );

            let (vertex_uniform_bind_group, fragment_uniform_bind_group) = self
                .configure_uniform_bind_groups(
                    device,
                    &draw_call.shader_id,
                    &vertex_uniform_buf,
                    &blend_uniform_buf,
                    &combine_uniform_buf,
                    &frame_uniform_buf,
                );

            // Create mutable draw_call
            let mut wgpu_draw_call = WgpuDrawCall::new(
                draw_call.shader_id,
                pipeline_id,
                vertex_buffer,
                vertex_uniform_buf,
                vertex_uniform_bind_group,
                blend_uniform_buf,
                combine_uniform_buf,
                frame_uniform_buf,
                fragment_uniform_bind_group,
                draw_call.viewport,
                draw_call.scissor,
            );
            wgpu_draw_call.vertex_count = draw_call.vbo.num_tris * 3;

            // Process textures
            for (index, tex_cache_id) in draw_call.texture_indices.iter().enumerate() {
                let sampler = draw_call.samplers[index];
                assert_eq!(tex_cache_id.is_none(), sampler.is_none());

                if let Some(tex_cache_id) = tex_cache_id {
                    let texture = output.texture_cache.get_mut(*tex_cache_id).unwrap();
                    let sampler = sampler.unwrap();

                    self.configure_textures(
                        device,
                        queue,
                        index,
                        texture,
                        &sampler,
                        &mut wgpu_draw_call,
                    );
                }
            }

            self.draw_calls.push(wgpu_draw_call);
        }
    }

    pub fn draw<'r>(
        &'r mut self,
        output_size: &OutputDimensions,
        rpass: &mut wgpu::RenderPass<'r>,
    ) {
        for draw_call in &self.draw_calls {
            let pipeline = self.pipeline_cache.get(&draw_call.pipeline_id).unwrap();

            if self.last_pipeline_id != Some(draw_call.pipeline_id) {
                rpass.set_pipeline(pipeline);
                self.last_pipeline_id = Some(draw_call.pipeline_id);
            }

            rpass.set_bind_group(0, &draw_call.vertex_uniform_bind_group, &[]);
            rpass.set_bind_group(1, &draw_call.fragment_uniform_bind_group, &[]);

            for i in 0..2 {
                if let Some(texture_id) = draw_call.textures[i] {
                    let texture_bind_group = self
                        .texture_bind_groups
                        .get(&texture_id)
                        .expect("Texture bind group not found");

                    rpass.set_bind_group(2 + i as u32, texture_bind_group, &[]);
                }
            }

            rpass.set_vertex_buffer(0, draw_call.vertex_buffer.slice(..));

            let wgpu_y = output_size.height as f32 - draw_call.viewport.y - draw_call.viewport.w;
            rpass.set_viewport(
                draw_call.viewport.x,
                wgpu_y,
                draw_call.viewport.z,
                draw_call.viewport.w,
                0.0,
                1.0,
            );

            let wgpu_y = output_size.height - draw_call.scissor[1] - draw_call.scissor[3];
            rpass.set_scissor_rect(
                draw_call.scissor[0],
                wgpu_y,
                draw_call.scissor[2],
                draw_call.scissor[3],
            );

            rpass.draw(0..draw_call.vertex_count as u32, 0..1);
        }
    }

    pub fn resize(&mut self, _config: &wgpu::SurfaceConfiguration, _device: &wgpu::Device) {}

    pub fn update_frame_count(&mut self) {
        self.frame_count += 1;
    }

    // MARK: - Helpers

    pub fn configure_textures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        tile: usize,
        texture: &mut OutputTexture,
        sampler: &OutputSampler,
        output_draw_call: &mut WgpuDrawCall,
    ) {
        // check if we've already uploaded this texture to the GPU
        if let Some(texture_id) = texture.device_id {
            output_draw_call.textures[tile] = Some(texture_id as usize);
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

        output_draw_call.textures[tile] = Some(self.texture_cache.len());
        texture.device_id = Some(self.texture_cache.len() as u32);

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

        // Create the bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Texture Bind Group"),
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        self.texture_bind_groups
            .insert(self.texture_cache.len(), bind_group);
        self.texture_cache
            .push(TextureData::new(texture_view, sampler));
    }

    fn prepare_shader(
        &mut self,
        device: &wgpu::Device,
        shader_id: ShaderId,
        shader_config: ShaderConfig,
    ) {
        // check if the shader is in the cache
        if self.shader_cache.contains_key(&shader_id) {
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

        // create the shader entry
        let shader_entry = ShaderEntry::new(program, device);
        self.shader_cache.insert(shader_id, shader_entry);
    }

    pub fn configure_uniforms(
        &self,
        device: &wgpu::Device,
        shader_id: &ShaderId,
        projection_matrix: glam::Mat4,
        fog: &OutputFogParams,
        uniforms: &OutputUniforms,
    ) -> (
        wgpu::Buffer,
        wgpu::Buffer,
        wgpu::Buffer,
        Option<wgpu::Buffer>,
    ) {
        // Grab the shader entry
        let shader_entry = self.shader_cache.get(shader_id).unwrap();

        // Update the vertex uniforms
        let vertex_uniform_buf = if shader_entry.program.uses_fog() {
            let uniform = VertexWithFogUniforms::new(
                projection_matrix.to_cols_array_2d(),
                fog.multiplier as f32,
                fog.offset as f32,
            );

            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Uniform Buffer"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            })
        } else {
            let uniform = VertexUniforms {
                projection_matrix: projection_matrix.to_cols_array_2d(),
            };

            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Uniform Buffer"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            })
        };

        // Update the blend uniforms
        let blend_uniform_buf = if shader_entry.program.uses_fog() {
            let uniform = FragmentBlendWithFogUniforms::new(
                uniforms.blend.blend_color.to_array(),
                uniforms.blend.fog_color.xyz().to_array(),
            );

            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Blend Uniform Buffer"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            })
        } else {
            let uniform = FragmentBlendUniforms {
                blend_color: uniforms.blend.blend_color.to_array(),
            };

            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Blend Uniform Buffer"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            })
        };

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

        let combine_uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Combine Uniform Buffer"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        // Update the frame uniforms
        let frame_uniform_buf = if shader_entry.program.uses_alpha()
            && shader_entry.program.uses_alpha_compare_dither()
        {
            let uniform = FragmentFrameUniforms {
                count: self.frame_count as u32,
                height: self.current_height as u32,
            };

            Some(
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Frame Uniform Buffer"),
                    contents: bytemuck::bytes_of(&uniform),
                    usage: wgpu::BufferUsages::UNIFORM,
                }),
            )
        } else {
            None
        };

        (
            vertex_uniform_buf,
            blend_uniform_buf,
            combine_uniform_buf,
            frame_uniform_buf,
        )
    }

    pub fn configure_uniform_bind_groups(
        &self,
        device: &wgpu::Device,
        shader_id: &ShaderId,
        vertex_uniform_buffer: &wgpu::Buffer,
        blend_uniform_buf: &wgpu::Buffer,
        combine_uniform_buf: &wgpu::Buffer,
        frame_uniform_buf: &Option<wgpu::Buffer>,
    ) -> (wgpu::BindGroup, wgpu::BindGroup) {
        let shader_entry = self.shader_cache.get(shader_id).unwrap();

        let vertex_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Vertex Uniform Bind Group"),
            layout: &shader_entry.vertex_uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: vertex_uniform_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(shader_entry.vertex_uniform_size),
                }),
            }],
        });

        let mut bind_group_entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: blend_uniform_buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(shader_entry.blend_uniform_size),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: combine_uniform_buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(shader_entry.combine_uniform_size),
                }),
            },
        ];

        if let Some(frame_uniform_buf) = frame_uniform_buf {
            bind_group_entries.push(wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: frame_uniform_buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(shader_entry.frame_uniform_size),
                }),
            });
        }

        let fragment_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Fragment Uniform Bind Group"),
            layout: &shader_entry.fragment_uniform_bind_group_layout,
            entries: &bind_group_entries,
        });

        (vertex_uniform_bind_group, fragment_uniform_bind_group)
    }

    pub fn configure_pipeline(
        &mut self,
        device: &wgpu::Device,
        surface_texture_format: wgpu::TextureFormat,
        shader_id: &ShaderId,
        pipeline_id: PipelineId,
        blend_state: Option<BlendState>,
        cull_mode: Option<Face>,
        depth_stencil: Option<OutputStencil>,
    ) {
        // Grab current program
        let program = self.shader_cache.get_mut(shader_id).unwrap();

        // Check if we have a cached pipeline
        if self.pipeline_cache.contains_key(&pipeline_id) {
            return;
        }

        // Create the pipeline layout
        let mut bind_group_layout_entries = vec![
            &program.vertex_uniform_bind_group_layout,
            &program.fragment_uniform_bind_group_layout,
        ];

        if program.program.uses_texture_0() {
            bind_group_layout_entries.push(&self.texture_bind_group_layout);
        }

        if program.program.uses_texture_1() {
            bind_group_layout_entries.push(&self.texture_bind_group_layout);
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
            format: DEPTH_FORMAT,
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
