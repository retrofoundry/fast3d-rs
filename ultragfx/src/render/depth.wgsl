@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

struct Attributes {
    @location(0) delta: u32,
    @builtin(frag_depth) depth: f32,
};

fn unpack_z(packed: u32) -> f32 {
    let exponent = packed >> 13u;
    let mantissa = (packed & 0x1ffcu) >> 2u;
    let fixed = (mantissa << (6u - min(6u, exponent))) + 0x40000u - (0x40000u >> exponent);
    return f32(fixed) / 262144.0;
}

@group(0) @binding(0) var<uniform> fill: vec4<u32>;
@group(0) @binding(1) var source: texture_depth_2d;
@group(0) @binding(3) var source_delta: texture_2d<u32>;

@fragment
fn fill_depth(@builtin(position) position: vec4<f32>) -> Attributes {
    let native = vec2<u32>(position.xy) / fill.z;
    let pixel = native.y * fill.y + native.x;
    let packed = select(fill.x >> 16u, fill.x & 0xffffu, (pixel & 1u) != 0u);
    let code = ((packed & 3u) << 2u) | (3u * (packed & 1u));
    return Attributes(code, unpack_z(packed));
}

@fragment
fn copy_depth(@builtin(position) position: vec4<f32>) -> @builtin(frag_depth) f32 {
    return textureLoad(source, vec2<i32>(position.xy), 0);
}

@fragment
fn copy_attributes(@builtin(position) position: vec4<f32>) -> Attributes {
    let coord = vec2<i32>(position.xy);
    return Attributes(textureLoad(source_delta, coord, 0).r, textureLoad(source, coord, 0));
}

@group(0) @binding(2) var imported: texture_2d<u32>;

@fragment
fn import_depth(@builtin(position) position: vec4<f32>) -> Attributes {
    let word = textureLoad(imported, vec2<i32>(position.xy), 0).r;
    // Coherent imports contain visible words only; missing hidden dz bits are zero.
    return Attributes((word & 3u) << 2u, unpack_z(word));
}

@group(0) @binding(4) var<uniform> scale: vec4<u32>;

@fragment
fn expand_depth(@builtin(position) position: vec4<f32>) -> Attributes {
    let coord = vec2<i32>(vec2<u32>(position.xy) / scale.x);
    return Attributes(textureLoad(source_delta, coord, 0).r, textureLoad(source, coord, 0));
}
