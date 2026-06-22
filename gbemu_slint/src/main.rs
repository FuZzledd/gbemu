// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![feature(uint_gather_scatter_bits)]
#![feature(hash_map_macro)]

use core::{array, time::Duration};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize,
};
use dasp::{Frame, Signal};
use etcetera::{AppStrategy, AppStrategyArgs};
use gbemu_core::{
    context::{InterruptRegister, Io, Memory},
    ppu::Pixel,
    GameBoy, GameBoyButton,
};
use indexmap::IndexSet;
use itertools::Itertools;
use ringbuf::traits::Consumer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

use parking_lot::{Mutex, RwLock};
use slint::{
    quit_event_loop,
    CloseRequestResponse::{self},
    Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, Weak,
};
use std::{
    collections::VecDeque,
    env,
    error::Error,
    io::Read,
    path::PathBuf,
    process::exit,
    rc::Rc,
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
        receiver: crossbeam::channel::Receiver<[f64; 8]>,
        blip_bufs: [blip_buf::BlipBuf; 2],
        clocks: u32,
        prev_frame: [f64; 2],
        buffer: VecDeque<[f64; 2]>,
    }

    impl GBSignal {
        fn create(
            receiver: crossbeam::channel::Receiver<[f64; 8]>,
        ) -> impl Signal<Frame = [f64; 2]> {
            

            Self {
                receiver,
                blip_bufs: array::from_fn(|_i| {
                    let mut buf = blip_buf::BlipBuf::new(48000 / 5);
                    buf.set_rates(48000f64, 48_000f64);
                    buf
                }),
                clocks: 0,
                prev_frame: [0.0; 2],
                buffer: VecDeque::from([[0.0, 0.0]; 512]),
            }
        }
    }

    impl Signal for GBSignal {
        type Frame = [f64; 2];

        fn next(&mut self) -> Self::Frame {
            
            self
                .receiver
                .recv()
                .map(|frame| {
                    [
                        (frame[0] + frame[2] + 0.0 + frame[6]) / 4.0,
                        (frame[1] + frame[3] + 0.0 + frame[7]) / 4.0,
                    ]
                })
                .unwrap()
            // while self.buffer.len() < 512 {
            //     for frame in self.receiver.iter() {
            //         let frame = [
            //             (frame[0] + frame[2] + frame[4] + frame[6]) / 4.0,
            //             (frame[1] + frame[3] + frame[5] + frame[7]) / 4.0,
            //         ];

            //         let delta: [i16; 2] = [
            //             frame[0].to_sample::<i16>() - self.prev_frame[0].to_sample::<i16>(),
            //             frame[1].to_sample::<i16>() - self.prev_frame[1].to_sample::<i16>(),
            //         ];
            //         self.blip_bufs[0].add_delta(self.clocks, delta[0] as i32);
            //         self.blip_bufs[1].add_delta(self.clocks, delta[1] as i32);
            //         self.prev_frame = frame;
            //         self.clocks += 1;
            //         if self.clocks > 2048 {
            //             self.blip_bufs[0].end_frame(self.clocks);
            //             self.blip_bufs[1].end_frame(self.clocks);
            //             self.clocks = 0;
            //             break;
            //         }
            //     }

            //     while self.blip_bufs[0].samples_avail() >= 512 {
            //         assert!(self.blip_bufs[1].samples_avail() >= 512);
            //         let mut bufs = [[0; 512]; 2];
            //         self.blip_bufs[0].read_samples(&mut bufs[0], false);
            //         self.blip_bufs[1].read_samples(&mut bufs[1], false);
            //         self.buffer.extend(
            //             bufs[0]
            //                 .into_iter()
            //                 .zip(bufs[1])
            //                 .map(|(l, r)| [l.to_sample(), r.to_sample()]),
            //         );
            //     }
            // }
            // self.buffer.pop_front().unwrap()
        }
    }

    let mut signal = GBSignal::create(gameboy.lock().apu.output_channel.1.clone())
        .buffered(dasp::ring_buffer::Bounded::from([[0.0; 2]; 512]))
        .delay(100);

    let stream = device
        .build_output_stream(
            stream_config,
            using!([], move |data: &mut [f64], _: &cpal::OutputCallbackInfo| {
                // let mut iter = data.iter_mut();
                // while let Some(sample) = iter.next() {
                //     let channel1_sample = channel1_stream.next().unwrap().to_float_sample();
                //     let channel2_sample = channel2_stream.next().unwrap().to_float_sample();
                //     let channel3_sample = channel3_stream.next().unwrap().to_float_sample();

                //     let output = (channel1_sample + channel2_sample + channel3_sample) / 3.0;
                //     *sample = output;
                //     *iter.next().unwrap() = output;
                // }

                for sample in data.iter_mut() {
                    *sample = signal.by_ref().into_interleaved_samples().next_sample();
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
    let _tile_viewer_handle = tile_viewer.as_weak();

    let tilemap_viewer = TileMapViewer::new()?;

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

    ui.window().on_close_requested(|| {
        quit_event_loop().unwrap();
        CloseRequestResponse::KeepWindowShown
    });

    ui.on_toggle_tile_viewer(using!([ui_handle, tile_viewer.as_weak()], move || {
        let tile_viewer_shown = ui_handle.upgrade().unwrap().get_tile_viewer_shown();
        let tile_viewer = tile_viewer.upgrade().unwrap();
        if tile_viewer_shown {
            tile_viewer.show().unwrap();
        } else {
            tile_viewer.hide().unwrap();
        }
    }));

    ui.on_toggle_tilemap_viewer(using!([ui_handle, tilemap_viewer.as_weak()], move || {
        let tilemap_viewer_shown = ui_handle.upgrade().unwrap().get_tilemap_viewer_shown();
        let tilemap_viewer = tilemap_viewer.upgrade().unwrap();
        if tilemap_viewer_shown {
            tilemap_viewer.show().unwrap();
        } else {
            tilemap_viewer.hide().unwrap();
        }
    }));

    let (playback_controller, playback_receiver) = crossbeam::channel::bounded::<bool>(10);

    thread::spawn(using!(
        [
            ui_handle,
            gameboy,
            tile_viewer.as_weak(),
            tilemap_viewer.as_weak()
        ],
        move || {
            let mut is_playing = false;
            let mut prev_frame_time = Instant::now();
            loop {
                if !is_playing {
                    let playback_status = playback_receiver.recv();
                    is_playing = playback_status.unwrap();
                } else {
                    if let Ok(playback_status) = playback_receiver.try_recv() {
                        is_playing = playback_status;
                    }

                    let target_time = Instant::now() + Duration::from_secs_f64(1.0 / 59.73);
                    loop {
                        let mut gameboy = gameboy.lock();
                        let redraw_request = gameboy.tick(false);
                        if redraw_request {
                            redraw(&gameboy, &ui_handle, &tile_viewer, &tilemap_viewer);
                            let delta_time =
                                prev_frame_time.elapsed().as_secs_f32().max(1.0 / 59.73);
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
        [
            gameboy,
            ui.as_weak(),
            playback_controller,
            recent,
            recent_path
        ],
        move || {
            let Some(rom_path) = rfd::FileDialog::new()
                .add_filter("GameBoy ROMs", &["gb"])
                .set_directory(
                    env::current_dir()
                        .unwrap_or_else(|_| env::home_dir().unwrap_or_else(|| PathBuf::from("/"))),
                )
                .pick_file()
            else {
                return;
            };
            let gameboy = &mut *gameboy.lock();

            gameboy.cpu.load_debug_initial_state(&mut gameboy.context);

            let rom = fs::read(rom_path.clone()).unwrap();

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
            gameboy.cpu.load_rom(&rom, &mut gameboy.context);

            ui.upgrade_in_event_loop(using!([playback_controller, recent], move |mut handle| {
                // handle.set_paused(false);
                // playback_controller.send(true).unwrap();
                set_recent(&mut handle, &recent.read());
                handle.invoke_focus();
            }))
            .unwrap();
        }
    ));

    ui.on_load_recent(using!(
        [
            gameboy,
            ui.as_weak(),
            recent,
            playback_controller,
            recent_path
        ],
        move |index| {
            let gameboy = &mut *gameboy.lock();

            gameboy.cpu.load_debug_initial_state(&mut gameboy.context);

            let rom_path = recent.read()[index as usize].clone();
            let rom = fs::read(rom_path.clone()).unwrap();
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

            gameboy.cpu.load_rom(&rom, &mut gameboy.context);

            ui.upgrade_in_event_loop(using!([playback_controller, recent], move |mut handle| {
                // handle.set_paused(false);
                // playback_controller.send(true).unwrap();
                set_recent(&mut handle, &recent.read());
                handle.invoke_focus();
            }))
            .unwrap();
        }
    ));

    ui.on_toggle_playback(using!([ui.as_weak(), playback_controller], move || {
        let ui = ui.upgrade().unwrap();
        playback_controller.send(!ui.get_paused()).unwrap();
    }));

    ui.on_step_tick(using!(
        [
            gameboy,
            ui.as_weak(),
            tile_viewer.as_weak(),
            tilemap_viewer.as_weak()
            playback_controller
        ],
        move || {
            let ui = ui.upgrade().unwrap();
            playback_controller.send(false).unwrap();
            ui.set_paused(true);

            let mut gameboy = gameboy.lock();
            gameboy.tick(true);
            redraw(&gameboy, &ui.as_weak(), &tile_viewer, &tilemap_viewer);
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

fn redraw(
    gameboy: &GameBoy,
    ui_handle: &Weak<AppWindow>,
    tile_viewer_handle: &Weak<TileViewer>,
    tilemap_viewer_handle: &Weak<TileMapViewer>,
) {
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&gameboy.buffer, 160, 144);

    let tiles: Vec<_> = gameboy
        .context
        .memory
        .vram
        .tile_data()
        .pipe(|data| data.as_chunks::<16>().0)
        .iter()
        .map(|x| {
            SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                &x.pipe(|data| data.as_chunks::<2>().0)
                    .iter()
                    .copied()
                    .flat_map(|[left, right]| {
                        let row = ((left as u16) << 8) | right as u16;
                        (0..8)
                            .map(move |index| row.extract_bits(0b1000_0000_1000_0000 >> index))
                            .map(|colour| gameboy.context.memory.io.lcd.bgp >> (colour * 2) & 0b11)
                            .flat_map(|colour| {
                                gameboy.palette[&Pixel::from_repr(colour).unwrap()]
                                    .conv::<[u8; 4]>()
                            })
                    })
                    .collect::<Vec<_>>(),
                8,
                8,
            )
        })
        .collect();

    let mapping = gameboy.context.memory.io.lcd.lcdc.tile_data_mapping();
    let tile_maps = [
        gameboy.context.memory.io.lcd.lcdc.bg_tile_map(),
        gameboy.context.memory.io.lcd.lcdc.window_tile_map(),
    ]
    .map(|map_area| {
        let map = gameboy.context.memory.vram.tile_map(map_area);
        map.iter()
            .copied()
            .map(|tile_id| {
                let tile = gameboy.context.memory.vram.bg_tile_data(mapping, tile_id);

                SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                    &tile
                        .pipe(|data| data.as_chunks::<2>().0)
                        .iter()
                        .copied()
                        .flat_map(|[left, right]| {
                            let row = ((left as u16) << 8) | right as u16;
                            (0..8)
                                .map(move |index| row.extract_bits(0b1000_0000_1000_0000 >> index))
                                .map(|colour| {
                                    gameboy.context.memory.io.lcd.bgp >> (colour * 2) & 0b11
                                })
                                .flat_map(|colour| {
                                    gameboy.palette[&Pixel::from_repr(colour).unwrap()]
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
    let (scroll_x, scroll_y) = gameboy
        .context
        .memory
        .io
        .lcd
        .pipe_ref(|lcd| (lcd.scx, lcd.scy));

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

    ui_handle
        .upgrade_in_event_loop(move |handle| {
            handle.set_screen(Image::from_rgba8(buffer));
            handle.window().request_redraw();
        })
        .unwrap();

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
