// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![feature(uint_gather_scatter_bits)]
#![feature(hash_map_macro)]

use core::{
    mem,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize,
};

use crossbeam::channel;
use dasp::{
    interpolate::{linear::Linear, sinc::Sinc},
    ring_buffer::Fixed,
    signal::interpolate::Converter,
    Frame, Signal,
};
use etcetera::{AppStrategy, AppStrategyArgs};
use gbemu_core::{
    context::{InterruptRegister, Io, Memory},
    ppu::Pixel,
    GameBoy, GameBoyButton, PLAYING,
};
use indexmap::{IndexMap, IndexSet};
use itertools::Itertools;
use png_achunk::{Chunk, ChunkType};
use ringbuf::traits::Consumer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

use parking_lot::{Mutex, RwLock};
use rayon::prelude::*;
use slint::{
    language::KeyEvent,
    platform::Key,
    quit_event_loop,
    CloseRequestResponse::{self},
    ComponentHandle, Global, Image, Keys, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer,
    SharedString, ToSharedString, Weak,
};
use std::{
    collections::VecDeque,
    env,
    error::Error,
    io::Read,
    path::{Path, PathBuf},
    process::exit,
    rc::Rc,
    sync::{LazyLock, OnceLock},
    thread,
    time::Instant,
};
use std::{fs, sync::Arc};
use std::{
    hash_map,
    io::{BufWriter, Write},
};
use tap::{Conv, Pipe};
use uzi::using;

use slint_generated::*;

use crate::util::ShortcutsModel;

mod util;

#[derive(Default, Debug)]
struct WindowStatus {
    tile_viewer: AtomicBool,
    tilemap_viewer: AtomicBool,
}

static WINDOWS_ACTIVE: LazyLock<WindowStatus> = LazyLock::new(Default::default);

fn set_recent(ui: &mut AppWindow, recent: &IndexSet<PathBuf>) {
    ui.set_recent(ModelRc::from(
        recent
            .iter()
            .enumerate()
            .map(|(index, path)| RecentRom {
                index: index as i32,
                path: (*path.to_string_lossy()).into(),
            })
            .collect::<Vec<_>>()
            .as_slice(),
    ));
}

fn get_keys_from_keyevent(event: KeyEvent) -> Keys {
    println!("{:?}", event.text);
    let mut keys = vec![];
    if event.modifiers.control {
        keys.push("Control");
    }
    if event.modifiers.alt {
        keys.push("Alt");
    }
    if event.modifiers.shift {
        keys.push("Shift");
    }
    let key = match event.text.as_str() {
        "\t" => "Tab",
        "\n" => "Return",
        " " => "Space",
        key => key,
    };
    keys.push(key);
    Keys::from_parts(keys)
        .inspect_err(|e| println!("{}", e))
        .unwrap_or_default()
}

