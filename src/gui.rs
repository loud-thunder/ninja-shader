use crate::cli::GuiArgs;
use crate::gpu::{resolve_shader_manifest, GpuEngine};
use crate::manifest::{ParamType, ParamValue, ShaderManifest};
use crate::pipeline_config::{
    DistanceMetric, GeneratorConfig, GeneratorKind, GeneratorMode, PipelineConfig, TextureInput,
};
use crate::video::{VideoDecoder, VideoEncoder, VideoMetadata, VideoProbe};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

const BLIT_SHADER_WGSL: &str = r#"
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
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

@group(0) @binding(0) var blit_sampler: sampler;
@group(0) @binding(1) var blit_texture: texture_2d<f32>;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(blit_texture, blit_sampler, in.uv);
}
"#;

#[derive(Clone, Debug)]
pub struct PipelineItem {
    pub path: PathBuf,
    pub filename: String,
    pub name: String,
    pub description: String,
    pub passes_count: usize,
}

#[derive(Clone, Debug)]
pub enum RenderProgressMsg {
    Progress {
        current_frame: u32,
        total_frames: u64,
        fps: f32,
    },
    Completed {
        frames: u32,
        duration_secs: f32,
        output_path: PathBuf,
    },
    Cancelled,
    Failed(String),
}

pub struct ActiveRenderTask {
    pub cancel_flag: Arc<AtomicBool>,
    pub rx: Receiver<RenderProgressMsg>,
    pub start_time: Instant,
    pub current_frame: u32,
    pub total_frames: u64,
    pub fps: f32,
    pub status: String,
}

pub struct GuiApp {
    // Config & discovery
    shaders_dirs: Vec<PathBuf>,
    pipelines_dir: PathBuf,
    pipeline_items: Vec<PipelineItem>,
    pipeline_filter: String,

    // File selection
    input_video_path: Option<PathBuf>,
    output_dir: PathBuf,
    output_filename: String,
    meta: Option<VideoMetadata>,

    // Pipeline & Manifests
    selected_pipeline_path: Option<PathBuf>,
    active_pipeline_config: Option<PipelineConfig>,
    pass_manifests: Vec<Option<ShaderManifest>>,

    // Playback state
    decoder: Option<VideoDecoder>,
    frame_buffer: Vec<u8>,
    current_frame: u32,
    total_frames: u64,
    paused: bool,
    loop_video: bool,
    last_frame_instant: Instant,
    frame_duration: Duration,
    render_width: u32,
    render_height: u32,
    preview_scale: f32,

    // Render configuration
    render_scale: f32,
    render_crf: u32,
    active_render: Option<ActiveRenderTask>,
    last_render_result: Option<Result<PathBuf, String>>,

    // Egui state
    egui_ctx: egui::Context,
    egui_winit_state: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,
    sidebar_width: f32,

    // Winit & WGPU
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    device: Option<Arc<wgpu::Device>>,
    queue: Option<Arc<wgpu::Queue>>,
    adapter: Option<wgpu::Adapter>,
    instance: Option<wgpu::Instance>,
    engine: Option<GpuEngine>,

    // Blit to surface
    blit_pipeline: Option<wgpu::RenderPipeline>,
    blit_bg_layout: Option<wgpu::BindGroupLayout>,
    blit_sampler: Option<wgpu::Sampler>,

    // Notifications
    notification: Option<(String, bool, Instant)>,
    scroll_to_selected: bool,
}

impl GuiApp {
    pub fn new(args: GuiArgs, shaders_dir: PathBuf) -> Result<Self> {
        let shaders_dirs = vec![shaders_dir, PathBuf::from("shaders")];
        let pipelines_dir = args.pipelines_dir;

        // Scan pipelines directory
        let pipeline_items = scan_pipelines(&pipelines_dir);

        // Determine default input video
        let input_video_path = if let Some(p) = args.input {
            Some(p)
        } else if Path::new("test_input.mp4").exists() {
            Some(PathBuf::from("test_input.mp4"))
        } else {
            // Check assets or pipeline items for an input
            None
        };

        // Determine output directory
        let output_dir = if let Some(d) = args.output_dir {
            d
        } else {
            PathBuf::from("output")
        };
        let _ = std::fs::create_dir_all(&output_dir);

        // Determine initial pipeline
        let selected_pipeline_path = if let Some(p) = args.pipeline {
            Some(p)
        } else {
            pipeline_items.first().map(|item| item.path.clone())
        };

        let mut app = Self {
            shaders_dirs,
            pipelines_dir,
            pipeline_items,
            pipeline_filter: String::new(),
            input_video_path: None,
            output_dir,
            output_filename: String::from("rendered_output.mp4"),
            meta: None,
            selected_pipeline_path,
            active_pipeline_config: None,
            pass_manifests: Vec::new(),
            decoder: None,
            frame_buffer: Vec::new(),
            current_frame: 0,
            total_frames: 0,
            paused: false,
            loop_video: true,
            last_frame_instant: Instant::now(),
            frame_duration: Duration::from_millis(33),
            render_width: 640,
            render_height: 360,
            preview_scale: args.scale.unwrap_or(1.0),
            render_scale: 1.0,
            render_crf: 18,
            active_render: None,
            last_render_result: None,
            egui_ctx: egui::Context::default(),
            egui_winit_state: None,
            egui_renderer: None,
            sidebar_width: 380.0,
            window: None,
            surface: None,
            surface_config: None,
            device: None,
            queue: None,
            adapter: None,
            instance: None,
            engine: None,
            blit_pipeline: None,
            blit_bg_layout: None,
            blit_sampler: None,
            notification: None,
            scroll_to_selected: false,
        };

        // If we have an input video path, set it
        if let Some(path) = input_video_path {
            app.set_input_video(path);
        }

        // Load initial pipeline config
        if let Some(ref p_path) = app.selected_pipeline_path.clone() {
            app.load_pipeline(p_path);
        }

        Ok(app)
    }

