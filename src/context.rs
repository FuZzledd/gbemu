use core::ops::BitAnd;

use better_default::Default;
use bitvec::{access::BitSafe, prelude::*};
use bytemuck::TransparentWrapper;
use tracing::debug;

use crate::{
    bit_getters,
    ppu::{LCDRegisters, Oam, Vram},
};

pub(crate) type Memory16K = [u8; 1024 * 16];

pub(crate) type Memory8K = [u8; 1024 * 8];

pub(crate) type Memory4K = [u8; 1024 * 4];

#[derive(Default)]
pub struct IoRegisters {
    pub joypad: JoypadRegister,
    pub serial: SerialTransferRegisters,
    pub timer: Timer,
    pub interrupt: InterruptFlag,
    pub lcd: LCDRegisters,
}
#[derive(Default, TransparentWrapper)]
#[repr(transparent)]
struct RawJoypadRegister(#[default(0b11001111)] u8);

impl RawJoypadRegister {
    bit_getters!(select_buttons, 5);
    bit_getters!(select_dpad, 4);

    bit_getters!(start_down, 3);
    bit_getters!(select_up, 2);
    bit_getters!(b_left, 1);
    bit_getters!(a_right, 0);
}

#[derive(Default)]
pub struct JoypadRegister {
    buttons_selected: bool,
    dpad_selected: bool,
    #[default(bitvec!(u8, Lsb0; 1; 4))]
    pub buttons_state: BitVec<u8>,
    #[default(bitvec!(u8, Lsb0; 1; 4))]
    pub dpad_state: BitVec<u8>,
}

impl JoypadRegister {
    pub(crate) fn read(&self) -> u8 {
        let state = if !self.buttons_selected {
            self.buttons_state.clone()
        } else {
            bitvec![ u8, Lsb0; 1; 4]
        } & if !self.dpad_selected {
            self.dpad_state.clone()
        } else {
            bitvec![u8, Lsb0; 1; 4]
        };

        let mut selected = BitVec::new();
        selected.push(self.dpad_selected);
        selected.push(self.buttons_selected);

        let mut return_value = bitvec![u8, Lsb0; 1; 8];
        return_value[4..6].copy_from_bitslice(&selected);
        return_value[0..4].copy_from_bitslice(&state);

        return_value.load::<u8>()
    }

    pub(crate) fn write(&mut self, value: u8) {
        self.buttons_selected = 0b1 & value >> 5 == 0b1;
        self.dpad_selected = 0b1 & value >> 4 == 0b1;
    }
}

#[derive(Default)]
pub struct SerialTransferRegisters;

impl SerialTransferRegisters {
    pub(crate) fn read(&self, _address: u8) -> u8 {
        // todo!()
        0xFF
    }

    pub(crate) fn write(&mut self, _address: u8, _value: u8) {
        //todo!()
    }
}

#[derive(Default)]
pub struct Timer {
    pub(crate) sc: u16,
    pub(crate) tima: u8,
    pub(crate) tma: u8,
    pub(crate) tac: u8,
    pub(crate) tima_overflow: Option<u8>,
    pub(crate) tima_written: bool,
}

impl Timer {
    pub(crate) fn read(&self, address: u8) -> u8 {
        match address {
            0x00..=0x03 => unreachable!(),
            0x04 => (self.sc >> 6) as u8,
            0x05 => self.tima,
            0x06 => self.tma,
            0x07 => self.tac,
            0x08.. => unreachable!(),
        }
    }

    pub(crate) fn write(&mut self, address: u8, value: u8) {
        match address {
            0x00..=0x03 => unreachable!(),
            0x04 => {
                let tac_enable = self.tac >> 2 & 0b1 == 0b1;
                let selected_bit = match self.tac & 0b11 {
                    0b00 => 8,
                    0b01 => 2,
                    0b10 => 4,
                    0b11 => 6,
                    _ => unreachable!(),
                };
                let sc_bit_prev = self.sc >> (selected_bit - 1) & 0b1 == 1;
                self.sc = 0;
                if sc_bit_prev && tac_enable {
                    self.timer_tick();
                }
            }
            0x05 => {
                self.tima_written = true;
                self.tima = value;
            }
            0x06 => {
                self.tma = value;
            }
            0x07 => {
                let tac_enable = self.tac >> 2 & 0b1 == 0b1;
                let selected_bit = match self.tac & 0b11 {
                    0b00 => 8,
                    0b01 => 2,
                    0b10 => 4,
                    0b11 => 6,
                    _ => unreachable!(),
                };
                let sc_bit_prev = self.sc >> (selected_bit - 1) & 0b1 == 1;
                self.tac = value;
                let selected_bit = match self.tac & 0b11 {
                    0b00 => 8,
                    0b01 => 2,
                    0b10 => 4,
                    0b11 => 6,
                    _ => unreachable!(),
                };
                let sc_bit_after = self.sc >> (selected_bit - 1) & 0b1 == 1;
                if sc_bit_prev && !sc_bit_after && tac_enable {
                    self.timer_tick();
                }
            }
            0x08.. => unreachable!(),
        };
    }

    pub(crate) fn timer_tick(&mut self) {
        let (result, overflow) = self.tima.overflowing_add(1);
        self.tima = result;
        if overflow && !self.tima_written {
            self.tima_overflow = Some(self.tma);
        }
    }

    pub(crate) fn clock_tick(&mut self) {
        let tac_enable = self.tac >> 2 & 0b1 == 0b1;
        let selected_bit = match self.tac & 0b11 {
            0b00 => 8,
            0b01 => 2,
            0b10 => 4,
            0b11 => 6,
            _ => unreachable!(),
        };
        let sc_bit_prev = self.sc >> (selected_bit - 1) & 0b1 == 1;
        self.sc = self.sc.wrapping_add(1);
        let sc_bit_new = self.sc >> (selected_bit - 1) & 0b1 == 1;
        if sc_bit_prev && !sc_bit_new && tac_enable {
            self.timer_tick();
        }
    }

    pub(crate) fn handle_overflow(&mut self, tma: u8, interrupts: &mut impl InterruptRegister) {
        self.tima = tma;

        interrupts.schedule_interrupt(InterruptType::Timer);
    }
}

impl TimerRegisters for Timer {
    fn tick(&mut self, interrupts: &mut impl InterruptRegister) {
        self.tima_written = false;
        let tima_overflow = self.tima_overflow;
        self.tima_overflow = None;
        self.clock_tick();
        if let Some(tma) = tima_overflow {
            self.handle_overflow(tma, interrupts);
        }
    }
}

#[derive(Default)]
pub struct InterruptFlag {
    pub(crate) interrupt_flag: u8,
}

#[derive(Debug, Copy, Clone)]
#[repr(u8)]
pub enum InterruptType {
    Joypad = 4,
    Serial = 3,
    Timer = 2,
    LCD = 1,
    VBlank = 0,
}

impl InterruptRegister for InterruptFlag {
    fn read(&self) -> u8 {
        self.interrupt_flag
    }
    fn write(&mut self, value: u8) {
        self.interrupt_flag = value;
    }

    fn schedule_interrupt(&mut self, interrupt: InterruptType) {
        self.interrupt_flag |= 1 << (interrupt as u8);
    }

    fn clear_interrupt(&mut self, interrupt: InterruptType) {
        self.interrupt_flag &= !(1 << (interrupt as u8));
    }
}

#[allow(unused)]
fn unimplemented_io_read(address: u8) -> u8 {
    0xFF
}

#[allow(unused)]
fn unimplemented_io_write(address: u8, value: u8) {}

impl IoRegisters {
    pub(crate) fn read_u8(&self, address: u8) -> u8 {
        match address {
            0x00 => self.joypad.read(),
            0x01..=0x02 => self.serial.read(address),
            0x03 => unimplemented_io_read(address),
            0x04..=0x07 => self.timer.read(address),
            0x08..0x0F => unimplemented_io_read(address),
            0x0F => self.interrupt.read(),
            0x10..=0x26 => unimplemented_io_read(address),
            0x27..0x30 => unimplemented_io_read(address),
            0x30..=0x3F => unimplemented_io_read(address),
            0x40..=0x4B => self.lcd.read(address),

            0x50 => {
                // Bootrom bank control, write-only
                unimplemented_io_read(address)
            }
            0x80.. => unreachable!(),
            _ => {
                debug!("CGB/unused IO address: {address}");
                unimplemented_io_read(address)
            }
        }
    }

    pub(crate) fn write_u8(&mut self, address: u8, value: u8) {
        match address {
            0x00 => self.joypad.write(value),
            0x01..=0x02 => self.serial.write(address, value),
            0x03 => unimplemented_io_write(address, value),
            0x04..=0x07 => self.timer.write(address, value),
            0x08..0x0F => unimplemented_io_write(address, value),
            0x0F => self.interrupt.write(value),
            0x10..=0x26 => unimplemented_io_write(address, value),
            0x27..0x30 => unimplemented_io_write(address, value),
            0x30..=0x3F => unimplemented_io_write(address, value),
            0x40..=0x4B => self.lcd.write(address, value),

            0x50 => {
                // Bootrom bank control, write-only
                unimplemented_io_write(address, value)
            }
            0x80.. => unreachable!(),
            _ => {
                debug!("CGB/unused IO address: {address}");
                unimplemented_io_write(address, value)
            }
        }
    }
}

#[derive(Default)]
pub struct MemoryBus {
    #[default([0; 1024*16])]
    pub(crate) rom: Memory16K,
    #[default(vec![[0; 1024*16]])]
    pub(crate) rom_banks: Vec<Memory16K>,
    pub vram: Vram,
    #[default([0; 1024 * 8])]
    pub(crate) external_ram: Memory8K,
    #[default([0; 1024 * 4])]
    pub(crate) wram1: Memory4K,
    #[default([0; 1024 * 4])]
    pub(crate) wram2: Memory4K,
    pub(crate) oam: Oam,
    pub io: IoRegisters,
    #[default([0; 0xFFFF-0xFF80])]
    pub(crate) hram: [u8; 0xFFFF - 0xFF80],
    pub(crate) ie: u8,
}

impl MemoryBus {
    pub fn read_u8(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x3FFF => self.rom[address as usize],
            0x4000..=0x7FFF => {
                // TODO: switchable rom banks
                self.rom_banks[0][address as usize - 0x4000]
            }
            0x8000..=0x9FFF => self.vram[address as usize - 0x8000],
            0xA000..=0xBFFF => self.external_ram[address as usize - 0xA000],
            0xC000..=0xCFFF => self.wram1[address as usize - 0xC000],
            0xD000..=0xDFFF => self.wram2[address as usize - 0xD000],
            0xE000..=0xEFFF => {
                //Echo RAM
                self.wram1[address as usize - 0xE000]
            }
            0xF000..=0xFDFF => {
                //Echo RAM 2
                self.wram2[address as usize - 0xF000]
            }
            0xFE00..=0xFE9F => self.oam[address as usize - 0xFE00],
            0xFEA0..=0xFEFF => {
                0x00 // Prohibited Region, on DMG reads return $00
            }
            0xFF00..=0xFF7F => self.io.read_u8(address as u8),
            0xFF80..=0xFFFE => self.hram[address as usize - 0xFF80],
            0xFFFF => self.ie,
        }
    }

    pub fn write_u8(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x3FFF => {
                println!(
                    "Attempted to write to ROM at address 0x{address:04X} with value 0x{value:02X}"
                )
            } //self.rom[address as usize] = value,
            0x4000..=0x7FFF => {
                // TODO: switchable rom banks
                //self.rom_banks[0][address as usize - 0x4000] = value
                println!(
                    "Attempted to write to ROM at address 0x{address:04X} with value 0x{value:02X}"
                )
            }
            0x8000..=0x9FFF => self.vram[address as usize - 0x8000] = value,
            0xA000..=0xBFFF => self.external_ram[address as usize - 0xA000] = value,
            0xC000..=0xCFFF => self.wram1[address as usize - 0xC000] = value,
            0xD000..=0xDFFF => self.wram2[address as usize - 0xD000] = value,
            0xE000..=0xEFFF => {
                //Echo RAM
                self.wram1[address as usize - 0xE000] = value
            }
            0xF000..=0xFDFF => {
                //Echo RAM 2
                self.wram2[address as usize - 0xF000] = value
            }
            0xFE00..=0xFE9F => self.oam[address as usize - 0xFE00] = value,
            0xFEA0..=0xFEFF => {
                // todo!("Prohibited region, implement undefined behaviour")
            }
            0xFF00..=0xFF7F => {
                self.io.write_u8(address as u8, value);
            }
            0xFF80..=0xFFFE => self.hram[address as usize - 0xFF80] = value,
            0xFFFF => self.ie = value,
        }
    }
}

