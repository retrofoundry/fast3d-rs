use fast3d_gbi::defines::{ColorVertex, Vertex};
use pigment64::color::Color;

pub const SHADE_VTX: [Vertex; 4] = [
    Vertex {
        color: ColorVertex::new([-64, 64, -5], [0, 0], Color::RGBA(0, 0xFF, 0, 0xFF)),
    },
    Vertex {
        color: ColorVertex::new([64, 64, -5], [0, 0], Color::RGBA(0, 0, 0, 0xFF)),
    },
    Vertex {
        color: ColorVertex::new([64, -64, -5], [0, 0], Color::RGBA(0, 0, 0xFF, 0xFF)),
    },
    Vertex {
        color: ColorVertex::new([-64, -64, -5], [0, 0], Color::RGBA(0xFF, 0, 0, 0xFF)),
    },
];

pub fn create_vertices() -> Vec<Vertex> {
    SHADE_VTX.to_vec()
}
