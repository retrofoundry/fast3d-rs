@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

@group(0) @binding(0) var native_color: texture_2d<f32>;
@group(0) @binding(1) var<uniform> scale: vec4<u32>;

@fragment
fn import_color(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(native_color, vec2<i32>(vec2<u32>(position.xy) / scale.x), 0);
}
