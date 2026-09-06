use std::collections::BTreeMap;

use crate::hle::InterpResult;
use crate::scene::SceneOp;

use super::fixtures::FIXTURES;

pub(crate) fn builders_match_frozen_inputs() {
    let mut checked = 0;
    for fixture in FIXTURES {
        let Some(build) = fixture.build else { continue };
        let frozen_name = match fixture.name.split_once("--time-") {
            Some((family, _)) => format!("{family}--t{:08x}", fixture.time_bits),
            None => fixture.name.to_owned(),
        };
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/assembled")
            .join(format!("{frozen_name}.rdram"));
        let frozen = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("{}: {error}", fixture.name),
        };
        let built = build(fixture);
        let expected = normalize(
            crate::hle::interpret_rdram(&frozen, fixture.entry_addr),
            &frozen,
            fixture.entry_addr,
        );
        let actual = normalize(
            crate::hle::interpret_rdram(&built.rdram, built.entry),
            &built.rdram,
            built.entry,
        );
        if actual != expected {
            panic!(
                "{} ({}): {}",
                fixture.name,
                fixture.source,
                first_difference(&actual, &expected)
            );
        }
        eprintln!("{}: equal", fixture.name);
        checked += 1;
    }
    assert_eq!(checked, 27, "all pilot variants must be compared");
    eprintln!("{checked} builders match normalized frozen inputs");
}

fn normalize(mut result: InterpResult, bytes: &[u8], entry: u32) -> InterpResult {
    let mut ranges: Vec<_> = result
        .scene
        .framebuffer_pairs
        .iter()
        .map(|pair| {
            let length = u64::from(pair.color_image.width)
                * u64::from(pair.size_extent.1)
                * crate::hle::rsp::bpp(pair.color_image.siz);
            pair.color_image.addr..pair.color_image.addr + length
        })
        .filter(|range| !range.is_empty())
        .collect();
    ranges.sort_by_key(|range| range.start);
    let mut regions: Vec<std::ops::Range<u64>> = Vec::new();
    for range in ranges {
        if let Some(previous) = regions
            .last_mut()
            .filter(|previous| range.start < previous.end)
        {
            previous.end = previous.end.max(range.end);
        } else {
            regions.push(range);
        }
    }
    let mut addresses = BTreeMap::from([(0, 0)]);
    let mut remap = |address: &mut u64| {
        let base = regions
            .iter()
            .find(|range| range.contains(address))
            .map_or(*address, |range| range.start);
        let next = (addresses.len() as u64) << 32;
        *address = *addresses.entry(base).or_insert(next) + (*address - base);
    };
    remap(&mut result.scene.color_image.addr);
    for pair in &mut result.scene.framebuffer_pairs {
        remap(&mut pair.color_image.addr);
        if let Some(address) = &mut pair.depth_image {
            remap(address);
        }
        for op in &mut pair.ops {
            if let SceneOp::TexRect {
                fb_source: Some(address),
                ..
            } = op
            {
                remap(address);
            }
        }
    }
    remap(&mut result.rdp.color_image.addr);
    remap(&mut result.rdp.depth_image);
    remap(&mut result.rdp.tex_image.3);
    if !result.diags.is_empty() {
        let locations = dl_locations(bytes, entry);
        for diag in &mut result.diags {
            diag.at = *locations
                .get(&diag.at)
                .unwrap_or_else(|| panic!("unmapped diagnostic: {diag:?}"));
        }
    }
    result
}

fn dl_locations(bytes: &[u8], entry: u32) -> BTreeMap<u64, u64> {
    use n64_gbi::consts::*;
    let mut memory = crate::hle::mem::RdramImage::new(bytes);
    let mut pc = entry;
    let mut start = entry;
    let mut lists = BTreeMap::from([(entry, 0u64)]);
    let mut stack = Vec::new();
    let mut locations = BTreeMap::new();
    for _ in 0..=1 << 20 {
        locations.insert(u64::from(pc), (lists[&start] << 32) | u64::from(pc - start));
        let Some(command) = bytes.get(pc as usize..pc as usize + 8) else {
            break;
        };
        let w0 = u32::from_be_bytes(command[..4].try_into().unwrap());
        let w1 = u32::from_be_bytes(command[4..].try_into().unwrap());
        match (w0 >> 24) as u8 {
            G_DL => {
                if w0 & (1 << 16) == 0 {
                    stack.push((start, pc + 8));
                }
                pc = memory.from_segmented_masked(w1);
                let next = lists.len() as u64;
                lists.entry(pc).or_insert(next);
                start = pc;
            }
            G_ENDDL => match stack.pop() {
                Some((parent, ret)) => {
                    start = parent;
                    pc = ret;
                }
                None => break,
            },
            G_MOVEWORD if (w0 >> 16) & 0xff == u32::from(G_MW_SEGMENT) => {
                memory.set_segment((w0 >> 2) & 15, w1);
                pc += 8;
            }
            G_TEXRECT | G_TEXRECTFLIP => pc += 24,
            _ => pc += 8,
        }
    }
    locations
}

