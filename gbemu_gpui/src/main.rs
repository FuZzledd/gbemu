use core::{
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::Duration,
};
use cpal::{
    BufferSize,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use dasp::Signal;
use etcetera::{AppStrategy, AppStrategyArgs};
use gbemu_core::{GameBoy, PLAYBACK_CONTROLLER, PLAYING, Palette, PlaybackMessage, TickStatus};
use gpui::prelude::*;
use gpui::*;
use gpui_elements::editable_text::{self, actions::DEFAULT_INPUT_CONTEXT};
use indexmap::IndexSet;
use parking_lot::{Mutex, RwLock};
use rfd::{MessageButtons, MessageLevel};
use ringbuf::{
    storage::Heap,
    traits::{Consumer, Observer, Producer},
};
use serde::Serialize;
use spire_enum::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::ops::{Deref, DerefMut};
use std::{
    env, fs,
    io::{BufWriter, Write},
    mem,
    path::PathBuf,
    sync::Arc,
    thread,
    time::Instant,
};
use strum::{EnumIter, EnumString};
use tap::{Conv, Tap};
use tracing::error;
use tracing_flame::FlameLayer;
use tracing_subscriber::{EnvFilter, fmt, prelude::*, registry::Registry};

pub mod actions;
pub mod components;

pub mod assets;
pub mod screen;
pub mod theme;

pub mod debugger;
pub mod settings;

use components::root::Root;

struct GlobalState {
    gameboy: Arc<Mutex<GameBoy>>,
    scale_factor: u32,
    integer_scaling: bool,
    fixed_size: bool,
    linear_filtering: bool,
    show_fps: bool,
    fast_forward_held: bool,
    fast_forward_on: bool,
}
impl Global for GlobalState {}

#[repr(transparent)]
#[derive(Debug, Clone)]
struct RecentFiles(Arc<RwLock<IndexSet<PathBuf>>>);
impl Global for RecentFiles {}
impl Deref for RecentFiles {
    type Target = Arc<RwLock<IndexSet<PathBuf>>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for RecentFiles {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Clone, Debug)]
#[delegated_enum(impl_conversions)]
enum EtceteraStrategy {
    Xdg(etcetera::app_strategy::Xdg),
    Unix(etcetera::app_strategy::Unix),
    Apple(etcetera::app_strategy::Apple),
    Windows(etcetera::app_strategy::Windows),
}
impl Global for EtceteraStrategy {}

#[delegate_impl]
impl AppStrategy for EtceteraStrategy {
    fn home_dir(&self) -> &std::path::Path;

    fn config_dir(&self) -> PathBuf;

    fn data_dir(&self) -> PathBuf;

    fn cache_dir(&self) -> PathBuf;

    fn state_dir(&self) -> Option<PathBuf>;

    fn runtime_dir(&self) -> Option<PathBuf>;

    fn in_config_dir<P: AsRef<OsStr>>(&self, path: P) -> PathBuf;

    fn in_data_dir<P: AsRef<OsStr>>(&self, path: P) -> PathBuf;

    fn in_cache_dir<P: AsRef<OsStr>>(&self, path: P) -> PathBuf;

    fn in_state_dir<P: AsRef<OsStr>>(&self, path: P) -> Option<PathBuf>;

    fn in_runtime_dir<P: AsRef<OsStr>>(&self, path: P) -> Option<PathBuf>;
}

thread_local! {
    pub static APP: Cell<Option<AsyncApp>> = Cell::new(None);
}

fn main() -> Result<()> {
    dioxus_devtools::connect_subsecond();

    let filter = EnvFilter::from_default_env();
    let fmt_layer = fmt::Layer::default().with_filter(filter);

    // let (flame_layer, _guard) = FlameLayer::with_file("./tracing.folded").unwrap();

    let subscriber = Registry::default().with(fmt_layer);
    // .with(flame_layer);

    tracing::subscriber::set_global_default(subscriber).expect("Could not set global default");

    let gameboy: Arc<Mutex<GameBoy>> = Default::default();

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("No output device available");

    let mut supported_configs_range = device
        .supported_output_configs()
        .expect("error while querying configs");
    for conf in supported_configs_range.clone() {
        dbg!(conf);
    }
    let supported_config = supported_configs_range
        .next()
        .expect("no supported config?!")
        .with_max_sample_rate();
    let mut stream_config = supported_config.config();

    match supported_config.buffer_size() {
        cpal::SupportedBufferSize::Range { min: _, max: _ } => {
            stream_config.buffer_size = BufferSize::Fixed(1024);
        }
        cpal::SupportedBufferSize::Unknown => todo!(),
    }
    stream_config.sample_rate = 48_000;

    let (audio_controller_sender, audio_controller_receiver) = crossbeam::channel::bounded(1);

    let mut signal = GBSignal::create(
        audio_controller_receiver,
        gameboy.lock().apu.output_channel.1.clone(),
    );

    let stream = device.build_output_stream(
        stream_config,
        using!([], move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
            for sample in data.as_chunks_mut::<2>().0.iter_mut() {
                *sample = signal.next();
            }
        }),
        move |err| {
            println!("{}", err);
        },
        None,
    )?;
    stream.play()?;

    fn to_serialized(x: impl Action + Serialize + Clone) -> (serde_json::Value, Box<dyn Action>) {
        (serde_json::to_value(x.clone()).unwrap(), x.boxed_clone())
    }

    gpui_platform::application()
        .with_assets(Icons)
        .run(move |cx: &mut App| {
            let strategy = EtceteraStrategy::from(
                etcetera::app_strategy::choose_native_strategy(AppStrategyArgs {
                    top_level_domain: "uk".into(),
                    author: "fuzzle".into(),
                    app_name: "gbemu".into(),
                })
                .unwrap(),
            );

            fs::create_dir_all(strategy.data_dir()).unwrap();
            fs::create_dir_all(strategy.config_dir()).unwrap();

            let recent_path = strategy.in_data_dir("recent.json");

            let recent = RecentFiles(Arc::new(RwLock::<IndexSet<_>>::new(
                if let Ok(bytes) = fs::read(recent_path.clone()) {
                    serde_json::from_slice::<IndexSet<PathBuf>>(&bytes)
                        .unwrap_or_else(|_| IndexSet::with_capacity(5))
                } else {
                    IndexSet::with_capacity(5)
                },
            )));

            cx.default_global::<ThemeRegistry>();
            cx.default_global::<WindowMap>();
            cx.set_global(recent.clone());
            cx.set_global(strategy.clone());

            let global_state = GlobalState {
                gameboy: gameboy.clone(),
                scale_factor: 1,
                integer_scaling: true,
                fixed_size: false,
                linear_filtering: false,
                show_fps: false,
                fast_forward_held: false,
                fast_forward_on: false,
            };

            cx.set_global(global_state);

            let bounds = Bounds::centered(None, size(px(500.), px(500.0)), cx);
            cx.spawn(async move |cx| {
                APP.set(Some(cx.clone()));

                let config_path = strategy.in_config_dir("config.json");
                let settings: Settings = {
                    if let Ok(bytes) = fs::read(config_path.clone()) {
                        serde_json::from_slice(&bytes).unwrap()
                    } else {
                        fs::write(
                            config_path.clone(),
                            serde_json::to_string(&Settings::default()).unwrap(),
                        )
                        .unwrap();
                        Settings::default()
                    }
                };

                cx.open_window(
                    WindowOptions {
                        window_decorations: Some(WindowDecorations::Client),
                        titlebar: Some(TitlebarOptions {
                            title: Some("gbemu".into()),
                            ..Default::default() // ..TitleBar::title_bar_options()
                        }),
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        window_min_size: Some(size(px(200.0), px(200.0))),
                        ..Default::default()
                    },
                    |window, cx| {
                        window.set_app_id("uk.fuzzle.gbemu");

                        cx.set_global(settings);

                        reload_keys(cx);

                        let view = MainWindow::new(
                            gameboy.clone(),
                            window,
                            cx,
                            Arc::clone(&recent),
                            audio_controller_sender,
                        );

                        let window_id = window.window_handle().window_id();
                        cx.on_window_closed(move |cx, closed_window| {
                            if window_id == closed_window {
                                cx.quit()
                            }
                        })
                        .detach();

                        cx.on_action::<actions::file::OpenRom>(using!(
                            [gameboy, Arc::clone(&recent), recent_path, view],
                            move |_a, cx| {
                                let Some(rom_path) = rfd::FileDialog::new()
                                    .add_filter("GameBoy ROMs (.gb/.gbc)", &["gb"])
                                    .add_filter(
                                        "Archives (.zip/.tar.gz/.rar/etc.)",
                                        &["zip", "gz", "tar", "rar", "zst", "xz"],
                                    )
                                    .add_filter("ROM Embedded Images (.png)", &["png"])
                                    .set_directory(env::current_dir().unwrap_or_else(|_| {
                                        env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
                                    }))
                                    .pick_file()
                                else {
                                    return;
                                };

                                load_rom(
                                    &mut gameboy.lock(),
                                    rom_path,
                                    recent.clone(),
                                    recent_path.clone(),
                                );

                                cx.notify(view.entity_id());
                            }
                        ));

                        cx.on_action::<actions::file::OpenRomPath>(using!(
                            [gameboy, Arc::clone(&recent), recent_path, view],
                            move |actions::file::OpenRomPath(rom_path), cx| {
                                load_rom(
                                    &mut gameboy.lock(),
                                    rom_path,
                                    recent.clone(),
                                    recent_path.clone(),
                                );

                                cx.notify(view.entity_id());
                            }
                        ));

                        cx.on_action::<actions::file::CloseRom>(using!(
                            [gameboy, recent, recent_path, view],
                            move |_a, cx| {}
                        ));

                        Root::new(view, window, cx)
                    },
                )
                .unwrap();
            })
            .detach();
        });

    Ok(())
}

