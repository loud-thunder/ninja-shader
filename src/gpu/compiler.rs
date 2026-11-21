use crate::manifest::{ShaderLanguage, ShaderManifest};
use anyhow::{Context, Result};
use std::borrow::Cow;

pub const FULLSCREEN_VERTEX_WGSL: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let x = f32(i32(in_vertex_index & 1u) * 4 - 1);
    let y = f32(i32((in_vertex_index >> 1u) & 1u) * 4 - 1);
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    // UV 0,0 is top-left, 1,1 is bottom-right
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}
"#;

const FULLSCREEN_VERTEX_GLSL: &str = r#"#version 450
layout(location = 0) out vec2 uv;

void main() {
    float x = float(int(gl_VertexIndex & 1u) * 4 - 1);
    float y = float(int((gl_VertexIndex >> 1u) & 1u) * 4 - 1);
    gl_Position = vec4(x, y, 0.0, 1.0);
    uv = vec2((x + 1.0) * 0.5, (1.0 - y) * 0.5);
}
"#;

const WGSL_GROUP0_HEADER: &str = r#"
struct Globals {
    resolution: vec2<f32>,
    inv_resolution: vec2<f32>,
    time: f32,
    delta_time: f32,
    frame: u32,
    fps: f32,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var nearest_sampler: sampler;
@group(0) @binding(3) var input_tex: texture_2d<f32>;
@group(0) @binding(4) var original_tex: texture_2d<f32>;
@group(0) @binding(5) var prev_frame_tex: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};
"#;

const GLSL_GROUP0_HEADER: &str = r#"#version 450
precision highp float;

layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 out_color;

layout(set = 0, binding = 0) uniform Globals {
    vec2 resolution;
    vec2 inv_resolution;
    float time;
    float delta_time;
    uint frame;
    float fps;
} globals;

layout(set = 0, binding = 1) uniform sampler linear_sampler;
layout(set = 0, binding = 2) uniform sampler nearest_sampler;
layout(set = 0, binding = 3) uniform texture2D input_tex;
layout(set = 0, binding = 4) uniform texture2D original_tex;
layout(set = 0, binding = 5) uniform texture2D prev_frame_tex;
"#;

pub struct ShaderCompiler;

impl ShaderCompiler {
    pub fn create_vertex_module(device: &wgpu::Device, lang: ShaderLanguage) -> wgpu::ShaderModule {
        match lang {
            ShaderLanguage::Wgsl => device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("ninja_fullscreen_vertex_wgsl"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(FULLSCREEN_VERTEX_WGSL)),
            }),
            ShaderLanguage::Glsl => device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("ninja_fullscreen_vertex_glsl"),
                source: wgpu::ShaderSource::Glsl {
                    shader: Cow::Borrowed(FULLSCREEN_VERTEX_GLSL),
                    stage: naga::ShaderStage::Vertex,
                    defines: Default::default(),
                },
            }),
        }
    }

    pub fn compile_fragment_shader(
        device: &wgpu::Device,
        manifest: &ShaderManifest,
    ) -> Result<wgpu::ShaderModule> {
        let source_path = manifest.source_path();
        let raw_code = std::fs::read_to_string(&source_path)
            .with_context(|| format!("Failed to read shader file at {:?}", source_path))?;

        let full_code = Self::prepare_shader_source(manifest, &raw_code);

        let module = match manifest.language {
            ShaderLanguage::Wgsl => {
                device.push_error_scope(wgpu::ErrorFilter::Validation);
                let sm = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(&manifest.name),
                    source: wgpu::ShaderSource::Wgsl(Cow::Owned(full_code)),
                });
                sm
            }
            ShaderLanguage::Glsl => {
                device.push_error_scope(wgpu::ErrorFilter::Validation);
                let sm = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(&manifest.name),
                    source: wgpu::ShaderSource::Glsl {
                        shader: Cow::Owned(full_code),
                        stage: naga::ShaderStage::Fragment,
                        defines: Default::default(),
                    },
                });
                sm
            }
        };

        Ok(module)
    }

    pub fn prepare_shader_source(manifest: &ShaderManifest, raw_code: &str) -> String {
        let mut full_code = String::new();

        match manifest.language {
            ShaderLanguage::Wgsl => {
                let needs_header = !raw_code.contains("struct Globals");
                if needs_header {
                    full_code.push_str(WGSL_GROUP0_HEADER);
                    full_code.push('\n');
                }

                // If user didn't define struct Params but defined params in manifest:
                let params_preamble = manifest.generate_params_preamble();
                if !params_preamble.is_empty() && !raw_code.contains("struct Params") {
                    full_code.push_str(&params_preamble);
                }

                // Append any custom textures declared in manifest
                for (idx, tex) in manifest.textures.iter().enumerate() {
                    let binding = idx + 1; // binding 0 is Params UBO
                    let tex_decl = format!("@group(1) @binding({}) var {}: texture_2d<f32>;\n", binding, tex.name);
                    if !raw_code.contains(&format!("var {}:", tex.name)) {
                        full_code.push_str(&tex_decl);
                    }
                }

                full_code.push_str(raw_code);
            }
            ShaderLanguage::Glsl => {
                let has_version = raw_code.contains("#version");
                if !has_version {
                    full_code.push_str(GLSL_GROUP0_HEADER);
                    full_code.push('\n');
                }

                let params_preamble = manifest.generate_params_preamble();
                if !params_preamble.is_empty() && !raw_code.contains("uniform Params") {
                    full_code.push_str(&params_preamble);
                }

                for (idx, tex) in manifest.textures.iter().enumerate() {
                    let binding = idx + 1;
                    let tex_decl = format!("layout(set = 1, binding = {}) uniform texture2D {};\n", binding, tex.name);
                    if !raw_code.contains(&format!("uniform texture2D {}", tex.name)) {
                        full_code.push_str(&tex_decl);
                    }
                }

                full_code.push_str(raw_code);
            }
        }

        full_code
    }
}
