#![allow(clippy::upper_case_acronyms)]
#![feature(uint_gather_scatter_bits)]
use core::{cmp::min, iter, ops::RangeInclusive, time::Duration};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, sleep},
};

use better_default::Default;

use bytes::{Bytes, BytesMut};
use iced::{
    Background, Element,
    Length::{Fill, FillPortion, Shrink},
    Padding, Rectangle, Task, exit,
    font::{self, Font},
    futures::SinkExt,
    stream,
    time::every,
    widget::{
        self, image::Handle, operation::RelativeOffset, scrollable::Viewport, space::vertical,
        text::primary,
    },
    window::Settings,
};
use iced::{
    Subscription,
    widget::canvas::{Cache, Image},
};
use iced::{
    widget::{column, *},
    window,
};
use tap::Pipe;
use tracing::{debug, info, level_filters::LevelFilter};
use tracing_subscriber::{EnvFilter, fmt};

use gbemu::{
    context::{Context, Memory, MemoryBus},
    cpu,
    ppu::{self, Mode},
};

fn main() -> iced::Result {
    let format = fmt::format()
        .with_level(false) // don't include levels in formatted output
        .with_target(false) // don't include targets
        .without_time()
        .compact();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .event_format(format)
        .init();

    iced::daemon(App::new, App::update, App::view)
        .subscription(App::subscription)
        .theme(App::theme)
        .title(App::title)
        .run()?;

    Ok(())
}

