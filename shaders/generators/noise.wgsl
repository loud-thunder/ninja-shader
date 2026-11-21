// Procedural Simplex / Perlin / FBM Noise Generator Shader
// Generates gradient noise and multi-octave FBM on GPU

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct NoiseParams {
    resolution: vec2<f32>,
    scale: f32,
    seed: u32,
    octaves: u32,
    persistence: f32,
    lacunarity: f32,
    animate: u32,    // 0: false, 1: true
    time: f32,
    speed: f32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0) var<uniform> gen: NoiseParams;

fn hash22(p: vec2<f32>, s: f32) -> vec2<f32> {
    var p3 = fract(vec3<f32>(p.xyx) * vec3<f32>(0.1031, 0.1030, 0.0973) + s * 0.0137);
    p3 = p3 + dot(p3, p3.yzx + 33.33);
    let r = fract((p3.xx + p3.yz) * p3.zy);
    // Map to [-1, 1] unit vectors
    let angle = r.x * 6.2831853;
    return vec2<f32>(cos(angle), sin(angle));
}

// 2D Perlin gradient noise
fn perlin2d(p: vec2<f32>, s: f32) -> f32 {
    let pi = floor(p);
    let pf = fract(p);

    // Quintic interpolation curve: 6t^5 - 15t^4 + 10t^3
    let w = pf * pf * pf * (pf * (pf * 6.0 - 15.0) + 10.0);

    let g00 = dot(hash22(pi + vec2<f32>(0.0, 0.0), s), pf - vec2<f32>(0.0, 0.0));
    let g10 = dot(hash22(pi + vec2<f32>(1.0, 0.0), s), pf - vec2<f32>(1.0, 0.0));
    let g01 = dot(hash22(pi + vec2<f32>(0.0, 1.0), s), pf - vec2<f32>(0.0, 1.0));
    let g11 = dot(hash22(pi + vec2<f32>(1.0, 1.0), s), pf - vec2<f32>(1.0, 1.0));

    let nx0 = mix(g00, g10, w.x);
    let nx1 = mix(g01, g11, w.x);
    return mix(nx0, nx1, w.y);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let aspect = gen.resolution.x / max(gen.resolution.y, 1.0);
    var p = vec2<f32>(in.uv.x * aspect, in.uv.y) * gen.scale;

    if (gen.animate != 0u) {
        let t = gen.time * gen.speed * 0.5;
        p = p + vec2<f32>(t * 0.7, t * 0.4);
    }

    var total: f32 = 0.0;
    var amplitude: f32 = 1.0;
    var max_amplitude: f32 = 0.0;
    let octs = max(gen.octaves, 1u);
    let seed_f = f32(gen.seed);

    for (var i: u32 = 0u; i < octs; i = i + 1u) {
        total = total + perlin2d(p, seed_f + f32(i) * 17.13) * amplitude;
        max_amplitude = max_amplitude + amplitude;
        amplitude = amplitude * gen.persistence;
        p = p * gen.lacunarity;
    }

    // Normalize from [-1, 1] to [0, 1]
    let normalized = (total / max_amplitude) * 0.5 + 0.5;
    let val = clamp(normalized, 0.0, 1.0);

    return vec4<f32>(vec3<f32>(val), 1.0);
}
