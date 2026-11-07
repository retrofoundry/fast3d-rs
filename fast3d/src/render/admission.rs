use super::{
    extents::TargetExtents, framebuffers::ImageLayout, inputs::RenderInputs, workload::TargetId,
    SceneRenderer,
};
use crate::{diag::ResourceAdmission, DiagKind, Diagnostic};
use std::collections::{HashMap, HashSet};

struct Admission<'a> {
    limits: &'a wgpu::Limits,
    budget: u64,
    bytes: u64,
    pc: u64,
}

impl Admission<'_> {
    fn failure(&self, reason: ResourceAdmission, required: u64, limit: u64) -> Diagnostic {
        Diagnostic {
            at: self.pc,
            kind: DiagKind::ResourceAdmission {
                reason,
                required,
                limit,
            },
        }
    }

    fn overflow(&self) -> Diagnostic {
        self.failure(ResourceAdmission::ArithmeticOverflow, u64::MAX, u64::MAX)
    }

    fn charge(&mut self, bytes: u64) -> Result<(), Diagnostic> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| self.overflow())?;
        if self.bytes > self.budget {
            return Err(self.failure(ResourceAdmission::AttachmentBudget, self.bytes, self.budget));
        }
        Ok(())
    }

    fn image(&self, extent: (u32, u32)) -> Result<u64, Diagnostic> {
        let dimension = extent.0.max(extent.1);
        if extent.0 == 0 || extent.1 == 0 || dimension > self.limits.max_texture_dimension_2d {
            return Err(self.failure(
                ResourceAdmission::Dimensions,
                u64::from(dimension),
                u64::from(self.limits.max_texture_dimension_2d),
            ));
        }
        u64::from(extent.0)
            .checked_mul(u64::from(extent.1))
            .and_then(|bytes| bytes.checked_mul(4))
            .ok_or_else(|| self.overflow())
    }

    fn buffer(&self, bytes: u64, storage: bool) -> Result<u64, Diagnostic> {
        if bytes > self.limits.max_buffer_size {
            return Err(self.failure(
                ResourceAdmission::BufferSize,
                bytes,
                self.limits.max_buffer_size,
            ));
        }
        if storage && bytes > self.limits.max_storage_buffer_binding_size {
            return Err(self.failure(
                ResourceAdmission::StorageBinding,
                bytes,
                self.limits.max_storage_buffer_binding_size,
            ));
        }
        Ok(bytes)
    }

    fn padded(&self, extent: (u32, u32), pixel_bytes: u32) -> Result<u64, Diagnostic> {
        let row = extent
            .0
            .checked_mul(pixel_bytes)
            .and_then(|row| row.checked_add(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT - 1))
            .map(|row| {
                row / wgpu::COPY_BYTES_PER_ROW_ALIGNMENT * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
            })
            .ok_or_else(|| {
                self.failure(
                    ResourceAdmission::RowStride,
                    u64::from(extent.0) * u64::from(pixel_bytes),
                    u64::from(u32::MAX),
                )
            })?;
        self.buffer(u64::from(row) * u64::from(extent.1), false)
    }
}

#[derive(Clone, Copy)]
struct Retained {
    layout: ImageLayout,
    height: u32,
    allocation_height: u32,
}

impl SceneRenderer {
    fn attachment_textures(&self) -> impl Iterator<Item = &wgpu::Texture> {
        self.framebuffers
            .values()
            .flat_map(|fb| {
                std::iter::once(&fb.color).chain(fb.presentation.as_ref().map(|p| &p.texture))
            })
            .chain(self.depthbuffers.values().flat_map(|depth| {
                std::iter::once(depth)
                    .chain(depth.presentation.as_deref())
                    .flat_map(|image| {
                        [
                            &image.texture,
                            image.delta_view.texture(),
                            image.delta_depth.as_ref().unwrap().texture(),
                        ]
                    })
            }))
            .chain(self.color_versions.values())
            .chain(self.presentation_versions.values())
    }

