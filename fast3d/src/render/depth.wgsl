@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

@group(0) @binding(0) var<uniform> fill: vec4<u32>;
@group(0) @binding(1) var source: texture_depth_2d;

@fragment
fn fill_depth(@builtin(position) position: vec4<f32>) -> @builtin(frag_depth) f32 {
    let native = vec2<u32>(position.xy) / fill.z;
    let pixel = native.y * fill.y + native.x;
    let packed = select(fill.x >> 16u, fill.x & 0xffffu, (pixel & 1u) != 0u);
    let exponent = packed >> 13u;
    let mantissa = (packed & 0x1ffcu) >> 2u;
    let fixed = (mantissa << (6u - min(6u, exponent))) + 0x40000u - (0x40000u >> exponent);
    return f32(fixed) / 262144.0;
}

@fragment
fn copy_depth(@builtin(position) position: vec4<f32>) -> @builtin(frag_depth) f32 {
    return textureLoad(source, vec2<i32>(position.xy), 0);
}

@group(0) @binding(2) var imported: texture_2d<f32>;

@fragment
fn import_depth(@builtin(position) position: vec4<f32>) -> @builtin(frag_depth) f32 {
    return textureLoad(imported, vec2<i32>(position.xy), 0).r;
}

@group(0) @binding(3) var<uniform> scale: vec4<u32>;

@fragment
fn expand_depth(@builtin(position) position: vec4<f32>) -> @builtin(frag_depth) f32 {
    return textureLoad(source, vec2<i32>(vec2<u32>(position.xy) / scale.x), 0);
}
