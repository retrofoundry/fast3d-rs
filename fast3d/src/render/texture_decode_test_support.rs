use super::vectors::{self, Vector};

#[cfg(not(target_arch = "wasm32"))]
use super::export;

pub(super) fn vectors() -> Vec<Vector> {
    vectors::readback_vectors(cfg!(target_os = "windows"))
}

pub(super) fn verify(device: &wgpu::Device, vectors: &[Vector], decoded: &[Vec<u8>]) {
    #[cfg(not(target_arch = "wasm32"))]
    for (vector, actual) in vectors.iter().zip(decoded) {
        export::write_row(
            "FAST3D_GPU_DECODE_OUTPUT",
            &vector.name,
            "decode",
            "gpu-compute",
            "none",
            actual,
            vector.request.extent[0],
            actual.len() as u32 / (vector.request.extent[0] * 4),
            &vector.request.canonical(),
            Some(device),
        );
        if let Some(directory) = std::env::var_os("FAST3D_GPU_DECODE_OUTPUT") {
            std::fs::write(
                std::path::Path::new(&directory).join(format!("{}.expected.bin", vector.name)),
                &vector.expected,
            )
            .unwrap();
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = device;
    assert_decoded(vectors, decoded);
}

pub(super) fn assert_decoded(vectors: &[Vector], decoded: &[Vec<u8>]) {
    assert_eq!(decoded.len(), vectors.len());
    for (vector, actual) in vectors.iter().zip(decoded) {
        assert_eq!(
            vectors::oracle(&vector.request),
            vector.expected,
            "{}: CPU oracle",
            vector.name
        );
        assert_eq!(
            actual.len(),
            vector.expected.len(),
            "{}: output length",
            vector.name
        );
        if let Some((byte, (actual, expected))) = actual
            .iter()
            .zip(&vector.expected)
            .enumerate()
            .find(|(_, (a, b))| a != b)
        {
            panic!(
                "{}: GPU pixel {}, channel {}: got {}, expected {}",
                vector.name,
                byte / 4,
                byte % 4,
                actual,
                expected
            );
        }
    }
}
