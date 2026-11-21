// Procedural Voronoi / Cellular Generator Shader
// Generates high-performance Voronoi diagrams on GPU

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct GeneratorParams {
    resolution: vec2<f32>,
    scale: f32,
    jitter: f32,
    seed: u32,
    mode: u32,       // 0: Distance (F1), 1: Borders (F2 - F1), 2: Cells (Cell ID color)
    metric: u32,     // 0: Euclidean, 1: Manhattan
    animate: u32,    // 0: false, 1: true
    time: f32,
    speed: f32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0) var<uniform> gen: GeneratorParams;

fn hash22(p: vec2<f32>, s: f32) -> vec2<f32> {
    var p3 = fract(vec3<f32>(p.xyx) * vec3<f32>(0.1031, 0.1030, 0.0973) + s * 0.0137);
    p3 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p3.xx + p3.yz) * p3.zy);
}

fn hash23(p: vec2<f32>, s: f32) -> vec3<f32> {
    var p3 = fract(vec3<f32>(p.xyx) * vec3<f32>(0.1031, 0.1030, 0.0973) + s * 0.0179);
    p3 = p3 + dot(p3, p3.yxz + 33.33);
    return fract((p3.xxy + p3.yzz) * p3.zyx);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let aspect = gen.resolution.x / max(gen.resolution.y, 1.0);
    let p = vec2<f32>(in.uv.x * aspect, in.uv.y) * gen.scale;

    let base_cell = floor(p);
    let frac_cell = fract(p);

    var f1: f32 = 8.0;
    var f2: f32 = 8.0;
    var best_cell = vec2<f32>(0.0);

    for (var y: i32 = -1; y <= 1; y = y + 1) {
        for (var x: i32 = -1; x <= 1; x = x + 1) {
            let neighbor = vec2<f32>(f32(x), f32(y));
            let cell = base_cell + neighbor;
            var rand_pt = hash22(cell, f32(gen.seed));

            if (gen.animate != 0u) {
                let t = gen.time * gen.speed;
                rand_pt = 0.5 + 0.5 * sin(rand_pt * 6.2831853 + vec2<f32>(t, t * 1.3));
            }

            let pt = neighbor + (rand_pt - 0.5) * gen.jitter + vec2<f32>(0.5);
            let diff = pt - frac_cell;

            var dist: f32 = 0.0;
            if (gen.metric == 1u) {
                dist = abs(diff.x) + abs(diff.y);
            } else {
                dist = length(diff);
            }

            if (dist < f1) {
                f2 = f1;
                f1 = dist;
                best_cell = cell;
            } else if (dist < f2) {
                f2 = dist;
            }
        }
    }

    var color = vec3<f32>(1.0);

    if (gen.mode == 0u) {
        // Distance (F1)
        let d = clamp(f1, 0.0, 1.0);
        color = vec3<f32>(d);
    } else if (gen.mode == 1u) {
        // Borders (F2 - F1)
        let border = clamp(f2 - f1, 0.0, 1.0);
        color = vec3<f32>(border);
    } else {
        // Cells (Cell ID color)
        color = hash23(best_cell, f32(gen.seed));
    }

    return vec4<f32>(color, 1.0);
}
