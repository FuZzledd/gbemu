use core::ops::BitAnd;
use std::{
    io::{self, Write},
    process::Output,
};

use better_default::Default;
use bitvec::{access::BitSafe, prelude::*};
use bytemuck::TransparentWrapper;
use serde::de::value;
use spire_enum::prelude::{delegate_impl, delegated_enum};
use strum::FromRepr;
use tracing::debug;

use crate::{
    apu::{AudioRegister, AudioRegisters},
    bit_getters,
    ppu::{LCDRegisters, Oam, Vram},
};

pub(crate) type Memory16K = [u8; 1024 * 16];

pub(crate) type Memory8K = [u8; 1024 * 8];

pub(crate) type Memory4K = [u8; 1024 * 4];

#[derive(Default)]
pub struct IoRegisters {
    pub joypad: JoypadRegister,
    pub serial: Serial,
    pub timer: Timer,
    pub interrupt: InterruptFlag,
    pub lcd: LCDRegisters,
    pub audio: AudioRegisters,
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
pub struct Serial {
    serial_data: u8,
    serial_control: SerialControlRegister,
    transfer_status: u8,
    output_byte: u8,
}

#[repr(transparent)]
#[derive(Default, TransparentWrapper)]
pub struct SerialControlRegister(#[default(0b01111111)] u8);
impl SerialControlRegister {
    bit_getters!(transfer_enable, 7);
    bit_getters!(clock_speed, 1);
    bit_getters!(clock_select, 0);

    fn read(&self) -> u8 {
        self.0 | 0b01111100
    }

    fn write(&mut self, value: u8) {
        self.0 = value | 0b01111100;
    }
}

impl Serial {
    pub fn tick(ctx: &mut Context<MemoryBus>) {
        let serial = &mut ctx.memory.io.serial;
        if serial.serial_control.transfer_enable() && serial.serial_control.clock_select() {
            if serial.transfer_status == 0 {
                serial.output_byte = 0;
            }

            serial.output_byte |= serial.serial_data >> 7;

            if serial.transfer_status == 7 {
                ctx.memory
                    .io
                    .interrupt
                    .schedule_interrupt(InterruptType::Serial);
                serial.serial_control.set_transfer_enable(false);
                print!("{}", char::from_u32(serial.output_byte as u32).unwrap());
                let _ = io::stdout().flush();
            }

            serial.output_byte <<= 1;
            serial.serial_data = (serial.serial_data << 1) | 0b1;
            // TODO: Shift in input data

            serial.transfer_status = (serial.transfer_status + 1) % 8;
        }
    }
}

pub trait SerialRegisters {
    fn read(&self, _address: u8) -> u8;

    fn write(&mut self, _address: u8, _value: u8);
}

impl SerialRegisters for Serial {
    fn read(&self, address: u8) -> u8 {
        match address {
            0x00 => unreachable!(),
            0x01 => self.serial_data,
            0x02 => self.serial_control.read(),
            0x03.. => unreachable!(),
        }
    }

    fn write(&mut self, address: u8, value: u8) {
        match address {
            0x00 => unreachable!(),
            0x01 => self.serial_data = value,
            0x02 => self.serial_control.write(value),
            0x03.. => unreachable!(),
        }
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

impl TimerRegister for Timer {
    fn tick(&mut self, interrupts: &mut impl InterruptRegister) {
        self.tima_written = false;
        let tima_overflow = self.tima_overflow;
        self.tima_overflow = None;
        self.clock_tick();
        if let Some(tma) = tima_overflow {
            self.handle_overflow(tma, interrupts);
        }
    }

    fn div(&self) -> u8 {
        self.read(0x04)
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
            0x10..=0x3F => self.audio.read_u8(address),
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
            0x10..=0x3F => self.audio.write_u8(address, value),
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

#[delegated_enum(impl_conversions)]
#[derive(Default)]
pub enum MemoryBankController {
    #[default]
    MBC0(Mbc0),
    MBC1(Mbc1),
}

#[delegate_impl]
impl MemoryBankController {
    pub fn read_u8(&self, address: u16) -> u8;
    pub fn write_u8(&mut self, address: u16, value: u8);
}

#[derive(Default)]
pub struct Mbc0 {
    #[default([0; 1024*16])]
    pub(crate) rom: Memory16K,
    #[default([0; 1024*16])]
    pub(crate) rom2: Memory16K,
    #[default([0; 1024 * 8])]
    pub(crate) external_ram: Memory8K,
}

impl Mbc0 {
    pub fn read_u8(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x3FFF => self.rom[address as usize],
            0x4000..=0x7FFF => self.rom2[address as usize - 0x4000],
            0xA000..=0xBFFF => self.external_ram[address as usize - 0xA000],
            _ => unreachable!(),
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
            0xA000..=0xBFFF => self.external_ram[address as usize - 0xA000] = value,
            _ => unreachable!(),
        }
    }

    pub fn new(rom: &[u8]) -> Self {
        let mut ret = Self::default();
        ret.rom.copy_from_slice(&rom[..1024 * 16]);
        ret.rom2.copy_from_slice(&rom[1024 * 16..]);

        ret
    }
}

#[derive(Default, FromRepr)]
pub enum BankingMode {
    #[default]
    Simple = 0,
    Advanced = 1,
}

#[derive(Default)]
pub struct Mbc1 {
    #[default(vec![[0; 1024*16]])]
    pub(crate) rom_banks: Vec<Memory16K>,
    #[default(vec![[0; 1024 * 8]])]
    pub(crate) ram_banks: Vec<Memory8K>,

    pub ram_enable: bool,
    pub rom_bank: usize,
    pub ram_bank: usize,
    pub banking_mode: BankingMode,
}

impl Mbc1 {
    pub fn read_u8(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x3FFF => match self.banking_mode {
                BankingMode::Simple => self.rom_banks[0][address as usize],
                BankingMode::Advanced => {
                    let bank = (self.ram_bank << 5) % self.rom_banks.len();
                    self.rom_banks[bank][address as usize]
                }
            },
            0x4000..=0x7FFF => {
                let bank = ((self.ram_bank << 5) + self.rom_bank.max(1)) % self.rom_banks.len();
                self.rom_banks[bank][address as usize - 0x4000]
            }
            0xA000..=0xBFFF => {
                if self.ram_enable && !self.ram_banks.is_empty() {
                    match self.banking_mode {
                        BankingMode::Simple => self.ram_banks[0][address as usize - 0xA000],
                        BankingMode::Advanced => {
                            self.ram_banks[self.ram_bank % self.ram_banks.len()]
                                [address as usize - 0xA000]
                        }
                    }
                } else {
                    0xFF
                }
            }
            _ => unreachable!(),
        }
    }
    pub fn write_u8(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => self.ram_enable = value & 0b1111 == 0xA,
            0x2000..=0x3FFF => self.rom_bank = value as usize & 0b11111,
            0x4000..=0x5FFF => self.ram_bank = value as usize & 0b11,
            0x6000..=0x7FFF => {
                self.banking_mode = BankingMode::from_repr(value as usize & 0b1).unwrap()
            }
            0xA000..=0xBFFF => {
                if self.ram_enable && !self.ram_banks.is_empty() {
                    match self.banking_mode {
                        BankingMode::Simple => self.ram_banks[0][address as usize - 0xA000] = value,
                        BankingMode::Advanced => {
                            let bank = self.ram_bank % self.ram_banks.len();
                            self.ram_banks[bank][address as usize - 0xA000] = value
                        }
                    }
                }
            }
            _ => unreachable!("Wrote to address 0x{address:04X}"),
        }
    }

    pub fn new(rom: &[u8]) -> Self {
        let rom_banks = match rom[0x148] {
            0x00 => 2,
            0x01 => 4,
            0x02 => 8,
            0x03 => 16,
            0x04 => 32,
            0x05 => 64,
            0x06 => 128,
            0x07 => 256,
            0x08 => 512,
            _ => unimplemented!(),
        };

        let ram_banks = match rom[0x149] {
            0x00 => 0,
            0x01 => unimplemented!(),
            0x02 => 1,
            0x03 => 4,
            _ => unimplemented!(),
        };

        Self {
            rom_banks: Vec::from(rom[0..rom_banks * 1024 * 16].as_chunks().0),
            ram_banks: vec![[0; 1024 * 8]; ram_banks],
            ..Default::default()
        }
    }
}

#[derive(Default)]
pub struct MemoryBus {
    pub mapper: MemoryBankController,
    pub vram: Vram,
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
            0x0000..=0x7FFF | 0xA000..=0xBFFF => self.mapper.read_u8(address),
            0x8000..=0x9FFF => self.vram[address as usize - 0x8000],
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
            0x0000..=0x7FFF | 0xA000..=0xBFFF => self.mapper.write_u8(address, value),
            0x8000..=0x9FFF => self.vram[address as usize - 0x8000] = value,
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
        // self.rom[..rom.len()].copy_from_slice(rom);
    }

    fn load_rom(&mut self, rom: &[u8]) {
        self.mapper = match rom[0x147] {
            0x00 => Mbc0::new(rom).into(),
            0x01..=0x03 => Mbc1::new(rom).into(),
            _ => unimplemented!(),
        }
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
    fn serial(&self) -> &impl SerialRegisters;

    fn serial_mut(&mut self) -> &mut impl SerialRegisters;

    fn timer(&self) -> &impl TimerRegister;
    fn timer_mut(&mut self) -> &mut impl TimerRegister;

    fn interrupt_flag(&self) -> &impl InterruptRegister;
    fn interrupt_flag_mut(&mut self) -> &mut impl InterruptRegister;

    fn audio(&self) -> &impl AudioRegister;

    fn audio_mut(&mut self) -> &mut impl AudioRegister;

    fn tick_timer(&mut self);
}

impl Io for IoRegisters {
    fn timer(&self) -> &impl TimerRegister {
        &self.timer
    }

    fn timer_mut(&mut self) -> &mut impl TimerRegister {
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

    fn audio(&self) -> &impl AudioRegister {
        &self.audio
    }

    fn audio_mut(&mut self) -> &mut impl AudioRegister {
        &mut self.audio
    }

    fn serial(&self) -> &impl SerialRegisters {
        &self.serial
    }

    fn serial_mut(&mut self) -> &mut impl SerialRegisters {
        &mut self.serial
    }
}

pub trait TimerRegister {
    fn tick(&mut self, interrupts: &mut impl InterruptRegister);

    fn div(&self) -> u8;
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
    fn timer(&self) -> &impl TimerRegister {
        self
    }

    fn timer_mut(&mut self) -> &mut impl TimerRegister {
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

    fn audio(&self) -> &impl AudioRegister {
        self
    }

    fn audio_mut(&mut self) -> &mut impl AudioRegister {
        self
    }

    fn serial(&self) -> &impl SerialRegisters {
        self
    }

    fn serial_mut(&mut self) -> &mut impl SerialRegisters {
        self
    }
}
impl TimerRegister for FlatMemory {
    fn tick(&mut self, _interrupts: &mut impl InterruptRegister) {}
    fn div(&self) -> u8 {
        self.0[0xFF04]
    }
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

use crate::apu::registers;

impl AudioRegister for FlatMemory {
    fn nr10_mut(&mut self) -> &mut registers::ChannelSweep {
        todo!()
    }

    fn nr11_mut(&mut self) -> &mut registers::ChannelLengthTimerWithDuty {
        todo!()
    }

    fn nr12_mut(&mut self) -> &mut registers::ChannelVolumeEnvelope {
        todo!()
    }

    fn nr13_14_mut(&mut self) -> &mut registers::ChannelPeriodControl {
        todo!()
    }

    fn nr21_mut(&mut self) -> &mut registers::ChannelLengthTimerWithDuty {
        todo!()
    }

    fn nr22_mut(&mut self) -> &mut registers::ChannelVolumeEnvelope {
        todo!()
    }

    fn nr23_24_mut(&mut self) -> &mut registers::ChannelPeriodControl {
        todo!()
    }

    fn nr30_mut(&mut self) -> &mut registers::ChannelDacEnable {
        todo!()
    }

    fn nr31_mut(&mut self) -> &mut registers::ChannelLengthTimer {
        todo!()
    }

    fn nr32_mut(&mut self) -> &mut registers::ChannelVolume {
        todo!()
    }

    fn nr33_34_mut(&mut self) -> &mut registers::ChannelPeriodControl {
        todo!()
    }

    fn nr41_mut(&mut self) -> &mut registers::ChannelLengthTimer {
        todo!()
    }

    fn nr42_mut(&mut self) -> &mut registers::ChannelVolumeEnvelope {
        todo!()
    }

    fn nr43_mut(&mut self) -> &mut registers::ChannelFrequencyRandomness {
        todo!()
    }

    fn nr44_mut(&mut self) -> &mut registers::ChannelControl {
        todo!()
    }

    fn nr50_mut(&mut self) -> &mut registers::AudioVolume {
        todo!()
    }

    fn nr51_mut(&mut self) -> &mut registers::AudioPanning {
        todo!()
    }

    fn nr52_mut(&mut self) -> &mut registers::AudioEnable {
        todo!()
    }

    fn nr10(&self) -> &registers::ChannelSweep {
        todo!()
    }

    fn nr11(&self) -> &registers::ChannelLengthTimerWithDuty {
        todo!()
    }

    fn nr12(&self) -> &registers::ChannelVolumeEnvelope {
        todo!()
    }

    fn nr13_14(&self) -> &registers::ChannelPeriodControl {
        todo!()
    }

    fn nr21(&self) -> &registers::ChannelLengthTimerWithDuty {
        todo!()
    }

    fn nr22(&self) -> &registers::ChannelVolumeEnvelope {
        todo!()
    }

    fn nr23_24(&self) -> &registers::ChannelPeriodControl {
        todo!()
    }

    fn nr30(&self) -> &registers::ChannelDacEnable {
        todo!()
    }

    fn nr31(&self) -> &registers::ChannelLengthTimer {
        todo!()
    }

    fn nr32(&self) -> &registers::ChannelVolume {
        todo!()
    }

    fn nr33_34(&self) -> &registers::ChannelPeriodControl {
        todo!()
    }

    fn nr41(&self) -> &registers::ChannelLengthTimer {
        todo!()
    }

    fn nr42(&self) -> &registers::ChannelVolumeEnvelope {
        todo!()
    }

    fn nr43(&self) -> &registers::ChannelFrequencyRandomness {
        todo!()
    }

    fn nr44(&self) -> &registers::ChannelControl {
        todo!()
    }

    fn nr50(&self) -> &registers::AudioVolume {
        todo!()
    }

    fn nr51(&self) -> &registers::AudioPanning {
        todo!()
    }

    fn nr52(&self) -> &registers::AudioEnable {
        todo!()
    }

    fn wave_pattern_ram(&self) -> &[u8; 16] {
        todo!()
    }

    fn wave_pattern_ram_mut(&mut self) -> &mut [u8; 16] {
        todo!()
    }

    fn write_u8(&mut self, address: u8, value: u8) {
        todo!()
    }

    fn read_u8(&self, address: u8) -> u8 {
        todo!()
    }
}

impl SerialRegisters for FlatMemory {
    fn read(&self, _address: u8) -> u8 {
        todo!()
    }

    fn write(&mut self, _address: u8, _value: u8) {
        todo!()
    }
}
