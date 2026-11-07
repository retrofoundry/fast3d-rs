fn shade_decal(in: VsOut) -> vec4<f32> {
    decal_test(in);
    return shade_fragment(in);
}

@fragment
fn fs_decal(in: VsOut) -> @location(0) vec4<f32> {
    return shade_decal(in);
}

@fragment
fn fs_decal_depth(in: VsOut) -> DepthOutput {
    return DepthOutput(shade_decal(in), fragment_depth(in.clip_position.z));
}
