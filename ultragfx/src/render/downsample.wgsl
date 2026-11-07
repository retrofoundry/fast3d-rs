@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: vec4<u32>;
@group(0) @binding(2) var output: texture_storage_2d<rgba8unorm, write>;

var<workgroup> partials: array<vec4<f32>, 64>;

fn sum_rows(base: vec2<u32>, first_row: u32, row_step: u32) -> vec4<f32> {
    let extent = vec2<u32>(textureDimensions(source).x, params.y - params.x);
    let factor = params.z;
    let count = min(extent, vec2<u32>(factor));
    let origin = vec2<u32>(0u, params.x);
    var sum = vec4<f32>(0.0);
    for (var y = first_row; y < count.y; y += row_step) {
        var row_bytes = vec4<u32>(0u);
        var edge_bytes = vec4<u32>(0u);
        for (var x = 0u; x < count.x; x++) {
            let coord = origin + min(base + vec2<u32>(x, y), extent - vec2<u32>(1u));
            edge_bytes = vec4<u32>(round(textureLoad(source, vec2<i32>(coord), 0) * 255.0));
            row_bytes += edge_bytes;
        }
        let row_average = (vec4<f32>(row_bytes) + vec4<f32>(edge_bytes) * f32(factor - count.x)) / f32(factor);
        let weight = f32(select(1u, factor - y, y + 1u == count.y));
        sum += row_average * weight;
    }
    return sum;
}

@compute @workgroup_size(64)
fn box_filter(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    let factor = params.z;
    if (factor <= 8u) {
        let coord = group.xy * 8u + vec2<u32>(lane % 8u, lane / 8u);
        if (all(coord < textureDimensions(output))) {
            textureStore(output, vec2<i32>(coord), sum_rows(coord * factor, 0u, 1u) / f32(factor) / 255.0);
        }
    } else {
        partials[lane] = sum_rows(group.xy * factor, lane, 64u);
        workgroupBarrier();
        for (var stride = 32u; stride > 0u; stride /= 2u) {
            if (lane < stride) {
                partials[lane] += partials[lane + stride];
            }
            workgroupBarrier();
        }
        if (lane == 0u) {
            textureStore(output, vec2<i32>(group.xy), partials[0] / f32(factor) / 255.0);
        }
    }
}