use crate::settings::{SerializableAction, Settings};
use crate::{
    assets::Icons, components::menubar::MenuBar, debugger::Debugger, screen::Screen,
    settings::SettingsWindow,
};
use crate::{components::titlebar::TitleBar, theme::ThemeRegistry};
use gbemu_core::ppu::Pixel;
use uzi::using;
use wgpu::util::{BufferInitDescriptor, DeviceExt};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent, BlendState, Buffer,
    BufferBindingType, BufferDescriptor, BufferUsages, ErrorFilter, FragmentState, FrontFace,
    Operations, PipelineLayoutDescriptor, PrimitiveState, PrimitiveTopology,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor,
    ShaderModule, ShaderStages, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
    TextureViewDescriptor, TextureViewDimension, VertexAttribute, VertexBufferLayout, VertexFormat,
    VertexState, VertexStepMode,
};

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, EnumIter, EnumString)]
enum WindowType {
    Debugger,
    TileViewer,
    TileMapViewer,
    MemoryViewer,
    Settings,
}

#[derive(Default, Debug, Clone)]
struct WindowMap(HashMap<WindowType, AnyWindowHandle>);

impl Deref for WindowMap {
    type Target = HashMap<WindowType, AnyWindowHandle>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for WindowMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Global for WindowMap {}

struct MainWindow {
    // app_menu_bar: Entity<AppMenuBar>,
    gameboy: Arc<Mutex<GameBoy>>,
    focus_handle: FocusHandle,
    menu_bar: Entity<MenuBar>,
    recent: Arc<parking_lot::lock_api::RwLock<parking_lot::RawRwLock, IndexSet<PathBuf>>>,
    gpu_context: Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)>,
    render_state: Option<Arc<RenderState>>,
    frame_delta: Arc<AtomicU32>,
    audio_controller_sender: crossbeam::channel::Sender<AudioControllerMessage>,
    fast_forwarding: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub struct RenderState {
    pub pipeline: RenderPipeline,
    pub bind_group: BindGroup,
    pub shader_module: ShaderModule,
    pub palette_buffer: Buffer,
    pub bind_group_layout: BindGroupLayout,
    pub screen_texture: Arc<wgpu::Texture>,
    pub vertex_buffer: Buffer,
    pub screen_source_view: wgpu::TextureView,
    pub screen_source: wgpu::Texture,
    pub screen_texture_view: wgpu::TextureView,
}

impl MainWindow {
    fn new(
        gameboy: Arc<Mutex<GameBoy>>,
        window: &mut Window,
        cx: &mut App,
        recent: Arc<parking_lot::lock_api::RwLock<parking_lot::RawRwLock, IndexSet<PathBuf>>>,
        audio_controller_sender: crossbeam::channel::Sender<AudioControllerMessage>,
    ) -> Entity<MainWindow> {
        let gpu_context = window.gpu_context().and_then(|gpu_context| {
            gpu_context
                .downcast::<(Arc<wgpu::Device>, Arc<wgpu::Queue>)>()
                .ok()
                .map(|gpu_context| gpu_context.as_ref().clone())
        });

        let frame_delta = Arc::new(AtomicU32::new(0));
        let fast_forwarding = Arc::new(AtomicBool::default());

        let vertices = &[
            [-1.0f32, -1.0, 0.0],
            [-1.0, 1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
        ];

        let render_state = gpu_context.as_ref().map(|(device, queue)| {
            let validation_errors = device.push_error_scope(ErrorFilter::Validation);
            let internal_errors = device.push_error_scope(ErrorFilter::Internal);
            let memory_errors = device.push_error_scope(ErrorFilter::OutOfMemory);
            let vertex_buffer = device.create_buffer_init(&BufferInitDescriptor {
                label: "Vertex Buffer".into(),
                contents: bytemuck::cast_slice(vertices),
                usage: BufferUsages::VERTEX,
            });

            let screen_source = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Input screen texture"),
                size: wgpu::Extent3d {
                    width: 160,
                    height: 144,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::R8Uint,
                usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });

            let screen_source_view = screen_source.create_view(&TextureViewDescriptor::default());

            let screen_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Screen Texture"),
                size: wgpu::Extent3d {
                    width: 160,
                    height: 144,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            let screen_texture_view =
                screen_texture.create_view(&wgpu::TextureViewDescriptor::default());

            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &screen_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                [[0, 0, 0, 255]; 160 * 144].as_flattened(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(160 * 4),
                    rows_per_image: Some(144),
                },
                wgpu::Extent3d {
                    width: 160,
                    height: 144,
                    depth_or_array_layers: 1,
                },
            );

            let shader_module = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

            let palette_buffer = device.create_buffer(&BufferDescriptor {
                label: "palette buffer".into(),
                size: size_of::<Palette<f32>>() as u64,
                usage: BufferUsages::COPY_DST | BufferUsages::UNIFORM,
                mapped_at_creation: false,
            });

            let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: "screen bind layout".into(),
                entries: &[
                    //Palette
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // Screen Input
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Texture {
                            sample_type: TextureSampleType::Uint,
                            view_dimension: TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });

            let bind_group = device.create_bind_group(&BindGroupDescriptor {
                label: "screen bind group".into(),
                layout: &bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: palette_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::TextureView(&screen_source_view),
                    },
                ],
            });

            let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
                label: "screen pipeline layout".into(),
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            });

