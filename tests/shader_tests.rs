use ninja_shader::gpu::{GpuEngine, ShaderCompiler};
use ninja_shader::pipeline_config::PipelineConfig;
use std::path::PathBuf;

#[test]
fn test_discover_and_parse_all_starter_shaders() {
    let shaders_dir = PathBuf::from("shaders");
    assert!(shaders_dir.exists(), "shaders/ directory must exist");

    let mut shader_names = Vec::new();
    for entry in std::fs::read_dir(&shaders_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "toml") {
            let manifest = ninja_shader::manifest::ShaderManifest::load_from_file(&path)
                .expect(&format!("Failed to parse {:?}", path));
            assert!(!manifest.name.is_empty());
            assert!(
                manifest.source_path().exists(),
                "Source file {:?} does not exist",
                manifest.source_path()
            );
            shader_names.push(manifest.name);
        }
    }

    assert!(
        shader_names.len() >= 35,
        "Expected at least 35 shaders, found {}",
        shader_names.len()
    );
}

#[test]
fn test_compile_all_starter_shaders_headless_wgpu() {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("GPU adapter required for testing");

    let (device, _) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("test_device"),
            ..Default::default()
        },
        None,
    ))
    .expect("Device creation failed");

    let shaders_dir = PathBuf::from("shaders");
    for entry in std::fs::read_dir(&shaders_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "toml") {
            let manifest = ninja_shader::manifest::ShaderManifest::load_from_file(&path).unwrap();
            let _module = ShaderCompiler::compile_fragment_shader(&device, &manifest)
                .expect(&format!("Failed to compile shader {}", manifest.name));
        }
    }
}

#[test]
fn test_single_pass_wgsl_grayscale() {
    let toml_str = r#"
        name = "test_gray"
        [[passes]]
        shader = "grayscale"
        enabled = true
        [passes.params]
        strength = 1.0
    "#;
    let pipeline: PipelineConfig = toml::from_str(toml_str).unwrap();
    let shader_dirs = vec![PathBuf::from("shaders")];
    let width = 4u32;
    let height = 4u32;

    let engine = pollster::block_on(GpuEngine::new_headless(
        width,
        height,
        &pipeline,
        &shader_dirs,
    ))
    .unwrap();

    let mut frame_rgba = Vec::new();
    for _ in 0..16 {
        frame_rgba.extend_from_slice(&[255, 0, 0, 255]);
    }
    engine.upload_source_frame(&frame_rgba);
    engine.update_globals(0, 30.0);

    let mut encoder = engine.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("test_encoder"),
    });
    let final_tex = engine.render_frame(&mut encoder);
    engine.queue.submit(Some(encoder.finish()));

    let mut out_rgba = Vec::new();
    engine.readback_frame(final_tex, &mut out_rgba).unwrap();
    assert_eq!(out_rgba.len(), 64);
    assert_eq!(&out_rgba[0..4], &[54, 54, 54, 255]);
}

#[test]
fn test_single_pass_glsl_chromatic_aberration() {
    let toml_str = r#"
        name = "test_ca"
        [[passes]]
        shader = "chromatic_aberration"
        enabled = true
        [passes.params]
        amount = 0.01
    "#;
    let pipeline: PipelineConfig = toml::from_str(toml_str).unwrap();
    let shader_dirs = vec![PathBuf::from("shaders")];
    let width = 4u32;
    let height = 4u32;

    let engine = pollster::block_on(GpuEngine::new_headless(
        width,
        height,
        &pipeline,
        &shader_dirs,
    ))
    .unwrap();

    let mut frame_rgba = Vec::new();
    for _ in 0..16 {
        frame_rgba.extend_from_slice(&[255, 128, 64, 255]);
    }
    engine.upload_source_frame(&frame_rgba);
    engine.update_globals(0, 30.0);

    let mut encoder = engine.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("test_encoder"),
    });
    let final_tex = engine.render_frame(&mut encoder);
    engine.queue.submit(Some(encoder.finish()));

    let mut out_rgba = Vec::new();
    engine.readback_frame(final_tex, &mut out_rgba).unwrap();
    assert_eq!(out_rgba.len(), 64);
    assert_eq!(&out_rgba[0..4], &[255, 128, 64, 255]);
}

