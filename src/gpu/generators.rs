use crate::pipeline_config::{DistanceMetric, GeneratorConfig, GeneratorKind, GeneratorMode};
use anyhow::Result;

pub const VORONOI_WGSL: &str = include_str!("../../shaders/generators/voronoi.wgsl");
pub const NOISE_WGSL: &str = include_str!("../../shaders/generators/noise.wgsl");

pub struct GeneratorPipelines {
    pub bg_layout: wgpu::BindGroupLayout,
    pub voronoi_pipeline: wgpu::RenderPipeline,
    pub noise_pipeline: wgpu::RenderPipeline,
}

impl GeneratorPipelines {
    pub fn new(device: &wgpu::Device) -> Result<Self> {
        let bg_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("generator_params_bg_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("generator_pipeline_layout"),
            bind_group_layouts: &[&bg_layout],
            push_constant_ranges: &[],
        });

        let voronoi_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("voronoi_generator_module"),
            source: wgpu::ShaderSource::Wgsl(VORONOI_WGSL.into()),
        });

        let noise_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("noise_generator_module"),
            source: wgpu::ShaderSource::Wgsl(NOISE_WGSL.into()),
        });

        let vs_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fullscreen_generator_vs"),
            source: wgpu::ShaderSource::Wgsl(crate::gpu::compiler::FULLSCREEN_VERTEX_WGSL.into()),
        });

        let voronoi_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("voronoi_render_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vs_module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &voronoi_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let noise_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("noise_render_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vs_module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &noise_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Ok(Self {
            bg_layout,
            voronoi_pipeline,
            noise_pipeline,
        })
    }
}

pub struct AnimatedTextureInstance {
    pub target_view: wgpu::TextureView,
    pub uniform_buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub kind: GeneratorKind,
    pub config: GeneratorConfig,
    pub width: u32,
    pub height: u32,
}

pub struct GpuTextureGeneratorManager {
    pub pipelines: GeneratorPipelines,
    pub animated_textures: Vec<AnimatedTextureInstance>,
}

impl GpuTextureGeneratorManager {
    pub fn new(device: &wgpu::Device) -> Result<Self> {
        let pipelines = GeneratorPipelines::new(device)?;
        Ok(Self {
            pipelines,
            animated_textures: Vec::new(),
        })
    }

    pub fn clear_animated(&mut self) {
        self.animated_textures.clear();
    }

    pub fn pack_params(config: &GeneratorConfig, width: u32, height: u32, time: f32) -> [u8; 48] {
        let mut bytes = [0u8; 48];
        let res = [width as f32, height as f32];

        // bytes 0..8: resolution
        bytes[0..4].copy_from_slice(&res[0].to_le_bytes());
        bytes[4..8].copy_from_slice(&res[1].to_le_bytes());
        // bytes 8..12: scale
        bytes[8..12].copy_from_slice(&config.scale.to_le_bytes());

        match config.generator {
            GeneratorKind::Voronoi => {
                // bytes 12..16: jitter
                bytes[12..16].copy_from_slice(&config.jitter.to_le_bytes());
                // bytes 16..20: seed
                bytes[16..20].copy_from_slice(&config.seed.to_le_bytes());
                // bytes 20..24: mode
                let mode_u32 = match config.mode {
                    GeneratorMode::Distance => 0u32,
                    GeneratorMode::Borders => 1u32,
                    GeneratorMode::Cells => 2u32,
                };
                bytes[20..24].copy_from_slice(&mode_u32.to_le_bytes());
                // bytes 24..28: metric
                let metric_u32 = match config.metric {
                    DistanceMetric::Euclidean => 0u32,
                    DistanceMetric::Manhattan => 1u32,
                };
                bytes[24..28].copy_from_slice(&metric_u32.to_le_bytes());
                // bytes 28..32: animate
                let anim_u32 = if config.animate { 1u32 } else { 0u32 };
                bytes[28..32].copy_from_slice(&anim_u32.to_le_bytes());
                // bytes 32..36: time
                bytes[32..36].copy_from_slice(&time.to_le_bytes());
                // bytes 36..40: speed
                bytes[36..40].copy_from_slice(&config.speed.to_le_bytes());
                // bytes 40..48: padding
            }
            GeneratorKind::Noise => {
                // bytes 12..16: seed
                bytes[12..16].copy_from_slice(&config.seed.to_le_bytes());
                // bytes 16..20: octaves
                bytes[16..20].copy_from_slice(&config.octaves.to_le_bytes());
                // bytes 20..24: persistence
                bytes[20..24].copy_from_slice(&config.persistence.to_le_bytes());
                // bytes 24..28: lacunarity
                bytes[24..28].copy_from_slice(&config.lacunarity.to_le_bytes());
                // bytes 28..32: animate
                let anim_u32 = if config.animate { 1u32 } else { 0u32 };
                bytes[28..32].copy_from_slice(&anim_u32.to_le_bytes());
                // bytes 32..36: time
                bytes[32..36].copy_from_slice(&time.to_le_bytes());
                // bytes 36..40: speed
                bytes[36..40].copy_from_slice(&config.speed.to_le_bytes());
                // bytes 40..48: padding
            }
        }

        bytes
    }

    pub fn generate_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &GeneratorConfig,
        default_width: u32,
        default_height: u32,
    ) -> Result<(wgpu::Texture, wgpu::TextureView)> {
        let width = config.width.unwrap_or(default_width).max(1);
        let height = config.height.unwrap_or(default_height).max(1);

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("generative_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let packed_params = Self::pack_params(config, width, height, 0.0);
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("generator_params_buffer"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&uniform_buffer, 0, &packed_params);

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("generator_bind_group"),
            layout: &self.pipelines.bg_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Execute initial render pass into texture
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("generator_render_encoder"),
        });

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("generator_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            match config.generator {
                GeneratorKind::Voronoi => {
                    rpass.set_pipeline(&self.pipelines.voronoi_pipeline);
                }
                GeneratorKind::Noise => {
                    rpass.set_pipeline(&self.pipelines.noise_pipeline);
                }
            }
            rpass.set_bind_group(0, &bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }

        queue.submit(Some(encoder.finish()));

        // If animated, register for per-frame updates
        if config.animate {
            self.animated_textures.push(AnimatedTextureInstance {
                target_view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
                uniform_buffer,
                bind_group,
                kind: config.generator,
                config: config.clone(),
                width,
                height,
            });
        }

        Ok((texture, view))
    }

    pub fn render_animated(&self, device: &wgpu::Device, queue: &wgpu::Queue, time: f32) {
        if self.animated_textures.is_empty() {
            return;
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("animated_generators_encoder"),
        });

        for anim in &self.animated_textures {
            let packed = Self::pack_params(&anim.config, anim.width, anim.height, time);
            queue.write_buffer(&anim.uniform_buffer, 0, &packed);

            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("animated_gen_rpass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &anim.target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            match anim.kind {
                GeneratorKind::Voronoi => {
                    rpass.set_pipeline(&self.pipelines.voronoi_pipeline);
                }
                GeneratorKind::Noise => {
                    rpass.set_pipeline(&self.pipelines.noise_pipeline);
                }
            }
            rpass.set_bind_group(0, &anim.bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }

        queue.submit(Some(encoder.finish()));
    }
}
