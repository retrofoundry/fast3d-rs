struct Pixel {
    word: u32,
    coverage: u32,
    valid: u32,
}

struct Fill {
    bounds: vec4<u32>,
    width: u32,
    parity: u32,
    word: u32,
    valid: u32,
}

@group(0) @binding(0) var<storage, read_write> pixels: array<Pixel>;
@group(0) @binding(1) var<uniform> fill: Fill;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= fill.bounds.zw) {
        return;
    }
    let position = fill.bounds.xy + id.xy;
    let pixel = position.y * fill.width + position.x;
    let shift = 16u * (1u - ((pixel + fill.parity) & 1u));
    let word = (fill.word >> shift) & 0xffffu;
    pixels[pixel] = Pixel(word, (word & 1u) * 7u, fill.valid);
}