fn field_difference<T: PartialEq + std::fmt::Debug>(
    field: &str,
    actual: &T,
    expected: &T,
) -> Option<String> {
    if actual == expected {
        return None;
    }
    let a = format!("{actual:#?}");
    let e = format!("{expected:#?}");
    let (line, (actual, expected)) = a
        .lines()
        .zip(e.lines())
        .enumerate()
        .find(|(_, (a, e))| a != e)
        .unwrap_or((0, ("<different lengths>", "<different lengths>")));
    Some(format!(
        "first differing field {field}, line {}: built={} frozen={}",
        line + 1,
        actual.trim(),
        expected.trim()
    ))
}

fn first_difference(actual: &InterpResult, expected: &InterpResult) -> String {
    macro_rules! fields {
        ($($($field:ident).+),+ $(,)?) => { $(
            if let Some(diff) = field_difference(stringify!($($field).+), &actual.$($field).+, &expected.$($field).+) { return diff; }
        )+ };
    }
    fields!(
        scene.indices,
        scene.materials,
        scene.render_modes,
        scene.raw_pos,
        scene.modify_flags,
        scene.modify_screen,
        scene.mtx_index,
        scene.viewport_index,
        scene.mvp_table,
        scene.viewport_table,
        scene.raw_st,
        scene.texcoord_index,
        scene.texcoord_table,
        scene.draw_runs,
        scene.framebuffer_pairs,
        scene.color_image,
        scene.cn,
        scene.light_index,
        scene.light_count,
        scene.lights_table,
        scene.texgen_mode,
        scene.fog,
        scene.fog_table,
        scene.lookat_index,
        scene.lookat_table,
        scene.texgen_scale_table,
        diags,
        geometry_mode,
        rdp.tmem,
        rdp.tmem_bank,
        rdp.load_via_tile,
        rdp.tiles,
        rdp.tex_image,
        rdp.combine_l,
        rdp.combine_h,
        rdp.other_mode_h,
        rdp.other_mode_l,
        rdp.fog_color,
        rdp.fog_mul,
        rdp.fog_offset,
        rdp.prim,
        rdp.prim_lod_frac,
        rdp.prim_min_level,
        rdp.env,
        rdp.blend_color,
        rdp.tlut_fmt,
        rdp.color_image,
        rdp.color_changed,
        rdp.depth_image,
        rdp.depth_changed,
        rdp.scissor,
        rdp.fill_color_raw,
        commands,
        dropped_runs,
    );
    "unlisted InterpResult field".into()
}

#[test]
fn normalization_preserves_aliases_zero_and_none() {
    use crate::scene::FramebufferPair;
    let result = |address| {
        let mut r = InterpResult::default();
        r.scene.color_image.addr = address;
        r.scene.framebuffer_pairs = vec![FramebufferPair {
            color_image: r.scene.color_image,
            depth_image: Some(0),
            ..Default::default()
        }];
        r.rdp.color_image = r.scene.color_image;
        r.rdp.tex_image.3 = address;
        r
    };
    let expected = normalize(result(100), &[], 0);
    assert_eq!(expected, normalize(result(800), &[], 0));
    let mut distinct_texture = result(800);
    distinct_texture.rdp.tex_image.3 = 900;
    assert_ne!(expected, normalize(distinct_texture, &[], 0));
    let mut no_depth = result(800);
    no_depth.scene.framebuffer_pairs[0].depth_image = None;
    assert_ne!(expected, normalize(no_depth, &[], 0));
    assert_ne!(expected, normalize(result(0), &[], 0));
}

#[test]
fn diagnostics_are_relative_to_their_sub_list() {
    use super::dl_builder::{seg, DlBuilder};
    use n64_gbi::encode::*;
    let result = |padding, before| {
        let mut b = DlBuilder::new();
        b.bytes(16, &vec![0; padding]);
        let mut commands = vec![gdp_pipe_sync(); before];
        commands.extend([(0xab000000, 0), gsp_enddl()]);
        b.list("child", &commands);
        b.list(
            "main",
            &[
                b.segment(6, "child"),
                gsp_displaylist(seg(6, 0)),
                gsp_enddl(),
            ],
        );
        let built = b.finish("main");
        normalize(
            crate::hle::interpret_rdram(&built.rdram, built.entry),
            &built.rdram,
            built.entry,
        )
    };
    assert_eq!(result(0, 1), result(48, 1));
    assert_eq!(result(0, 1).diags[0].at, (1 << 32) | 8);
    assert_ne!(result(0, 1).diags, result(48, 2).diags);
}

#[test]
fn normalization_preserves_framebuffer_offsets() {
    use crate::scene::{ColorImage, FramebufferPair};
    let result = |base, offset| {
        let mut result = InterpResult::default();
        result.scene.color_image = ColorImage {
            fmt: 0,
            siz: 2,
            width: 32,
            addr: base,
        };
        result.scene.framebuffer_pairs.push(FramebufferPair {
            color_image: result.scene.color_image,
            depth_image: Some(base + 4096),
            size_extent: (32, 32),
            ..Default::default()
        });
        result.rdp.tex_image.3 = base + offset;
        normalize(result, &[], 0)
    };
    assert_eq!(result(100, 4), result(800, 4));
    assert_ne!(result(100, 4), result(800, 8));
    assert_ne!(result(100, 2047), result(800, 2048));
}
