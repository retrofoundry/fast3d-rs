use crate::hle::{
    combiner::{decode_combine, physical_texture_uses},
    rdp::{Rdp, TileDescriptor},
    rsp::Rsp,
    texture_request::{self, TextureSource},
    tile_sampling::TileSampling,
};

#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "profiling", derive(serde::Serialize))]
pub struct CensusCommand {
    pub pc: u64,
    pub words: [u32; 2],
    pub image: (u8, u8, u16, u64),
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "profiling", derive(serde::Serialize))]
pub struct CensusLoad {
    pub id: u64,
    pub command: CensusCommand,
    pub kind: String,
    pub tile: Option<TileDescriptor>,
    pub parameters: Vec<u64>,
    pub source_hash: u64,
    pub source: Vec<u8>,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "profiling", derive(serde::Serialize))]
pub struct CensusAttempt {
    pub pc: u64,
    pub role: String,
    pub tile: TileDescriptor,
    pub tlut: u8,
    pub outcome: String,
    pub witness: Option<Vec<u8>>,
    pub draw_rejection: Option<String>,
    pub framebuffer: bool,
    pub framebuffer_decode: Option<String>,
    pub sampled_framebuffer: bool,
    pub framebuffer_tags: Vec<(u16, u64, u32)>,
    pub bank: Vec<u8>,
    pub sampled: Vec<u16>,
    pub sampled_hash: u64,
    pub writers: Vec<(u16, u64)>,
    pub physical_representation: String,
    pub extent: [u32; 2],
    pub physical: Vec<u8>,
    pub class: String,
    pub decode_error: Option<String>,
}

pub(crate) fn census_material(
    rdp: &Rdp,
    rsp: &Rsp,
    pc: u64,
    rect_tile: Option<u8>,
    rejection: Option<crate::DiagKind>,
) {
    if !rsp.profiling.censusing() {
        return;
    }
    let cycle = (rdp.other_mode_h >> 20) & 3;
    if crate::hle::blender::decode_render_mode(rdp.other_mode_l, rdp.other_mode_h, 0).blend_class
        == crate::hle::BlendClass::BlendColor
    {
        return;
    }
    let (tex0, tex1) = physical_texture_uses(&decode_combine(rdp.combine_l, rdp.combine_h), cycle);
    let base = usize::from(rect_tile.unwrap_or(rsp.texture_state.tile) & 7);
    let mut roles = Vec::new();
    if tex0 {
        roles.push((base, "tex0".to_owned()));
    }
    if tex1 {
        roles.push(((base + 1) & 7, "tex1".to_owned()));
    }
    if rect_tile.is_none() && (tex0 || tex1) && rdp.lod_enable() && rsp.texture_state.level > 0 {
        for k in 0..(usize::from(rsp.texture_state.level) + 1).min(8) {
            roles.push(((base + k) & 7, format!("lod{k}")));
        }
        if rdp.text_detail() & 2 != 0 {
            roles.push((0, "detail".into()));
        }
    }
    let mut diags = Vec::new();
    let selectors = decode_combine(rdp.combine_l, rdp.combine_h);
    let _ = crate::hle::combiner::validate_combiner(&selectors, cycle, &mut diags, pc)
        && crate::hle::combiner::validate_conversion_modes(rdp, &mut diags, pc);
    let global_rejection = rejection
        .or_else(|| diags.first().map(|diag| diag.kind))
        .or_else(|| {
            roles.iter().find_map(|(index, _)| {
                rdp.tmem_bank
                    .rejection(&rdp.tiles[*index], ((rdp.other_mode_h >> 14) & 3) as u8)
                    .map(|diag| diag.kind)
            })
        });
    let mut blocked = global_rejection.map(|kind| format!("{kind:?}"));
    for (index, role) in roles {
        let mut attempt = CensusAttempt::capture(rdp, &rdp.tiles[index], pc, role);
        attempt.draw_rejection = blocked.clone();
        if !rdp.texture_loaded && cycle < 2 && global_rejection.is_none() {
            attempt.outcome = "NoTextureLoaded".into();
            attempt.class = "unaffected".into();
        }
        if blocked.is_none()
            && !matches!(
                attempt.outcome.as_str(),
                "Tile" | "Lookup4096x4" | "Framebuffer"
            )
        {
            blocked = Some(format!("{}: {}", attempt.role, attempt.outcome));
            attempt.draw_rejection = blocked.clone();
        }
        rsp.profiling.census_attempt(attempt);
    }
}

