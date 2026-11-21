// CRT Hardware Phosphor Subpixel Mask

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(input_tex, linear_sampler, in.uv);
    let s = max(params.scale, 1.0);
    let px = in.uv * globals.resolution / s;

    var subpixel: i32 = 0;
    if (params.mask_type == 0) {
        // Vertical Aperture Grille (Trinitron striping)
        subpixel = ((i32(floor(px.x)) % 3) + 3) % 3;
    } else {
        // Shadow Mask (triad with offset scanlines)
        let y_odd = ((i32(floor(px.y)) % 2) + 2) % 2;
        let x_shift = select(0, 1, y_odd == 1);
        subpixel = (((i32(floor(px.x)) + x_shift) % 3) + 3) % 3;
    }

    var mask = vec3<f32>(0.25, 0.25, 0.25);
    if (subpixel == 0) {
        mask = vec3<f32>(1.0, 0.2, 0.2);
    } else if (subpixel == 1) {
        mask = vec3<f32>(0.2, 1.0, 0.2);
    } else {
        mask = vec3<f32>(0.2, 0.2, 1.0);
    }

    let masked = color.rgb * mask * params.brightness_boost;
    let final_rgb = mix(color.rgb, masked, params.intensity);

    return vec4<f32>(clamp(final_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}
