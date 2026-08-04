use core::{mem, sync::atomic::Ordering, time::Duration};
use std::{
    collections::VecDeque,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::Instant,
};

use anyrender::{Paint, PaintRef, PaintScene as _, RenderContext, ResourceId, Scene};
use blitz_dom::node::ComputedStyles;
use blitz_traits::{events::UiEvent, shell::ShellProvider};
use cpal::{
    BufferSize,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use dioxus_native::{
    CustomWidgetAttr, DeviceHandle, Features, Limits, Widget, WindowAttributes, prelude::*,
};
use etcetera::{AppStrategy, AppStrategyArgs};
use gbemu_core::{GameBoy, PLAYING};
use indexmap::IndexSet;
use kurbo::Vec2;
use parking_lot::{Mutex, RwLock};
use peniko::{
    Color, Fill, ImageBrush, ImageQuality,
    kurbo::{Affine, Rect},
};
use rayon::iter::{
    IndexedParallelIterator, IntoParallelRefIterator, IntoParallelRefMutIterator, ParallelIterator,
};
use rfd::{MessageButtons, MessageLevel};
use tap::Conv;
use uzi::using;
use wgpu::Texture;
use winit_wayland::WindowAttributesWayland;

use ringbuf::traits::Consumer;

use dasp::Signal;

use crate::components::{root::Root, titlebar::TitleBar};

static MAIN_CSS: Asset = asset!("/assets/main.css");
static TAILWIND: Asset = asset!("/assets/tailwind.css");

mod components;

fn main() -> anyhow::Result<()> {
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

    dbg!(supported_config);
    match supported_config.buffer_size() {
        cpal::SupportedBufferSize::Range { min: _, max: _ } => {
            stream_config.buffer_size = BufferSize::Fixed(1024);
        }
        cpal::SupportedBufferSize::Unknown => todo!(),
    }
    stream_config.sample_rate = 48_000;

    struct GBSignal {
        receiver: crossbeam::channel::Receiver<VecDeque<[i16; 2]>>,
        buffer: VecDeque<[i16; 2]>,
    }

    impl GBSignal {
        fn create(
            receiver: crossbeam::channel::Receiver<VecDeque<[i16; 2]>>,
        ) -> impl Signal<Frame = [i16; 2]> {
            Self {
                receiver,
                buffer: VecDeque::from([[0, 0]; 512]),
            }
        }
    }

    impl Signal for GBSignal {
        type Frame = [i16; 2];

        fn next(&mut self) -> Self::Frame {
            if self.buffer.len() < 4096 {
                for buf in self.receiver.try_iter() {
                    self.buffer.extend(buf);
                }
            }
            self.buffer.pop_front().unwrap_or_default()
        }
    }

    let mut signal = GBSignal::create(gameboy.lock().apu.output_channel.1.clone());

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

    let strategy = etcetera::app_strategy::choose_native_strategy(AppStrategyArgs {
        top_level_domain: "uk".into(),
        author: "fuzzle".into(),
        app_name: "gbemu".into(),
    })?;

    let recent_path = strategy.in_data_dir("recent.json");

    fs::create_dir_all(strategy.data_dir())?;

    let recent = Arc::new(RwLock::<IndexSet<_>>::new(
        if let Ok(bytes) = fs::read(recent_path.clone()) {
            serde_json::from_slice::<IndexSet<PathBuf>>(&bytes)
                .unwrap_or_else(|_| IndexSet::with_capacity(5))
        } else {
            IndexSet::with_capacity(5)
        },
    ));

    dioxus_native::launch_cfg(
        App,
        vec![
            Box::new(move || Box::new(strategy.clone())),
            Box::new(move || Box::new(gameboy.clone())),
        ],
        vec![
            Box::new(Features::IMMEDIATES),
            Box::new(Limits::default()),
            Box::new(
                WindowAttributes::default()
                    .with_decorations(false)
                    .with_transparent(true)
                    .with_title("gbemu")
                    .with_platform_attributes(Box::new(
                        WindowAttributesWayland::default().with_prefer_csd(true),
                    )),
            ),
        ],
    );

    Ok(())
}

struct ScreenWidget {
    buffer: Arc<Mutex<Arc<Vec<u8>>>>,
    resource_id: Option<ResourceId>,
    device_handle: Option<Box<DeviceHandle>>,
    texture: Option<Texture>,
}

impl ScreenWidget {
    fn new() -> Self {
        let buffer = Arc::new(Mutex::new(Arc::new(vec![0; 160 * 144 * 4])));
        Self {
            buffer,
            device_handle: None,
            texture: None,
            resource_id: None,
        }
    }
}

impl Widget for ScreenWidget {
    fn connected(&mut self) {}

    fn disconnected(&mut self) {}

    fn attribute_changed(&mut self, name: &str, old_value: Option<&str>, new_value: Option<&str>) {
        let _ = (name, old_value, new_value);
    }

    fn can_create_surfaces(&mut self, render_ctx: &mut dyn RenderContext) {
        if let Some(renderer_specific_context) = render_ctx.renderer_specific_context() {
            match renderer_specific_context.downcast::<dioxus_native::DeviceHandle>() {
                Ok(device_handle) => {
                    let texture_size = wgpu::Extent3d {
                        width: 160,
                        height: 144,
                        depth_or_array_layers: 1,
                    };

                    let texture = device_handle
                        .device
                        .create_texture(&wgpu::TextureDescriptor {
                            size: texture_size,
                            mip_level_count: 1, // We'll talk about this a little later
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            // Most images are stored using sRGB, so we need to reflect that here.
                            format: wgpu::TextureFormat::Rgba8UnormSrgb,
                            // TEXTURE_BINDING tells wgpu that we want to use this texture in shaders
                            // COPY_DST means that we want to copy data to this texture
                            usage: wgpu::TextureUsages::TEXTURE_BINDING
                                | wgpu::TextureUsages::COPY_DST,
                            label: Some("screen texture"),
                            // This is the same as with the SurfaceConfig. It
                            // specifies what texture formats can be used to
                            // create TextureViews for this texture. The base
                            // texture format (Rgba8UnormSrgb in this case) is
                            // always supported. Note that using a different
                            // texture format is not supported on the WebGL2
                            // backend.
                            view_formats: &[],
                        });

                    if let Ok(resource_id) =
                        render_ctx.try_register_custom_resource(Box::new(texture.clone()))
                    {
                        self.resource_id = Some(resource_id);
                        self.texture = Some(texture);
                        self.device_handle = Some(device_handle);
                    } else {
                        panic!("Couldn't register resource");
                    }
                }
                Err(val) => {
                    panic!(
                        "Couldn't get device handle {:?} {:?}",
                        val.type_id(),
                        std::any::type_name_of_val(&val)
                    );
                }
            }
        } else {
            panic!("Couldn't get renderer specific context");
        }
    }

    fn destroy_surfaces(&mut self) {}

    fn handle_event(&mut self, event: &UiEvent) {
        let _ = event;
    }

    fn paint(
        &mut self,
        render_ctx: &mut dyn RenderContext,
        styles: &ComputedStyles,
        width: u32,
        height: u32,
        scale: f64,
    ) -> Scene {
        let _ = (render_ctx, styles, width, height, scale);

        let scale_factor = (height / 144).min(width / 160);

        let screen_width = 160 * scale_factor;
        let screen_height = 144 * scale_factor;
        let mut scene = Scene::new();

        scene.fill(
            Fill::NonZero,
            Affine::scale(scale_factor as f64).then_translate(Vec2::new(
                (width / 2 - screen_width / 2) as f64,
                (height / 2 - screen_height / 2) as f64,
            )),
            Color::BLACK,
            None,
            &Rect::from_origin_size((0.0, 0.0), (160 as f64, 144 as f64)),
        );

        if let Some(device_handle) = &self.device_handle
            && let Some(texture) = &self.texture
        {
            device_handle.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &self.buffer.lock(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * 160),
                    rows_per_image: Some(144),
                },
                wgpu::Extent3d {
                    width: 160,
                    height: 144,
                    depth_or_array_layers: 1,
                },
            );
            scene.fill(
                Fill::NonZero,
                Affine::scale(scale_factor as f64).then_translate(Vec2::new(
                    (width / 2 - screen_width / 2) as f64,
                    (height / 2 - screen_height / 2) as f64,
                )),
                Paint::Resource(
                    ImageBrush {
                        image: self.resource_id.unwrap(),
                        sampler: Default::default(),
                    }
                    .with_quality(ImageQuality::Low),
                ),
                None,
                &Rect::from_origin_size((0.0, 0.0), (160 as f64, 144 as f64)),
            );
        } else {
            println!("Device handle or texture is None");
        }

        scene
    }
}