            let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
                label: "screen pipeline".into(),
                layout: Some(&pipeline_layout),
                vertex: VertexState {
                    module: &shader_module,
                    entry_point: Some("screen_vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[VertexBufferLayout {
                        array_stride: size_of::<[f32; 3]>() as u64,
                        step_mode: VertexStepMode::Vertex,
                        attributes: &[VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: VertexFormat::Float32x3,
                        }],
                    }],
                },
                primitive: PrimitiveState {
                    topology: PrimitiveTopology::TriangleStrip,
                    strip_index_format: None,
                    front_face: FrontFace::Ccw,
                    cull_mode: None,
                    unclipped_depth: false,
                    polygon_mode: Default::default(),
                    conservative: false,
                },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(FragmentState {
                    module: &shader_module,
                    entry_point: Some("screen_fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: TextureFormat::Rgba8Unorm,
                        blend: Some(BlendState {
                            alpha: BlendComponent::REPLACE,
                            color: BlendComponent::REPLACE,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });

            if let Some(err) = block_on(memory_errors.pop()) {
                panic!("{}", err);
            }
            if let Some(err) = block_on(internal_errors.pop()) {
                panic!("{}", err);
            }
            if let Some(err) = block_on(validation_errors.pop()) {
                panic!("{}", err);
            }

            Arc::new(RenderState {
                pipeline,
                bind_group,
                shader_module,
                palette_buffer,
                bind_group_layout,
                screen_texture: screen_texture.into(),
                screen_texture_view,
                screen_source,
                screen_source_view,
                vertex_buffer,
            })
        });

        thread::spawn(using!(
            [
                gameboy,
                render_state,
                gpu_context,
                frame_delta,
                fast_forwarding
            ],
            move || {
                let mut prev_frame_time = Instant::now();
                let mut target_time = Instant::now() + Duration::from_secs_f64(1.0 / 59.73);
                loop {
                    if PLAYING.load(Ordering::Relaxed) {
                        let mut redraw_requested = false;
                        {
                            let mut gameboy = gameboy.lock();
                            for _ in 0..70225 {
                                redraw_requested |= gameboy.tick(false).should_redraw();
                                if redraw_requested {
                                    break;
                                }
                            }
                        }

                        if redraw_requested {
                            redraw_screen(
                                gameboy.clone(),
                                render_state.clone(),
                                gpu_context.clone(),
                            );

                            let delta_time = prev_frame_time.elapsed().as_secs_f32();
                            frame_delta.store(delta_time.to_bits(), Ordering::Relaxed);

                            prev_frame_time = Instant::now();
                        }

                        if !fast_forwarding.load(Ordering::Relaxed) {
                            spin_sleep::SpinSleeper::default()
                                .with_spin_strategy(spin_sleep::SpinStrategy::SpinLoopHint)
                                .sleep_until(target_time);
                            target_time = Instant::now() + Duration::from_secs_f64(1.0 / 59.73);
                        }
                    }
                }
            }
        ));

        let entity = cx.new(move |cx| Self {
            gameboy,
            recent,
            focus_handle: cx.focus_handle(),
            menu_bar: cx.new(|_| MenuBar::new()),
            gpu_context,
            render_state,
            frame_delta,
            audio_controller_sender,
            fast_forwarding,
        });

        let weak_entity = entity.downgrade();

        let playback_receiver = PLAYBACK_CONTROLLER.1.clone();
        cx.spawn(async move |cx| {
            loop {
                match playback_receiver.recv_async().await {
                    Ok(message) => match message {
                        PlaybackMessage::TogglePlayback => {
                            PLAYING.fetch_not(Ordering::Relaxed);
                        }
                        PlaybackMessage::Pause => {
                            PLAYING.store(false, Ordering::Relaxed);
                        }
                        PlaybackMessage::Play => {
                            PLAYING.store(true, Ordering::Relaxed);
                        }
                        PlaybackMessage::StepTick(ticks) => {
                            if let Some(entity) = weak_entity.upgrade() {
                                entity.update(cx, |this, cx| this.step_tick(ticks));
                            }
                        }
                        PlaybackMessage::StepFrame(frames) => {
                            if let Some(entity) = weak_entity.upgrade() {
                                entity.update(cx, |this, cx| this.step_frame(frames));
                            }
                        }
                    },
                    Err(err) => {
                        error!("{}", err);
                        return;
                    }
                };
            }
        })
        .detach();

        entity
    }

    fn step_tick(&mut self, ticks: usize) {
        using!([self.gameboy, self.render_state, self.gpu_context], {
            PLAYING.store(false, Ordering::Relaxed);
            {
                let mut gameboy = gameboy.lock();

                for _ in 0..ticks {
                    if let TickStatus::BreakpointHit = gameboy.tick(true) {
                        break;
                    };
                }
            }

            redraw_screen(gameboy, render_state, gpu_context);
        });
    }

    fn step_frame(&mut self, frames: usize) {
        using!([self.gameboy, self.render_state, self.gpu_context], {
            PLAYING.store(false, Ordering::Relaxed);
            'outer: {
                for _ in 0..frames {
                    loop {
                        let mut gameboy = gameboy.lock();
                        match gameboy.tick(true) {
                            TickStatus::DrawRequested => break,
                            TickStatus::BreakpointHit => break 'outer,
                            _ => {}
                        }
                    }

                    redraw_screen(gameboy.clone(), render_state.clone(), gpu_context.clone());
                }
            }
            redraw_screen(gameboy, render_state, gpu_context);
        })
    }
}

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = [
            Menu::new("File").items([
                MenuItem::action("Open ROM", actions::file::OpenRom),
                MenuItem::submenu(Menu::new("Recent").items(self.recent.read().iter().map(
                    |path| {
                        MenuItem::action(
                            path.to_string_lossy(),
                            actions::file::OpenRomPath(path.clone()),
                        )
                    },
                ))),
                MenuItem::separator(),
                MenuItem::action("Exit", actions::file::Exit),
            ]),
            Menu::new("Playback").items([
                MenuItem::action("Pause", actions::playback::TogglePause)
                    .checked(!PLAYING.load(Ordering::Relaxed)),
                MenuItem::action("Step Frame", actions::playback::StepFrame),
                MenuItem::action("Step Tick", actions::playback::StepTick),
            ]),
            Menu::new("Video").items([
                MenuItem::action("Toggle Fullscreen", actions::video::ToggleFullscreen)
                    .checked(window.is_fullscreen()),
                MenuItem::action("Show FPS", actions::video::ToggleShowFps)
                    .checked(cx.global::<GlobalState>().show_fps),
                MenuItem::action("Bilinear Filtering", actions::video::ToggleLinearFiltering)
                    .checked(cx.global::<GlobalState>().linear_filtering),
                MenuItem::separator(),
                MenuItem::submenu(
                    Menu::new("Video Scaling").items(
                        [
                            MenuItem::action(
                                "Force Integer Scaling",
                                actions::video::ToggleIntegerScaling,
                            )
                            .checked(cx.global::<GlobalState>().integer_scaling),
                            MenuItem::action(
                                "Resize to Fit Window",
                                actions::video::ToggleFixedSize,
                            )
                            .checked(!cx.global::<GlobalState>().fixed_size),
                            MenuItem::separator(),
                        ]
                        .into_iter()
                        .chain((1..=8).map(|scale_factor| {
                            MenuItem::action(
                                format!(
                                    "{scale_factor}x ({width}x{height})",
                                    width = 160 * scale_factor,
                                    height = 144 * scale_factor
                                ),
                                actions::video::ToggleScaleFactor(scale_factor),
                            )
                            .checked(cx.global::<GlobalState>().scale_factor == scale_factor)
                            .disabled(!cx.global::<GlobalState>().fixed_size)
                        })),
                    ),
                ),
            ]),
            Menu::new("Tools").items([
                MenuItem::action("Settings", actions::tools::Settings),
                MenuItem::separator(),
                MenuItem::action("Debugger", actions::tools::ToggleDebugger).checked(
                    cx.global::<WindowMap>()
                        .get(&WindowType::Debugger)
                        .is_some(),
                ),
            ]),
        ];

        {
            let global = cx.global::<GlobalState>();
            self.fast_forwarding.store(
                global.fast_forward_held || global.fast_forward_on,
                Ordering::Relaxed,
            );
        }

        let self_entity_id = cx.entity_id();

        cx.set_menus(menu);

        let menu_bar = self.menu_bar.clone();

        div()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .h_full()
            .w_full()
            .items_stretch()
            .when(!window.is_fullscreen(), |this| {
                this.child(
                    TitleBar::new(("titlebar", self_entity_id.clone()))
                        .flex()
                        .items_stretch()
                        .content_center()
                        .child(
                            div()
                                .flex_grow()
                                .flex()
                                .justify_center()
                                .items_center()
                                .child("gbemuu"),
                        ),
                )
            })
            .child(div().child(menu_bar).when(window.is_fullscreen(), |this| {
                this.opacity(0.0).hover(|style| style.opacity(1.0))
            }))
            .child(
                div()
                    .id(("screen", self_entity_id.clone()))
                    .flex()
                    .flex_grow()
                    .child(
                        Screen::new(self.render_state.clone(), self.frame_delta.clone())
                            .min_w(px(160.0))
                            .min_h(px(144.0))
                            .size_full(),
                    )
                    .on_click(|event, window, cx| {
                        if event.standard_click() && event.click_count() == 2 {
                            window.dispatch_action(Box::new(actions::video::ToggleFullscreen), cx);
                        }
                    }),
            )
            .on_key_down(using!(
                [],
                cx.listener(move |this, ev: &KeyDownEvent, _window, cx| {
                    if !ev.is_held {
                        let (bindings, _) = cx
                            .key_bindings()
                            .borrow()
                            .bindings_for_input(std::slice::from_ref(&ev.keystroke), &[]);

                        if let Some(binding) = bindings.first() {
                            let name = binding.action().name();

                            if name.starts_with("game") {
                                let button = {
                                    use gbemu_core::GameBoyButton::*;

                                    match name {
                                        "game::Up" => Up,
                                        "game::Down" => Down,
                                        "game::Right" => Right,
                                        "game::Left" => Left,
                                        "game::A" => A,
                                        "game::B" => B,
                                        "game::Select" => Select,
                                        "game::Start" => Start,
                                        _ => return,
                                    }
                                };

                                this.gameboy.lock().set_joypad_state(button, false);
                            } else if name.starts_with("playback") {
                                match name {
                                    "playback::FastForward" => {
                                        cx.global_mut::<GlobalState>().fast_forward_held = true;
                                    }
                                    _ => return,
                                }
                            }
                        }
                    }
                })
            ))
            .on_key_up(using!(
                [],
                cx.listener(move |this, ev: &KeyUpEvent, _window, cx| {
                    let (bindings, _) = cx
                        .key_bindings()
                        .borrow()
                        .bindings_for_input(std::slice::from_ref(&ev.keystroke), &[]);

                    if let Some(binding) = bindings.first() {
                        let name = binding.action().name();

                        if name.starts_with("game") {
                            let button = {
                                use gbemu_core::GameBoyButton::*;

                                match name {
                                    "game::Up" => Up,
                                    "game::Down" => Down,
                                    "game::Right" => Right,
                                    "game::Left" => Left,
                                    "game::A" => A,
                                    "game::B" => B,
                                    "game::Select" => Select,
                                    "game::Start" => Start,
                                    _ => return,
                                }
                            };

                            this.gameboy.lock().set_joypad_state(button, true);
                        } else if name.starts_with("playback") {
                            match name {
                                "playback::FastForward" => {
                                    cx.global_mut::<GlobalState>().fast_forward_held = false;
                                    this.audio_controller_sender
                                        .send(AudioControllerMessage::ClearBuffer)
                                        .unwrap();
                                }
                                _ => return,
                            }
                        }
                    }
                })
            ))
            .on_drop(|paths: &ExternalPaths, window, cx| {
                let paths = paths.paths();
                if let Some(path) = paths.first() {
                    window.dispatch_action(Box::new(actions::file::OpenRomPath(path.clone())), cx)
                }
            })
            .can_drop(|data, window, cx| {
                if data.is::<ExternalPaths>() {
                    return true;
                }
                false
            })
            .on_action::<actions::dev::ToggleInspector>(|_, window, cx| {
                #[cfg(debug_assertions)]
                window.toggle_inspector(cx);
            })
            .on_action::<actions::video::ToggleScaleFactor>(using!(
                [],
                move |action, window, cx| {
                    cx.global_mut::<GlobalState>().scale_factor = action.0;
                    cx.notify(self_entity_id);
                }
            ))
            .on_action::<actions::video::ToggleFullscreen>(|event, window, cx| {
                window.toggle_fullscreen();
            })
            .on_action::<actions::file::Exit>(|event, window, cx| {
                window.remove_window();
            })
            .on_action::<actions::playback::TogglePause>(|event, window, cx| {
                PLAYING.fetch_not(Ordering::Relaxed);
            })
            .on_action::<actions::video::ToggleFixedSize>(|event, window, cx| {
                cx.global_mut::<GlobalState>().tap_deref_mut(|global| {
                    global.fixed_size = !global.fixed_size;
                });
            })
            .on_action::<actions::video::ToggleIntegerScaling>(|event, window, cx| {
                cx.global_mut::<GlobalState>().tap_deref_mut(|global| {
                    global.integer_scaling = !global.integer_scaling;
                });
            })
            .on_action::<actions::video::ToggleLinearFiltering>(|event, window, cx| {
                cx.global_mut::<GlobalState>().tap_deref_mut(|global| {
                    global.linear_filtering = !global.linear_filtering;
                });
            })
            .on_action::<actions::video::ToggleShowFps>(|event, window, cx| {
                cx.global_mut::<GlobalState>().tap_deref_mut(|global| {
                    global.show_fps = !global.show_fps;
                });
            })
            .on_action::<actions::playback::StepFrame>(
                cx.listener(move |this, event, window, cx| this.step_frame(1)),
            )
            .on_action::<actions::playback::StepTick>(
                cx.listener(|this, event, window, cx| this.step_tick(1)),
            )
            .on_action::<actions::playback::ToggleFastForward>(cx.listener(
                |this, event, window, cx| {
                    cx.global_mut::<GlobalState>().tap_deref_mut(|global| {
                        global.fast_forward_on = !global.fast_forward_on;
                        this.audio_controller_sender
                            .send(AudioControllerMessage::ClearBuffer)
                            .unwrap();
                    });
                },
            ))
            .on_action::<actions::tools::ToggleDebugger>(cx.listener(|this, event, window, cx| {
                if let Some(window_handle) =
                    cx.global_mut::<WindowMap>().remove(&WindowType::Debugger)
                {
                    window_handle
                        .update(cx, |_, window, cx| window.remove_window())
                        .unwrap();
                } else {
                    let window_handle = Debugger::open(window, cx).unwrap().into();
                    cx.global_mut::<WindowMap>()
                        .insert(WindowType::Debugger, window_handle);
                }
            }))
            .on_action::<actions::tools::Settings>(cx.listener(|this, event, window, cx| {
                if let Some(window_handle) = cx.global_mut::<WindowMap>().get(&WindowType::Settings)
                {
                    window_handle
                        .update(cx, |_, window, cx| window.activate_window())
                        .unwrap();
                } else {
                    let window_handle = SettingsWindow::open(window, cx).unwrap().into();
                    cx.global_mut::<WindowMap>()
                        .insert(WindowType::Settings, window_handle);
                }
            }))
    }
}

