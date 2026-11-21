// Ordered 8x8 Bayer Matrix Dithering
// Globals and Params are auto-injected by ninja-shader

fn bayer8x8(coord: vec2<u32>) -> f32 {
    let bayer = array<u32, 64>(
         0u, 32u,  8u, 40u,  2u, 34u, 10u, 42u,
        48u, 16u, 56u, 24u, 50u, 18u, 58u, 26u,
        12u, 44u,  4u, 36u, 14u, 46u,  6u, 38u,
        60u, 28u, 52u, 20u, 62u, 30u, 54u, 22u,
         3u, 35u, 11u, 43u,  1u, 33u,  9u, 41u,
        51u, 19u, 59u, 27u, 49u, 17u, 57u, 25u,
        15u, 47u,  7u, 39u, 13u, 45u,  5u, 37u,
        63u, 31u, 55u, 23u, 61u, 29u, 53u, 21u
    );
    let idx = (coord.y % 8u) * 8u + (coord.x % 8u);
    return f32(bayer[idx]) / 64.0 - 0.5;
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.299, 0.587, 0.114));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let base = textureSample(input_tex, linear_sampler, in.uv);
    let s = max(params.scale, 1.0);
    let screen_px = vec2<u32>(in.uv * globals.resolution / s);
    let dither = bayer8x8(screen_px) * params.spread;

    let lum = clamp(luma(base.rgb) + dither, 0.0, 1.0);
    var dithered_rgb = vec3<f32>(0.0);

    if (params.palette == 1) {
        // Game Boy 4-shade olive green
        let c0 = vec3<f32>(0.059, 0.220, 0.059); // Darkest
        let c1 = vec3<f32>(0.188, 0.384, 0.188); // Dark olive
        let c2 = vec3<f32>(0.545, 0.675, 0.059); // Light green
        let c3 = vec3<f32>(0.608, 0.737, 0.059); // Bright olive

        let step_val = floor(lum * 3.999);
        if (step_val < 1.0) {
            dithered_rgb = c0;
        } else if (step_val < 2.0) {
            dithered_rgb = c1;
        } else if (step_val < 3.0) {
            dithered_rgb = c2;
        } else {
            dithered_rgb = c3;
        }
    } else if (params.palette == 2) {
        // CGA Mode 1 (Black, Cyan, Magenta, White)
        let c_black = vec3<f32>(0.0, 0.0, 0.0);
        let c_cyan = vec3<f32>(0.333, 1.0, 1.0);
        let c_mag = vec3<f32>(1.0, 0.333, 1.0);
        let c_white = vec3<f32>(1.0, 1.0, 1.0);

        let step_val = floor(lum * 3.999);
        if (step_val < 1.0) {
            dithered_rgb = c_black;
        } else if (step_val < 2.0) {
            dithered_rgb = c_cyan;
        } else if (step_val < 3.0) {
            dithered_rgb = c_mag;
        } else {
            dithered_rgb = c_white;
        }
    } else if (params.palette == 3) {
        // Amber CRT Phosphor (Monochrome amber ramp)
        let c_bg = vec3<f32>(0.02, 0.015, 0.0);
        let c_fg = vec3<f32>(1.0, 0.69, 0.0);
        let stepped_lum = floor(lum * 7.999) / 7.0;
        dithered_rgb = mix(c_bg, c_fg, stepped_lum);
    } else if (params.palette == 4) {
        // Apple Macintosh 1-bit Monochrome (pure black or white)
        if (lum > 0.5) {
            dithered_rgb = vec3<f32>(1.0, 1.0, 1.0);
        } else {
            dithered_rgb = vec3<f32>(0.0, 0.0, 0.0);
        }
    } else {
        // Custom quantization levels
        let l = max(params.levels, 2.0);
        let rgb_dither = clamp(base.rgb + vec3<f32>(dither), vec3<f32>(0.0), vec3<f32>(1.0));
        dithered_rgb = floor(rgb_dither * (l - 1.0) + 0.5) / (l - 1.0);
    }

    let final_rgb = mix(base.rgb, dithered_rgb, params.blend);
    return vec4<f32>(clamp(final_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), base.a);
}
