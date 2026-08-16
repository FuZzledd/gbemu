#![feature(uint_gather_scatter_bits)]

use core::{
    borrow::Borrow,
    ops::{BitAnd, BitOr, Index, IndexMut, Not, Shl, Shr},
    sync::atomic::{AtomicBool, Ordering},
};
use std::{mem, path::PathBuf};
use std::{path::Path, sync::LazyLock};

use gbemu_common::theme::Color;
use palette::{IntoColor, Srgba};
use parking_lot::Mutex;
use rgb::{ComponentMap, Gray, Rgba};
use serde::{Deserialize, Serialize};
use tap::Conv;
use tracing::instrument;

use crate::{
    context::{Context, InterruptRegister, InterruptType::Joypad, Memory, MemoryBus, Serial},
    cpu::CPU,
    debugging::{BreakpointData, Breakpoints},
    ppu::{Mode, Pixel},
};

use crate::context::LoadRomError;
use crate::debugging::{BREAKPOINT_REPORTER, HitBreakpoint};
use bytemuck::{Pod, Zeroable};
use std::default::Default;

pub static PLAYING: AtomicBool = AtomicBool::new(false);

pub mod apu;
pub mod context;
pub mod cpu;
pub mod debugging;
pub mod opcode;
pub mod ppu;

#[macro_export]
macro_rules! bit_getters {
    ($name:ident,$bit:literal) => {
        fn $name(&self) -> bool {
            $crate::get_bit(self.0, $bit)
        }

        paste::paste! {
            fn [<set_ $name>](&mut self, value: bool) {
                $crate::set_bit(&mut self.0, $bit, value);
            }
        }
    };
}

pub fn set_bit<T>(num: &mut T, index: u8, value: bool)
where
    T: BitAnd<T, Output = T> + BitOr<T, Output = T>,
    T: From<bool> + Copy,
    T: Shl<u8, Output = T>,
    T: Not<Output = T>,
{
    *num = (*num & !(T::from(true) << index)) | (T::from(value) << index);
}
pub fn get_bit<T>(num: T, index: u8) -> bool
where
    T: BitAnd<T, Output = T> + BitOr<T, Output = T>,
    T: From<bool> + Copy,
    T: Shr<u8, Output = T>,
    T: Not<Output = T>,
    T: PartialEq,
{
    (num >> index) & T::from(true) == T::from(true)
}

#[derive(Debug, Clone, Copy)]
pub enum GameBoyButton {
    Select,
    Start,
    A,
    B,
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, Pod, Zeroable, Serialize, Deserialize, PartialEq, Eq)]
#[repr(transparent)]
pub struct Palette<T = u8> {
    inner: [Rgba<T>; 4],
}

impl From<Palette<u8>> for [Color; 4] {
    fn from(value: Palette<u8>) -> Self {
        value
            .inner
            .map(|Rgba { r, g, b, a }| Srgba::from_components((r, g, b, a)).into())
    }
}

impl From<[Color; 4]> for Palette<u8> {
    fn from(value: [Color; 4]) -> Self {
        value
            .map(|color| {
                let (r, g, b, a) = <Srgba<u8>>::from(color.0).into_components();
                Rgba { r, g, b, a }
            })
            .into()
    }
}

impl<U> Index<usize> for Palette<U> {
    type Output = Rgba<U>;
    fn index(&self, index: usize) -> &Self::Output {
        &self.inner[index]
    }
}

impl<U> IndexMut<usize> for Palette<U> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.inner[index]
    }
}

impl<T: Borrow<Pixel>, U> IndexMut<T> for Palette<U> {
    fn index_mut(&mut self, index: T) -> &mut Self::Output {
        &mut self.inner[*index.borrow() as usize]
    }
}

impl<T: Borrow<Pixel>, U> Index<T> for Palette<U> {
    type Output = Rgba<U>;

    fn index(&self, index: T) -> &Self::Output {
        &self.inner[*index.borrow() as usize]
    }
}

impl Default for Palette {
    fn default() -> Self {
        use ppu::Pixel::*;
        let mut palette = Self {
            inner: Default::default(),
        };

        palette[White] = Gray::new(0xFF).conv::<Rgba<u8>>();
        palette[LightGray] = Gray::new(0xAA).conv::<Rgba<u8>>();
        palette[DarkGrey] = Gray::new(0x55).conv::<Rgba<u8>>();
        palette[Black] = Gray::new(0x00).conv::<Rgba<u8>>();
        palette
    }
}

impl<T> From<[Rgba<T>; 4]> for Palette<T> {
    fn from(value: [Rgba<T>; 4]) -> Self {
        Self { inner: value }
    }
}

impl<T> From<Palette<T>> for [Rgba<T>; 4] {
    fn from(value: Palette<T>) -> [Rgba<T>; 4] {
        value.inner
    }
}

impl From<[u32; 4]> for Palette<u8> {
    fn from(value: [u32; 4]) -> Self {
        bytemuck::cast::<_, [Rgba<u8>; 4]>(value).into()
    }
}

impl From<[[u8; 3]; 4]> for Palette<u8> {
    fn from(value: [[u8; 3]; 4]) -> Self {
        value.map(|[r, g, b]| [r, g, b, 0xFF]).into()
    }
}

impl From<[[u8; 4]; 4]> for Palette<u8> {
    fn from(value: [[u8; 4]; 4]) -> Self {
        bytemuck::cast::<_, [Rgba<u8>; 4]>(value).into()
    }
}

impl From<Palette<u8>> for Palette<f32> {
    fn from(value: Palette<u8>) -> Self {
        Palette {
            inner: value
                .inner
                .map(|color| color.map(|component| component as f32 / 255.0)),
        }
    }
}

