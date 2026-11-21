// Cross Process Film Color Emulation

fn scurve(x: f32, c: f32) -> f32 {
    let centered = (x - 0.5) * c;
    return clamp(0.5 + centered * (1.0 - 0.25 * centered * centered), 0.0, 1.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(input_tex, linear_sampler, in.uv);
    let c = max(params.contrast, 0.1);

    // Cross-process channel response
    var r = pow(scurve(color.r, c), 0.92);
    var g = scurve(color.g, c * 1.05);
    var b = scurve(color.b, c * 0.85);

    // Chemical highlight/shadow split: cyan shadows, amber/gold highlights
    let shadow_lift = (1.0 - color.r) * 0.12;
    b = b * 0.88 + shadow_lift;
    r = r * 1.08;

    let balance = params.shift_balance;
    if (balance > 0.0) {
        // Golden warm bias
        r = r + balance * 0.1;
        g = g + balance * 0.04;
        b = b - balance * 0.06;
    } else {
        // Cold cyan/teal bias
        let neg = -balance;
        r = r - neg * 0.08;
        g = g + neg * 0.06;
        b = b + neg * 0.12;
    }

    let processed = clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0));
    let final_rgb = mix(color.rgb, processed, params.strength);

    return vec4<f32>(final_rgb, color.a);
}