static EXIT_REQUESTED: GlobalSignal<bool> = dioxus_native::prelude::Signal::global(|| false);

#[component]
fn App() -> Element {
    let mut keyboard_modifiers = use_signal(|| winit_core::event::Modifiers::default());
    let gameboy = use_context::<Arc<Mutex<GameBoy>>>();

    dioxus_native::use_window_event(using!([gameboy], move |event, event_loop| {
        if EXIT_REQUESTED() {
            event_loop.exit();
        };
        match event {
            winit_core::event::WindowEvent::KeyboardInput {
                device_id,
                event,
                is_synthetic,
            } => {
                if !event.repeat {
                    if event.key_without_modifiers == "o"
                        && keyboard_modifiers().state().control_key()
                        && event.state.is_pressed()
                    {
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

                        load_rom(&mut gameboy.lock(), rom_path);
                    }
                }
            }
            winit_core::event::WindowEvent::ModifiersChanged(modifiers) => {
                keyboard_modifiers.set(*modifiers);
            }
            _ => {}
        }
    }));

    let mut _thread = use_signal(|| None);

    let screen_widget = use_memo(move || {
        let widget = ScreenWidget::new();
        let screen_buffer = widget.buffer.clone();

        let mut local_buffer = Arc::new(vec![0u8; 160 * 144 * 4]);
        let gameboy = gameboy.clone();
        _thread.set(Some(thread::spawn(move || {
            let mut prev_frame_time = Instant::now();
            loop {
                if PLAYING.load(Ordering::Relaxed) {
                    let target_time = Instant::now() + Duration::from_secs_f64(1.0 / 59.73);
                    loop {
                        let mut gameboy = gameboy.lock();
                        let redraw_request = gameboy.tick(false);
                        if redraw_request {
                            let delta_time =
                                prev_frame_time.elapsed().as_secs_f32().max(1.0 / 59.73);

                            let palette = &gameboy.palette;

                            {
                                let local_buffer = Arc::make_mut(&mut local_buffer);
                                gameboy
                                    .get_screen()
                                    .par_iter()
                                    .map(|pixel| palette[pixel].conv::<[u8; 4]>())
                                    .zip(local_buffer.as_chunks_mut::<4>().0.par_iter_mut())
                                    .for_each(|(pixel, buffer_pixel)| {
                                        *buffer_pixel = pixel.into();
                                    });
                            }
                            mem::swap(&mut local_buffer, &mut screen_buffer.lock());

                            // ui.upgrade_in_event_loop(using!([image_buffer], move |handle| {
                            //     handle.set_screen(Image::from_rgba8(image_buffer));
                            //     handle.window().request_redraw();
                            //     handle.set_fps(1.0 / delta_time);
                            // }))
                            // .unwrap();
                            // redraw_sender.send(()).unwrap();
                            prev_frame_time = Instant::now();
                            break;
                        }
                        for byte in gameboy.context.memory.io.serial.output.pop_iter() {
                            println!("0x{byte:02X}");
                        }
                    }

                    spin_sleep::SpinSleeper::default()
                        .with_spin_strategy(spin_sleep::SpinStrategy::SpinLoopHint)
                        .sleep_until(target_time);
                }
            }
        })));

        CustomWidgetAttr::new(widget)
    });

    rsx! {
        document::Stylesheet { href: TAILWIND }

        document::Stylesheet { href: MAIN_CSS }

        Root {
            TitleBar {}
            div { class: "grow bg-gray-500",
                div { class: "size-full grid", id: "canvas-container",
                    object { class: "", "data": screen_widget }
                }
            }
        }

    }
}

