// Glitch Block Slice Displacement

fn hash21(p: vec2<f32>) -> f32 {
    let p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    let p4 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p4.x + p4.y) * p4.z);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let freq = max(params.band_frequency, 2.0);
    let band_idx = floor(in.uv.y * freq);
    let time_step = floor(globals.time * params.speed);

    let trigger = hash21(vec2<f32>(band_idx, time_step));
    var shift: f32 = 0.0;

    if (trigger < params.intensity) {
        let raw_shift = hash21(vec2<f32>(band_idx * 17.3, time_step * 3.7)) - 0.5;
        shift = raw_shift * 0.25 * params.intensity;
    }

    let drift = params.color_drift;
    let uv_r = clamp(vec2<f32>(in.uv.x + shift + drift, in.uv.y), vec2<f32>(0.0), vec2<f32>(1.0));
    let uv_g = clamp(vec2<f32>(in.uv.x + shift, in.uv.y), vec2<f32>(0.0), vec2<f32>(1.0));
    let uv_b = clamp(vec2<f32>(in.uv.x + shift - drift, in.uv.y), vec2<f32>(0.0), vec2<f32>(1.0));

    let r = textureSample(input_tex, linear_sampler, uv_r).r;
    let g = textureSample(input_tex, linear_sampler, uv_g).g;
    let b = textureSample(input_tex, linear_sampler, uv_b).b;
    let a = textureSample(input_tex, linear_sampler, in.uv).a;

    return vec4<f32>(r, g, b, a);
}
