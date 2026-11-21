// Luminous Edge Glow

fn lum(c: vec4<f32>) -> f32 {
    return dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let d = globals.inv_resolution;
    let base = textureSample(input_tex, linear_sampler, in.uv);

    // 3x3 Sobel neighborhood
    let tl = lum(textureSample(input_tex, linear_sampler, in.uv + vec2<f32>(-d.x, -d.y)));
    let tc = lum(textureSample(input_tex, linear_sampler, in.uv + vec2<f32>(0.0, -d.y)));
    let tr = lum(textureSample(input_tex, linear_sampler, in.uv + vec2<f32>(d.x, -d.y)));

    let ml = lum(textureSample(input_tex, linear_sampler, in.uv + vec2<f32>(-d.x, 0.0)));
    let mr = lum(textureSample(input_tex, linear_sampler, in.uv + vec2<f32>(d.x, 0.0)));

    let bl = lum(textureSample(input_tex, linear_sampler, in.uv + vec2<f32>(-d.x, d.y)));
    let bc = lum(textureSample(input_tex, linear_sampler, in.uv + vec2<f32>(0.0, d.y)));
    let br = lum(textureSample(input_tex, linear_sampler, in.uv + vec2<f32>(d.x, d.y)));

    let gx = (tr + 2.0 * mr + br) - (tl + 2.0 * ml + bl);
    let gy = (bl + 2.0 * bc + br) - (tl + 2.0 * tc + tr);
    let grad = length(vec2<f32>(gx, gy));

    let edge = smoothstep(params.threshold, params.threshold * 2.0, grad);
    let glow = params.edge_color * (edge * params.glow_intensity);

    let final_rgb = base.rgb * params.mix_source + glow;
    return vec4<f32>(clamp(final_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), base.a);
}
