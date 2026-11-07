use std::collections::{HashMap, HashSet};

use super::{
    framebuffers::{
        coherence::{Coherence, Writer},
        targets::TargetDescriptors,
    },
    workload::TargetId,
    DepthImage, Framebuffer, SceneRenderer,
};

pub(crate) struct StrictTask {
    id: TargetId,
    framebuffer: Option<Framebuffer>,
    coherence: Coherence,
    descriptors: TargetDescriptors,
    first_touch: HashSet<TargetId>,
    depth_first_touch: HashSet<TargetId>,
    versions: HashMap<Writer, wgpu::Texture>,
    presentation_versions: HashMap<Writer, wgpu::Texture>,
    depth: HashMap<TargetId, DepthImage>,
}

impl SceneRenderer {
    pub(crate) fn stage_strict_task(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: TargetId,
        depth_id: Option<TargetId>,
    ) -> StrictTask {
        let mut encoder = device.create_command_encoder(&Default::default());
        let framebuffer = self.framebuffers.remove(&id);
        if let Some(old) = &framebuffer {
            let staged = self.make_fb(
                device,
                old.layout,
                (old.color.width(), old.color.height()),
                old.height,
            );
            encoder.copy_texture_to_texture(
                old.color.as_image_copy(),
                staged.color.as_image_copy(),
                old.color.size(),
            );
            if let (Some(old), Some(staged)) = (&old.presentation, &staged.presentation) {
                encoder.copy_texture_to_texture(
                    old.texture.as_image_copy(),
                    staged.texture.as_image_copy(),
                    old.texture.size(),
                );
            }
            self.framebuffers.insert(id, staged);
        }
        let depth = self.depthbuffers.clone();
        let original = depth_id.and_then(|id| self.depthbuffers.get(&id));
        if let Some(original) = original {
            let stage = |original: &DepthImage, encoder: &mut wgpu::CommandEncoder| {
                let staged = DepthImage::new(
                    device,
                    original.layout,
                    (original.texture.width(), original.texture.height()),
                );
                self.depth_pipeline.copy(device, encoder, original, &staged);
                self.depth_pipeline.sync_delta_depth(
                    device,
                    encoder,
                    &staged,
                    (0, 0, original.texture.width(), original.texture.height()),
                );
                staged
            };
            let mut staged = stage(original, &mut encoder);
            staged.presentation = original
                .presentation
                .as_deref()
                .map(|image| Box::new(stage(image, &mut encoder)));
            self.depthbuffers.insert(depth_id.unwrap(), staged);
        }
        if framebuffer.is_some() || depth_id.is_some_and(|id| depth.contains_key(&id)) {
            queue.submit([encoder.finish()]);
        }
        let staged = self.coherence.as_mut().unwrap().stage_task();
        StrictTask {
            id,
            framebuffer,
            coherence: self.coherence.replace(staged).unwrap(),
            descriptors: self.target_descriptors.clone(),
            first_touch: self.first_touch.clone(),
            depth_first_touch: self.depth_first_touch.clone(),
            versions: self.color_versions.clone(),
            presentation_versions: self.presentation_versions.clone(),
            depth,
        }
    }

    pub(crate) fn commit_strict_task(&mut self, task: StrictTask) {
        self.coherence
            .as_mut()
            .unwrap()
            .recycle_native(task.coherence);
    }

    pub(crate) fn discard_strict_task(&mut self, mut task: StrictTask) {
        self.framebuffers.remove(&task.id);
        if let Some(original) = task.framebuffer {
            self.framebuffers.insert(task.id, original);
        }
        task.coherence
            .recycle_native(self.coherence.take().unwrap());
        self.coherence = Some(task.coherence);
        self.target_descriptors = task.descriptors;
        self.first_touch = task.first_touch;
        self.depth_first_touch = task.depth_first_touch;
        self.color_versions = task.versions;
        self.presentation_versions = task.presentation_versions;
        self.depthbuffers = task.depth;
    }
}