#[derive(Default)]
pub struct Context<M: Memory + Default> {
    pub memory: M,
}

pub trait Memory {
    fn read_u8(&self, address: u16) -> u8;
    fn write_u8(&mut self, address: u16, value: u8);

    fn io(&self) -> &impl Io;
    fn io_mut(&mut self) -> &mut impl Io;

    fn ie(&self) -> &u8;
    fn ie_mut(&mut self) -> &mut u8;

    fn load_boot_rom(&mut self, rom: &[u8]);
    fn load_rom(&mut self, rom: &[u8]);

    fn tick_oam_dma(&mut self);
}

impl Memory for MemoryBus {
    fn read_u8(&self, address: u16) -> u8 {
        self.read_u8(address)
    }

    fn write_u8(&mut self, address: u16, value: u8) {
        self.write_u8(address, value);
    }

    fn io(&self) -> &impl Io {
        &self.io
    }

    fn io_mut(&mut self) -> &mut impl Io {
        &mut self.io
    }

    fn ie(&self) -> &u8 {
        &self.ie
    }

    fn ie_mut(&mut self) -> &mut u8 {
        &mut self.ie
    }

    fn load_boot_rom(&mut self, rom: &[u8]) {
        self.rom[..rom.len()].copy_from_slice(rom);
    }

