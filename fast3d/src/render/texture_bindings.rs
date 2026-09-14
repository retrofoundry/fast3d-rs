use std::sync::Arc;

use super::{
    inputs::TextureInputs,
    texture_resources::{upload_rgba, Image, RgbaImage, TextureResources},
    TileSamplingArray, LOD_BINDING_BASE,
};
use crate::hle::texture_request::TextureSource;

enum BindingImage {
    Encoded(Image),
    Rgba(Arc<RgbaImage>),
    Dummy,
}

enum PreparedImage {
    Encoded(Image),
    Rgba(Arc<RgbaImage>),
    Dummy,
}

impl PreparedImage {
    fn matches(&self, old: &BindingImage) -> bool {
        match (self, old) {
            (Self::Encoded(image), BindingImage::Encoded(old)) => Arc::ptr_eq(image, old),
            (Self::Rgba(image), BindingImage::Rgba(old)) => Arc::ptr_eq(image, old),
            (Self::Dummy, BindingImage::Dummy) => true,
            _ => false,
        }
    }

    fn view<'a>(&'a self, dummy: &'a wgpu::TextureView) -> &'a wgpu::TextureView {
        match self {
            Self::Encoded(image) => &image.value.view,
            Self::Rgba(image) => &image.view,
            Self::Dummy => dummy,
        }
    }

    fn binding(self) -> BindingImage {
        match self {
            Self::Encoded(image) => BindingImage::Encoded(image),
            Self::Rgba(image) => BindingImage::Rgba(image),
            Self::Dummy => BindingImage::Dummy,
        }
    }
}

pub(super) struct MaterialBinding {
    sampling: TileSamplingArray,
    wrap: [u8; 2],
    images: Vec<BindingImage>,
    pub bind_group: wgpu::BindGroup,
}

impl MaterialBinding {
    pub fn may_reuse(
        &self,
        mat: &TextureInputs,
        resources: &TextureResources,
        profiling: &crate::profiling::Recorder,
    ) -> bool {
        if self.sampling != mat.sampling || self.wrap != [mat.wrap_s, mat.wrap_t] {
            return false;
        }
        roles(mat)
            .into_iter()
            .zip(&self.images)
            .enumerate()
            .all(|(index, (source, image))| match (source, image) {
                (Some(binding), BindingImage::Encoded(image)) => match &binding.source {
                    TextureSource::Encoded(request) => {
                        (image.admitted || resources.cache.is_pending(image))
                            && request.key_profiled(profiling) == image.key
                    }
                    _ => false,
                },
                (Some(binding), BindingImage::Rgba(image)) => match &binding.source {
                    TextureSource::Rgba(bytes) => {
                        image.source == *bytes
                            && image.extent
                                == if index == 0 {
                                    mat.allocation_extent
                                } else {
                                    binding.sampling.allocation_extent()
                                }
                    }
                    _ => false,
                },
                (None, BindingImage::Dummy) => true,
                _ => false,
            })
    }

    pub fn rgba_bytes(&self) -> u64 {
        self.images
            .iter()
            .map(|image| match image {
                BindingImage::Rgba(image) => {
                    u64::from(image.extent[0]) * u64::from(image.extent[1]) * 4
                }
                _ => 0,
            })
            .sum()
    }
}

pub(super) struct BindingBuilder<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub layout: &'a wgpu::BindGroupLayout,
    pub samplers: &'a [[wgpu::Sampler; 3]; 3],
    pub dummy: &'a wgpu::TextureView,
    pub resources: &'a mut TextureResources,
    pub profiling: &'a crate::profiling::Recorder,
}

impl BindingBuilder<'_> {
    pub fn build(&mut self, mat: &TextureInputs, old: Option<MaterialBinding>) -> MaterialBinding {
        let _span = self.profiling.span("resources");
        let roles = roles(mat);
        let mut images = Vec::with_capacity(roles.len());
        for (index, role) in roles.into_iter().enumerate() {
            let Some(binding) = role else {
                images.push(PreparedImage::Dummy);
                continue;
            };
            let name = [
                if mat.mip_levels.is_empty() {
                    "texture0"
                } else {
                    "lod0"
                },
                "texture1",
                "detail",
                "lod1",
                "lod2",
                "lod3",
                "lod4",
                "lod5",
                "lod6",
                "lod7",
            ][index];
            let image = match &binding.source {
                TextureSource::Encoded(request) => PreparedImage::Encoded(self.resources.resolve(
                    self.device,
                    self.queue,
                    request,
                    name,
                    self.profiling,
                )),
                TextureSource::Rgba(bytes) => {
                    let extent = if index == 0 {
                        mat.allocation_extent
                    } else {
                        binding.sampling.allocation_extent()
                    };
                    let reused = old.as_ref().and_then(|old| match &old.images[index] {
                        BindingImage::Rgba(image)
                            if image.extent == extent && image.source == *bytes =>
                        {
                            Some(image.clone())
                        }
                        _ => None,
                    });
                    PreparedImage::Rgba(reused.unwrap_or_else(|| {
                        upload_rgba(
                            self.device,
                            self.queue,
                            &binding.source,
                            extent,
                            name,
                            self.profiling,
                        )
                    }))
                }
            };
            images.push(image);
        }
        let wrap = [mat.wrap_s, mat.wrap_t];
        if let Some(old) = old {
            if old.sampling == mat.sampling
                && old.wrap == wrap
                && images
                    .iter()
                    .zip(&old.images)
                    .all(|(image, old)| image.matches(old))
            {
                return old;
            }
        }
        let sampling = super::sampling_buffer_profiled(self.device, &mat.sampling, self.profiling);
        let samp1 = mat.tex1.as_ref().map_or(&self.samplers[2][2], |input| {
            &self.samplers[(input.sampling.modes[0] as usize).min(2)]
                [(input.sampling.modes[1] as usize).min(2)]
        });
        let mut entries = vec![super::sampling_entry(&sampling)];
        for (index, image) in images.iter().enumerate() {
            let binding = if index < 3 {
                index as u32 * 2
            } else {
                LOD_BINDING_BASE + index as u32 - 3
            };
            entries.push(wgpu::BindGroupEntry {
                binding,
                resource: wgpu::BindingResource::TextureView(image.view(self.dummy)),
            });
        }
        for (binding, sampler) in [
            (
                1,
                &self.samplers[(wrap[0] as usize).min(2)][(wrap[1] as usize).min(2)],
            ),
            (3, samp1),
            (5, &self.samplers[2][2]),
        ] {
            entries.push(wgpu::BindGroupEntry {
                binding,
                resource: wgpu::BindingResource::Sampler(sampler),
            });
        }
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("n64-bg"),
            layout: self.layout,
            entries: &entries,
        });
        self.profiling.count("tmem.sampling_bindings_created", 1);
        MaterialBinding {
            sampling: mat.sampling,
            wrap,
            images: images.into_iter().map(PreparedImage::binding).collect(),
            bind_group,
        }
    }
}

fn roles(mat: &TextureInputs) -> [Option<&crate::hle::texture_request::TextureBindingInput>; 10] {
    let count = super::uploaded_level_count(mat.num_levels);
    std::array::from_fn(|index| match index {
        0 => Some(&mat.texture),
        1 => mat.tex1.as_ref(),
        2 => mat.detail_tex.as_ref(),
        _ if (index - 2) < count as usize => mat.mip_levels.get(index - 2),
        _ => None,
    })
}
