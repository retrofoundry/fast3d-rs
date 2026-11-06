struct Pixel { word: u32, coverage: u32, writer: u32 }
struct Vertex { position: vec4<f32>, color: vec4<f32>, uv: vec2<f32> }
struct Triangle {
    indices: vec4<u32>,
    scissor: vec4<u32>,
    extent: vec2<u32>,
    render_height: f32,
    word: u32,
    aa: u32,
    cull: u32,
    destination: u32,
    image_read: u32,
}
@group(0) @binding(0) var<storage, read_write> pixels: array<Pixel>;
@group(0) @binding(1) var<storage, read> vertices: array<Vertex>;
@group(0) @binding(2) var<uniform> triangle: Triangle;

fn project(p: vec4<f32>) -> vec2<f32> {
    return vec2((p.x / p.w + 1.0) * 0.5 * f32(triangle.extent.x),
                (1.0 - p.y / p.w) * 0.5 * triangle.render_height);
}

fn slope(a: vec2<i32>, b: vec2<i32>) -> i32 {
    let n = (b.x - a.x) * 65536;
    let d = max(b.y - a.y, 1);
    return (sign(n) * ((abs(n) + d - 1) / d)) & ~7;
}

fn quantize(x: i32) -> i32 {
    return ((x >> 13) & ~1) | i32((x & 0x3ffe) != 0);
}

fn after(a: vec2<i32>, b: vec2<i32>) -> bool {
    return a.y > b.y || (a.y == b.y && a.x > b.x);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= triangle.extent) || any(id.xy < triangle.scissor.xy)
        || any(id.xy - triangle.scissor.xy >= triangle.scissor.zw) { return; }
    var q: array<vec2<i32>, 3>;
    for (var i = 0u; i < 3u; i++) {
        let p = vertices[triangle.indices[i]].position;
        let v = project(p);
        if !(p.w == 1.0 && p.z >= 0.0 && p.z <= p.w)
            || !all(v >= vec2(-1024.0, -2048.0))
            || !all(v <= vec2(1023.75, 2047.75))
            || any(v * 4.0 != trunc(v * 4.0)) { return; }
        q[i] = vec2<i32>(v * 4.0);
    }
    let ab = q[1] - q[0];
    let ac = q[2] - q[0];
    let area = ab.x * ac.y - ab.y * ac.x;
    if area == 0 || (triangle.cull != 0u && f32(ab.x) * f32(ac.y) >= f32(ab.y) * f32(ac.x)) { return; }
    if after(q[0], q[1]) { let t = q[0]; q[0] = q[1]; q[1] = t; }
    if after(q[1], q[2]) { let t = q[1]; q[1] = q[2]; q[2] = t; }
    if after(q[0], q[1]) { let t = q[0]; q[0] = q[1]; q[1] = t; }
    let a = q[0]; let b = q[1]; let c = q[2];
    let major = slope(a, c);
    let upper = slope(a, b);
    let lower = slope(b, c);
    let ybase = a.y & ~3;
    let xmajor = a.x * 16384 - (major >> 2) * (a.y & 3);
    let xupper = a.x * 16384 - (upper >> 2) * (a.y & 3);
    var mask = 0u;
    for (var sub = 0u; sub < 4u; sub++) {
        let scan = i32(id.y) * 4 + i32(sub);
        if scan < a.y || scan >= c.y { continue; }
        let mx = xmajor + ((major >> 2) & ~1) * (scan - ybase);
        var nx: i32;
        if scan < b.y {
            nx = xupper + ((upper >> 2) & ~1) * (scan - ybase);
        } else {
            nx = b.x * 16384 + ((lower >> 2) & ~1) * (scan - b.y);
        }
        let l = quantize(select(nx, mx, upper >= major));
        let r = quantize(select(mx, nx, upper >= major));
        for (var sample = 0u; sample < 2u; sample++) {
            let x = i32(id.x) * 8 + i32(sub & 1u) * 2 + i32(sample) * 4;
            if l <= x && x < r { mask |= 1u << (7u - sub * 2u - sample); }
        }
    }
    if mask == 0u || (triangle.aa == 0u && (mask & 0x80u) == 0u) { return; }
    let pixel = id.y * triangle.extent.x + id.x;
    let n = countOneBits(mask);
    let memory = select(7u, pixels[pixel].coverage, triangle.image_read != 0u);
    var coverage = n - 1u;
    if triangle.destination == 2u {
        coverage = 7u;
    } else if triangle.aa != 0u && ((n + memory) & 8u) == 0u {
        coverage = min(n + memory, 7u);
    }
    pixels[pixel] = Pixel(triangle.word | (coverage >> 2u), coverage, triangle.indices.w);
}