fn load_rom(
    gameboy: &mut GameBoy,
    rom_path: impl AsRef<std::path::Path>,
    recent: Arc<RwLock<IndexSet<PathBuf>>>,
    recent_path: impl AsRef<std::path::Path>,
) {
    let rom_path = rom_path.as_ref();
    {
        let mut recent = recent.write();
        recent.shift_insert(0, rom_path.to_path_buf());
        recent.truncate(5);
    }

    let mut recent_writer = BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&recent_path)
            .expect("Couldn't open recent data file"),
    );
    recent_writer
        .write_all(&serde_json::to_vec(&recent.read().as_slice()).unwrap())
        .unwrap();
    recent_writer.flush().unwrap();
    let Ok(_) = gameboy.load_rom(rom_path) else {
        rfd::MessageDialog::new()
            .set_title("Failed to load rom")
            .set_buttons(MessageButtons::Ok)
            .set_level(MessageLevel::Error)
            .set_description("Couldn't load selected ROM. It may be missing or moved.")
            .show();

        return;
    };

    PLAYING.store(true, Ordering::Relaxed);
}

fn redraw_screen(
    gameboy: Arc<Mutex<GameBoy>>,
    render_state: Option<Arc<RenderState>>,
    gpu_context: Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)>,
) {
    let screen = *gameboy.lock().get_screen();
    rayon::spawn(move || {
        let palette = gbemu_core::Palette::default().conv::<Palette<f32>>();

        if let Some(render_state) = render_state
            && let Some((device, queue)) = gpu_context
        {
            queue.write_buffer(
                &render_state.palette_buffer,
                0,
                bytemuck::bytes_of(&palette),
            );

            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &render_state.screen_source,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&screen),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(160),
                    rows_per_image: Some(144),
                },
                wgpu::Extent3d {
                    width: 160,
                    height: 144,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([]);
            let mut command_encoder = device.create_command_encoder(&Default::default());
            {
                let mut render_pass = command_encoder.begin_render_pass(&RenderPassDescriptor {
                    label: "Screen Render Pass".into(),
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: &render_state.screen_texture_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });

                render_pass.set_pipeline(&render_state.pipeline);
                render_pass.set_bind_group(0, &render_state.bind_group, &[]);
                render_pass.set_vertex_buffer(0, render_state.vertex_buffer.slice(..));
                render_pass.draw(0..4, 0..1);
            }

            queue.submit(std::iter::once(command_encoder.finish()));
        }
    })
}

