// VHF/UHF Broadcast RF Static, Hum Bars, and Multipath Ghosting
// Globals and Params are auto-injected by ninja-shader

fn hash21(p: vec2<f32>) -> f32 {
    let p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    let d = dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * d);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let base = textureSample(input_tex, linear_sampler, in.uv);

    // Antenna multipath ghost reflection (offset horizontally)
    let ghost_uv = vec2<f32>(clamp(in.uv.x - params.ghost_lag, 0.0, 1.0), in.uv.y);
    let ghost_color = textureSample(input_tex, linear_sampler, ghost_uv);
    var composite_rgb = mix(base.rgb, ghost_color.rgb, params.ghost_opacity);

    // High-frequency temporal white noise (snow)
    let px = in.uv * globals.resolution;
    let seed = px + vec2<f32>(f32(globals.frame) * 137.5, globals.time * 59.3);
    let snow = hash21(seed);
    let snow_signed = (snow - 0.5) * 2.0;

    // Rolling 60Hz power-line AC hum bar
    let hum_phase = in.uv.y * 3.0 + globals.time * params.hum_bar_speed;
    let hum_wave = sin(hum_phase * 6.2831853) * 0.5 + 0.5;
    let hum_multiplier = 1.0 + (hum_wave - 0.5) * params.hum_bar_intensity;

    composite_rgb = composite_rgb * hum_multiplier;

    // Additive / multiplicative RF snow
    composite_rgb = composite_rgb + vec3<f32>(snow_signed * params.noise_intensity);

    // Signal loss: blend towards pure snow hiss
    let pure_snow = vec3<f32>(snow);
    let final_rgb = mix(composite_rgb, pure_snow, params.signal_loss);

    return vec4<f32>(clamp(final_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), base.a);
}
