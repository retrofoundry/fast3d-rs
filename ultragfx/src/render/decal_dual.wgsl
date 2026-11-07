fn shade_decal_dual(in: VsOut) -> FragOut {
    decal_test(in);
    return shade_dual(in);
}

@fragment
fn fs_decal(in: VsOut) -> FragOut {
    return shade_decal_dual(in);
}

@fragment
fn fs_decal_depth(in: VsOut) -> DualDepthOutput {
    let color = shade_decal_dual(in);
    return DualDepthOutput(color.color0, color.color1, fragment_depth(in.clip_position.z));
}
