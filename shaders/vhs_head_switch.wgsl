// VCR Drum Head-Switching Noise Band and Skew Distortion
// Globals and Params are auto-injected by ninja-shader

fn hash12(p: vec2<f32>) -> f32 {
    let p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    let d = dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * d);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let edge_dist = select(1.0 - in.uv.y, in.uv.y, params.position == 1);
    let h = max(params.band_height, 0.001);

    var uv = in.uv;
    var in_band = false;
    var band_factor = 0.0;

    if (edge_dist < h) {
        in_band = true;
        band_factor = (h - edge_dist) / h;

        // Flutter oscillation driven by frame and time
        let flutter_val = (sin(globals.time * 45.0) + sin(globals.time * 110.0)) * 0.5 * params.flutter;

        // Progressive horizontal skew
        let skew = (band_factor * band_factor) * (params.skew_amount + flutter_val * 0.015);
        uv.x = fract(uv.x + skew);
    }

    let base = textureSample(input_tex, linear_sampler, uv);
    let orig = textureSample(input_tex, linear_sampler, in.uv);

    var final_rgb = base.rgb;

    if (in_band) {
        // High-frequency magnetic tracking static lines
        let static_seed = vec2<f32>(
            floor(in.uv.x * 240.0),
            floor(in.uv.y * globals.resolution.y) + f32(globals.frame) * 17.3
        );
        let static_val = hash12(static_seed);

        // Desaturate and corrupt color inside the switching point
        let lum = dot(final_rgb, vec3<f32>(0.299, 0.587, 0.114));
        let corrupted_lum = mix(lum, static_val, params.noise_intensity * band_factor);
        let noise_color = vec3<f32>(corrupted_lum);

        final_rgb = mix(final_rgb, noise_color, band_factor * 0.85);
    }

    let blended_rgb = mix(orig.rgb, final_rgb, params.intensity);
    return vec4<f32>(clamp(blended_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), orig.a);
}