    fn set_input_video(&mut self, path: PathBuf) {
        if !path.exists() {
            self.set_notification(format!("File not found: {:?}", path), true);
            return;
        }

        match VideoProbe::probe(&path) {
            Ok(meta) => {
                let render_width = ((meta.width as f32 * self.preview_scale).round() as u32).max(2) & !1;
                let render_height = ((meta.height as f32 * self.preview_scale).round() as u32).max(2) & !1;
                let fps = if meta.fps > 0.0 { meta.fps } else { 30.0 };

                self.frame_duration = Duration::from_secs_f32(1.0 / fps);
                self.render_width = render_width;
                self.render_height = render_height;
                self.total_frames = meta.total_frames;
                self.meta = Some(meta);
                self.input_video_path = Some(path.clone());

                // Update default output filename based on video and pipeline stem
                self.update_output_filename();

                // Respawn decoder
                self.respawn_decoder();

                // Re-initialize engine if device is ready
                self.reinit_engine();

                self.set_notification(format!("Loaded video: {}", path.file_name().unwrap_or_default().to_string_lossy()), false);
            }
            Err(e) => {
                self.set_notification(format!("Failed to probe video: {:#}", e), true);
            }
        }
    }

    fn update_output_filename(&mut self) {
        let video_stem = self
            .input_video_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "video".to_string());

        let pipe_stem = self
            .selected_pipeline_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "effect".to_string());

        self.output_filename = format!("{}_{}.mp4", video_stem, pipe_stem);
    }

    fn respawn_decoder(&mut self) {
        if let Some(ref path) = self.input_video_path {
            match VideoDecoder::spawn(path, self.render_width, self.render_height, None, None) {
                Ok(dec) => {
                    self.decoder = Some(dec);
                    self.current_frame = 0;
                    self.frame_buffer.clear();
                }
                Err(e) => {
                    eprintln!("Failed to spawn video decoder: {:?}", e);
                    self.decoder = None;
                }
            }
        }
    }

    fn load_pipeline(&mut self, path: &Path) {
        match PipelineConfig::load_from_file(path) {
            Ok(config) => {
                self.selected_pipeline_path = Some(path.to_path_buf());
                self.update_output_filename();

                // Resolve manifests for all passes
                self.pass_manifests = config
                    .passes
                    .iter()
                    .map(|pass| resolve_shader_manifest(&pass.shader, &self.shaders_dirs).ok())
                    .collect();

                self.active_pipeline_config = Some(config);

                // Rebuild engine passes if engine already exists
                if let (Some(ref mut engine), Some(ref config)) =
                    (&mut self.engine, &self.active_pipeline_config)
                {
                    if let Err(e) = engine.build_passes(config, &self.shaders_dirs) {
                        self.set_notification(format!("Pipeline build error: {:#}", e), true);
                    } else {
                        self.set_notification(format!("Pipeline: {}", config.name), false);
                    }
                }
            }
            Err(e) => {
                self.set_notification(format!("Failed to load pipeline: {:#}", e), true);
            }
        }
    }

    fn get_filtered_pipeline_indices(&self) -> Vec<usize> {
        let filter_lower = self.pipeline_filter.to_lowercase();
        self.pipeline_items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                filter_lower.is_empty()
                    || item.name.to_lowercase().contains(&filter_lower)
                    || item.filename.to_lowercase().contains(&filter_lower)
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    fn select_next_pipeline(&mut self) {
        let indices = self.get_filtered_pipeline_indices();
        if indices.is_empty() {
            return;
        }

        let current_pos = self
            .selected_pipeline_path
            .as_ref()
            .and_then(|p| indices.iter().position(|&idx| &self.pipeline_items[idx].path == p));

        let next_idx = match current_pos {
            Some(pos) => indices[(pos + 1) % indices.len()],
            None => indices[0],
        };

        let path = self.pipeline_items[next_idx].path.clone();
        self.load_pipeline(&path);
        self.scroll_to_selected = true;
    }

    fn select_prev_pipeline(&mut self) {
        let indices = self.get_filtered_pipeline_indices();
        if indices.is_empty() {
            return;
        }

        let current_pos = self
            .selected_pipeline_path
            .as_ref()
            .and_then(|p| indices.iter().position(|&idx| &self.pipeline_items[idx].path == p));

        let prev_idx = match current_pos {
            Some(pos) => {
                if pos == 0 {
                    indices[indices.len() - 1]
                } else {
                    indices[pos - 1]
                }
            }
            None => indices[indices.len() - 1],
        };

        let path = self.pipeline_items[prev_idx].path.clone();
        self.load_pipeline(&path);
        self.scroll_to_selected = true;
    }

    fn reinit_engine(&mut self) {
        if let (Some(instance), Some(adapter), Some(device), Some(queue), Some(config)) = (
            &self.instance,
            &self.adapter,
            &self.device,
            &self.queue,
            &self.active_pipeline_config,
        ) {
            match GpuEngine::init_with_device(
                instance.clone(),
                adapter.clone(),
                device.clone(),
                queue.clone(),
                self.render_width,
                self.render_height,
                config,
                &self.shaders_dirs,
            ) {
                Ok(engine) => {
                    self.engine = Some(engine);
                }
                Err(e) => {
                    self.set_notification(format!("Engine init error: {:#}", e), true);
                }
            }
        }
    }

    fn set_notification(&mut self, msg: String, is_error: bool) {
        self.notification = Some((msg, is_error, Instant::now()));
    }

    fn start_background_render(&mut self) {
        if self.active_render.is_some() {
            self.set_notification("A render is already in progress!".into(), true);
            return;
        }

        let input_path = match self.input_video_path.clone() {
            Some(p) => p,
            None => {
                self.set_notification("Please select an input video first!".into(), true);
                return;
            }
        };

        let pipeline_config = match self.active_pipeline_config.clone() {
            Some(p) => p,
            None => {
                self.set_notification("No pipeline selected!".into(), true);
                return;
            }
        };

        let output_path = self.output_dir.join(&self.output_filename);
        let shader_dirs = self.shaders_dirs.clone();
        let scale = self.render_scale;
        let crf = self.render_crf;
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();

        let (tx, rx) = channel();

        let start_time = Instant::now();
        let meta = self.meta.clone();

        std::thread::spawn(move || {
            run_render_worker(
                input_path,
                output_path,
                pipeline_config,
                shader_dirs,
                scale,
                crf,
                meta,
                cancel_worker,
                tx,
            );
        });

        self.active_render = Some(ActiveRenderTask {
            cancel_flag,
            rx,
            start_time,
            current_frame: 0,
            total_frames: self.total_frames,
            fps: 0.0,
            status: "Initializing rendering engine...".to_string(),
        });
        self.last_render_result = None;
    }

    fn poll_render_task(&mut self) {
        let mut finished = false;
        if let Some(ref mut task) = self.active_render {
            while let Ok(msg) = task.rx.try_recv() {
                match msg {
                    RenderProgressMsg::Progress {
                        current_frame,
                        total_frames,
                        fps,
                    } => {
                        task.current_frame = current_frame;
                        task.total_frames = total_frames;
                        task.fps = fps;
                        task.status = format!(
                            "Rendering frame {}/{} ({:.1} fps)",
                            current_frame, total_frames, fps
                        );
                    }
                    RenderProgressMsg::Completed {
                        frames,
                        duration_secs,
                        output_path,
                    } => {
                        self.set_notification(
                            format!(
                                "✓ Render complete! ({} frames in {:.1}s)",
                                frames, duration_secs
                            ),
                            false,
                        );
                        self.last_render_result = Some(Ok(output_path));
                        finished = true;
                        break;
                    }
                    RenderProgressMsg::Cancelled => {
                        self.set_notification("Render was cancelled by user".into(), false);
                        self.last_render_result = Some(Err("Render cancelled".into()));
                        finished = true;
                        break;
                    }
                    RenderProgressMsg::Failed(err) => {
                        self.set_notification(format!("Render failed: {}", err), true);
                        self.last_render_result = Some(Err(err));
                        finished = true;
                        break;
                    }
                }
            }
        }

        if finished {
            self.active_render = None;
        }
    }
}