#[derive(Debug)]
struct Window {
    window_type: WindowType,
    title: String,
}
impl Window {
    fn new(window_type: WindowType) -> Self {
        Self {
            window_type,
            title: match window_type {
                WindowType::Main => "gbemu".into(),
                WindowType::TileViewer => "Tile Viewer".into(),
                WindowType::MemoryViewer => "Memory Viewer".into(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum WindowType {
    Main,
    TileViewer,
    MemoryViewer,
}

#[derive(Default, Clone)]
struct ThreadSafeGameBoy(Arc<Mutex<GameBoy>>);

struct App {
    windows: BTreeMap<window::Id, Window>,
    gameboy: ThreadSafeGameBoy,
    tile_viewer: TileViewer,
    memory_viewer: MemoryViewer,
    is_playing: bool,
    frame_count: u64,
    redraw_requested: Option<Receiver<()>>,
    playback_controller: mpsc::Sender<bool>,
    subscription_sender: Option<iced::futures::channel::mpsc::Sender<Receiver<()>>>,
}
#[derive(Default)]
struct MemoryViewer {
    #[default(widget::Id::unique())]
    scrollable_id: widget::Id,
    current_view: Vec<u8>,
    row_start: u16,
    current_viewport: Option<Viewport>,
    selected_address: u16,
    selected_address_value: u8,
}

impl MemoryViewer {
    fn view<'a>(&self) -> impl Into<Element<'a, Message>> {
        let scroller = scrollable(space().height(0xFFFF * 2).width(Fill))
            .id(self.scrollable_id.clone())
            .on_scroll(|viewport| {
                Message::MemoryViewerMessage(MemoryViewerMessage::OnScroll(viewport))
            })
            .auto_scroll(true);

        let container_style = |row| {
            let row_start = self.row_start;
            move |theme: &iced::Theme| {
                let palette = theme.extended_palette();
                let pair = if (row as u16 + row_start).is_multiple_of(2) {
                    palette.background.weak
                } else {
                    palette.background.weakest
                };
                container::Style::default()
                    .background(pair.color)
                    .color(pair.text)
            }
        };

        let table = table(
            iter::once(table::column(space(), |row| {
                container(
                    text!("{:#06X}", (row as u16 + self.row_start) * 16)
                        .height(18)
                        .size(12)
                        .font(Font::MONOSPACE),
                )
                .style(container_style(row))
                .padding(3)
                .height(21)
            }))
            .chain((0..16).map(|i| {
                table::column(
                    text!("{:x}", i).height(18).font(Font::MONOSPACE).size(12),
                    move |row| {
                        mouse_area(
                            container(
                                text!("{:02X}", self.current_view[row * 16 + i])
                                    .height(18)
                                    .size(12)
                                    .font(Font::MONOSPACE),
                            )
                            .style(container_style(row))
                            .padding(5)
                            .height(21),
                        )
                        .on_press(
                            MemoryViewerMessage::AddressClicked(
                                (row as u16 + self.row_start) * 16 + i as u16,
                            )
                            .into(),
                        )
                    },
                )
            })),
            0..(self.current_view.len() / 16),
        )
        .padding(0);

        // column![
        //         self.row_offsets.map(|address| text!("{address:#02X"))
        //     ].hea,
        //     grid(self.current_view.iter().map(|b| text!("{b:02X}").into()))
        //     .columns(16)
        //     .height(Fill)
        row![
            stack![scroller, table],
            column![
                text!("Selected Address: 0x{:04X}", self.selected_address),
                text!("Hex: 0x{:02X}", self.selected_address_value),
                text!("Binary: 0b{:08b}", self.selected_address_value),
                text!("Decimal: {}", self.selected_address_value),
            ]
        ]
    }

    fn update(&mut self, message: MemoryViewerMessage) -> Task<Message> {
        match message {
            MemoryViewerMessage::OnScroll(viewport) => {
                self.current_viewport = Some(viewport);
                return Task::done(Message::UpdateMemoryViewer);
            }
            MemoryViewerMessage::AddressClicked(address) => {
                self.selected_address = address;
                return Task::done(Message::UpdateMemoryViewer);
            }
        }
        Task::none()
    }
}

#[derive(Clone, Copy, Debug)]
enum MemoryViewerMessage {
    OnScroll(Viewport),
    AddressClicked(u16),
}

impl From<MemoryViewerMessage> for Message {
    fn from(value: MemoryViewerMessage) -> Self {
        Message::MemoryViewerMessage(value)
    }
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let (_, main_window) = window::open(Settings::default());
        let (_, tile_viewer) = window::open(Settings::default());
        let (_, memory_viewer) = window::open(Settings::default());

        let (redraw_requester, redraw_requested) = mpsc::channel::<()>();
        let (playback_controller, playback_receiver) = mpsc::channel::<bool>();

        let app = App {
            windows: <BTreeMap<window::Id, Window> as core::default::Default>::default(),
            gameboy: <ThreadSafeGameBoy as core::default::Default>::default(),
            tile_viewer: <TileViewer as core::default::Default>::default(),
            memory_viewer: <MemoryViewer as core::default::Default>::default(),
            is_playing: <bool as core::default::Default>::default(),
            frame_count: <u64 as core::default::Default>::default(),
            redraw_requested: Some(redraw_requested),
            playback_controller,
            subscription_sender: None,
        };
        {
            let gameboy = app.gameboy.clone();
            thread::spawn(move || {
                let mut is_playing = false;
                loop {
                    if let Ok(status) = playback_receiver.try_recv() {
                        is_playing = status;
                    }
                    if is_playing {
                        {
                            let mut gameboy = gameboy.0.lock().unwrap();
                            let redraw_request = gameboy.tick(false);
                            if redraw_request {
                                redraw_requester.send(());
                            }
                        }

                        spin_sleep::SpinSleeper::default()
                            .with_spin_strategy(spin_sleep::SpinStrategy::SpinLoopHint)
                            .sleep_ns(238);
                    }
                }
            });
        }
        (
            app,
            Task::batch([
                main_window.map(|id| Message::WindowOpened(id, WindowType::Main)),
                tile_viewer.map(|id| Message::WindowOpened(id, WindowType::TileViewer)),
                memory_viewer.map(|id| Message::WindowOpened(id, WindowType::MemoryViewer)),
            ]),
        )
    }
    fn view(&self, window_id: window::Id) -> Element<'_, Message> {
        if let Some(window) = self.windows.get(&window_id) {
            match window.window_type {
                WindowType::Main => column![
                    row![
                        button("Start").on_press(GameBoyMessage::Play.into()),
                        button("Toggle Playback").on_press(GameBoyMessage::TogglePlayback.into()),
                        button("Tick").on_press(GameBoyMessage::ManualTick.into())
                    ],
                    canvas(self.gameboy.clone()).width(160 * 3).height(144 * 3)
                ]
                .into(),
                WindowType::TileViewer => self.tile_viewer.view().into(),
                WindowType::MemoryViewer => self.memory_viewer.view().into(),
            }
        } else {
            column![].into()
        }
    }

    fn title(&self, window_id: window::Id) -> String {
        if let Some(window) = self.windows.get(&window_id) {
            window.title.clone()
        } else {
            "gbemu".into()
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::GameBoyMessage(message) => match message {
                GameBoyMessage::ManualTick => {
                    let mut gameboy: std::sync::MutexGuard<'_, GameBoy> =
                        self.gameboy.0.lock().unwrap();
                    gameboy.tick(true);

                    if gameboy.ppu.current_mode == Mode::VBlank
                        && gameboy.context.memory.io.lcd.ly == 144
                    {
                        self.frame_count += 1;
                        self.tile_viewer.tiles = gameboy
                            .context
                            .memory
                            .vram
                            .tile_data()
                            .pipe(|data| data.as_chunks::<16>().0)
                            .iter()
                            .map(|x| {
                                x.pipe(|data| data.as_chunks::<2>().0)
                                    .iter()
                                    .cloned()
                                    .flat_map(|[left, right]| {
                                        let row = ((left as u16) << 8) | right as u16;
                                        (0..8)
                                            .map(move |index| {
                                                row.extract_bits(0b1000_0000_1000_0000 >> index)
                                            })
                                            .flat_map(|colour| match colour {
                                                0 => [0xFF, 0xFF, 0xFF, 0xFF],
                                                1 => [0xBC, 0xBC, 0xBC, 0xFF],
                                                2 => [0x80, 0x80, 0x80, 0xFF],
                                                _ => [0x0, 0x0, 0x0, 0xFF],
                                            })
                                    })
                                    .collect()
                            })
                            .collect();
                        self.tile_viewer.cache.clear();
                    }
                }
                GameBoyMessage::Play => {
                    let Some(rom_path) = rfd::FileDialog::new()
                        .add_filter("GameBoy ROMs", &["gb"])
                        .set_directory(env::current_dir().unwrap_or_else(|_| {
                            env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
                        }))
                        .pick_file()
                    else {
                        return Task::none();
                    };

                    let gameboy = &mut *self.gameboy.0.lock().unwrap();
                    gameboy.cpu.load_debug_initial_state(&mut gameboy.context);
                    // state.gameboy.cpu.load_boot_rom(
                    //     include_bytes!("../bootrom/dmg_boot.bin"),
                    //     &mut state.gameboy.context,
                    // );

                    let rom = fs::read(rom_path).unwrap();
                    gameboy.cpu.load_rom(&rom, &mut gameboy.context);
                    self.is_playing = true;
                    self.playback_controller.send(self.is_playing).unwrap();
                }
                GameBoyMessage::TogglePlayback => {
                    self.is_playing = !self.is_playing;
                    self.playback_controller.send(self.is_playing).unwrap();
                    println!("{}", self.is_playing);
                }
            },
            Message::WindowOpened(id, window_type) => {
                let window = Window::new(window_type);
                self.windows.insert(id, window);
            }
            Message::WindowClosed(id) => {
                if let Some(Window {
                    window_type: WindowType::Main,
                    ..
                }) = self.windows.get(&id)
                {
                    return exit();
                }
            }
            Message::MemoryViewerMessage(message) => {
                return self.memory_viewer.update(message);
            }
            Message::SubscriberReady(mut sender) => {
                if let Some(redraw_requested) = self.redraw_requested.take() {
                    sender.send(redraw_requested);
                    self.subscription_sender = Some(sender);
                }
            }
            Message::RedrawRequested => {
                self.frame_count += 1;
                let gameboy: std::sync::MutexGuard<'_, GameBoy> = self.gameboy.0.lock().unwrap();

                self.tile_viewer.tiles = gameboy
                    .context
                    .memory
                    .vram
                    .tile_data()
                    .pipe(|data| data.as_chunks::<16>().0)
                    .iter()
                    .map(|x| {
                        x.pipe(|data| data.as_chunks::<2>().0)
                            .iter()
                            .cloned()
                            .flat_map(|[left, right]| {
                                let row = ((left as u16) << 8) | right as u16;
                                (0..8)
                                    .map(move |index| {
                                        row.extract_bits(0b1000_0000_1000_0000 >> index)
                                    })
                                    .flat_map(|colour| match colour {
                                        0 => [0xFF, 0xFF, 0xFF, 0xFF],
                                        1 => [0xBC, 0xBC, 0xBC, 0xFF],
                                        2 => [0x80, 0x80, 0x80, 0xFF],
                                        _ => [0x0, 0x0, 0x0, 0xFF],
                                    })
                            })
                            .collect()
                    })
                    .collect();
                self.tile_viewer.cache.clear();
                return Task::done(Message::UpdateMemoryViewer);
            }
            Message::UpdateMemoryViewer => {
                if let Some(viewport) = self.memory_viewer.current_viewport {
                    let range = (viewport.bounds().height / 23.0) as usize;
                    let mem_start: usize = min(
                        (viewport.relative_offset().y * 0x1000 as f32) as usize,
                        0x1000 - range + 1,
                    );

                    let gameboy: std::sync::MutexGuard<'_, GameBoy> =
                        self.gameboy.0.lock().unwrap();

                    self.memory_viewer.current_view = ((mem_start * 16)
                        ..=min((mem_start + range) * 16, 0xFFFF))
                        .map(|addr| gameboy.context.memory.read_u8(addr as u16))
                        .collect();
                    self.memory_viewer.row_start = mem_start as u16;
                    self.memory_viewer.selected_address_value = gameboy
                        .context
                        .memory
                        .read_u8(self.memory_viewer.selected_address);
                }
            }
        }
        Task::none()
    }
    fn subscription(&self) -> Subscription<Message> {
        let window_events = window::events().filter_map(|(id, event)| match event {
            window::Event::Closed => Some(Message::WindowClosed(id)),
            _ => None,
        });

        let redrawer = Subscription::run(|| {
            stream::channel(100, async move |mut output| {
                let (tx, mut rx) = iced::futures::channel::mpsc::channel(100);

                output.send(Message::SubscriberReady(tx)).await.unwrap();

                let redraw_requested = rx.recv().await.unwrap();
                loop {
                    if let Ok(()) = redraw_requested.recv() {
                        output.send(Message::RedrawRequested).await.unwrap();
                    }
                }
            })
        });

        Subscription::batch([
            window_events,
            redrawer,
            every(Duration::from_millis(8)).map(|_| Message::RedrawRequested),
        ])
    }

    fn theme(&self, _window_id: window::Id) -> Theme {
        Theme::CatppuccinFrappe
    }
}

#[derive(Clone, Debug)]

enum Message {
    WindowOpened(window::Id, WindowType),
    GameBoyMessage(GameBoyMessage),
    WindowClosed(window::Id),
    MemoryViewerMessage(MemoryViewerMessage),
    SubscriberReady(iced::futures::channel::mpsc::Sender<Receiver<()>>),
    RedrawRequested,
    UpdateMemoryViewer,
}
#[derive(Clone, Debug, Copy)]

enum GameBoyMessage {
    ManualTick,
    Play,
    TogglePlayback,
}
impl From<GameBoyMessage> for Message {
    fn from(value: GameBoyMessage) -> Self {
        Message::GameBoyMessage(value)
    }
}

#[derive(Debug, Default)]
struct TileViewer {
    tiles: Vec<Bytes>,
    cache: Cache,
}

impl TileViewer {
    fn view<'a, Message: 'a>(&'a self) -> impl Into<Element<'a, Message>> {
        column![
            text("Tile Viewer"),
            canvas(self).width(8 * 16 * 3).height(8 * 24 * 3)
        ]
    }
}

impl<Message> canvas::Program<Message> for TileViewer {
    type State = ();

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        theme: &iced_renderer::core::Theme,
        bounds: iced::Rectangle,
        cursor: iced::advanced::mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        let screen = self.cache.draw(renderer, bounds.size(), |frame| {
            for y in 0..24 {
                for x in 0..16 {
                    let image = Image::from(&Handle::from_rgba(
                        8,
                        8,
                        self.tiles.get(y * 16 + x).map_or_else(
                            || (0..64).flat_map(|_| [0, 0, 0, 255]).collect(),
                            |x| x.clone(),
                        ),
                    ))
                    .snap(true)
                    .filter_method(image::FilterMethod::Nearest);
                    frame.draw_image(
                        Rectangle::new(
                            iced::Point {
                                x: x as f32 * 8.0 * 3.0,
                                y: y as f32 * 8.0 * 3.0,
                            },
                            iced::Size::from([8.0 * 3.0; 2]),
                        ),
                        image,
                    );
                }
            }
        });

        vec![screen]
    }
}

struct GameBoy {
    buffer: BytesMut,
    cache: Cache,
    context: Context<MemoryBus>,
    cpu: cpu::CPU<MemoryBus>,
    ppu: ppu::PPU,
    counter: u64,
}
impl GameBoy {
    fn tick(&mut self, manual: bool) -> bool {
        if self.counter.is_multiple_of(4) {
            self.cpu.tick(&mut self.context);
        }
        self.ppu.tick(&mut self.context);

        self.counter = self.counter.wrapping_add(1);

        if (self.ppu.current_mode == Mode::VBlank
            && self.context.memory.io.lcd.ly == 144
            && self.ppu.cycle_counter == 0)
            || manual
        {
            self.buffer = self
                .ppu
                .screen
                .iter()
                .flat_map(|pixel| match pixel {
                    ppu::Pixel::White => [220, 220, 220, 255],
                    ppu::Pixel::LightGray => [160, 160, 160, 255],
                    ppu::Pixel::DarkGrey => [80, 80, 80, 255],
                    ppu::Pixel::Black => [0, 0, 0, 255],
                })
                .collect();

            self.cache.clear();
            return true;
        }
        false
    }
}

impl Default for GameBoy {
    fn default() -> Self {
        let context = Context::default();
        let cpu = cpu::CPU::default();
        let ppu = ppu::PPU::default();

        let mut buffer = BytesMut::zeroed(160 * 144 * 4);
        for pixel in buffer.as_chunks_mut::<4>().0 {
            pixel[3] = 0xFF
        }

        Self {
            buffer,
            cache: Cache::default(),
            context,
            cpu,
            ppu,
            counter: 0,
        }
    }
}

impl<Message> canvas::Program<Message> for ThreadSafeGameBoy {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &iced_renderer::core::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::advanced::mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        let gameboy = self.0.lock().unwrap();
        let screen = gameboy.cache.draw(renderer, bounds.size(), |frame| {
            let image = Image::from(&Handle::from_rgba(160, 144, gameboy.buffer.clone()))
                .snap(true)
                .filter_method(image::FilterMethod::Nearest);

            frame.draw_image(bounds.shrink(Padding::from([77, 80])), image);
        });

        vec![screen]
    }
}