impl CensusAttempt {
    fn capture(rdp: &Rdp, tile: &TileDescriptor, pc: u64, role: String) -> Self {
        let tlut = ((rdp.other_mode_h >> 14) & 3) as u8;
        let sampling = TileSampling::from_tile(tile, tlut);
        let lookup = sampling.image[2] == 1;
        let old = texture_request::prepare(rdp, tile, tlut, &Default::default());
        let outcome = match &old {
            Ok(binding) => match &binding.source {
                TextureSource::Encoded(request) => format!("{:?}", request.recipe().representation),
                TextureSource::Framebuffer(_) => "Framebuffer".into(),
                TextureSource::Rgba(_) => unreachable!(),
            },
            Err(kind) => format!("{kind:?}"),
        };
        let framebuffer_decode = old.as_ref().ok().and_then(|binding| match &binding.source {
            TextureSource::Framebuffer(request) => Some(if request.materialize {
                "materialized".into()
            } else {
                "exact".into()
            }),
            _ => None,
        });
        let witness = old.as_ref().ok().and_then(|binding| match &binding.source {
            TextureSource::Encoded(request) => {
                Some(request.identity_profiled(&Default::default()).1.to_vec())
            }
            _ => None,
        });
        let framebuffer = rdp.tmem_bank.has_framebuffer(tile, tlut);
        let bank = rdp.tmem_bank.share_bank();
        let framebuffer_tags =
            rdp.tmem_bank
                .framebuffer_tags
                .as_ref()
                .map_or_else(Vec::new, |tags| {
                    tags.bytes
                        .iter()
                        .enumerate()
                        .filter(|(_, tag)| tag.copy != 0)
                        .map(|(address, tag)| {
                            (
                                address as u16,
                                tags.copies[usize::from(tag.copy) - 1].sequence,
                                tag.pointer,
                            )
                        })
                        .collect::<Vec<_>>()
                });
        let mut reads = [false; 4096];
        let mut read = |address| reads[address] = true;
        let footprint = if let Ok(binding) = &old {
            let request = match &binding.source {
                TextureSource::Encoded(request) => request,
                TextureSource::Framebuffer(request) => &request.encoded,
                TextureSource::Rgba(_) => unreachable!(),
            };
            Ok(request.decode_observed(&mut read))
        } else if lookup {
            rdp.tmem_bank
                .sampling_lookup_observed(tile, tlut, &mut read)
        } else {
            let sized = TileDescriptor {
                width: tile.width.max(1),
                height: tile.height.max(1),
                ..tile.clone()
            };
            rdp.tmem_bank.sample_tile_observed(&sized, tlut, &mut read)
        };
        let sampled: Vec<u16> = reads
            .iter()
            .enumerate()
            .filter(|(_, read)| **read)
            .map(|(address, _)| address as u16)
            .collect();
        let sampled_framebuffer = framebuffer_tags
            .iter()
            .any(|(address, _, _)| reads[usize::from(*address)]);
        let sample_bytes: Vec<u8> = sampled
            .iter()
            .map(|&address| bank[usize::from(address)])
            .collect();
        let writers = sampled
            .iter()
            .map(|&address| {
                (
                    address,
                    rdp.tmem_bank
                        .census_writers
                        .as_ref()
                        .map_or(0, |writers| writers[usize::from(address)]),
                )
            })
            .collect();
        let physical = match &old {
            Ok(binding) => match &binding.source {
                TextureSource::Encoded(request) => Ok(request.decode()),
                _ => Err("GPU framebuffer copies unavailable on CPU".into()),
            },
            Err(kind) => Err(format!("{kind:?}")),
        };
        if let (Ok(footprint), Ok(physical)) = (&footprint, &physical) {
            assert_eq!(footprint, physical);
        }
        let decode_error = physical.as_ref().err().cloned();
        let mut physical = physical.unwrap_or_default();
        let class = if framebuffer {
            physical.clear();
            "unverified"
        } else if decode_error.is_some() {
            "unverified"
        } else {
            "unaffected"
        }
        .into();
        Self {
            pc,
            role,
            tile: tile.clone(),
            tlut,
            outcome,
            witness,
            draw_rejection: None,
            framebuffer,
            framebuffer_decode,
            sampled_framebuffer,
            framebuffer_tags,
            bank: bank.to_vec(),
            sampled,
            sampled_hash: twox_hash::XxHash3_64::oneshot(&sample_bytes),
            writers,
            physical_representation: if lookup { "Lookup4096x4" } else { "Tile" }.into(),
            extent: old.as_ref().map_or_else(
                |_| sampling.allocation_extent(),
                |binding| match &binding.source {
                    TextureSource::Encoded(request) => request.recipe().allocation_extent(),
                    TextureSource::Framebuffer(request) => {
                        request.encoded.recipe().allocation_extent()
                    }
                    TextureSource::Rgba(_) => unreachable!(),
                },
            ),
            physical,
            class,
            decode_error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framebuffer_census_and_trace_do_not_decode_placeholder_bytes() {
        use crate::hle::framebuffer_load::FramebufferCopy;
        use crate::render::framebuffers::{targets::ImageDescriptor, ImageLayout};
        use std::sync::Arc;
        for (fmt, siz, mode) in [
            (0, 2, "exact"),
            (3, 1, "materialized"),
            (4, 0, "materialized"),
        ] {
            let mut rdp = Rdp {
                texture_loaded: true,
                combine_l: 0x00ff_ffff,
                combine_h: 0xfffc_f279,
                ..Default::default()
            };
            let source = ImageDescriptor {
                address: 0x10000,
                layout: ImageLayout {
                    width: 4,
                    fmt: 0,
                    siz: 2,
                },
                height: 1,
                generation: 1,
                depth: false,
            };
            let copy = Arc::new(FramebufferCopy::new(1, source, source.address, 8, 8, 1));
            rdp.tmem_bank
                .copy_load(copy, |bank| bank.write_tile(&[], 0, 1, 1, 1, 8, 1));
            rdp.tiles[0] = TileDescriptor {
                fmt,
                siz,
                width: 4,
                height: 1,
                line: 1,
                ..Default::default()
            };
            let attempt = CensusAttempt::capture(&rdp, &rdp.tiles[0], 0x20, "tex0".into());
            assert_eq!(attempt.outcome, "Framebuffer");
            assert_eq!(attempt.framebuffer_decode.as_deref(), Some(mode));
            assert_eq!(attempt.class, "unverified");
            assert_eq!(
                attempt.decode_error.as_deref(),
                Some("GPU framebuffer copies unavailable on CPU")
            );
            assert!(attempt.physical.is_empty());
            assert!(attempt.witness.is_none());
            assert!(attempt.framebuffer && attempt.sampled_framebuffer);
            let mut rsp = Rsp::default();
            rsp.profiling = crate::profiling::Recorder::new(crate::profiling::Mode::Trace);
            let material =
                crate::hle::combiner::build_material(&rdp, &rsp, &mut Vec::new(), 0x20).unwrap();
            assert!(matches!(material.texture, TextureSource::Framebuffer(_)));
            let snapshot = rsp.profiling.drain();
            assert!(snapshot.requests.is_empty());
            assert_eq!(snapshot.counters["framebuffer.texture_requests"], 1);
            assert_eq!(
                snapshot.counters[&format!("framebuffer.{mode}_requests")],
                1
            );
        }
    }

    #[test]
    fn census_palette_planes_have_exportable_extents_and_lane_footprints() {
        let mut rdp = Rdp {
            other_mode_h: 3 << 14,
            ..Default::default()
        };
        rdp.tmem_bank.write_tlut(&[0x10, 255], 1, 256);
        rdp.tmem_bank.write_tile(
            &[0x10, 255, 0x30, 255, 0x50, 255, 0x70, 255],
            256,
            0,
            1,
            1,
            8,
            1,
        );
        let tile = TileDescriptor {
            fmt: 2,
            siz: 1,
            width: 1,
            height: 1,
            ..Default::default()
        };
        let attempt = CensusAttempt::capture(&rdp, &tile, 0, "tex0".into());
        assert_eq!(attempt.extent, [1, 4]);
        assert_eq!(
            attempt.physical,
            [
                0x10, 0x10, 0x10, 255, 0x30, 0x30, 0x30, 255, 0x50, 0x50, 0x50, 255, 0x70, 0x70,
                0x70, 255
            ]
        );
        assert_eq!(
            attempt.sampled,
            [0, 2048, 2049, 2050, 2051, 2052, 2053, 2054, 2055]
        );
    }

    #[test]
    fn framebuffer_footprints_follow_lookup_and_palette_lanes() {
        use crate::hle::framebuffer_load::FramebufferCopy;
        use crate::render::framebuffers::{targets::ImageDescriptor, ImageLayout};
        use std::sync::Arc;
        let source = ImageDescriptor {
            address: 0x10000,
            layout: ImageLayout {
                width: 4,
                fmt: 0,
                siz: 2,
            },
            height: 1,
            generation: 1,
            depth: false,
        };
        let copy = || Arc::new(FramebufferCopy::new(1, source, source.address, 8, 8, 1));

        let mut rdp = Rdp::default();
        rdp.tmem_bank
            .copy_load(copy(), |bank| bank.write_tile(&[], 1, 1, 1, 1, 8, 1));
        let tile = TileDescriptor {
            fmt: 0,
            siz: 2,
            width: 4,
            height: 1,
            line: 1,
            masks: 3,
            ..Default::default()
        };
        let attempt = CensusAttempt::capture(&rdp, &tile, 0, "tex0".into());
        assert_eq!(attempt.outcome, "Framebuffer");
        assert_eq!(attempt.physical_representation, "Lookup4096x4");
        assert!(attempt.sampled_framebuffer);

        let mut rdp = Rdp {
            other_mode_h: 3 << 14,
            ..Default::default()
        };
        rdp.tmem_bank
            .copy_load(copy(), |bank| bank.write_tile(&[], 0, 1, 1, 1, 8, 1));
        rdp.tmem_bank.write_tile(
            &[0x10, 255, 0x30, 255, 0x50, 255, 0x70, 255],
            256,
            0,
            1,
            1,
            8,
            1,
        );
        let tile = TileDescriptor {
            fmt: 2,
            siz: 1,
            width: 1,
            height: 1,
            ..Default::default()
        };
        let attempt = CensusAttempt::capture(&rdp, &tile, 0, "tex0".into());
        assert_eq!(attempt.outcome, "Framebuffer");
        assert_eq!(
            attempt.sampled,
            [0, 2048, 2049, 2050, 2051, 2052, 2053, 2054, 2055]
        );
    }

    #[test]
    fn narrow_blocks_decode_physical_rows() {
        let mut rdp = Rdp::default();
        rdp.tmem_bank
            .write_block(&[1, 2, 3, 4, 0, 0, 0, 0], 0, 0, 0, 1, 1);
        let mut tile = TileDescriptor {
            fmt: 4,
            siz: 1,
            width: 4,
            height: 1,
            line: 1,
            ..Default::default()
        };
        let equal = CensusAttempt::capture(&rdp, &tile, 0x20, "tex0".into());
        assert_eq!(equal.class, "unaffected");
        assert_eq!(equal.physical, [[1; 4], [2; 4], [3; 4], [4; 4]].concat());
        rdp.tmem_bank.write_block(
            &[
                0x12, 0x30, 0x45, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x56, 0x70, 0, 0,
            ],
            0,
            0,
            0,
            2,
            0,
        );
        tile.siz = 0;
        tile.width = 3;
        tile.height = 2;
        let changed = CensusAttempt::capture(&rdp, &tile, 0x28, "tex0".into());
        assert_eq!(changed.class, "unaffected");
        assert_eq!(
            changed.physical,
            [[17; 4], [34; 4], [51; 4], [85; 4], [102; 4], [119; 4]].concat()
        );
    }

    #[test]
    fn narrow_rgba32_request_has_a_physical_footprint() {
        let mut rdp = Rdp::default();
        rdp.tmem_bank.write_block(
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 0, 0, 0, 0],
            0,
            0,
            0,
            2,
            3,
        );
        let tile = TileDescriptor {
            fmt: 0,
            siz: 3,
            width: 3,
            height: 1,
            line: 1,
            ..Default::default()
        };
        let attempt = CensusAttempt::capture(&rdp, &tile, 0x30, "tex1".into());
        assert_eq!(attempt.class, "unaffected");
        assert_eq!(attempt.outcome, "Tile");
        assert_eq!(attempt.physical, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(
            attempt.sampled,
            [0, 1, 2, 3, 4, 5, 2048, 2049, 2050, 2051, 2052, 2053]
        );
    }

    #[test]
    fn narrow_rgba32_routing_keeps_literal_texels() {
        let mut rdp = Rdp::default();
        rdp.tmem_bank.write_block(
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 0, 0, 0, 0],
            0,
            0,
            0,
            2,
            3,
        );
        let tile = TileDescriptor {
            fmt: 0,
            siz: 3,
            width: 3,
            height: 1,
            line: 1,
            ..Default::default()
        };
        let binding = texture_request::prepare(&rdp, &tile, 0, &Default::default()).unwrap();
        assert_eq!(
            binding.source.decode().as_ref(),
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
        );
    }

    #[test]
    fn poison_precedes_format_validation_in_the_candidate() {
        let mut rdp = Rdp::default();
        let diagnostic = crate::Diagnostic {
            at: 0x10,
            kind: crate::DiagKind::UnsupportedFramebufferAccess {
                address: 0x1000,
                reason: crate::FramebufferAccess::Reinterpretation,
            },
        };
        rdp.tmem_bank
            .reject_load(diagnostic, |bank| bank.write_tile(&[], 0, 1, 1, 1, 8, 1));
        let tile = TileDescriptor {
            fmt: 7,
            siz: 1,
            width: 1,
            height: 1,
            line: 1,
            ..Default::default()
        };
        let attempt = CensusAttempt::capture(&rdp, &tile, 0x20, "tex0".into());
        assert_eq!(attempt.decode_error.as_ref(), Some(&attempt.outcome));
        rdp.tiles[0] = tile;
        rdp.combine_l = 0x00ff_ffff;
        rdp.combine_h = 0xfffc_f279;
        let mut rsp = Rsp::default();
        rsp.profiling = crate::profiling::Recorder::new(crate::profiling::Mode::Census);
        census_material(&rdp, &rsp, 0x20, Some(0), None);
        assert_eq!(
            rsp.profiling.drain().census_attempts[0].outcome,
            attempt.outcome
        );
    }

    #[test]
    fn unset_extents_decode_one_texel() {
        let mut rdp = Rdp::default();
        rdp.tmem_bank.write_block(&[0x12; 8], 0, 0, 0, 1, 1);
        let tile = TileDescriptor {
            fmt: 4,
            siz: 1,
            line: 1,
            ..Default::default()
        };
        let attempt = CensusAttempt::capture(&rdp, &tile, 0x20, "tex0".into());
        assert_eq!(attempt.class, "unaffected");
        assert_eq!(attempt.extent, [1, 1]);
        assert_eq!(attempt.physical, [0x12; 4]);
        assert_eq!(attempt.decode_error, None);
    }

    #[test]
    fn textured_fill_before_color_image_is_observed() {
        use n64_gbi::encode::*;
        let mut dl = crate::tests::dl_builder::DlBuilder::new();
        dl.list(
            "main",
            &[
                gdp_set_cycle_type(0),
                (0xfcff_ffff, 0xfffc_f279),
                gdp_fill_rectangle(0, 0, 16, 16),
                gsp_enddl(),
            ],
        );
        let built = dl.finish("main");
        let mut cpu = crate::profiling::CpuInterpreter {
            recorder: crate::profiling::Recorder::new(crate::profiling::Mode::Census),
            ..Default::default()
        };
        let (_, diags) = cpu.process(
            crate::RdramImage::new(&built.rdram),
            built.entry.into(),
            crate::Microcode::F3d,
            crate::DataFormat::Fixed,
        );
        assert!(diags
            .iter()
            .any(|diag| diag.kind == crate::DiagKind::DrawBeforeCimg));
        let attempts = cpu.recorder.drain().census_attempts;
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].role, "tex0");
        assert_eq!(
            attempts[0].draw_rejection.as_deref(),
            Some("DrawBeforeCimg")
        );
    }
}