    fn load_rom(&mut self, rom: &[u8]) {
        self.rom.copy_from_slice(&rom[..1024 * 16]);
        self.rom_banks[0].copy_from_slice(&rom[1024 * 16..]);
    }

    fn tick_oam_dma(&mut self) {
        self.io.lcd.dma_counter = if let Some(counter) = self.io.lcd.dma_counter {
            if counter > 0 {
                let offset = counter - 1;

                let source_address = self.io.lcd.dma_source_address;
                let value = self.read_u8(u16::from_le_bytes([offset, source_address]));
                self.write_u8(u16::from_le_bytes([offset, 0xFE]), value);
            }
            if counter == 159 {
                None
            } else {
                Some(counter + 1)
            }
        } else {
            None
        };
    }
}

pub trait Io {
    fn timer(&self) -> &impl TimerRegisters;
    fn timer_mut(&mut self) -> &mut impl TimerRegisters;

    fn interrupt_flag(&self) -> &impl InterruptRegister;
    fn interrupt_flag_mut(&mut self) -> &mut impl InterruptRegister;

    fn tick_timer(&mut self);
}

impl Io for IoRegisters {
    fn timer(&self) -> &impl TimerRegisters {
        &self.timer
    }

    fn timer_mut(&mut self) -> &mut impl TimerRegisters {
        &mut self.timer
    }

