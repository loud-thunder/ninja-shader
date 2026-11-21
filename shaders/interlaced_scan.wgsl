// Interlaced Broadcast Scanfields and Motion Comb Artifacts
// Globals and Params are auto-injected by ninja-shader

fn hash11(p: f32) -> f32 {
    let p3 = fract(vec3<f32>(p * 0.1031, p * 0.1030, p * 0.0973));
    let d = dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * d);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let thick = max(params.line_thickness, 1.0);
    let line_y = in.uv.y * globals.resolution.y / thick;
    let line_idx = u32(floor(line_y));
    let is_odd_line = (line_idx % 2u) == 1u;

    // Field alternation based on temporal frame counter
    let active_field = (globals.frame % 2u) == u32(params.field_order);
    let line_active = (is_odd_line == active_field);

    // Micro vertical jitter
    let jitter_offset = (hash11(f32(globals.frame) * 1.7) - 0.5) * params.jitter;
    let uv_sampled = vec2<f32>(in.uv.x, clamp(in.uv.y + jitter_offset, 0.0, 1.0));

    let cur_sample = textureSample(input_tex, linear_sampler, uv_sampled);
    let prev_sample = textureSample(prev_frame_tex, linear_sampler, uv_sampled);

    // Comb artifacts: when rendering moving frames, inactive field holds previous frame's raster lines
    var interlace_rgb: vec3<f32>;
    if (line_active) {
        interlace_rgb = cur_sample.rgb;
    } else {
        // Inactive field shows comb tear by blending previous frame
        interlace_rgb = mix(cur_sample.rgb, prev_sample.rgb, params.comb_intensity);
    }

    // Scanfield luminance line darkening (raster lines)
    let line_phase = fract(line_y);
    let raster_falloff = 0.85 + 0.15 * sin(line_phase * 3.14159265);
    interlace_rgb = interlace_rgb * raster_falloff;

    // Field flicker (temporal oscillation)
    let flicker_factor = 1.0 + (select(-1.0, 1.0, active_field)) * (params.flicker * 0.04);
    interlace_rgb = interlace_rgb * flicker_factor;

    let final_rgb = mix(cur_sample.rgb, interlace_rgb, params.intensity);
    return vec4<f32>(clamp(final_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), cur_sample.a);
}