#[test]
fn test_all_pipelines_compile_and_render_headless() {
    let pipelines_dir = PathBuf::from("pipelines");
    assert!(pipelines_dir.exists(), "pipelines/ directory must exist");
    let shader_dirs = vec![PathBuf::from("shaders")];

    let mut pipeline_paths = Vec::new();
    for entry in std::fs::read_dir(&pipelines_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "toml") {
            pipeline_paths.push(path);
        }
    }
    pipeline_paths.sort();

    assert!(
        pipeline_paths.len() >= 32,
        "Expected at least 32 pipelines, found {}",
        pipeline_paths.len()
    );

    let width = 32u32;
    let height = 32u32;
    let frame_rgba = vec![128u8; (width * height * 4) as usize];

    for pipeline_path in &pipeline_paths {
        let pipeline = PipelineConfig::load_from_file(pipeline_path)
            .unwrap_or_else(|e| panic!("Failed to load {:?}: {:?}", pipeline_path, e));

        let engine = pollster::block_on(GpuEngine::new_headless(
            width,
            height,
            &pipeline,
            &shader_dirs,
        ))
        .unwrap_or_else(|e| panic!("Failed to build GpuEngine for {:?}: {:?}", pipeline_path, e));

        // Test frame 0
        engine.upload_source_frame(&frame_rgba);
        engine.update_globals(0, 30.0);
        let mut encoder0 = engine.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("test_encoder_frame0"),
        });
        let final_tex0 = engine.render_frame(&mut encoder0);
        engine.queue.submit(Some(encoder0.finish()));

        let mut out0 = Vec::new();
        engine.readback_frame(final_tex0, &mut out0)
            .unwrap_or_else(|e| panic!("Readback failed on frame 0 for {:?}: {:?}", pipeline_path, e));
        assert_eq!(out0.len(), (width * height * 4) as usize);

        // Test frame 1 (exercises temporal feedback e.g. datamosh, motion_trails)
        engine.upload_source_frame(&frame_rgba);
        engine.update_globals(1, 30.0);
        let mut encoder1 = engine.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("test_encoder_frame1"),
        });
        let final_tex1 = engine.render_frame(&mut encoder1);
        engine.queue.submit(Some(encoder1.finish()));

        let mut out1 = Vec::new();
        engine.readback_frame(final_tex1, &mut out1)
            .unwrap_or_else(|e| panic!("Readback failed on frame 1 for {:?}: {:?}", pipeline_path, e));
        assert_eq!(out1.len(), (width * height * 4) as usize);
    }
}

#[test]
fn test_generative_textures_voronoi_and_noise() {
    use ninja_shader::pipeline_config::{
        DistanceMetric, GeneratorConfig, GeneratorKind, GeneratorMode, PassConfig, TextureInput,
    };
    use std::collections::HashMap;

    let shader_dirs = vec![PathBuf::from("shaders")];
    let width = 64u32;
    let height = 64u32;
    let frame_rgba = vec![200u8; (width * height * 4) as usize];

    // Test matrix of generative configurations
    let test_configs = vec![
        // Voronoi Borders (Euclidean)
        GeneratorConfig {
            generator: GeneratorKind::Voronoi,
            seed: 42,
            scale: 8.0,
            jitter: 0.9,
            mode: GeneratorMode::Borders,
            metric: DistanceMetric::Euclidean,
            animate: false,
            ..Default::default()
        },
        // Voronoi Distance (Manhattan)
        GeneratorConfig {
            generator: GeneratorKind::Voronoi,
            seed: 999,
            scale: 12.0,
            jitter: 0.5,
            mode: GeneratorMode::Distance,
            metric: DistanceMetric::Manhattan,
            animate: true,
            speed: 1.5,
            ..Default::default()
        },
        // Voronoi Cells (Cell ID color)
        GeneratorConfig {
            generator: GeneratorKind::Voronoi,
            seed: 12345,
            scale: 6.0,
            jitter: 1.0,
            mode: GeneratorMode::Cells,
            metric: DistanceMetric::Euclidean,
            animate: false,
            ..Default::default()
        },
        // Noise / FBM
        GeneratorConfig {
            generator: GeneratorKind::Noise,
            seed: 31415,
            scale: 10.0,
            octaves: 4,
            persistence: 0.5,
            lacunarity: 2.0,
            animate: true,
            speed: 2.0,
            ..Default::default()
        },
    ];

    for (idx, gen_config) in test_configs.into_iter().enumerate() {
        let mut textures = HashMap::new();
        textures.insert("overlay_tex".to_string(), TextureInput::Generator(gen_config));

        let mut params = HashMap::new();
        params.insert("opacity".to_string(), toml::Value::Float(0.8));
        params.insert("uv_scale".to_string(), toml::Value::Float(1.0));

        let pipeline = PipelineConfig {
            name: format!("gen_test_{}", idx),
            description: None,
            input: None,
            output: None,
            settings: Default::default(),
            passes: vec![PassConfig {
                shader: "texture_overlay".to_string(),
                enabled: true,
                params,
                textures,
            }],
            config_path: PathBuf::from("."),
        };

        let engine = pollster::block_on(GpuEngine::new_headless(
            width,
            height,
            &pipeline,
            &shader_dirs,
        ))
        .unwrap_or_else(|e| panic!("Failed to build GpuEngine for gen test {}: {:?}", idx, e));

        engine.upload_source_frame(&frame_rgba);
        engine.update_globals(0, 30.0);

        let mut encoder = engine.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("test_gen_encoder"),
        });
        let final_tex = engine.render_frame(&mut encoder);
        engine.queue.submit(Some(encoder.finish()));

        let mut out = Vec::new();
        engine
            .readback_frame(final_tex, &mut out)
            .unwrap_or_else(|e| panic!("Readback failed for gen test {}: {:?}", idx, e));

        assert_eq!(out.len(), (width * height * 4) as usize);

        // Verify that the output has varied pixels (not blank/uniform 0)
        let first_pixel = &out[0..4];
        let has_variation = out.chunks(4).any(|p| p != first_pixel);
        assert!(
            has_variation,
            "Expected generative texture pass {} to produce varied pixel output",
            idx
        );
    }
}
