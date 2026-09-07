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
    let pixel = u32(position.y) * fill.y + u32(position.x);
    let packed = select(fill.x >> 16u, fill.x & 0xffffu, (pixel & 1u) != 0u);
    let exponent = packed >> 13u;
    let mantissa = (packed & 0x1ffcu) >> 2u;
    let fixed = (mantissa << (6u - min(6u, exponent))) + 0x40000u - (0x40000u >> exponent);
    return f32(fixed) / 262143.0;
}

@fragment
fn copy_depth(@builtin(position) position: vec4<f32>) -> @builtin(frag_depth) f32 {
    return textureLoad(source, vec2<i32>(position.xy), 0);
}
