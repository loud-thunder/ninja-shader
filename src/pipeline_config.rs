use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderSettings {
    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default = "default_crf")]
    pub crf: u32,
    #[serde(default = "default_preset")]
    pub preset: String,
    #[serde(default = "default_copy_audio")]
    pub copy_audio: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            scale: default_scale(),
            crf: default_crf(),
            preset: default_preset(),
            copy_audio: default_copy_audio(),
        }
    }
}

fn default_scale() -> f32 {
    1.0
}
fn default_crf() -> u32 {
    18
}
fn default_preset() -> String {
    "medium".to_string()
}
fn default_copy_audio() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GeneratorKind {
    Voronoi,
    Noise,
}

impl Default for GeneratorKind {
    fn default() -> Self {
        GeneratorKind::Voronoi
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GeneratorMode {
    Distance, // F1
    Borders,  // F2 - F1
    Cells,    // Cell ID color
}

impl Default for GeneratorMode {
    fn default() -> Self {
        GeneratorMode::Distance
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DistanceMetric {
    Euclidean,
    Manhattan,
}

impl Default for DistanceMetric {
    fn default() -> Self {
        DistanceMetric::Euclidean
    }
}

fn default_gen_scale() -> f32 {
    8.0
}
fn default_jitter() -> f32 {
    1.0
}
fn default_octaves() -> u32 {
    4
}
fn default_persistence() -> f32 {
    0.5
}
fn default_lacunarity() -> f32 {
    2.0
}
fn default_speed() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneratorConfig {
    pub generator: GeneratorKind,
    #[serde(default)]
    pub seed: u32,
    #[serde(default = "default_gen_scale")]
    pub scale: f32,
    #[serde(default = "default_jitter")]
    pub jitter: f32,
    #[serde(default)]
    pub mode: GeneratorMode,
    #[serde(default)]
    pub metric: DistanceMetric,
    #[serde(default = "default_octaves")]
    pub octaves: u32,
    #[serde(default = "default_persistence")]
    pub persistence: f32,
    #[serde(default = "default_lacunarity")]
    pub lacunarity: f32,
    #[serde(default)]
    pub animate: bool,
    #[serde(default = "default_speed")]
    pub speed: f32,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            generator: GeneratorKind::Voronoi,
            seed: 0,
            scale: default_gen_scale(),
            jitter: default_jitter(),
            mode: GeneratorMode::Distance,
            metric: DistanceMetric::Euclidean,
            octaves: default_octaves(),
            persistence: default_persistence(),
            lacunarity: default_lacunarity(),
            animate: false,
            speed: default_speed(),
            width: None,
            height: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum TextureInput {
    Path(String),
    Generator(GeneratorConfig),
}

impl Default for TextureInput {
    fn default() -> Self {
        TextureInput::Path(String::new())
    }
}

impl TextureInput {
    pub fn as_path(&self) -> Option<&str> {
        match self {
            TextureInput::Path(p) => Some(p.as_str()),
            TextureInput::Generator(_) => None,
        }
    }

    pub fn as_generator(&self) -> Option<&GeneratorConfig> {
        match self {
            TextureInput::Generator(g) => Some(g),
            TextureInput::Path(_) => None,
        }
    }

    pub fn as_generator_mut(&mut self) -> Option<&mut GeneratorConfig> {
        match self {
            TextureInput::Generator(g) => Some(g),
            TextureInput::Path(_) => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassConfig {
    pub shader: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub params: HashMap<String, toml::Value>,
    #[serde(default)]
    pub textures: HashMap<String, TextureInput>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub input: Option<String>,
    pub output: Option<String>,
    #[serde(default)]
    pub settings: RenderSettings,
    #[serde(default)]
    pub passes: Vec<PassConfig>,

    #[serde(skip)]
    pub config_path: PathBuf,
}

impl PipelineConfig {
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read pipeline file at {:?}", path))?;
        let mut config: PipelineConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse pipeline TOML at {:?}", path))?;
        config.config_path = path.to_path_buf();
        Ok(config)
    }

    /// Resolve relative path from pipeline config file directory
    pub fn resolve_relative_path(&self, p: &str) -> PathBuf {
        let parent = self.config_path.parent().unwrap_or_else(|| Path::new("."));
        parent.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_parse() {
        let toml_str = r#"
            name = "retro_crt"
            description = "CRT pipeline"
            input = "in.mp4"
            output = "out.mp4"

            [settings]
            scale = 0.5
            crf = 20
            copy_audio = false

            [[passes]]
            shader = "crt_scanlines"
            enabled = true
            [passes.params]
            curvature = 0.1
        "#;

        let config: PipelineConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.name, "retro_crt");
        assert_eq!(config.settings.scale, 0.5);
        assert_eq!(config.passes.len(), 1);
        assert_eq!(config.passes[0].shader, "crt_scanlines");
    }

    #[test]
    fn test_generative_texture_parse() {
        let toml_str = r#"
            name = "voronoi_test"
            [[passes]]
            shader = "texture_overlay"
            [passes.textures.overlay_tex]
            generator = "voronoi"
            seed = 1234
            scale = 16.0
            jitter = 0.8
            mode = "borders"
            metric = "manhattan"
            animate = true
            speed = 2.0
        "#;

        let config: PipelineConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.passes.len(), 1);
        let tex = config.passes[0].textures.get("overlay_tex").unwrap();
        match tex {
            TextureInput::Generator(gen) => {
                assert_eq!(gen.generator, GeneratorKind::Voronoi);
                assert_eq!(gen.seed, 1234);
                assert_eq!(gen.scale, 16.0);
                assert_eq!(gen.jitter, 0.8);
                assert_eq!(gen.mode, GeneratorMode::Borders);
                assert_eq!(gen.metric, DistanceMetric::Manhattan);
                assert!(gen.animate);
                assert_eq!(gen.speed, 2.0);
            }
            _ => panic!("Expected GeneratorConfig"),
        }
    }

    #[test]
    fn test_legacy_texture_path_parse() {
        let toml_str = r#"
            name = "legacy_test"
            [[passes]]
            shader = "texture_overlay"
            textures = { overlay_tex = "assets/pattern.png" }
        "#;

        let config: PipelineConfig = toml::from_str(toml_str).unwrap();
        let tex = config.passes[0].textures.get("overlay_tex").unwrap();
        match tex {
            TextureInput::Path(p) => assert_eq!(p, "assets/pattern.png"),
            _ => panic!("Expected TextureInput::Path"),
        }
    }
}
