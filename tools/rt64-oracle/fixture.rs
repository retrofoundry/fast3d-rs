use crate::capture::{Fixture, Frame, Provenance, RecordingHardware};
use crate::hle::gbi::GbiUcode;
use crate::{ClearPolicy, DataFormat, Hardware, Microcode, Rdram, RdramImage, RendererConfig};
use n64_gbi::encode::{
    gdp_fill_rectangle, gdp_set_color_image, gdp_set_cycle_type_f3d, gdp_set_fill_color,
    gdp_set_scissor, gsp_clear_geometrymode_f3d, gsp_displaylist_f3d, gsp_enddl_f3d,
    gsp_matrix_f3d, gsp_set_geometrymode_f3d, gsp_viewport_f3d, mtx_identity_bytes, Vp,
};

const FRAMEBUFFER_ADDRESS: u32 = 0x0010_0000;
const DEPTH_ADDRESS: u32 = 0x0020_0000;

struct Image(Vec<u8>);

impl Hardware for Image {
    fn rdram(&self) -> impl Rdram + '_ {
        RdramImage::new(&self.0)
    }
}

fn append(bytes: &mut Vec<u8>, data: &[u8]) -> u32 {
    bytes.resize(bytes.len().next_multiple_of(8), 0);
    let address = u32::try_from(bytes.len()).unwrap();
    bytes.extend_from_slice(data);
    address
}

fn append_commands(bytes: &mut Vec<u8>, commands: &[(u32, u32)]) -> u32 {
    let mut data = Vec::with_capacity(commands.len() * 8);
    for &(w0, w1) in commands {
        data.extend_from_slice(&w0.to_be_bytes());
        data.extend_from_slice(&w1.to_be_bytes());
    }
    append(bytes, &data)
}

fn wrapper(bytes: &mut Vec<u8>, scene_entry: u32, width: u32, height: u32) -> u32 {
    let matrix = append(bytes, &mtx_identity_bytes());
    let viewport = append(
        bytes,
        &Vp {
            vscale: [(width * 2) as i16, (height * 2) as i16, 511, 0],
            vtrans: [(width * 2) as i16, (height * 2) as i16, 511, 0],
        }
        .to_bytes(),
    );
    append_commands(
        bytes,
        &[
            // A Z-buffered scene needs a depth image and a cleared depth buffer, or a renderer
            // that writes depth to RAM (rt64) rejects every fragment and clobbers address 0.
            (0xfe00_0000, DEPTH_ADDRESS),
            gdp_set_color_image(0, 2, width, DEPTH_ADDRESS),
            gdp_set_scissor(0, 0, 0, width * 4, height * 4),
            gdp_set_cycle_type_f3d(3),
            gdp_set_fill_color(0xfffc_fffc),
            gdp_fill_rectangle(0, 0, (width - 1) * 4, (height - 1) * 4),
            (0xe700_0000, 0),
            gdp_set_color_image(0, 2, width, FRAMEBUFFER_ADDRESS),
            gdp_set_fill_color(0x0001_0001),
            gdp_fill_rectangle(0, 0, (width - 1) * 4, (height - 1) * 4),
            (0xe700_0000, 0),
            gsp_clear_geometrymode_f3d(u32::MAX),
            gsp_set_geometrymode_f3d(n64_gbi::consts::rsp_f3d::G_CLIPPING),
            gsp_matrix_f3d(matrix, false, true, false),
            gsp_matrix_f3d(matrix, true, true, false),
            gsp_viewport_f3d(viewport),
            gsp_displaylist_f3d(scene_entry),
            (0xe900_0000, 0),
            gsp_enddl_f3d(),
        ],
    )
}

pub(super) fn make(
    mut bytes: Vec<u8>,
    scene_entry: u32,
    width: u32,
    height: u32,
    provenance: Provenance,
) -> Fixture {
    let entry = wrapper(&mut bytes, scene_entry, width, height);
    let hardware = Image(bytes);
    let recording = RecordingHardware::new(&hardware);
    let result = crate::hle::interpret(
        recording.rdram(),
        entry.into(),
        GbiUcode::F3d,
        DataFormat::Fixed,
        None,
    );
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    let task = recording
        .finish(entry.into(), Microcode::F3d, DataFormat::Fixed, 0)
        .unwrap();
    let color_image = task.final_color_image().unwrap();
    assert_eq!(color_image.addr, FRAMEBUFFER_ADDRESS.into());
    assert_eq!(color_image.width, u16::try_from(width).unwrap());
    assert_eq!((color_image.fmt, color_image.siz), (0, 2));
    let fixture = Fixture {
        frame: Frame {
            serial: 0,
            dither_seed: 0,
            config: RendererConfig {
                resolution_multiplier: 1,
                sample_count: 1,
                present_mode: wgpu::PresentMode::Fifo,
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                clear_policy: ClearPolicy::PerFrame,
                power_preference: wgpu::PowerPreference::None,
            },
            width,
            height,
            vi: None,
            dual_source_blending: false,
        },
        tasks: vec![task],
        provenance,
    };
    let encoded = fixture.to_bytes().unwrap();
    assert_eq!(Fixture::from_bytes(&encoded).unwrap(), fixture);
    fixture
}

pub(super) fn write(
    bytes: Vec<u8>,
    scene_entry: u32,
    width: u32,
    height: u32,
    filename: &str,
    provenance: Provenance,
) {
    let Some(output_dir) = std::env::var_os("FAST3D_WRITE_FIXTURES") else {
        eprintln!("FAST3D_WRITE_FIXTURES is not set; skipping {filename}");
        return;
    };
    let fixture = make(bytes, scene_entry, width, height, provenance);
    std::fs::create_dir_all(&output_dir).unwrap();
    std::fs::write(
        std::path::Path::new(&output_dir).join(filename),
        fixture.to_bytes().unwrap(),
    )
    .unwrap();
}

#[test]
fn wrapper_loads_both_identity_matrix_stacks() {
    let mut bytes = Vec::new();
    let entry = wrapper(&mut bytes, 0, 320, 240) as usize;
    let matrices: Vec<_> = bytes[entry..]
        .as_chunks::<8>()
        .0
        .iter()
        .filter_map(|command| {
            let w0 = u32::from_be_bytes(command[..4].try_into().unwrap());
            (w0 >> 24 == 0x01).then(|| (w0, u32::from_be_bytes(command[4..].try_into().unwrap())))
        })
        .collect();
    assert_eq!(matrices, [(0x0102_0000, 0), (0x0103_0000, 0)]);
}
