use crate::gpu::compiler::ShaderCompiler;
use crate::gpu::generators::GpuTextureGeneratorManager;
use crate::gpu::types::GlobalsUniform;
use crate::manifest::{AddressMode, ShaderLanguage, ShaderManifest, TextureFilter};
use crate::pipeline_config::{PipelineConfig, TextureInput};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct PassInstance {
    pub name: String,
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group_1: wgpu::BindGroup,
    pub enabled: bool,
    pub loaded_textures: Vec<wgpu::Texture>,
}

pub struct GpuEngine {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,

    pub width: u32,
    pub height: u32,

    // Global textures
    pub original_texture: wgpu::Texture,
    pub original_view: wgpu::TextureView,

    pub ping_texture: wgpu::Texture,
    pub ping_view: wgpu::TextureView,

    pub pong_texture: wgpu::Texture,
    pub pong_view: wgpu::TextureView,

    pub prev_frame_texture: wgpu::Texture,
    pub prev_frame_view: wgpu::TextureView,

    // Readback buffer
    pub readback_buffer: wgpu::Buffer,
    pub padded_bytes_per_row: u32,

    // Global uniform & samplers
    pub globals_buffer: wgpu::Buffer,
    pub linear_sampler: wgpu::Sampler,
    pub nearest_sampler: wgpu::Sampler,

    // BindGroupLayouts
    pub group0_layout: wgpu::BindGroupLayout,

    // BindGroups for Group 0
    pub bg0_from_original: wgpu::BindGroup,
    pub bg0_from_ping: wgpu::BindGroup,
    pub bg0_from_pong: wgpu::BindGroup,

    // Passes
    pub passes: Vec<PassInstance>,

    // Generative texture manager
    pub generator_manager: GpuTextureGeneratorManager,
}

