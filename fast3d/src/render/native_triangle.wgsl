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
struct Edges {
    y: vec4<i32>,
    x: vec4<i32>,
    dx: vec4<i32>,
    bounds: vec4<i32>,
}
struct Primitive { triangle: Triangle, edges: Edges }
struct Span { left: vec4<i32>, right: vec4<i32> }
struct PrimitiveData { items: array<Primitive, 256>, spans: array<Span> }
@group(0) @binding(0) var<storage, read_write> pixels: array<Pixel>;
@group(0) @binding(1) var<storage, read> vertices: array<Vertex>;
@group(0) @binding(2) var<storage, read_write> data: PrimitiveData;
@group(0) @binding(3) var<storage, read_write> bins: array<u32>;
struct Batch {
    extent: vec2<u32>, count: u32, tiles_x: u32,
    tile_start: u32, span_rows: u32,
}
@group(0) @binding(4) var<uniform> batch: Batch;
var<workgroup> tile_mask: array<atomic<u32>, 16>;

fn project(p: vec4<f32>, triangle: Triangle) -> vec2<f32> {
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

@compute @workgroup_size(64)
fn setup(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= batch.count { return; }
    let triangle = data.items[id.x].triangle;
    var q: array<vec2<i32>, 3>;
    for (var i = 0u; i < 3u; i++) {
        let p = vertices[triangle.indices[i]].position;
        let v = project(p, triangle);
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
    let lo = vec2<i32>(floor(vec2<f32>(min(q[0], min(q[1], q[2]))) / 32.0)) - vec2(1);
    let hi = vec2<i32>(floor(vec2<f32>(max(q[0], max(q[1], q[2]))) / 32.0)) + vec2(1);
    data.items[id.x].edges = Edges(
        vec4(a.y, b.y, c.y, ybase),
        vec4(xmajor, xupper, b.x * 16384, i32(upper >= major)),
        vec4((major >> 2) & ~1, (upper >> 2) & ~1, (lower >> 2) & ~1, 1),
        vec4(lo, hi));
}

fn edge_limits(edge: Edges, scan: i32) -> vec2<i32> {
    let mx = edge.x.x + edge.dx.x * (scan - edge.y.w);
    var nx: i32;
    if scan < edge.y.y {
        nx = edge.x.y + edge.dx.y * (scan - edge.y.w);
    } else {
        nx = edge.x.z + edge.dx.z * (scan - edge.y.y);
    }
    return vec2(quantize(select(nx, mx, edge.x.w != 0)),
                quantize(select(mx, nx, edge.x.w != 0)));
}

fn tile_coverage(edge: Edges, origin: vec2<u32>) -> u32 {
    let top = i32(origin.y) * 4;
    let first = max(top, edge.y.x);
    let last = min(top + 31, edge.y.z - 1);
    if first > last { return 0u; }
    let a = edge_limits(edge, first);
    let b = edge_limits(edge, last);
    var lo = min(a, b);
    var hi = max(a, b);
    if first < edge.y.y && last >= edge.y.y {
        let c = edge_limits(edge, edge.y.y - 1);
        let d = edge_limits(edge, edge.y.y);
        lo = min(lo, min(c, d));
        hi = max(hi, max(c, d));
    }
    let left = i32(origin.x) * 8;
    let right = left + 62;
    if lo.x > right || hi.y <= left { return 0u; }
    if first == top && last == top + 31 && hi.x <= left && lo.y > right { return 2u; }
    return 1u;
}

@compute @workgroup_size(8, 8)
fn spans(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= batch.count || id.y >= batch.span_rows { return; }
    let edge = data.items[id.x].edges;
    var span = Span(vec4(0x7fffffff), vec4(-2147483647));
    for (var sub = 0u; sub < 4u; sub++) {
        let scan = i32(id.y) * 4 + i32(sub);
        if edge.dx.w == 0 || scan < edge.y.x || scan >= edge.y.z { continue; }
        let limits = edge_limits(edge, scan);
        span.left[sub] = limits.x;
        span.right[sub] = limits.y;
    }
    data.spans[id.x * batch.span_rows + id.y] = span;
}

fn edge_mask(index: u32, triangle: Triangle, id: vec2<u32>) -> u32 {
    if id.y >= batch.span_rows || any(id >= triangle.extent) || any(id < triangle.scissor.xy)
        || any(id - triangle.scissor.xy >= triangle.scissor.zw) { return 0u; }
    let span = data.spans[index * batch.span_rows + id.y];
    let x = vec4(i32(id.x) * 8) + vec4(0, 2, 0, 2);
    let a = vec4<u32>((span.left <= x) & (x < span.right));
    let b = vec4<u32>((span.left <= x + 4) & (x + 4 < span.right));
    return (a.x << 7u) | (b.x << 6u) | (a.y << 5u) | (b.y << 4u)
        | (a.z << 3u) | (b.z << 2u) | (a.w << 1u) | b.w;
}

@compute @workgroup_size(64)
fn bin(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    if lane < 16u { atomicStore(&tile_mask[lane], 0u); }
    workgroupBarrier();
    let tile = batch.tile_start + group.x;
    let xy = vec2<i32>(i32(tile % batch.tiles_x), i32(tile / batch.tiles_x));
    for (var i = lane; i < batch.count; i += 64u) {
        let primitive = data.items[i];
        let triangle = primitive.triangle;
        let lo = primitive.edges.bounds.xy;
        let hi = primitive.edges.bounds.zw;
        let origin = vec2<u32>(xy) * 8u;
        if primitive.edges.dx.w != 0 && all(xy >= lo) && all(xy <= hi)
            && all(origin < triangle.scissor.xy + triangle.scissor.zw)
            && all(origin + 8u > triangle.scissor.xy) {
            let coverage = tile_coverage(primitive.edges, origin);
            if coverage != 0u { atomicOr(&tile_mask[i / 32u], 1u << (i % 32u)); }
            if coverage == 2u && all(origin >= triangle.scissor.xy)
                && all(origin + 8u <= triangle.scissor.xy + triangle.scissor.zw) {
                atomicOr(&tile_mask[8u + i / 32u], 1u << (i % 32u));
            }
        }
    }
    workgroupBarrier();
    if lane < 16u { bins[group.x * 16u + lane] = atomicLoad(&tile_mask[lane]); }
}

@compute @workgroup_size(8, 8)
fn main(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_id) local: vec3<u32>) {
    let tile = batch.tile_start + group.x;
    let id = vec2(tile % batch.tiles_x, tile / batch.tiles_x) * 8u + local.xy;
    if any(id >= batch.extent) { return; }
    let pixel = id.y * batch.extent.x + id.x;
    var state = pixels[pixel];
    for (var word = 0u; word < (batch.count + 31u) / 32u; word++) {
        var bits = bins[group.x * 16u + word];
        let full = bins[group.x * 16u + 8u + word];
        for (var step = 0u; step < 32u && bits != 0u; step++) {
            let bit = firstTrailingBit(bits);
            bits &= bits - 1u;
            let primitive = data.items[word * 32u + bit];
            let triangle = primitive.triangle;
            var mask = 0xffu;
            if (full & (1u << bit)) == 0u { mask = edge_mask(word * 32u + bit, triangle, id); }
            if mask == 0u || (triangle.aa == 0u && (mask & 0x80u) == 0u) { continue; }
            let n = countOneBits(mask);
            let memory = select(7u, state.coverage, triangle.image_read != 0u);
            var coverage = n - 1u;
            switch triangle.destination {
                case 0u: {
                    if triangle.aa != 0u && ((n + memory) & 8u) == 0u {
                        coverage = min(n + memory, 7u);
                    }
                }
                case 1u: { coverage = (n + memory) & 7u; }
                case 2u: { coverage = 7u; }
                default: { coverage = memory; }
            }
            state = Pixel(triangle.word | (coverage >> 2u), coverage, triangle.indices.w);
        }
    }
    pixels[pixel] = state;
}