#[derive(Debug, Copy, Clone)]
pub enum AudioControllerMessage {
    ClearBuffer,
    ReallocBuffer(usize),
}

struct GBSignal {
    controller_receiver: crossbeam::channel::Receiver<AudioControllerMessage>,
    audio_receiver: crossbeam::channel::Receiver<Vec<[i16; 2]>>,
    buffer: ringbuf::LocalRb<Heap<[i16; 2]>>,
}

impl GBSignal {
    fn create(
        controller_receiver: crossbeam::channel::Receiver<AudioControllerMessage>,
        audio_receiver: crossbeam::channel::Receiver<Vec<[i16; 2]>>,
    ) -> impl Signal<Frame = [i16; 2]> {
        Self {
            controller_receiver,
            audio_receiver,
            buffer: ringbuf::LocalRb::new(8192),
        }
    }
}

impl Signal for GBSignal {
    type Frame = [i16; 2];

    fn next(&mut self) -> Self::Frame {
        if let Ok(message) = self.controller_receiver.try_recv() {
            match message {
                AudioControllerMessage::ClearBuffer => {
                    self.buffer.clear();
                }
                AudioControllerMessage::ReallocBuffer(capacity) => {
                    self.buffer = ringbuf::LocalRb::new(capacity)
                }
            }
        };
        if self.buffer.occupied_len() < self.buffer.capacity().get() / 2 {
            for buf in self.audio_receiver.try_iter() {
                self.buffer.push_slice(&buf);
            }
        }
        self.buffer.try_pop().unwrap_or_default()
    }
}

pub fn reload_keys(cx: &mut App) {
    cx.clear_key_bindings();

    cx.bind_keys(
        editable_text::actions::default_bindings().as_keybindings(Some(DEFAULT_INPUT_CONTEXT)),
    );

    cx.global::<Settings>().input.clone().set_keybinds(cx);
}