fn load_rom(
    gameboy: &mut GameBoy,
    rom_path: impl AsRef<Path>,
    // ui: &Weak<AppWindow>,
    // recent: Arc<RwLock<IndexSet<PathBuf>>>,
    // recent_path: impl AsRef<Path>,
) {
    let rom_path = rom_path.as_ref();
    // {
    //     let mut recent = recent.write();
    //     recent.shift_insert(0, rom_path.to_path_buf());
    //     recent.truncate(5);
    // }

    // let mut recent_writer = BufWriter::new(
    //     fs::OpenOptions::new()
    //         .write(true)
    //         .create(true)
    //         .truncate(true)
    //         .open(&recent_path)
    //         .expect("Couldn't open recent data file"),
    // );
    // recent_writer
    //     .write_all(&serde_json::to_vec(&recent.read().as_slice()).unwrap())
    //     .unwrap();
    // recent_writer.flush().unwrap();

    let Ok(_) = gameboy.load_rom(rom_path) else {
        rfd::MessageDialog::new()
            .set_title("Failed to load rom")
            .set_buttons(MessageButtons::Ok)
            .set_level(MessageLevel::Error)
            .set_description("Couldn't load selected ROM. It may be missing or moved.")
            .show();

        return;
    };

    // handle.set_rom_loaded(true);
    // handle.set_paused(false);
    PLAYING.store(true, Ordering::Relaxed);
    // set_recent(&mut handle, &recent.read());
    // handle.invoke_focus();
}

mod utils;