fn param_value_to_toml(val: &ParamValue) -> toml::Value {
    match val {
        ParamValue::Float(f) => toml::Value::Float(*f),
        ParamValue::Int(i) => toml::Value::Integer(*i),
        ParamValue::Vec(v) => toml::Value::Array(v.iter().map(|f| toml::Value::Float(*f)).collect()),
    }
}

fn toml_to_param_value(val: &toml::Value, default: &ParamValue) -> ParamValue {
    match val {
        toml::Value::Float(f) => ParamValue::Float(*f),
        toml::Value::Integer(i) => ParamValue::Int(*i),
        toml::Value::Array(arr) => {
            let nums = arr
                .iter()
                .filter_map(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
                .collect();
            ParamValue::Vec(nums)
        }
        _ => default.clone(),
    }
}

fn scan_pipelines(dir: &Path) -> Vec<PipelineItem> {
    let mut items = Vec::new();
    if !dir.exists() {
        return items;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "toml") {
                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let (name, desc, count) = if let Ok(cfg) = PipelineConfig::load_from_file(&path) {
                    (cfg.name, cfg.description.unwrap_or_default(), cfg.passes.len())
                } else {
                    (filename.clone(), String::new(), 0)
                };

                items.push(PipelineItem {
                    path,
                    filename,
                    name,
                    description: desc,
                    passes_count: count,
                });
            }
        }
    }

    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    items
}

fn run_render_worker(
    input_path: PathBuf,
    output_path: PathBuf,
    pipeline: PipelineConfig,
    shader_dirs: Vec<PathBuf>,
    scale: f32,
    crf: u32,
    meta_opt: Option<VideoMetadata>,
    cancel_flag: Arc<AtomicBool>,
    tx: Sender<RenderProgressMsg>,
) {
    let worker_fn = || -> Result<()> {
        let meta = match meta_opt {
            Some(m) => m,
            None => VideoProbe::probe(&input_path)?,
        };

        let target_width = ((meta.width as f32 * scale).round() as u32).max(2) & !1;
        let target_height = ((meta.height as f32 * scale).round() as u32).max(2) & !1;
        let total_frames = meta.total_frames.max(1);

        let engine = pollster::block_on(GpuEngine::new_headless(
            target_width,
            target_height,
            &pipeline,
            &shader_dirs,
        ))?;

        let mut decoder = VideoDecoder::spawn(&input_path, target_width, target_height, None, None)?;
        let mut encoder = VideoEncoder::spawn(
            Some(&input_path),
            &output_path,
            target_width,
            target_height,
            meta.fps,
            crf,
            &pipeline.settings.preset,
            pipeline.settings.copy_audio,
        )?;

        let mut frame_buf = Vec::new();
        let mut readback_buf = Vec::new();
        let mut current_frame = 0u32;
        let start_time = Instant::now();
        let mut last_report = Instant::now();

        while decoder.read_frame(&mut frame_buf)? {
            if cancel_flag.load(Ordering::Relaxed) {
                let _ = tx.send(RenderProgressMsg::Cancelled);
                let _ = std::fs::remove_file(&output_path);
                return Ok(());
            }

            engine.upload_source_frame(&frame_buf);
            engine.update_globals(current_frame, meta.fps);

            let mut cmd_encoder = engine
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("worker_render_encoder"),
                });
            let final_tex = engine.render_frame(&mut cmd_encoder);
            engine.queue.submit(Some(cmd_encoder.finish()));

            engine.readback_frame(final_tex, &mut readback_buf)?;
            encoder.write_frame(&readback_buf)?;

            current_frame += 1;

            if last_report.elapsed() >= Duration::from_millis(100) || current_frame as u64 == total_frames {
                let elapsed_secs = start_time.elapsed().as_secs_f32().max(0.001);
                let fps = current_frame as f32 / elapsed_secs;
                let _ = tx.send(RenderProgressMsg::Progress {
                    current_frame,
                    total_frames,
                    fps,
                });
                last_report = Instant::now();
            }
        }

        encoder.finish()?;
        let total_duration = start_time.elapsed().as_secs_f32();
        let _ = tx.send(RenderProgressMsg::Completed {
            frames: current_frame,
            duration_secs: total_duration,
            output_path,
        });

        Ok(())
    };

    if let Err(e) = worker_fn() {
        let _ = tx.send(RenderProgressMsg::Failed(format!("{:#}", e)));
    }
}

