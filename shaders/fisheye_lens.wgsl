// Fisheye Lens & Barrel/Pincushion Distortion

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let aspect = globals.resolution.x / max(globals.resolution.y, 1.0);
    let centered = vec2<f32>((in.uv.x - 0.5) * aspect, in.uv.y - 0.5);
    let r = length(centered);

    if (r == 0.0) {
        return textureSample(input_tex, linear_sampler, in.uv);
    }

    let theta = atan2(centered.y, centered.x);
    let distorted_r = r * (1.0 + params.distortion * r * r);

    let new_uv = vec2<f32>(
        (distorted_r * cos(theta)) / aspect,
        distorted_r * sin(theta)
    ) / max(params.zoom, 0.01) + vec2<f32>(0.5);

    let in_bounds = new_uv.x >= 0.0 && new_uv.x <= 1.0 && new_uv.y >= 0.0 && new_uv.y <= 1.0;
    if (!in_bounds) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    let color = textureSample(input_tex, linear_sampler, new_uv);
    let border_dist = min(min(new_uv.x, 1.0 - new_uv.x), min(new_uv.y, 1.0 - new_uv.y));
    let mask = smoothstep(0.0, max(params.edge_feather, 0.001), border_dist);

    return vec4<f32>(color.rgb * mask, color.a);
}
