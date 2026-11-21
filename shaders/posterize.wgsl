// Posterize Color Quantization

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(input_tex, linear_sampler, in.uv);
    let l = max(params.levels, 2.0);
    let g = max(params.gamma, 0.01);

    var rgb = pow(clamp(color.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(g));
    rgb = floor(rgb * l) / (l - 1.0);
    rgb = pow(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / g));

    let lum = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    rgb = mix(vec3<f32>(lum), rgb, params.saturation);

    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}