impl GpuEngine {
    pub async fn new_headless(
        width: u32,
        height: u32,
        pipeline_config: &PipelineConfig,
        shader_dirs: &[PathBuf],
    ) -> Result<Self> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| anyhow::anyhow!("Failed to find suitable GPU adapter"))?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("ninja_shader_device"),
                    required_features: wgpu::Features::default(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .with_context(|| "Failed to create GPU device")?;

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        Self::init_with_device(instance, adapter, device, queue, width, height, pipeline_config, shader_dirs)
    }

    pub fn init_with_device(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        width: u32,
        height: u32,
        pipeline_config: &PipelineConfig,
        shader_dirs: &[PathBuf],
    ) -> Result<Self> {
        let texture_desc = wgpu::TextureDescriptor {
            label: Some("ninja_rt"),
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
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };

        let original_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("original_texture"),
            ..texture_desc
        });
        let original_view = original_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let ping_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ping_texture"),
            ..texture_desc
        });
        let ping_view = ping_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let pong_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pong_texture"),
            ..texture_desc
        });
        let pong_view = pong_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let prev_frame_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("prev_frame_texture"),
            ..texture_desc
        });
        let prev_frame_view = prev_frame_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Readback buffer setup
        let unpadded_bytes_per_row = width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = ((unpadded_bytes_per_row + align - 1) / align) * align;
        let readback_size = (padded_bytes_per_row * height) as wgpu::BufferAddress;

        let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback_buffer"),
            size: readback_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        // Globals uniform buffer (size of GlobalsUniform = 32 bytes)
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals_uniform_buffer"),
            size: std::mem::size_of::<GlobalsUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Samplers
        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // BindGroupLayout for Group 0
        let group0_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("group0_layout"),
            entries: &[
                // binding 0: Globals uniform
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // binding 1: linear sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // binding 2: nearest sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                // binding 3: input_tex
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // binding 4: original_tex
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // binding 5: prev_frame_tex
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        // Create Group 0 BindGroups
        let create_bg0 = |input_view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("group0_bind_group"),
                layout: &group0_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: globals_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&nearest_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(input_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&original_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(&prev_frame_view),
                    },
                ],
            })
        };

        let bg0_from_original = create_bg0(&original_view);
        let bg0_from_ping = create_bg0(&ping_view);
        let bg0_from_pong = create_bg0(&pong_view);

        let generator_manager = GpuTextureGeneratorManager::new(&device)?;

        let mut engine = Self {
            instance,
            adapter,
            device,
            queue,
            width,
            height,
            original_texture,
            original_view,
            ping_texture,
            ping_view,
            pong_texture,
            pong_view,
            prev_frame_texture,
            prev_frame_view,
            readback_buffer,
            padded_bytes_per_row,
            globals_buffer,
            linear_sampler,
            nearest_sampler,
            group0_layout,
            bg0_from_original,
            bg0_from_ping,
            bg0_from_pong,
            passes: Vec::new(),
            generator_manager,
        };

        engine.build_passes(pipeline_config, shader_dirs)?;
        Ok(engine)
    }

    pub fn build_passes(
        &mut self,
        pipeline_config: &PipelineConfig,
        shader_dirs: &[PathBuf],
    ) -> Result<()> {
        self.generator_manager.clear_animated();
        let mut pass_instances = Vec::new();

        for pass in &pipeline_config.passes {
            let manifest = resolve_shader_manifest(&pass.shader, shader_dirs)?;
            let vs_module = ShaderCompiler::create_vertex_module(&self.device, manifest.language);
            let fs_module = ShaderCompiler::compile_fragment_shader(&self.device, &manifest)?;

            // Build Group 1 layout and bind group
            let (bg1_layout, bg1, loaded_textures) = self.create_pass_group1(&manifest, pass)?;

            let pipeline_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(&format!("pipeline_layout_{}", pass.shader)),
                bind_group_layouts: &[&self.group0_layout, &bg1_layout],
                push_constant_ranges: &[],
            });

            self.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let render_pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&format!("render_pipeline_{}", pass.shader)),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &vs_module,
                    entry_point: Some(match manifest.language {
                        ShaderLanguage::Wgsl => "vs_main",
                        ShaderLanguage::Glsl => "main",
                    }),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &fs_module,
                    entry_point: Some(manifest.entry_point_str()),
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

            if let Some(err) = pollster::block_on(self.device.pop_error_scope()) {
                bail!("GPU RenderPipeline creation failed for pass '{}': {:?}", pass.shader, err);
            }

            pass_instances.push(PassInstance {
                name: pass.shader.clone(),
                pipeline: render_pipeline,
                bind_group_1: bg1,
                enabled: pass.enabled,
                loaded_textures,
            });
        }

        self.passes = pass_instances;
        Ok(())
    }

    fn create_pass_group1(
        &mut self,
        manifest: &ShaderManifest,
        pass: &crate::pipeline_config::PassConfig,
    ) -> Result<(wgpu::BindGroupLayout, wgpu::BindGroup, Vec<wgpu::Texture>)> {
        let packed_params = manifest.pack_params(&pass.params)?;

        let params_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("params_buffer_{}", manifest.name)),
            size: packed_params.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&params_buffer, 0, &packed_params);

        let mut layout_entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }];

        // Texture resources
        let mut loaded_textures = Vec::new();
        let mut loaded_views = Vec::new();

        for (idx, tex_def) in manifest.textures.iter().enumerate() {
            let binding = (idx + 1) as u32;
            layout_entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });

            let tex_input = pass.textures.get(&tex_def.name);

            let (tex, view) = match tex_input {
                Some(TextureInput::Generator(gen_cfg)) => {
                    self.generator_manager.generate_texture(
                        &self.device,
                        &self.queue,
                        gen_cfg,
                        self.width,
                        self.height,
                    )?
                }
                Some(TextureInput::Path(p)) => {
                    self.load_aux_texture(Path::new(p), tex_def.filter, tex_def.address_mode)?
                }
                None => {
                    if let Some(ref default_p) = tex_def.default {
                        self.load_aux_texture(Path::new(default_p), tex_def.filter, tex_def.address_mode)?
                    } else {
                        self.create_dummy_texture()?
                    }
                }
            };

            loaded_textures.push(tex);
            loaded_views.push(view);
        }

        let bg_layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("group1_layout_{}", manifest.name)),
            entries: &layout_entries,
        });

        let mut bg_entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: params_buffer.as_entire_binding(),
        }];

        for (idx, view) in loaded_views.iter().enumerate() {
            bg_entries.push(wgpu::BindGroupEntry {
                binding: (idx + 1) as u32,
                resource: wgpu::BindingResource::TextureView(view),
            });
        }

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("group1_bind_group_{}", manifest.name)),
            layout: &bg_layout,
            entries: &bg_entries,
        });

        Ok((bg_layout, bind_group, loaded_textures))
    }

    fn create_dummy_texture(&self) -> Result<(wgpu::Texture, wgpu::TextureView)> {
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("dummy_1x1"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255, 255, 255, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        Ok((tex, view))
    }

    fn load_aux_texture(
        &self,
        path: &Path,
        _filter: TextureFilter,
        _address_mode: AddressMode,
    ) -> Result<(wgpu::Texture, wgpu::TextureView)> {
        if !path.exists() {
            return self.create_dummy_texture();
        }

        let img = image::open(path)
            .with_context(|| format!("Failed to load auxiliary texture at {:?}", path))?
            .to_rgba8();
        let (width, height) = img.dimensions();

        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("aux_tex_{:?}", path)),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &img,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        Ok((tex, view))
    }

    pub fn upload_source_frame(&self, raw_rgba: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.original_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            raw_rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.width * 4),
                rows_per_image: Some(self.height),
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn update_globals(&self, frame: u32, fps: f32) {
        let globals = GlobalsUniform::new(self.width, self.height, frame, fps);
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
        self.generator_manager
            .render_animated(&self.device, &self.queue, globals.time);
    }

    /// Renders the sequential passes. Returns a reference to the texture holding the final rendered output.
    pub fn render_frame<'a>(&'a self, encoder: &mut wgpu::CommandEncoder) -> &'a wgpu::Texture {
        let active_passes: Vec<&PassInstance> = self.passes.iter().filter(|p| p.enabled).collect();

        if active_passes.is_empty() {
            // No passes active, return original texture
            return &self.original_texture;
        }

        // Ping-pong state: 0 = write to ping, 1 = write to pong
        let mut target_toggle = 0usize;

        for (pass_idx, pass) in active_passes.iter().enumerate() {
            let (render_target_view, input_bg0) = if pass_idx == 0 {
                // First pass reads from original_texture, writes to ping_view
                target_toggle = 0;
                (&self.ping_view, &self.bg0_from_original)
            } else if target_toggle == 0 {
                // Previous pass wrote to ping, so this pass reads ping, writes to pong
                target_toggle = 1;
                (&self.pong_view, &self.bg0_from_ping)
            } else {
                // Previous pass wrote to pong, so this pass reads pong, writes to ping
                target_toggle = 0;
                (&self.ping_view, &self.bg0_from_pong)
            };

            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(&format!("render_pass_{}", pass.name)),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: render_target_view,
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

            rpass.set_pipeline(&pass.pipeline);
            rpass.set_bind_group(0, input_bg0, &[]);
            rpass.set_bind_group(1, &pass.bind_group_1, &[]);
            // Fullscreen triangle: 3 vertices
            rpass.draw(0..3, 0..1);
        }

        let final_texture = if target_toggle == 0 {
            &self.ping_texture
        } else {
            &self.pong_texture
        };

        // Copy final output to prev_frame_texture for next frame's temporal feedback
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: final_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &self.prev_frame_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );

        final_texture
    }

    /// Read back the rendered frame to CPU memory buffer (packed RGBA8).
    pub fn readback_frame(&self, final_texture: &wgpu::Texture, out_rgba: &mut Vec<u8>) -> Result<()> {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback_encoder"),
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: final_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(Some(encoder.finish()));

        let slice = self.readback_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap();
        });

        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()??;

        {
            let mapped_view = slice.get_mapped_range();
            let unpadded_bytes = (self.width * 4) as usize;
            out_rgba.resize((self.width * self.height * 4) as usize, 0);

            for y in 0..self.height as usize {
                let src_start = y * (self.padded_bytes_per_row as usize);
                let src_end = src_start + unpadded_bytes;
                let dst_start = y * unpadded_bytes;
                let dst_end = dst_start + unpadded_bytes;
                out_rgba[dst_start..dst_end].copy_from_slice(&mapped_view[src_start..src_end]);
            }
        }

        self.readback_buffer.unmap();
        Ok(())
    }
}

pub fn resolve_shader_manifest(name: &str, search_dirs: &[PathBuf]) -> Result<ShaderManifest> {
    // 1. Direct path check
    let direct_path = Path::new(name);
    if direct_path.exists() && direct_path.extension().map_or(false, |ext| ext == "toml") {
        return ShaderManifest::load_from_file(direct_path);
    }

    // 2. Search in search_dirs (e.g. shaders/<name>.toml or shaders/<name>/manifest.toml)
    for dir in search_dirs {
        let p1 = dir.join(format!("{}.toml", name));
        if p1.exists() {
            return ShaderManifest::load_from_file(p1);
        }
        let p2 = dir.join(name).join("manifest.toml");
        if p2.exists() {
            return ShaderManifest::load_from_file(p2);
        }
    }

    bail!("Could not find shader '{}' in search paths {:?}", name, search_dirs);
}
