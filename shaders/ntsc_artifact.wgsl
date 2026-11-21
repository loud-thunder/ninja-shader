// NTSC Composite Video Chroma Fringing, Phase Shift, and Ringing
// Globals and Params are auto-injected by ninja-shader

fn rgb2yiq(rgb: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        dot(rgb, vec3<f32>(0.299, 0.587, 0.114)),
        dot(rgb, vec3<f32>(0.596, -0.274, -0.322)),
        dot(rgb, vec3<f32>(0.211, -0.523, 0.312))
    );
}

fn yiq2rgb(yiq: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        yiq.x + 0.956 * yiq.y + 0.621 * yiq.z,
        yiq.x - 0.272 * yiq.y - 0.647 * yiq.z,
        yiq.x - 1.106 * yiq.y + 1.703 * yiq.z
    );
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let base = textureSample(input_tex, linear_sampler, in.uv);
    let orig_yiq = rgb2yiq(base.rgb);

    let dx = globals.inv_resolution.x * 1.5;
    let sample_l = textureSample(input_tex, linear_sampler, in.uv - vec2<f32>(dx, 0.0)).rgb;
    let sample_r = textureSample(input_tex, linear_sampler, in.uv + vec2<f32>(dx, 0.0)).rgb;

    let luma_l = rgb2yiq(sample_l).x;
    let luma_r = rgb2yiq(sample_r).x;
    let luma_grad = (luma_r - luma_l) * 2.5;

    // Subcarrier spatial oscillation across scanline
    let subcarrier = sin(in.uv.x * params.subcarrier_freq + globals.time * 8.0 + f32(globals.frame % 2u) * 3.14159);

    // Cross-color fringing (luma gradient leaking into chroma carrier)
    let cross_color = luma_grad * subcarrier * params.chroma_fringing;

    // Chroma horizontal delay (smear)
    let delayed_uv = vec2<f32>(clamp(in.uv.x - params.chroma_delay, 0.0, 1.0), in.uv.y);
    let delayed_sample = textureSample(input_tex, linear_sampler, delayed_uv).rgb;
    var delayed_yiq = rgb2yiq(delayed_sample);

    // Subcarrier phase rotation (tint knob error)
    let phase_rad = params.phase_shift * 3.14159;
    let cos_p = cos(phase_rad);
    let sin_p = sin(phase_rad);
    let rot_i = delayed_yiq.y * cos_p - delayed_yiq.z * sin_p;
    let rot_q = delayed_yiq.y * sin_p + delayed_yiq.z * cos_p;

    // Composite ringing (overshoot on vertical luminance steps)
    let ringing_term = luma_grad * sin(in.uv.x * params.subcarrier_freq * 1.5) * params.ringing * 0.15;
    let final_y = clamp(orig_yiq.x + ringing_term, 0.0, 1.0);
    let final_i = rot_i + cross_color;
    let final_q = rot_q + cross_color * 0.7;

    let artifact_rgb = yiq2rgb(vec3<f32>(final_y, final_i, final_q));
    let final_rgb = mix(base.rgb, artifact_rgb, params.intensity);

    return vec4<f32>(clamp(final_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), base.a);
}