fn main() -> Result<(), Box<dyn Error>> {
    // let (flame_layer, _guard) = tracing_flame::FlameLayer::with_file("./tracing.folded").unwrap();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(EnvFilter::from_default_env()))
        // .with(tracing_samply::SamplyLayer::new()?)
        // .with(flame_layer)
        .init();
    // tracing_log::LogTracer::init()?;

    #[cfg(feature = "deadlock_detection")]
    {
        // only for #[cfg]
        use parking_lot::deadlock;
        use std::thread;
        use std::time::Duration;

        // Create a background thread which checks for deadlocks every 10s
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(10));
            let deadlocks = deadlock::check_deadlock();
            if deadlocks.is_empty() {
                continue;
            }

            println!("{} deadlocks detected", deadlocks.len());
            for (i, threads) in deadlocks.iter().enumerate() {
                println!("Deadlock #{}", i);
                for t in threads {
                    println!("Thread Id {:#?}", t.thread_id());
                    println!("{:#?}", t.backtrace());
                }
            }
        });
    } // only for #[cfg]

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

    let stream = device
        .build_output_stream(
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
        )
        .unwrap();
    stream.play().unwrap();

    let strategy = etcetera::app_strategy::choose_native_strategy(AppStrategyArgs {
        top_level_domain: "uk".into(),
        author: "fuzzle".into(),
        app_name: "gbemu".into(),
    })
    .unwrap();

    let recent_path = strategy.in_data_dir("recent.json");

    fs::create_dir_all(strategy.data_dir()).unwrap();

    let recent = Arc::new(RwLock::<IndexSet<_>>::new(
        if let Ok(bytes) = fs::read(recent_path.clone()) {
            serde_json::from_slice::<IndexSet<PathBuf>>(&bytes)
                .unwrap_or_else(|_| IndexSet::with_capacity(5))
        } else {
            IndexSet::with_capacity(5)
        },
    ));

    let mut ui = AppWindow::new()?;

    set_recent(&mut ui, &recent.read());

    let ui_handle = ui.as_weak();

    let tile_viewer = TileViewer::new()?;

    let tilemap_viewer = TileMapViewer::new()?;

    let settings_window = SettingsWindow::new()?;

    let keyboard_shortcuts = ModelRc::new(ShortcutsModel::from(IndexMap::from([
        (
            "load_rom".to_string(),
            ShortcutEntry {
                name: "Open ROM".to_shared_string(),
                shortcut: Keys::from_parts(["Control", "O"]).unwrap(),
            },
        ),
        (
            "pause".to_string(),
            ShortcutEntry {
                name: "Pause".to_shared_string(),
                shortcut: Keys::from_parts(["Control", "P"]).unwrap(),
            },
        ),
        (
            "step_tick".to_string(),
            ShortcutEntry {
                name: "Step Tick".to_shared_string(),
                shortcut: Keys::from_parts(["Control", "T"]).unwrap(),
            },
        ),
        (
            "step_frame".to_string(),
            ShortcutEntry {
                name: "Step Frame".to_shared_string(),
                shortcut: Keys::from_parts(["Control", "S"]).unwrap(),
            },
        ),
        (
            "fullscreen".to_string(),
            ShortcutEntry {
                name: "Fullscreen".to_shared_string(),
                shortcut: Keys::from_parts(["Alt", "Return"]).unwrap(),
            },
        ),
    ])));

    fn setup_key_helpers<'a, T: ComponentHandle>(
        keyboard_shortcuts: ModelRc<(ShortcutEntry, SharedString)>,
        window: &'a T,
    ) where
        KeyHelpers<'a>: Global<'a, T>,
    {
        let key_helpers = window.global::<KeyHelpers>();
        key_helpers.on_get_keys_from_keyevent(get_keys_from_keyevent);
        key_helpers.on_get_keyboard_shortcut(using!([keyboard_shortcuts], move |name| {
            let shortcuts = keyboard_shortcuts
                .as_any()
                .downcast_ref::<ShortcutsModel>()
                .expect("Should be a shortcuts model");
            shortcuts.get(&name.to_string()).unwrap_or_default()
        }));
        key_helpers.set_keyboard_shortcuts(keyboard_shortcuts.clone());
    }

    setup_key_helpers(keyboard_shortcuts.clone(), &ui);
    setup_key_helpers(keyboard_shortcuts.clone(), &settings_window);

    ui.on_show_settings(using!([settings_window.as_weak()], move || {
        settings_window.upgrade().unwrap().show().unwrap();
    }));

    tile_viewer
        .window()
        .on_close_requested(using!([ui_handle], move || {
            ui_handle.upgrade().unwrap().set_tile_viewer_shown(false);
            CloseRequestResponse::HideWindow
        }));

    tilemap_viewer
        .window()
        .on_close_requested(using!([ui_handle], move || {
            ui_handle.upgrade().unwrap().set_tilemap_viewer_shown(false);
            CloseRequestResponse::HideWindow
        }));

    ui.window().on_close_requested(using!([gameboy], move || {
        gameboy
            .lock()
            .context
            .save()
            .unwrap_or_else(|e| tracing::error!("{}", e));

        quit_event_loop().unwrap();
        CloseRequestResponse::KeepWindowShown
    }));

    ui.on_toggle_tile_viewer(using!([ui_handle, tile_viewer.as_weak()], move || {
        let tile_viewer_shown = ui_handle.upgrade().unwrap().get_tile_viewer_shown();
        let tile_viewer = tile_viewer.upgrade().unwrap();
        WINDOWS_ACTIVE
            .tile_viewer
            .store(tile_viewer_shown, Ordering::Relaxed);
        if tile_viewer_shown {
            tile_viewer.show().unwrap();
        } else {
            tile_viewer.hide().unwrap();
        }
    }));

    ui.on_toggle_tilemap_viewer(using!([ui_handle, tilemap_viewer.as_weak()], move || {
        let tilemap_viewer_shown = ui_handle.upgrade().unwrap().get_tilemap_viewer_shown();
        let tilemap_viewer = tilemap_viewer.upgrade().unwrap();
        WINDOWS_ACTIVE
            .tilemap_viewer
            .store(tilemap_viewer_shown, Ordering::Relaxed);
        if tilemap_viewer_shown {
            tilemap_viewer.show().unwrap();
        } else {
            tilemap_viewer.hide().unwrap();
        }
    }));

    ui.on_embed_rom(using!([ui_handle], move || {
        let Some(source_image_path) = rfd::FileDialog::new()
            .add_filter("Images (.png)", &["png"])
            .pick_file()
        else {
            return;
        };

        let (image, chunks) = png_achunk::Decoder::from_file(source_image_path)
            .map(|mut decoder| decoder.decode_all().expect("Couldn't load image"))
            .expect("Couldn't load image");

        let Some(source_rom_path) = rfd::FileDialog::new()
            .add_filter("Game Boy ROMs (.gb/.gbc)", &["gb", "gbc"])
            .set_title("Select source ROM")
            .pick_file()
        else {
            return;
        };
        let rom = fs::read(source_rom_path).expect("Couldn't load ROM");

        let Some(save_path) = rfd::FileDialog::new()
            .add_filter("Destination image (.png)", &["png"])
            .save_file()
        else {
            return;
        };

        let mut encoder = png_achunk::Encoder::new_to_file(save_path)
            .expect("Couldn't create new file")
            .with_custom_chunk(Chunk::new(ChunkType::from_ascii(&"gbRM").unwrap(), rom).unwrap());

        for chunk in chunks {
            encoder = encoder.with_custom_chunk(chunk);
        }

        image
            .write_with_encoder(encoder)
            .expect("Couldn't write image file");
    }));

    let (redraw_sender, redraw_receiver) = crossbeam::channel::bounded::<()>(1);

    thread::spawn(using!(
        [
            gameboy,
            ui.as_weak(),
            tile_viewer.as_weak(),
            tilemap_viewer.as_weak()
        ],
        move || {
            loop {
                if redraw_receiver.recv().is_ok() {
                    update_viewers(&gameboy, &tile_viewer, &tilemap_viewer);
                }
            }
        }
    ));

    let mut image_buffer = SharedPixelBuffer::new(160, 144);
    let mut local_buffer = SharedPixelBuffer::new(160, 144);

    thread::spawn(using!(
        [
            redraw_sender,
            ui_handle,
            gameboy,
            tile_viewer.as_weak(),
            tilemap_viewer.as_weak()
        ],
        move || {
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
                            let buffer = local_buffer.make_mut_slice();
                            gameboy
                                .get_screen()
                                .par_iter()
                                .map(|pixel| palette[pixel].conv::<[u8; 4]>())
                                .zip(buffer.par_iter_mut())
                                .for_each(|(pixel, buffer_pixel)| {
                                    *buffer_pixel = pixel.into();
                                });

                            mem::swap(&mut local_buffer, &mut image_buffer);

                            ui_handle
                                .upgrade_in_event_loop(using!([image_buffer], move |handle| {
                                    handle.set_screen(Image::from_rgba8(image_buffer));
                                    handle.window().request_redraw();
                                }))
                                .unwrap();
                            redraw_sender.send(()).unwrap();

                            // rate_sender.send(delta_time * 59.73).unwrap();
                            ui_handle
                                .upgrade_in_event_loop(move |handle| {
                                    handle.set_fps(1.0 / delta_time);
                                })
                                .unwrap();
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
        }
    ));

    ui.on_load_rom(using!(
        [gameboy, ui.as_weak(), recent, recent_path],
        move || {
            let Some(rom_path) = rfd::FileDialog::new()
                .add_filter("GameBoy ROMs (.gb/.gbc)", &["gb"])
                .add_filter(
                    "Archives (.zip/.tar.gz/.rar/etc.)",
                    &["zip", "gz", "tar", "rar", "zst", "xz"],
                )
                .add_filter("ROM Embedded Images (.png)", &["png"])
                .set_directory(
                    env::current_dir()
                        .unwrap_or_else(|_| env::home_dir().unwrap_or_else(|| PathBuf::from("/"))),
                )
                .pick_file()
            else {
                return;
            };

            let gameboy = &mut *gameboy.lock();
            {
                let mut recent = recent.write();
                recent.shift_insert(0, rom_path.clone());
                recent.truncate(5);
            }

            let mut recent_writer = BufWriter::new(
                fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(recent_path.clone())
                    .expect("Couldn't open recent data file"),
            );
            recent_writer
                .write_all(&serde_json::to_vec(&recent.read().as_slice()).unwrap())
                .unwrap();
            recent_writer.flush().unwrap();
            gameboy.load_rom(rom_path);

            ui.upgrade_in_event_loop(using!([recent], move |mut handle| {
                handle.set_paused(false);
                PLAYING.store(true, Ordering::Relaxed);
                set_recent(&mut handle, &recent.read());
                handle.invoke_focus();
            }))
            .unwrap();
        }
    ));

    ui.on_load_recent(using!(
        [gameboy, ui.as_weak(), recent, recent_path],
        move |index| {
            let gameboy = &mut *gameboy.lock();

            let rom_path = recent.read()[index as usize].clone();
            {
                let mut recent = recent.write();
                recent.shift_insert(0, rom_path.clone());
                recent.truncate(5);
            }

            let mut recent_writer = BufWriter::new(
                fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(recent_path.clone())
                    .expect("Couldn't open recent data file"),
            );
            recent_writer
                .write_all(&serde_json::to_vec(&recent.read().as_slice()).unwrap())
                .unwrap();
            recent_writer.flush().unwrap();

            gameboy.load_rom(rom_path);

            ui.upgrade_in_event_loop(using!([recent], move |mut handle| {
                handle.set_paused(false);
                PLAYING.store(true, Ordering::Relaxed);
                set_recent(&mut handle, &recent.read());
                handle.invoke_focus();
            }))
            .unwrap();
        }
    ));

    ui.on_dropped(using!(
        [gameboy, ui.as_weak(), recent, recent_path,],
        move |drop_event| {
            let file_url = drop_event.data.plain_text().unwrap();

            let stripped = file_url.strip_prefix("file://").unwrap();

            let rom_path = Path::new(&stripped);

            let gameboy = &mut *gameboy.lock();

            let mut recent_writer = BufWriter::new(
                fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(recent_path.clone())
                    .expect("Couldn't open recent data file"),
            );
            recent_writer
                .write_all(&serde_json::to_vec(&recent.read().as_slice()).unwrap())
                .unwrap();
            recent_writer.flush().unwrap();

            gameboy.load_rom(rom_path);

            ui.upgrade_in_event_loop(using!([recent], move |mut handle| {
                handle.set_paused(false);
                PLAYING.store(true, Ordering::Relaxed);
                set_recent(&mut handle, &recent.read());
                handle.invoke_focus();
            }))
            .unwrap();

            drop_event.proposed_action
        }
    ));

    ui.on_toggle_playback(using!([ui.as_weak()], move || {
        let ui = ui.upgrade().unwrap();
        PLAYING.store(!ui.get_paused(), Ordering::Relaxed);
    }));

    ui.on_step_tick(using!(
        [
            gameboy,
            ui.as_weak(),
            tile_viewer.as_weak(),
            tilemap_viewer.as_weak(),
        ],
        move || {
            let ui = ui.upgrade().unwrap();
            PLAYING.store(false, Ordering::Relaxed);
            ui.set_paused(true);

            {
                let mut gameboy = gameboy.lock();
                gameboy.tick(true);
            }

            update_viewers(&gameboy, &tile_viewer, &tilemap_viewer);
        }
    ));

    ui.on_clear_scale_selections(using!([ui.as_weak()], move |except_scale| {
        use slint::Model;
        let ui = ui.upgrade().unwrap();
        let scales = ui.get_scales();
        for (id, mut scale) in scales.iter().enumerate() {
            if scale.1 != except_scale {
                scale.0 = false;
                scales.set_row_data(id, scale);
            }
        }
    }));

    ui.on_step_frame(using!(
        [
            gameboy,
            ui.as_weak(),
            tile_viewer.as_weak(),
            tilemap_viewer.as_weak(),
        ],
        move || {
            let ui = ui.upgrade().unwrap();
            PLAYING.store(true, Ordering::Relaxed);
            ui.set_paused(true);

            {
                let mut gameboy = gameboy.lock();
                while !gameboy.tick(false) {}
            }
            PLAYING.store(false, Ordering::Relaxed);

            update_viewers(&gameboy, &tile_viewer, &tilemap_viewer);
        }
    ));

    let keybinds = Rc::new(hash_map! {
        "w" => GameBoyButton::Up,
        "s" => GameBoyButton::Down,
        "a" => GameBoyButton::Left,
        "d" => GameBoyButton::Right,
        "j" => GameBoyButton::A,
        "k" => GameBoyButton::B,
        "\u{8}" => GameBoyButton::Select,
        "\n" => GameBoyButton::Start,
    });

    ui.on_key_event(using!([gameboy, keybinds], move |keyboard_event| {
        let KeyboardEvent {
            event,
            r#type: event_type,
        } = keyboard_event;

        if let Some(&button) = keybinds.get(&*event.text) {
            using!([mut gameboy.lock()], {
                gameboy.set_joypad_state(
                    button,
                    match event_type {
                        KeyEventType::Press => false,
                        KeyEventType::Release => true,
                    },
                );
            });
        }
    }));

    ui.run()?;

    exit(0);

    Ok(())
}