impl ApplicationHandler for GuiApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window_attrs = Window::default_attributes()
            .with_title("ninja-shader studio")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(800.0, 500.0));

        let window = Arc::new(
            event_loop
                .create_window(window_attrs)
                .expect("Failed to create GUI window"),
        );
        self.window = Some(window.clone());

        // WGPU setup
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .expect("Failed to create wgpu surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("Failed to get surface-compatible adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("ninja_gui_device"),
                required_features: wgpu::Features::default(),
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
            },
            None,
        ))
        .expect("Failed to create device");

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // Blit pipeline
        let blit_sm = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gui_blit_shader"),
            source: wgpu::ShaderSource::Wgsl(BLIT_SHADER_WGSL.into()),
        });

        let blit_bg_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gui_blit_bg_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
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

        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gui_blit_pipeline_layout"),
            bind_group_layouts: &[&blit_bg_layout],
            push_constant_ranges: &[],
        });

        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gui_blit_pipeline"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blit_sm,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &blit_sm,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Initialize egui
        let egui_winit_state = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );

        let egui_renderer = egui_wgpu::Renderer::new(
            &device,
            surface_format,
            None,
            1,
            false,
        );

        self.surface = Some(surface);
        self.surface_config = Some(surface_config);
        self.device = Some(device);
        self.queue = Some(queue);
        self.adapter = Some(adapter);
        self.instance = Some(instance);
        self.blit_pipeline = Some(blit_pipeline);
        self.blit_bg_layout = Some(blit_bg_layout);
        self.blit_sampler = Some(blit_sampler);
        self.egui_winit_state = Some(egui_winit_state);
        self.egui_renderer = Some(egui_renderer);

        // Initialize engine if we have active config
        self.reinit_engine();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let window = match self.window {
            Some(ref w) => w.clone(),
            None => return,
        };

        // Forward event to egui
        let mut egui_consumed = false;
        if let Some(ref mut state) = self.egui_winit_state {
            let response = state.on_window_event(&window, &event);
            egui_consumed = response.consumed;
        }

        let is_arrow_nav = matches!(
            &event,
            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    logical_key: Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown),
                    state: ElementState::Pressed,
                    ..
                },
                ..
            }
        );

        if egui_consumed && self.egui_ctx.wants_keyboard_input() && !is_arrow_nav {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                if let (Some(surface), Some(ref mut config), Some(ref device)) =
                    (&self.surface, &mut self.surface_config, &self.device)
                {
                    config.width = new_size.width.max(1);
                    config.height = new_size.height.max(1);
                    surface.configure(device, config);
                }
                window.request_redraw();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => match logical_key {
                Key::Named(NamedKey::ArrowUp) => {
                    self.select_prev_pipeline();
                }
                Key::Named(NamedKey::ArrowDown) => {
                    self.select_next_pipeline();
                }
                Key::Named(NamedKey::Space) => {
                    self.paused = !self.paused;
                    self.set_notification(
                        if self.paused { "Paused" } else { "Playing" }.into(),
                        false,
                    );
                }
                Key::Character(c) if c.eq_ignore_ascii_case("r") => {
                    self.respawn_decoder();
                    self.set_notification("Restarted video playback".into(), false);
                }
                Key::Named(NamedKey::Escape) => {
                    event_loop.exit();
                }
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                self.poll_render_task();
                self.render_frame();
                window.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }
}

impl GuiApp {
    fn render_frame(&mut self) {
        let window = match self.window {
            Some(ref w) => w.clone(),
            None => return,
        };

        // 1. Advance video decoder if playing and enough time has elapsed
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame_instant);
        if !self.paused && dt >= self.frame_duration {
            self.last_frame_instant = now;
            if let Some(ref mut dec) = self.decoder {
                match dec.read_frame(&mut self.frame_buffer) {
                    Ok(true) => {
                        self.current_frame += 1;
                        if let Some(ref engine) = self.engine {
                            engine.upload_source_frame(&self.frame_buffer);
                            let fps = self.meta.as_ref().map_or(30.0, |m| m.fps);
                            engine.update_globals(self.current_frame, fps);
                        }
                    }
                    Ok(false) => {
                        if self.loop_video {
                            self.respawn_decoder();
                        } else {
                            self.paused = true;
                        }
                    }
                    Err(e) => {
                        eprintln!("Decoder error: {:?}", e);
                        self.decoder = None;
                    }
                }
            }
        }

        // 2. Run multi-pass GPU engine
        let device = match self.device {
            Some(ref d) => d.clone(),
            None => return,
        };
        let queue = match self.queue {
            Some(ref q) => q.clone(),
            None => return,
        };
        let surface_config = match self.surface_config {
            Some(ref c) => c.clone(),
            None => return,
        };

