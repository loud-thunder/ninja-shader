// Vertical Hold Loss De-Sync, VBI Sync Bar, and Tape Skew
// Globals and Params are auto-injected by ninja-shader

fn hash12(p: vec2<f32>) -> f32 {
    let p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    let d = dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * d);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Rolling vertical position
    let roll_offset = fract(globals.time * params.roll_speed);
    let rolled_y = fract(in.uv.y + roll_offset);

    // Distance to the rolling VBI sync bar (at rolled_y == 0.0)
    let dist_to_bar = min(rolled_y, 1.0 - rolled_y);

    // Exponential horizontal skew shearing near the sync pulse
    let skew_factor = exp(-dist_to_bar * 24.0) * params.skew;

    // Sporadic tape slice tearing
    let tear_band = floor(in.uv.y * 30.0);
    let tear_noise = hash12(vec2<f32>(tear_band, floor(globals.time * 12.0)));
    let tear_offset = select(0.0, (tear_noise - 0.5) * 2.0 * params.tear_intensity, tear_noise > 0.85);

    // Sample distorted UV
    let sample_uv = vec2<f32>(
        fract(in.uv.x + skew_factor + tear_offset),
        rolled_y
    );

    let base = textureSample(input_tex, linear_sampler, sample_uv);
    let orig = textureSample(input_tex, linear_sampler, in.uv);

    var final_rgb = base.rgb;

    // Inside the Vertical Blanking Interval (VBI) bar
    if (rolled_y < params.sync_bar_width) {
        let vbi_norm = rolled_y / params.sync_bar_width;
        // Equalizing pulses and noise lines
        let pulse = step(0.5, fract(vbi_norm * 8.0));
        let bar_noise = hash12(in.uv * globals.resolution + vec2<f32>(globals.time * 83.1, 0.0));
        let vbi_pattern = mix(vec3<f32>(0.05), vec3<f32>(0.3 * pulse), 0.5);
        final_rgb = mix(vbi_pattern, vec3<f32>(bar_noise), params.sync_bar_noise);
    }

    let blended_rgb = mix(orig.rgb, final_rgb, params.intensity);
    return vec4<f32>(clamp(blended_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), orig.a);
}
