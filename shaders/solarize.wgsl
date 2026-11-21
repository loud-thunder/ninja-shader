// Solarize (Sabattier Effect)

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(input_tex, linear_sampler, in.uv);
    var inverted = vec3<f32>(0.0);

    if (params.mode == 0) {
        let lum = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        if (lum > params.threshold) {
            inverted = vec3<f32>(1.0) - color.rgb;
        } else {
            inverted = color.rgb;
        }
    } else {
        inverted = vec3<f32>(
            select(color.r, 1.0 - color.r, color.r > params.threshold),
            select(color.g, 1.0 - color.g, color.g > params.threshold),
            select(color.b, 1.0 - color.b, color.b > params.threshold)
        );
    }

    let final_rgb = mix(color.rgb, inverted, params.intensity);
    return vec4<f32>(final_rgb, color.a);
}