    fn interrupt_flag(&self) -> &impl InterruptRegister {
        &self.interrupt
    }

    fn interrupt_flag_mut(&mut self) -> &mut impl InterruptRegister {
        &mut self.interrupt
    }

    fn tick_timer(&mut self) {
        self.timer.tick(&mut self.interrupt);
    }
}

pub trait TimerRegisters {
    fn tick(&mut self, interrupts: &mut impl InterruptRegister);
}

pub trait InterruptRegister {
    fn read(&self) -> u8;
    fn write(&mut self, value: u8);

    fn schedule_interrupt(&mut self, interrupt: InterruptType);

    fn clear_interrupt(&mut self, interrupt: InterruptType);
}

pub struct FlatMemory([u8; 64 * 1024]);
impl Default for FlatMemory {
    fn default() -> Self {
        Self([0; 64 * 1024])
    }
}
impl Memory for FlatMemory {
    fn read_u8(&self, address: u16) -> u8 {
        self.0[address as usize]
    }

    fn write_u8(&mut self, address: u16, value: u8) {
        self.0[address as usize] = value;
    }

    fn io(&self) -> &impl Io {
        self
    }

    fn io_mut(&mut self) -> &mut impl Io {
        self
    }

    fn ie(&self) -> &u8 {
        &self.0[0xFFFF]
    }

    fn ie_mut(&mut self) -> &mut u8 {
        &mut self.0[0xFFFF]
    }

    fn load_boot_rom(&mut self, _rom: &[u8]) {
        todo!()
    }

    fn load_rom(&mut self, _rom: &[u8]) {
        todo!()
    }

    fn tick_oam_dma(&mut self) {}
}

impl Io for FlatMemory {
    fn timer(&self) -> &impl TimerRegisters {
        self
    }

    fn timer_mut(&mut self) -> &mut impl TimerRegisters {
        self
    }

    fn interrupt_flag(&self) -> &impl InterruptRegister {
        self
    }

    fn interrupt_flag_mut(&mut self) -> &mut impl InterruptRegister {
        self
    }

    fn tick_timer(&mut self) {
        // self.tick(interrupts);
    }
}
impl TimerRegisters for FlatMemory {
    fn tick(&mut self, _interrupts: &mut impl InterruptRegister) {}
}

impl InterruptRegister for FlatMemory {
    fn read(&self) -> u8 {
        self.0[0xFF0F]
    }
    fn write(&mut self, value: u8) {
        self.0[0xFF0F] = value;
    }

    fn schedule_interrupt(&mut self, interrupt: InterruptType) {
        self.0[0xFF0F] |= 1 << (interrupt as u8);
    }

    fn clear_interrupt(&mut self, interrupt: InterruptType) {
        self.0[0xFF0F] &= !(1 << (interrupt as u8));
    }
}
