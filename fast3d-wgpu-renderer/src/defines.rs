use crate::wgpu_program::WgpuProgram;
use bytemuck::{Pod, Zeroable};
use fast3d::output::gfx::{BlendState, Face};
use fast3d::output::models::OutputStencil;
use fast3d::output::ShaderId;
use wgpu::util::align_to;
use wgpu::ShaderModule;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct VertexUniforms {
    pub screen_size: [f32; 2],
    _padding: [f32; 2],
    pub projection_matrix: [[f32; 4]; 4],
}

impl VertexUniforms {
    pub fn new(screen_size: [f32; 2], projection_matrix: [[f32; 4]; 4]) -> Self {
        Self {
            screen_size,
            _padding: [0.0; 2],
            projection_matrix,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct VertexWithFogUniforms {
    pub screen_size: [f32; 2],
    _padding: [f32; 2],
    pub projection_matrix: [[f32; 4]; 4],
    pub fog_multiplier: f32,
    pub fog_offset: f32,
    _padding2: [f32; 2],
}

impl VertexWithFogUniforms {
    pub fn new(screen_size: [f32; 2], projection_matrix: [[f32; 4]; 4], fog_multiplier: f32, fog_offset: f32) -> Self {
        Self {
            screen_size,
            _padding: [0.0; 2],
            projection_matrix,
            fog_multiplier,
            fog_offset,
            _padding2: [0.0; 2],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct FragmentBlendUniforms {
    pub blend_color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct FragmentBlendWithFogUniforms {
    pub blend_color: [f32; 4],
    pub fog_color: [f32; 3],
    _padding: f32,
}

impl FragmentBlendWithFogUniforms {
    pub fn new(blend_color: [f32; 4], fog_color: [f32; 3]) -> Self {
        Self {
            blend_color,
            fog_color,
            _padding: 0.0,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct FragmentCombineUniforms {
    prim_color: [f32; 4],
    env_color: [f32; 4],
    key_center: [f32; 3],
    // Due to uniforms requiring 16 byte (4 float) spacing, we need to use a padding field here
    _padding: f32,
    key_scale: [f32; 3],
    // Due to uniforms requiring 16 byte (4 float) spacing, we need to use a padding field here
    _padding2: f32,
    prim_lod_frac: f32,
    convert_k4: f32,
    convert_k5: f32,
    _padding3: f32,
}

impl FragmentCombineUniforms {
    pub fn new(
        prim_color: [f32; 4],
        env_color: [f32; 4],
        key_center: [f32; 3],
        key_scale: [f32; 3],
        prim_lod_frac: f32,
        convert_k4: f32,
        convert_k5: f32,
    ) -> Self {
        Self {
            prim_color,
            env_color,
            key_center,
            _padding: 0.0,
            key_scale,
            _padding2: 0.0,
            prim_lod_frac,
            convert_k4,
            convert_k5,
            _padding3: 0.0,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct FragmentFrameUniforms {
    pub count: u32,
    pub height: u32,
}

pub struct TextureData {
    pub texture_view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

impl TextureData {
    pub fn new(texture_view: wgpu::TextureView, sampler: wgpu::Sampler) -> Self {
        Self {
            texture_view,
            sampler,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PipelineId(pub PipelineConfig);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PipelineConfig {
    pub shader: ShaderId,
    pub blend_state: Option<BlendState>,
    pub cull_mode: Option<Face>,
    pub depth_stencil: Option<OutputStencil>,
}

pub struct ShaderEntry<'a> {
    pub program: WgpuProgram<ShaderModule>,
    pub vertex_buf_layout: wgpu::VertexBufferLayout<'a>,
    pub vertex_uniform_bind_group_layout: wgpu::BindGroupLayout,
    pub fragment_uniform_bind_group_layout: wgpu::BindGroupLayout,
}

impl<'a> ShaderEntry<'a> {
    pub fn new(program: WgpuProgram<ShaderModule>, device: &wgpu::Device) -> Self {
        let vertex_buf_layout = Self::create_vertex_buf_layout(&program);

        let vertex_uniform_bind_group_layout =
            Self::create_vertex_uniforms_resources(&program, device);
        let fragment_uniform_bind_group_layout =
            Self::create_fragment_uniforms_resources(&program, device);

        Self {
            program,

            vertex_buf_layout,
            vertex_uniform_bind_group_layout,
            fragment_uniform_bind_group_layout,
        }
    }

    fn create_vertex_buf_layout(
        program: &WgpuProgram<ShaderModule>,
    ) -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: (program.num_floats * std::mem::size_of::<f32>()) as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: if program.uses_texture_0() || program.uses_texture_1() {
                &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 0, // position
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: std::mem::size_of::<[f32; 4]>() as u64, // color
                        shader_location: 1,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: std::mem::size_of::<[f32; 8]>() as u64, // texcoord
                        shader_location: 2,
                    },
                ]
            } else {
                &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 0, // position
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: std::mem::size_of::<[f32; 4]>() as u64, // color
                        shader_location: 1,
                    },
                ]
            },
        }
    }

    fn create_vertex_uniforms_resources(
        program: &WgpuProgram<ShaderModule>,
        device: &wgpu::Device,
    ) -> wgpu::BindGroupLayout {
        let vertex_uniform_size = if program.uses_fog() {
            std::mem::size_of::<VertexWithFogUniforms>() as wgpu::BufferAddress
        } else {
            std::mem::size_of::<VertexUniforms>() as wgpu::BufferAddress
        };

        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Vertex Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(vertex_uniform_size),
                },
                count: None,
            }],
        })
    }

    fn create_fragment_uniforms_resources(
        program: &WgpuProgram<ShaderModule>,
        device: &wgpu::Device,
    ) -> wgpu::BindGroupLayout {
        // Handle blend uniforms
        let blend_uniform_size = if program.uses_fog() {
            std::mem::size_of::<FragmentBlendWithFogUniforms>() as wgpu::BufferAddress
        } else {
            std::mem::size_of::<FragmentBlendUniforms>() as wgpu::BufferAddress
        };

        // Handle combine uniforms
        let combine_uniform_size =
            std::mem::size_of::<FragmentCombineUniforms>() as wgpu::BufferAddress;

        // Handle frame uniforms
        let frame_uniform_buf = {
            if program.uses_alpha() && program.uses_alpha_compare_dither() {
                let frame_uniform_size =
                    std::mem::size_of::<FragmentFrameUniforms>() as wgpu::BufferAddress;

                let frame_uniform_alignment = {
                    let alignment =
                        device.limits().min_uniform_buffer_offset_alignment as wgpu::BufferAddress;
                    align_to(frame_uniform_size, alignment)
                };

                Some(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Frame Uniform Buffer"),
                    size: frame_uniform_alignment,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }))
            } else {
                None
            }
        };

        // Create bind group layout
        let mut bind_group_layout_entries = vec![
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(blend_uniform_size),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(combine_uniform_size),
                },
                count: None,
            },
        ];

        if let Some(_) = frame_uniform_buf {
            bind_group_layout_entries.push(wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<FragmentFrameUniforms>() as wgpu::BufferAddress,
                    ),
                },
                count: None,
            });
        }

        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Fragment Uniform Group Layout"),
            entries: &bind_group_layout_entries,
        })
    }
}
