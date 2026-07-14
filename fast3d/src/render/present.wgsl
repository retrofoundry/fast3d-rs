// present.wgsl — fullscreen-triangle blit shader.
// Vertex: @builtin(vertex_index) → clip-space position + texel-center UVs.
// Fragment: textureSample(src, samp, uv).
// Pipeline layout: group0_bgl (binding0=texture_2d<f32>, binding1=sampler).

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0)       uv:  vec2<f32>,
}

// Fullscreen triangle: three vertices cover the entire clip space.
// WebGPU NDC has y UP, while the framebuffer origin is top-left with y DOWN (viewport transform
// `yf = (1 − ndc_y)/2 · H`). The intermediate FB texture is stored row-0-at-top — the `textured_fb`
// passes AND the 2D rect-quad path both write framebuffer row 0 = screen top — and `textureSample`
// reads v=0 at the top texel row. So the UV must FLIP Y relative to NDC: v = (1 − y) * 0.5, NOT the
// old (y + 1) * 0.5 (which blitted the FB upside-down — a latent vertical flip that went unnoticed
// because the only paired test rendered vertically-symmetric content). Horizontal is unflipped.
//   vi=0: NDC (-1,-1) [screen bottom-left] → UV (0, 1)  [FB bottom-left]
//   vi=1: NDC ( 3,-1) → UV (2, 1)
//   vi=2: NDC (-1, 3) → UV (0, -1)
// The rasterizer clips to [−1,1]×[−1,1]; the triangle covers exactly that region.
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var x = array<f32, 3>(-1.0,  3.0, -1.0);
    var y = array<f32, 3>(-1.0, -1.0,  3.0);
    let px = x[vi];
    let py = y[vi];
    var out: VsOut;
    out.pos = vec4<f32>(px, py, 0.0, 1.0);
    out.uv  = vec2<f32>((px + 1.0) * 0.5, (1.0 - py) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(src, samp, in.uv);
}