fn update_viewers(
    gameboy: &Arc<Mutex<GameBoy>>,
    tile_viewer_handle: &Weak<TileViewer>,
    tilemap_viewer_handle: &Weak<TileMapViewer>,
) {
    let gameboy = gameboy.lock();
    let palette = gameboy.palette;
    let bgp = gameboy.context.memory.io.lcd.bgp;
    let mapping = gameboy.context.memory.io.lcd.lcdc.tile_data_mapping();
    let (scroll_x, scroll_y) = gameboy
        .context
        .memory
        .io
        .lcd
        .pipe_ref(|lcd| (lcd.scx, lcd.scy));
    let tile_data = gameboy.context.memory.vram.tile_data().to_vec();

    if WINDOWS_ACTIVE.tilemap_viewer.load(Ordering::Relaxed) {
        let tile_maps = [
            gameboy.context.memory.io.lcd.lcdc.bg_tile_map(),
            gameboy.context.memory.io.lcd.lcdc.window_tile_map(),
        ]
        .map(|map_area| {
            gameboy
                .context
                .memory
                .vram
                .tile_map(map_area)
                .iter()
                .copied()
                .map(|tile_id| *gameboy.context.memory.vram.bg_tile_data(mapping, tile_id))
                .collect_vec()
        });

        drop(gameboy);

        let tile_maps = tile_maps.map(|map| {
            map.iter()
                .map(|tile| {
                    SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                        &tile
                            .pipe(|data| data.as_chunks::<2>().0)
                            .iter()
                            .copied()
                            .flat_map(|[left, right]| {
                                let row = ((left as u16) << 8) | right as u16;
                                (0..8)
                                    .map(move |index| {
                                        row.extract_bits(0b1000_0000_1000_0000 >> index)
                                    })
                                    .map(|colour| bgp >> (colour * 2) & 0b11)
                                    .flat_map(|colour| {
                                        palette[&Pixel::from_repr(colour).unwrap()]
                                            .conv::<[u8; 4]>()
                                    })
                            })
                            .collect::<Vec<_>>(),
                        8,
                        8,
                    )
                })
                .collect_vec()
        });

        tilemap_viewer_handle
            .upgrade_in_event_loop(move |handle| {
                let tiles = match handle.get_selected_map() {
                    TileMap::Background => &tile_maps[0],
                    TileMap::Window => &tile_maps[1],
                };

                let model = ModelRc::from(
                    tiles
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(index, buffer)| Tile {
                            index: index as i32,
                            buffer: Image::from_rgba8(buffer),
                        })
                        .collect::<Vec<_>>()
                        .as_slice(),
                );
                handle.set_tiles(model);
                handle.set_scroll(if handle.get_selected_map() == TileMap::Background {
                    (scroll_x as f32, scroll_y as f32)
                } else {
                    (0.0, 0.0)
                });
            })
            .unwrap();
    }

    if WINDOWS_ACTIVE.tile_viewer.load(Ordering::Relaxed) {
        let tiles: Vec<_> = tile_data
            .pipe_ref(|data| data.as_chunks::<16>().0)
            .par_iter()
            .map(|x| {
                SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                    &x.pipe(|data| data.as_chunks::<2>().0)
                        .iter()
                        .copied()
                        .flat_map(|[left, right]| {
                            let row = ((left as u16) << 8) | right as u16;
                            (0..8)
                                .map(move |index| row.extract_bits(0b1000_0000_1000_0000 >> index))
                                .map(|colour| bgp >> (colour * 2) & 0b11)
                                .flat_map(|colour| {
                                    palette[&Pixel::from_repr(colour).unwrap()].conv::<[u8; 4]>()
                                })
                        })
                        .collect::<Vec<_>>(),
                    8,
                    8,
                )
            })
            .collect();

        tile_viewer_handle
            .upgrade_in_event_loop(move |handle| {
                let model = ModelRc::from(
                    tiles
                        .into_iter()
                        .enumerate()
                        .map(|(index, buffer)| Tile {
                            index: index as i32,
                            buffer: Image::from_rgba8(buffer),
                        })
                        .collect::<Vec<_>>()
                        .as_slice(),
                );
                handle.set_tiles(model);
            })
            .unwrap();
    }
}