impl From<Palette<f32>> for Palette<u8> {
    fn from(value: Palette<f32>) -> Self {
        Palette {
            inner: value
                .inner
                .map(|color| color.map(|component| (component * 255.0) as u8)),
        }
    }
}

pub struct GameBoy {
    pub context: Context<MemoryBus>,
    pub cpu: cpu::CPU<MemoryBus>,
    pub ppu: ppu::PPU,
    pub apu: apu::APU,
    pub counter: u64,
}

pub static DEBUGGING_ENABLED: AtomicBool = AtomicBool::new(false);
pub static BREAKPOINTS: LazyLock<Mutex<Breakpoints>> = LazyLock::new(Default::default);
pub static HIT_BREAKPOINTS: LazyLock<Mutex<Vec<HitBreakpoint>>> = LazyLock::new(Default::default);

#[derive(Debug, Clone, Copy)]
pub enum PlaybackMessage {
    TogglePlayback,
    Pause,
    Play,
    StepTick(usize),
    StepFrame(usize),
}

pub static PLAYBACK_CONTROLLER: LazyLock<(
    flume::Sender<PlaybackMessage>,
    flume::Receiver<PlaybackMessage>,
)> = LazyLock::new(|| flume::bounded(16));

#[derive(Debug, Clone, Copy)]
pub enum TickStatus {
    Normal,
    DrawRequested,
    BreakpointHit,
}

impl TickStatus {
    #[inline(always)]
    pub fn should_redraw(self) -> bool {
        !matches!(self, TickStatus::Normal)
    }
}

impl GameBoy {
    #[instrument(skip_all)]
    pub fn tick(&mut self, manual: bool) -> TickStatus {
        use TickStatus::*;
        let debugging_enabled = DEBUGGING_ENABLED.load(Ordering::Relaxed);
        if debugging_enabled {
            let breakpoints = BREAKPOINTS.lock();
            let hit_breakpoints = breakpoints
                .pc_value
                .iter()
                .filter(|(_, breakpoint)| {
                    breakpoint.enabled && breakpoint.breakpoint.pc == self.cpu.pc
                })
                .map(|(id, _breakpoint)| (*id, BreakpointData::Addr(self.cpu.pc)));

            HIT_BREAKPOINTS.lock().extend(hit_breakpoints);
        };

        if !PLAYING.load(Ordering::Relaxed) && !manual {
            return Normal;
        }

        if self.counter.is_multiple_of(4) {
            self.cpu.tick(&mut self.context);
            self.context.memory.tick_oam_dma();
        }

        if self.counter.is_multiple_of(128) {
            Serial::tick(&mut self.context);
        }

        self.ppu.tick(&mut self.context);

        self.apu.tick(&mut self.context);

        self.counter = self.counter.wrapping_add(1);

        if debugging_enabled
            && let mut hit_breakpoints = HIT_BREAKPOINTS.lock()
            && !hit_breakpoints.is_empty()
        {
            BREAKPOINT_REPORTER
                .0
                .send(mem::take(&mut hit_breakpoints))
                .unwrap();
            PLAYING.store(false, Ordering::Relaxed);

            return BreakpointHit;
        }

        if self.ppu.current_mode == Mode::VBlank
            && self.context.memory.io.lcd.ly == 144
            && self.ppu.cycle_counter == 0
        {
            return DrawRequested;
        }
        Normal
    }

    pub fn get_screen(&self) -> &[Pixel; 23040] {
        &self.ppu.screen
    }

    pub fn get_screen_mut(&mut self) -> &mut [Pixel; 23040] {
        &mut self.ppu.screen
    }

    pub fn set_joypad_state(&mut self, button: GameBoyButton, state: bool) {
        let button_state = &mut self.context.memory.io.joypad.buttons_state;
        let dpad_state = &mut self.context.memory.io.joypad.dpad_state;

        let prev_button_state = button_state.clone();
        let prev_dpad_state = dpad_state.clone();

        match button {
            GameBoyButton::Select => button_state.set(2, state),
            GameBoyButton::Start => button_state.set(3, state),
            GameBoyButton::A => button_state.set(0, state),
            GameBoyButton::B => button_state.set(1, state),
            GameBoyButton::Left => dpad_state.set(1, state),
            GameBoyButton::Right => dpad_state.set(0, state),
            GameBoyButton::Up => dpad_state.set(2, state),
            GameBoyButton::Down => dpad_state.set(3, state),
        }

        if (prev_button_state & !button_state.clone() | (prev_dpad_state & !dpad_state.clone()))
            .any()
        {
            self.context.memory.io.interrupt.schedule_interrupt(Joypad);
        }
    }

    pub fn reset(&mut self, fast_boot: bool) {
        self.cpu = CPU::default();
        self.context = Context::default();
        self.apu.reset();
        self.ppu.reset();
        self.counter = 0;

        if fast_boot {
            self.cpu.load_debug_initial_state(&mut self.context);
        }
    }

    pub fn load_rom(&mut self, path: impl AsRef<Path>) -> Result<(), LoadRomError> {
        self.context.load_rom(path)
    }

    pub fn load_boot_rom(&mut self, path: Option<impl AsRef<Path>>) -> Result<(), LoadRomError> {
        self.context.load_boot_rom(path)
    }
}

impl Default for GameBoy {
    fn default() -> Self {
        let mut context = Context::default();
        let cpu = cpu::CPU::default();
        let ppu = ppu::PPU::default();
        let mut apu = apu::APU::default();
        apu.create_support(&mut context).start();

        Self {
            context,
            cpu,
            ppu,
            apu,
            counter: 0,
        }
    }
}
