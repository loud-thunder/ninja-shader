// Handheld LCD Dot-Matrix Display with Passive Matrix Ghosting
// Globals and Params are auto-injected by ninja-shader

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let current = textureSample(input_tex, linear_sampler, in.uv);
    let prev = textureSample(prev_frame_tex, linear_sampler, in.uv);

    // Passive-matrix liquid crystal slow decay: slow response time leaves trailing ghost
    let ghost_rgb = mix(current.rgb, max(current.rgb, prev.rgb * 0.96), params.ghosting);

    // LCD Cell Grid Coordinates
    let s = max(params.cell_size, 1.0);
    let px = in.uv * globals.resolution / s;
    let cell_uv = fract(px);

    // Dark boundary gap around each pixel cell
    let gap = clamp(params.grid_gap * 0.5, 0.0, 0.45);
    let in_cell_x = smoothstep(0.0, gap, cell_uv.x) * (1.0 - smoothstep(1.0 - gap, 1.0, cell_uv.x));
    let in_cell_y = smoothstep(0.0, gap, cell_uv.y) * (1.0 - smoothstep(1.0 - gap, 1.0, cell_uv.y));
    let cell_mask = clamp(in_cell_x * in_cell_y, 0.15, 1.0);

    var styled_rgb = ghost_rgb * cell_mask * params.brightness;

    if (params.subpixel_type == 1) {
        // Vintage Olive Reflective LCD (Game Boy green tint with dark border lines)
        let olive_tint = vec3<f32>(0.55, 0.65, 0.35);
        let lum = dot(styled_rgb, vec3<f32>(0.299, 0.587, 0.114));
        let shadow_grid = (1.0 - cell_mask) * 0.25;
        styled_rgb = mix(vec3<f32>(0.15, 0.22, 0.1), olive_tint * (lum + 0.15), lum) - vec3<f32>(shadow_grid);
    } else if (params.subpixel_type == 2) {
        // RGB Subpixel Striping inside each cell
        let subpixel_idx = i32(floor(cell_uv.x * 3.0));
        var sub_tint = vec3<f32>(0.35, 0.35, 0.35);
        if (subpixel_idx == 0) {
            sub_tint = vec3<f32>(1.2, 0.4, 0.4);
        } else if (subpixel_idx == 1) {
            sub_tint = vec3<f32>(0.4, 1.2, 0.4);
        } else {
            sub_tint = vec3<f32>(0.4, 0.4, 1.2);
        }
        styled_rgb = styled_rgb * sub_tint;
    }

    let final_rgb = mix(current.rgb, styled_rgb, params.intensity);
    return vec4<f32>(clamp(final_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), current.a);
}
