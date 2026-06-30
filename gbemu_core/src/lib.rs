#![feature(uint_gather_scatter_bits)]
#![feature(hash_map_macro)]

use core::{
    borrow::Borrow,
    ops::{BitAnd, BitOr, Index, IndexMut, Not, Shl, Shr},
    sync::atomic::{AtomicBool, Ordering},
};
use std::{collections::HashMap, path::Path, sync::LazyLock};

use bytes::{Bytes, BytesMut};
use crossbeam::channel::{Receiver, Sender};
use rgb::Rgba;
use tap::{Conv, Pipe, Tap};
use tracing::instrument;

use crate::{
    apu::APU,
    context::{Context, InterruptRegister, InterruptType::Joypad, Memory, MemoryBus, Serial},
    cpu::CPU,
    ppu::{Mode, PPU, Pixel},
};
use rayon::prelude::*;

use std::hash_map;
use crate::context::LoadRomError;

pub static PLAYING: AtomicBool = AtomicBool::new(false);

pub mod apu;
pub mod context;
pub mod cpu;
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

#[derive(Default, Debug, Clone, Copy)]
pub struct Palette {
    inner: [Rgba<u8>; 4],
}

impl<T: Borrow<Pixel>> IndexMut<T> for Palette {
    fn index_mut(&mut self, index: T) -> &mut Self::Output {
        &mut self.inner[*index.borrow() as usize]
    }
}

impl<T: Borrow<Pixel>> Index<T> for Palette {
    type Output = Rgba<u8>;

    fn index(&self, index: T) -> &Self::Output {
        &self.inner[*index.borrow() as usize]
    }
}

pub struct GameBoy {
    pub buffer: BytesMut,
    pub context: Context<MemoryBus>,
    pub cpu: cpu::CPU<MemoryBus>,
    pub ppu: ppu::PPU,
    pub apu: apu::APU,
    pub counter: u64,
    pub palette: Palette,
}

impl GameBoy {
    #[instrument(skip_all)]
    pub fn tick(&mut self, manual: bool) -> bool {
        if !PLAYING.load(Ordering::Relaxed) && !manual {
            return false || manual;
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

        if (self.ppu.current_mode == Mode::VBlank
            && self.context.memory.io.lcd.ly == 144
            && self.ppu.cycle_counter == 0)
            || manual
        {
            return true;
        }
        false
    }

    pub fn get_screen(&self) -> &[Pixel; 23040] {
        &self.ppu.screen
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

    pub fn load_rom(&mut self, path: impl AsRef<Path>) -> Result<(), LoadRomError> {
        self.cpu = CPU::default();
        self.context = Context::default();

        self.cpu.load_debug_initial_state(&mut self.context);
        self.context.load_rom(path)
    }
}

impl Default for GameBoy {
    fn default() -> Self {
        let context = Context::default();
        let cpu = cpu::CPU::default();
        let ppu = ppu::PPU::default();
        let apu = apu::APU::default();

        let mut buffer = BytesMut::zeroed(160 * 144 * 4);
        for pixel in buffer.as_chunks_mut::<4>().0 {
            pixel[3] = 0xFF
        }

        let palette = Palette::default().tap_mut(|palette| {
            use ppu::Pixel::*;
            palette[White] = [220, 220, 220, 255].into();
            palette[LightGray] = [160, 160, 160, 255].into();
            palette[DarkGrey] = [80, 80, 80, 255].into();
            palette[Black] = [0, 0, 0, 255].into();
        });

        Self {
            buffer,
            context,
            cpu,
            ppu,
            apu,
            counter: 0,
            palette,
        }
    }
}