    pub(super) fn preflight_scale(
        &self,
        scale: u32,
        limits: &wgpu::Limits,
    ) -> Result<(), Diagnostic> {
        let mut plan = Admission {
            limits,
            budget: self.attachment_budget,
            bytes: 0,
            pc: 0,
        };
        for texture in self.attachment_textures() {
            let pixel_bytes = if texture.format() == wgpu::TextureFormat::R8Uint {
                1
            } else {
                4
            };
            plan.charge(u64::from(texture.width()) * u64::from(texture.height()) * pixel_bytes)?;
        }
        if scale > 1 {
            for texture in self
                .framebuffers
                .values()
                .map(|fb| &fb.color)
                .chain(self.depthbuffers.values().map(|depth| &depth.texture))
                .chain(self.color_versions.values())
            {
                let extent = (
                    texture
                        .width()
                        .checked_mul(scale)
                        .ok_or_else(|| plan.overflow())?,
                    texture
                        .height()
                        .checked_mul(scale)
                        .ok_or_else(|| plan.overflow())?,
                );
                let bytes = plan.image(extent)?;
                plan.charge(if texture.format() == super::DEPTH_FORMAT {
                    bytes / 4 * 9
                } else {
                    bytes
                })?;
            }
        }
        Ok(())
    }

    pub(super) fn preflight_inputs(
        &self,
        inputs: &mut RenderInputs<'_>,
        limits: &wgpu::Limits,
        strict: bool,
    ) -> Result<(), Diagnostic> {
        let mut plan = Admission {
            limits,
            budget: self.attachment_budget,
            bytes: 0,
            pc: 0,
        };
        for texture in self.attachment_textures() {
            let pixel_bytes = if texture.format() == wgpu::TextureFormat::R8Uint {
                1
            } else {
                4
            };
            plan.charge(u64::from(texture.width()) * u64::from(texture.height()) * pixel_bytes)?;
        }
        if strict {
            let target = &inputs.targets[0];
            if let Some(fb) = self.framebuffers.get(&target.id) {
                for texture in
                    std::iter::once(&fb.color).chain(fb.presentation.as_ref().map(|p| &p.texture))
                {
                    plan.charge(plan.image((texture.width(), texture.height()))?)?;
                }
            }
            if let Some(depth) = target.depth_id().and_then(|id| self.depthbuffers.get(&id)) {
                for image in std::iter::once(depth).chain(depth.presentation.as_deref()) {
                    plan.charge(
                        plan.image((image.texture.width(), image.texture.height()))? / 4 * 9,
                    )?;
                }
            }
        }
        let mut colors: HashMap<_, _> = self
            .framebuffers
            .iter()
            .map(|(&id, fb)| {
                (
                    id,
                    Retained {
                        layout: fb.layout,
                        height: fb.height,
                        allocation_height: fb.color.height(),
                    },
                )
            })
            .collect();
        let mut depths: HashMap<_, _> = self
            .depthbuffers
            .iter()
            .map(|(&id, depth)| {
                (
                    id,
                    Retained {
                        layout: depth.layout,
                        height: depth.texture.height(),
                        allocation_height: depth.texture.height(),
                    },
                )
            })
            .collect();
        let mut requests = HashSet::new();
        for texture in &inputs.textures {
            for binding in std::iter::once(&texture.texture)
                .chain(texture.tex1.iter())
                .chain(texture.mip_levels.iter())
                .chain(texture.detail_tex.iter())
            {
                let crate::hle::texture_request::TextureSource::Framebuffer(request) =
                    &binding.source
                else {
                    continue;
                };
                if !requests.insert(std::sync::Arc::as_ptr(request) as usize) {
                    continue;
                }
                let extent = request.encoded.recipe().allocation_extent();
                plan.image((extent[0], extent[1]))?;
                let mut input_bytes = 4096u64 + 4096 * 4;
                let mut tags = HashSet::new();
                for tag in request.tags.bytes.iter().filter(|tag| tag.copy != 0) {
                    if tags.insert(tag.copy) {
                        input_bytes = input_bytes
                            .checked_add(u64::from(
                                request.tags.copies[usize::from(tag.copy) - 1].size,
                            ))
                            .ok_or_else(|| plan.overflow())?;
                    }
                }
                plan.buffer(input_bytes, true)?;
                if self.render_scale > 1 && request.image_route() {
                    let mut scale = 1;
                    let mut image_bytes = 4096u64 + 4096 * 12;
                    for tag in tags {
                        let copy = &request.tags.copies[usize::from(tag) - 1];
                        let q = self
                            .framebuffer_copies
                            .source_scale(copy, self.render_scale);
                        scale = scale.max(q);
                        let size = if q > 1 {
                            super::framebuffers::copies::image_layout(copy, q)
                                .ok_or_else(|| plan.overflow())?
                                .1
                        } else {
                            u64::from(copy.size)
                        };
                        image_bytes = image_bytes
                            .checked_add(size)
                            .ok_or_else(|| plan.overflow())?;
                    }
                    plan.image((
                        extent[0]
                            .checked_mul(scale)
                            .ok_or_else(|| plan.overflow())?,
                        extent[1]
                            .checked_mul(scale)
                            .ok_or_else(|| plan.overflow())?,
                    ))?;
                    plan.buffer(image_bytes, true)?;
                }
            }
        }
        #[cfg(feature = "capture")]
        let mut depth_epoch = self.depth_color_epoch;
        let mut copies = std::collections::HashSet::new();
        for target in &mut inputs.targets {
            plan.pc = target.pc;
            for operation in &target.operations {
                if let super::inputs::DrawInputs::FramebufferCopy(copy) = &operation.draw {
                    if copies.insert(super::framebuffers::copies::key(copy)) {
                        plan.buffer(u64::from(copy.size), false)?;
                        if self.render_scale > 1 {
                            let (spans, size) =
                                super::framebuffers::copies::image_layout(copy, self.render_scale)
                                    .ok_or_else(|| plan.overflow())?;
                            for span in spans {
                                plan.image((span.width * self.render_scale, self.render_scale))?;
                            }
                            plan.buffer(size, false)?;
                        }
                    }
                }
            }
            if !target.valid
                || (!target.operations.is_empty()
                    && target
                        .operations
                        .iter()
                        .all(|op| matches!(op.draw, super::inputs::DrawInputs::FramebufferCopy(_))))
            {
                continue;
            }
            target.extents = TargetExtents::new(
                target.id,
                target.extents.logical,
                (self.fb_w, self.fb_h),
                self.render_scale,
            )
            .ok_or_else(|| plan.overflow())?;
            let native = target.extents.native;
            let presentation = target.extents.presentation;
            #[cfg(feature = "capture")]
            if self.depth_reset_policy == crate::DepthResetPolicy::ColorImageSwitch
                && depth_epoch != Some(target.color_image_epoch)
            {
                depths.clear();
                depth_epoch = Some(target.color_image_epoch);
            }
            let color_layout = ImageLayout {
                width: native.0,
                fmt: target.color_image.fmt,
                siz: target.color_image.siz,
            };
            let depth_layout = ImageLayout {
                width: native.0,
                fmt: 0,
                siz: 2,
            };
            let depth_id = target.depth_id();
            let compatible = |old: &Retained, id, layout| {
                old.layout == layout && (id != TargetId::Legacy || old.height == native.1)
            };
            let old_color = colors
                .get(&target.id)
                .filter(|old| !target.depth_clear && compatible(old, target.id, color_layout));
            let old_depth = depth_id.and_then(|id| {
                depths
                    .get(&id)
                    .filter(|old| compatible(old, id, depth_layout))
            });
            let height = native
                .1
                .max(old_color.map_or(0, |old| old.allocation_height))
                .max(old_depth.map_or(0, |old| old.allocation_height));
            let allocation = (native.0, height);
            let scaled = (
                presentation.0,
                height
                    .checked_mul(self.render_scale)
                    .ok_or_else(|| plan.overflow())?,
            );
            let native_bytes = plan.image(allocation)?;
            let scaled_bytes = if self.render_scale > 1 {
                plan.image(scaled)?
            } else {
                0
            };
            if !target.depth_clear {
                if old_color.is_none_or(|old| old.allocation_height < height) {
                    plan.charge(native_bytes)?;
                    plan.charge(scaled_bytes)?;
                }
                let retained_height = native.1.max(old_color.map_or(0, |old| old.height));
                colors.insert(
                    target.id,
                    Retained {
                        layout: color_layout,
                        height: retained_height,
                        allocation_height: height,
                    },
                );
            }
            if let Some(id) = depth_id {
                if old_depth.is_none_or(|old| old.allocation_height < height) {
                    plan.charge(native_bytes / 4 * 9)?;
                    plan.charge(scaled_bytes / 4 * 9)?;
                }
                depths.insert(
                    id,
                    Retained {
                        layout: depth_layout,
                        height,
                        allocation_height: height,
                    },
                );
            }
            if !target.depth_clear && !target.operations.is_empty() {
                let uniforms = (target.uniforms.len() as u64)
                    .checked_mul(if self.render_scale > 1 { 2 } else { 1 })
                    .ok_or_else(|| plan.overflow())?;
                plan.buffer(uniforms, false)?;
                plan.buffer(std::mem::size_of_val(&target.rectangles[..]) as u64, false)?;
                if self.coherence.is_some() {
                    plan.padded(allocation, 8)?;
                }
            }
            if self.coherence.is_some() {
                plan.padded(allocation, 4)?;
            }
            if self.coherence.is_some() && !target.depth_clear {
                if let TargetId::Guest(address) = target.id {
                    let end = address
                        .checked_add(
                            color_layout
                                .row_bytes()
                                .ok_or_else(|| plan.overflow())?
                                .checked_mul(u64::from(native.1))
                                .ok_or_else(|| plan.overflow())?,
                        )
                        .ok_or_else(|| plan.overflow())?;
                    depths.retain(|id, depth| {
                        let TargetId::Guest(start) = *id else {
                            return true;
                        };
                        let depth_end = start
                            + depth.layout.row_bytes().unwrap()
                                * u64::from(depth.allocation_height);
                        start >= end || address >= depth_end
                    });
                }
            }
        }
        if let Some(rsp) = &inputs.rsp {
            for bytes in [
                std::mem::size_of_val(&rsp.source[..]),
                std::mem::size_of_val(&rsp.mvp_table[..]),
                std::mem::size_of_val(&rsp.viewport_table[..]),
                std::mem::size_of_val(&rsp.texcoord_table[..]),
                std::mem::size_of_val(&rsp.lights_table[..]),
                std::mem::size_of_val(&rsp.lookat_table[..]),
                std::mem::size_of_val(&rsp.fog_table[..]),
            ] {
                plan.buffer(bytes as u64, true)?;
            }
            plan.buffer(std::mem::size_of_val(rsp.indices) as u64, false)?;
            if inputs.targets.iter().any(|target| !target.depth_clear) {
                let output_bytes = (rsp.source.len() as u64)
                    .checked_mul(std::mem::size_of::<super::OutVertex>() as u64)
                    .ok_or_else(|| plan.overflow())?;
                plan.buffer(output_bytes, true)?;
                if (rsp.source.len() as u64).div_ceil(64)
                    > u64::from(limits.max_compute_workgroups_per_dimension)
                {
                    return Err(plan.failure(
                        ResourceAdmission::Dimensions,
                        (rsp.source.len() as u64).div_ceil(64),
                        u64::from(limits.max_compute_workgroups_per_dimension),
                    ));
                }
            }
        }
        self.profiling
            .gauge("gpu_attachment_admitted_peak_bytes", plan.bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_row_stride_overflow() {
        let limits = wgpu::Limits::default();
        let plan = Admission {
            limits: &limits,
            budget: u64::MAX,
            bytes: 0,
            pc: 0,
        };
        assert!(matches!(
            plan.padded((u32::MAX, 1), 4).unwrap_err().kind,
            DiagKind::ResourceAdmission {
                reason: ResourceAdmission::RowStride,
                ..
            }
        ));
    }
}