        let output_surface = if let Some(ref surface) = self.surface {
            match surface.get_current_texture() {
                Ok(tex) => tex,
                Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                    let size = window.inner_size();
                    let mut cfg = surface_config.clone();
                    cfg.width = size.width.max(1);
                    cfg.height = size.height.max(1);
                    surface.configure(&device, &cfg);
                    return;
                }
                Err(_) => return,
            }
        } else {
            return;
        };

        let surface_view = output_surface
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gui_render_encoder"),
        });

        // Execute GPU passes
        let final_video_tex_view = if let Some(ref mut engine) = self.engine {
            let final_tex = engine.render_frame(&mut encoder);
            Some(final_tex.create_view(&wgpu::TextureViewDescriptor::default()))
        } else {
            None
        };

        // 3. Prepare egui UI
        let egui_winit_state = match self.egui_winit_state.as_mut() {
            Some(s) => s,
            None => return,
        };

        let raw_input = egui_winit_state.take_egui_input(&window);
        let mut trigger_rebuild_passes = false;
        let mut new_pipeline_to_load = None;
        let mut pick_video_requested = false;
        let mut pick_output_dir_requested = false;
        let mut pick_texture_requested: Option<(usize, String)> = None;
        let mut trigger_start_render = false;
        let mut trigger_cancel_render = false;
        let mut trigger_restart = false;

        let egui_output = self.egui_ctx.run(raw_input, |ctx| {
            // Theme settings
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = egui::Color32::from_rgb(20, 22, 28);
            visuals.window_fill = egui::Color32::from_rgb(24, 27, 34);
            visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(28, 32, 42);
            ctx.set_visuals(visuals);

            egui::SidePanel::left("left_control_panel")
                .exact_width(self.sidebar_width)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            ui.add_space(8.0);
                            ui.heading("🥷 ninja-shader studio");
                            ui.label(
                                egui::RichText::new("GPU Video Shader Pipeline & Preview")
                                    .small()
                                    .color(egui::Color32::from_rgb(140, 150, 170)),
                            );
                            ui.add_space(10.0);

                            // Section 1: Source & Output
                            egui::CollapsingHeader::new(egui::RichText::new("📁 Media Input & Output").strong())
                                .default_open(true)
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new("Input Video:").small());
                                    let name = self
                                        .input_video_path
                                        .as_ref()
                                        .and_then(|p| p.file_name())
                                        .map(|n| n.to_string_lossy().to_string())
                                        .unwrap_or_else(|| "No video selected".into());
                                    let full_path = self
                                        .input_video_path
                                        .as_ref()
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_else(|| "No video selected".into());

                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(name).strong().monospace(),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(full_path);

                                    if ui.button("Browse...").clicked() {
                                        pick_video_requested = true;
                                    }

                                    if let Some(ref meta) = self.meta {
                                        ui.add_space(2.0);
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "📐 {}x{} • {:.1} fps • {:.2}s ({} frames)",
                                                meta.width, meta.height, meta.fps, meta.duration, meta.total_frames
                                            ))
                                            .small()
                                            .color(egui::Color32::from_rgb(100, 200, 160)),
                                        );
                                    }

                                    ui.add_space(6.0);
                                    ui.label(egui::RichText::new("Output Directory:").small());
                                    let dir_str = self.output_dir.display().to_string();
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(&dir_str).monospace())
                                            .truncate(),
                                    )
                                    .on_hover_text(&dir_str);

                                    if ui.button("Choose...").clicked() {
                                        pick_output_dir_requested = true;
                                    }

                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.label("Filename:");
                                        ui.text_edit_singleline(&mut self.output_filename);
                                    });
                                });

                            ui.add_space(10.0);

                            // Section 2: Pipelines List
                            egui::CollapsingHeader::new(
                                egui::RichText::new(format!(
                                    "⚡ Pipelines ({})",
                                    self.pipeline_items.len()
                                ))
                                .strong(),
                            )
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("🔍");
                                    ui.text_edit_singleline(&mut self.pipeline_filter);
                                    if ui.button("🔄").on_hover_text("Rescan pipelines folder").clicked() {
                                        self.pipeline_items = scan_pipelines(&self.pipelines_dir);
                                    }
                                });

                                let filter_lower = self.pipeline_filter.to_lowercase();

                                egui::ScrollArea::vertical()
                                    .max_height(170.0)
                                    .show(ui, |ui| {
                                        for item in &self.pipeline_items {
                                            if !filter_lower.is_empty()
                                                && !item.name.to_lowercase().contains(&filter_lower)
                                                && !item.filename.to_lowercase().contains(&filter_lower)
                                            {
                                                continue;
                                            }

                                            let is_selected = self
                                                .selected_pipeline_path
                                                .as_ref()
                                                .map_or(false, |p| p == &item.path);

                                            let text = format!("{} ({} passes)", item.name, item.passes_count);
                                            let resp = ui.selectable_label(is_selected, text);
                                            if resp.clicked() && !is_selected {
                                                new_pipeline_to_load = Some(item.path.clone());
                                            }
                                            if is_selected && self.scroll_to_selected {
                                                resp.scroll_to_me(Some(egui::Align::Center));
                                            }
                                            if !item.description.is_empty() {
                                                resp.on_hover_text(&item.description);
                                            }
                                        }
                                    });
                                self.scroll_to_selected = false;
                            });

                            ui.add_space(10.0);

                            // Section 3: Live Shader Parameters
                            if let Some(ref mut config) = self.active_pipeline_config {
                                egui::CollapsingHeader::new(
                                    egui::RichText::new(format!("🎛 Shader Passes ({})", config.passes.len()))
                                        .strong(),
                                )
                                .default_open(true)
                                .show(ui, |ui| {
                                    for (pass_idx, pass) in config.passes.iter_mut().enumerate() {
                                        let title = format!("Pass #{}: {}", pass_idx + 1, pass.shader);
                                        let manifest_opt = self.pass_manifests.get(pass_idx).and_then(|m| m.as_ref());

                                        ui.collapsing(title, |ui| {
                                            ui.horizontal(|ui| {
                                                if ui.checkbox(&mut pass.enabled, "Enabled").changed() {
                                                    trigger_rebuild_passes = true;
                                                }
                                            });

                                            if let Some(manifest) = manifest_opt {
                                                for param_def in &manifest.params {
                                                    let toml_entry = pass
                                                        .params
                                                        .entry(param_def.name.clone())
                                                        .or_insert_with(|| param_value_to_toml(&param_def.default));

                                                    let param_val = toml_to_param_value(toml_entry, &param_def.default);

                                                    let desc_tooltip = param_def
                                                        .description
                                                        .as_deref()
                                                        .unwrap_or(&param_def.name);

                                                    match param_def.r#type {
                                                        ParamType::Float => {
                                                            let mut f = param_val.as_f32();
                                                            let min = param_def.min.unwrap_or(0.0) as f32;
                                                            let max = param_def.max.unwrap_or(1.0) as f32;
                                                            ui.horizontal(|ui| {
                                                                ui.label(&param_def.name).on_hover_text(desc_tooltip);
                                                                let slider = ui.add(
                                                                    egui::Slider::new(&mut f, min..=max)
                                                                        .step_by(0.01)
                                                                        .clamping(egui::SliderClamping::Never),
                                                                );
                                                                if slider.changed() {
                                                                    *toml_entry = toml::Value::Float(f as f64);
                                                                    trigger_rebuild_passes = true;
                                                                }
                                                            });
                                                        }
                                                        ParamType::Int => {
                                                            let mut i = param_val.as_i32();
                                                            let min = param_def.min.unwrap_or(0.0) as i32;
                                                            let max = param_def.max.unwrap_or(100.0) as i32;
                                                            ui.horizontal(|ui| {
                                                                ui.label(&param_def.name).on_hover_text(desc_tooltip);
                                                                let slider = ui.add(egui::Slider::new(&mut i, min..=max));
                                                                if slider.changed() {
                                                                    *toml_entry = toml::Value::Integer(i as i64);
                                                                    trigger_rebuild_passes = true;
                                                                }
                                                            });
                                                        }
                                                        ParamType::Color => {
                                                            let mut c = param_val.as_vec4();
                                                            let mut color32 = egui::Color32::from_rgba_unmultiplied(
                                                                (c[0] * 255.0).clamp(0.0, 255.0) as u8,
                                                                (c[1] * 255.0).clamp(0.0, 255.0) as u8,
                                                                (c[2] * 255.0).clamp(0.0, 255.0) as u8,
                                                                (c[3] * 255.0).clamp(0.0, 255.0) as u8,
                                                            );
                                                            ui.horizontal(|ui| {
                                                                ui.label(&param_def.name).on_hover_text(desc_tooltip);
                                                                if ui.color_edit_button_srgba(&mut color32).changed() {
                                                                    c[0] = color32.r() as f32 / 255.0;
                                                                    c[1] = color32.g() as f32 / 255.0;
                                                                    c[2] = color32.b() as f32 / 255.0;
                                                                    c[3] = color32.a() as f32 / 255.0;
                                                                    *toml_entry = toml::Value::Array(vec![
                                                                        toml::Value::Float(c[0] as f64),
                                                                        toml::Value::Float(c[1] as f64),
                                                                        toml::Value::Float(c[2] as f64),
                                                                        toml::Value::Float(c[3] as f64),
                                                                    ]);
                                                                    trigger_rebuild_passes = true;
                                                                }
                                                            });
                                                        }
                                                        _ => {}
                                                    }
                                                }

                                                if !manifest.textures.is_empty() {
                                                    ui.add_space(6.0);
                                                    ui.separator();
                                                    ui.label(egui::RichText::new("🖼 Textures").strong());

                                                    for tex_def in &manifest.textures {
                                                        let tex_entry = pass
                                                            .textures
                                                            .entry(tex_def.name.clone())
                                                            .or_insert_with(|| {
                                                                if let Some(ref def_p) = tex_def.default {
                                                                    TextureInput::Path(def_p.clone())
                                                                } else {
                                                                    TextureInput::Generator(GeneratorConfig::default())
                                                                }
                                                            });

                                                        ui.group(|ui| {
                                                            ui.horizontal(|ui| {
                                                                ui.label(egui::RichText::new(&tex_def.name).strong());

                                                                let current_type = match tex_entry {
                                                                    TextureInput::Path(_) => "📁 File",
                                                                    TextureInput::Generator(g) => match g.generator {
                                                                        GeneratorKind::Voronoi => "🌀 Voronoi",
                                                                        GeneratorKind::Noise => "🌊 Noise / FBM",
                                                                    },
                                                                };

                                                                egui::ComboBox::from_id_salt(format!("tex_type_{}_{}", pass_idx, tex_def.name))
                                                                    .selected_text(current_type)
                                                                    .show_ui(ui, |ui| {
                                                                        if ui.selectable_label(matches!(tex_entry, TextureInput::Path(_)), "📁 File").clicked() {
                                                                            *tex_entry = TextureInput::Path(tex_def.default.clone().unwrap_or_default());
                                                                            trigger_rebuild_passes = true;
                                                                        }
                                                                        if ui.selectable_label(
                                                                            matches!(tex_entry, TextureInput::Generator(g) if g.generator == GeneratorKind::Voronoi),
                                                                            "🌀 Voronoi",
                                                                        ).clicked() {
                                                                            *tex_entry = TextureInput::Generator(GeneratorConfig {
                                                                                generator: GeneratorKind::Voronoi,
                                                                                ..Default::default()
                                                                            });
                                                                            trigger_rebuild_passes = true;
                                                                        }
                                                                        if ui.selectable_label(
                                                                            matches!(tex_entry, TextureInput::Generator(g) if g.generator == GeneratorKind::Noise),
                                                                            "🌊 Noise / FBM",
                                                                        ).clicked() {
                                                                            *tex_entry = TextureInput::Generator(GeneratorConfig {
                                                                                generator: GeneratorKind::Noise,
                                                                                ..Default::default()
                                                                            });
                                                                            trigger_rebuild_passes = true;
                                                                        }
                                                                    });
                                                            });

                                                            match tex_entry {
                                                                TextureInput::Path(path_str) => {
                                                                    ui.horizontal(|ui| {
                                                                        let file_name = if path_str.is_empty() {
                                                                            "No file selected".to_string()
                                                                        } else {
                                                                            Path::new(&path_str)
                                                                                .file_name()
                                                                                .map(|n| n.to_string_lossy().to_string())
                                                                                .unwrap_or_else(|| path_str.clone())
                                                                        };

                                                                        ui.add(
                                                                            egui::Label::new(
                                                                                egui::RichText::new(&file_name).strong().monospace(),
                                                                            )
                                                                            .truncate(),
                                                                        )
                                                                        .on_hover_text(if path_str.is_empty() {
                                                                            "No texture file selected"
                                                                        } else {
                                                                            path_str.as_str()
                                                                        });

                                                                        if !path_str.is_empty() {
                                                                            if Path::new(&path_str).exists() {
                                                                                ui.colored_label(egui::Color32::GREEN, "✓");
                                                                            } else {
                                                                                ui.colored_label(egui::Color32::RED, "✗");
                                                                            }
                                                                        }

                                                                        if ui.button("Browse...").clicked() {
                                                                            pick_texture_requested = Some((pass_idx, tex_def.name.clone()));
                                                                        }
                                                                    });
                                                                }
                                                                TextureInput::Generator(gen_cfg) => {
                                                                    // Common: Seed & Roll
                                                                    ui.horizontal(|ui| {
                                                                        ui.label("Seed:");
                                                                        if ui.add(egui::DragValue::new(&mut gen_cfg.seed)).changed() {
                                                                            trigger_rebuild_passes = true;
                                                                        }
                                                                        if ui.button("🎲 Roll").clicked() {
                                                                            gen_cfg.seed = std::time::SystemTime::now()
                                                                                .duration_since(std::time::UNIX_EPOCH)
                                                                                .map(|d| d.subsec_nanos() % 100000)
                                                                                .unwrap_or(42);
                                                                            trigger_rebuild_passes = true;
                                                                        }
                                                                    });

                                                                    // Common: Scale
                                                                    ui.horizontal(|ui| {
                                                                        ui.label("Scale:");
                                                                        if ui.add(egui::Slider::new(&mut gen_cfg.scale, 0.5..=64.0).step_by(0.5)).changed() {
                                                                            trigger_rebuild_passes = true;
                                                                        }
                                                                    });

                                                                    match gen_cfg.generator {
                                                                        GeneratorKind::Voronoi => {
                                                                            ui.horizontal(|ui| {
                                                                                ui.label("Jitter:");
                                                                                if ui.add(egui::Slider::new(&mut gen_cfg.jitter, 0.0..=1.0).step_by(0.05)).changed() {
                                                                                    trigger_rebuild_passes = true;
                                                                                }
                                                                            });

                                                                            ui.horizontal(|ui| {
                                                                                ui.label("Mode:");
                                                                                let mode_label = match gen_cfg.mode {
                                                                                    GeneratorMode::Distance => "Distance (F1)",
                                                                                    GeneratorMode::Borders => "Borders (F2 - F1)",
                                                                                    GeneratorMode::Cells => "Cell Colors",
                                                                                };
                                                                                egui::ComboBox::from_id_salt(format!("vor_mode_{}_{}", pass_idx, tex_def.name))
                                                                                    .selected_text(mode_label)
                                                                                    .show_ui(ui, |ui| {
                                                                                        if ui.selectable_label(gen_cfg.mode == GeneratorMode::Distance, "Distance (F1)").clicked() {
                                                                                            gen_cfg.mode = GeneratorMode::Distance;
                                                                                            trigger_rebuild_passes = true;
                                                                                        }
                                                                                        if ui.selectable_label(gen_cfg.mode == GeneratorMode::Borders, "Borders (F2 - F1)").clicked() {
                                                                                            gen_cfg.mode = GeneratorMode::Borders;
                                                                                            trigger_rebuild_passes = true;
                                                                                        }
                                                                                        if ui.selectable_label(gen_cfg.mode == GeneratorMode::Cells, "Cell Colors").clicked() {
                                                                                            gen_cfg.mode = GeneratorMode::Cells;
                                                                                            trigger_rebuild_passes = true;
                                                                                        }
                                                                                    });
                                                                            });

                                                                            ui.horizontal(|ui| {
                                                                                ui.label("Metric:");
                                                                                let metric_label = match gen_cfg.metric {
                                                                                    DistanceMetric::Euclidean => "Euclidean",
                                                                                    DistanceMetric::Manhattan => "Manhattan",
                                                                                };
                                                                                egui::ComboBox::from_id_salt(format!("vor_metric_{}_{}", pass_idx, tex_def.name))
                                                                                    .selected_text(metric_label)
                                                                                    .show_ui(ui, |ui| {
                                                                                        if ui.selectable_label(gen_cfg.metric == DistanceMetric::Euclidean, "Euclidean").clicked() {
                                                                                            gen_cfg.metric = DistanceMetric::Euclidean;
                                                                                            trigger_rebuild_passes = true;
                                                                                        }
                                                                                        if ui.selectable_label(gen_cfg.metric == DistanceMetric::Manhattan, "Manhattan").clicked() {
                                                                                            gen_cfg.metric = DistanceMetric::Manhattan;
                                                                                            trigger_rebuild_passes = true;
                                                                                        }
                                                                                    });
                                                                            });
                                                                        }
                                                                        GeneratorKind::Noise => {
                                                                            ui.horizontal(|ui| {
                                                                                ui.label("Octaves:");
                                                                                if ui.add(egui::Slider::new(&mut gen_cfg.octaves, 1..=8)).changed() {
                                                                                    trigger_rebuild_passes = true;
                                                                                }
                                                                            });
                                                                            ui.horizontal(|ui| {
                                                                                ui.label("Persistence:");
                                                                                if ui.add(egui::Slider::new(&mut gen_cfg.persistence, 0.1..=1.0).step_by(0.05)).changed() {
                                                                                    trigger_rebuild_passes = true;
                                                                                }
                                                                            });
                                                                            ui.horizontal(|ui| {
                                                                                ui.label("Lacunarity:");
                                                                                if ui.add(egui::Slider::new(&mut gen_cfg.lacunarity, 1.0..=4.0).step_by(0.1)).changed() {
                                                                                    trigger_rebuild_passes = true;
                                                                                }
                                                                            });
                                                                        }
                                                                    }

                                                                    // Common: Animate & Speed
                                                                    ui.horizontal(|ui| {
                                                                        if ui.checkbox(&mut gen_cfg.animate, "Animate").changed() {
                                                                            trigger_rebuild_passes = true;
                                                                        }
                                                                        if gen_cfg.animate {
                                                                            ui.label("Speed:");
                                                                            if ui.add(egui::Slider::new(&mut gen_cfg.speed, 0.1..=10.0).step_by(0.1)).changed() {
                                                                                trigger_rebuild_passes = true;
                                                                            }
                                                                        }
                                                                    });
                                                                }
                                                            }
                                                        });
                                                    }
                                                }
                                            }
                                        });
                                    }
                                });
                            }

                            ui.add_space(10.0);

                            // Section 4: Playback Controls
                            egui::CollapsingHeader::new(egui::RichText::new("🎬 Playback Controls").strong())
                                .default_open(true)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let play_icon = if self.paused { "▶ Play" } else { "⏸ Pause" };
                                        if ui.button(play_icon).clicked() {
                                            self.paused = !self.paused;
                                        }

                                        if ui.button("⏮ Restart (R)").clicked() {
                                            trigger_restart = true;
                                        }

                                        ui.checkbox(&mut self.loop_video, "Loop");
                                    });

                                    ui.add_space(4.0);
                                    let mut frame_display = self.current_frame;
                                    let max_f = self.total_frames.max(1) as u32;
                                    ui.horizontal(|ui| {
                                        ui.label(format!("Frame {}/{}", frame_display, max_f));
                                    });
                                    let slider = ui.add(
                                        egui::Slider::new(&mut frame_display, 0..=max_f)
                                            .show_value(false),
                                    );
                                    if slider.changed() {
                                        // seek logic if desired
                                    }
                                });

                            ui.add_space(10.0);

                            // Section 5: Render Controls
                            egui::CollapsingHeader::new(egui::RichText::new("🚀 Render to File").strong())
                                .default_open(true)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Scale:");
                                        ui.add(egui::Slider::new(&mut self.render_scale, 0.25..=2.0).step_by(0.25));
                                    });

                                    ui.horizontal(|ui| {
                                        ui.label("CRF (Quality):");
                                        ui.add(egui::Slider::new(&mut self.render_crf, 0..=51));
                                    });

                                    ui.add_space(6.0);

                                    if let Some(ref render_task) = self.active_render {
                                        let progress = if render_task.total_frames > 0 {
                                            render_task.current_frame as f32 / render_task.total_frames as f32
                                        } else {
                                            0.0
                                        };

                                        ui.add(
                                            egui::ProgressBar::new(progress)
                                                .show_percentage()
                                                .animate(true)
                                                .text(&render_task.status),
                                        );

                                        ui.horizontal(|ui| {
                                            ui.label(format!(
                                                "Elapsed: {:.1}s",
                                                render_task.start_time.elapsed().as_secs_f32()
                                            ));
                                            if ui.button(egui::RichText::new("⏹ Cancel Render").color(egui::Color32::RED)).clicked() {
                                                trigger_cancel_render = true;
                                            }
                                        });
                                    } else {
                                        let btn = ui.add_sized(
                                            [ui.available_width(), 32.0],
                                            egui::Button::new(
                                                egui::RichText::new("🚀 Render Full Video")
                                                    .strong()
                                                    .color(egui::Color32::WHITE),
                                            )
                                            .fill(egui::Color32::from_rgb(30, 140, 220)),
                                        );

                                        if btn.clicked() {
                                            trigger_start_render = true;
                                        }

                                        if let Some(Ok(ref path)) = self.last_render_result {
                                            ui.label(
                                                egui::RichText::new(format!("✓ Saved: {}", path.display()))
                                                    .small()
                                                    .color(egui::Color32::GREEN),
                                            );
                                        } else if let Some(Err(ref err)) = self.last_render_result {
                                            ui.label(
                                                egui::RichText::new(format!("✗ Error: {}", err))
                                                    .small()
                                                    .color(egui::Color32::RED),
                                            );
                                        }
                                    }
                                });

                            ui.add_space(10.0);

                            // Notification toast
                            if let Some((ref text, is_error, timestamp)) = self.notification {
                                if timestamp.elapsed() < Duration::from_secs(4) {
                                    let color = if is_error {
                                        egui::Color32::from_rgb(240, 80, 80)
                                    } else {
                                        egui::Color32::from_rgb(80, 220, 140)
                                    };
                                    ui.colored_label(color, text);
                                }
                            }
                        });
                });

            // Transparent central panel allows underlying wgpu blit to show
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |_ui| {});
        });

        // Handle UI actions outside the egui closure
        if pick_video_requested {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Video Files", &["mp4", "mov", "mkv", "avi", "webm"])
                .pick_file()
            {
                self.set_input_video(path);
            }
        }

        if pick_output_dir_requested {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                self.output_dir = dir;
            }
        }

        if let Some((p_idx, tex_name)) = pick_texture_requested {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Image Files", &["png", "jpg", "jpeg"])
                .pick_file()
            {
                if let Some(ref mut config) = self.active_pipeline_config {
                    if let Some(pass) = config.passes.get_mut(p_idx) {
                        pass.textures.insert(
                            tex_name,
                            TextureInput::Path(path.display().to_string()),
                        );
                        trigger_rebuild_passes = true;
                    }
                }
            }
        }

        if let Some(path) = new_pipeline_to_load {
            self.load_pipeline(&path);
        }

        if trigger_rebuild_passes {
            if let (Some(ref mut engine), Some(ref config)) =
                (&mut self.engine, &self.active_pipeline_config)
            {
                let _ = engine.build_passes(config, &self.shaders_dirs);
            }
        }

        if trigger_restart {
            self.respawn_decoder();
        }

        if trigger_start_render {
            self.start_background_render();
        }

        if trigger_cancel_render {
            if let Some(ref task) = self.active_render {
                task.cancel_flag.store(true, Ordering::Relaxed);
            }
        }

        // 4. Render Video Blit to Surface Viewport
        let physical_width = surface_config.width as f32;
        let physical_height = surface_config.height as f32;
        let scale_factor = window.scale_factor() as f32;
        let sidebar_physical = self.sidebar_width * scale_factor;

        let avail_x = sidebar_physical;
        let avail_y = 0.0;
        let avail_w = (physical_width - avail_x).max(1.0);
        let avail_h = physical_height.max(1.0);

        let video_w = self.render_width as f32;
        let video_h = self.render_height as f32;
        let video_aspect = video_w / video_h.max(1.0);
        let avail_aspect = avail_w / avail_h;

        let (fit_w, fit_h) = if avail_aspect > video_aspect {
            let h = avail_h;
            let w = h * video_aspect;
            (w, h)
        } else {
            let w = avail_w;
            let h = w / video_aspect;
            (w, h)
        };

        let fit_x = avail_x + (avail_w - fit_w) * 0.5;
        let fit_y = avail_y + (avail_h - fit_h) * 0.5;

        // Blit pass
        if let (Some(ref blit_pipe), Some(ref blit_layout), Some(ref sampler), Some(ref final_view)) = (
            &self.blit_pipeline,
            &self.blit_bg_layout,
            &self.blit_sampler,
            &final_video_tex_view,
        ) {
            let blit_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gui_blit_bind_group"),
                layout: blit_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(final_view),
                    },
                ],
            });

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("video_blit_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05,
                            g: 0.06,
                            b: 0.08,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(blit_pipe);
            render_pass.set_viewport(fit_x, fit_y, fit_w, fit_h, 0.0, 1.0);
            render_pass.set_scissor_rect(
                fit_x.round() as u32,
                fit_y.round() as u32,
                fit_w.round().max(1.0) as u32,
                fit_h.round().max(1.0) as u32,
            );
            render_pass.set_bind_group(0, &blit_bg, &[]);
            render_pass.draw(0..3, 0..1);
        }

        // 5. Render egui on top
        let egui_winit_state = match self.egui_winit_state.as_mut() {
            Some(s) => s,
            None => return,
        };
        egui_winit_state.handle_platform_output(&window, egui_output.platform_output);

        let tris = self
            .egui_ctx
            .tessellate(egui_output.shapes, egui_output.pixels_per_point);

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [surface_config.width, surface_config.height],
            pixels_per_point: egui_output.pixels_per_point,
        };

        if let Some(ref mut renderer) = self.egui_renderer {
            for (id, image_delta) in &egui_output.textures_delta.set {
                renderer.update_texture(&device, &queue, *id, image_delta);
            }

            renderer.update_buffers(&device, &queue, &mut encoder, &tris, &screen_descriptor);

            {
                let mut egui_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui_render_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &surface_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load, // draw over blitted video
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                }).forget_lifetime();

                renderer.render(&mut egui_pass, &tris, &screen_descriptor);
            }

            for id in &egui_output.textures_delta.free {
                renderer.free_texture(id);
            }
        }

        queue.submit(Some(encoder.finish()));
        output_surface.present();
    }
}

pub fn run_gui(args: GuiArgs, shaders_dir: PathBuf) -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = GuiApp::new(args, shaders_dir)?;
    event_loop.run_app(&mut app)?;
    Ok(())
}
