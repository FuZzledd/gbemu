use core::{cell::Cell, range::Range};
use std::{
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Instant,
};

use better_default::Default;
use bitvec::{access::BitSafe, prelude::*};
use bytemuck::{NoUninit, Pod, TransparentWrapper, from_bytes};
use rgb::Zeroable;
use ringbuf::{StaticRb, traits::RingBuffer};
use spire_enum::prelude::{delegate_impl, delegated_enum};
use strum::FromRepr;
use tap::Pipe;
use time::UtcDateTime;
use tracing::{debug, info, warn};
use unarc_rs::unified::ArchiveFormat::Arc;

use crate::{
    apu::{AudioRegister, AudioRegisters},
    bit_getters,
    ppu::{DmaStatus, LCDRegisters, Oam, Vram},
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
    pub output: StaticRb<u8, 1024>,
    serial_data: u8,
    serial_control: SerialControlRegister,
    transfer_status: u8,
    output_byte: u8,
}

#[repr(transparent)]
#[derive(Default, TransparentWrapper)]
pub struct SerialControlRegister(#[default(0b01111110)] u8);
impl SerialControlRegister {
    bit_getters!(transfer_enable, 7);
    bit_getters!(clock_speed, 1);
    bit_getters!(clock_select, 0);

    fn read(&self) -> u8 {
        self.0 | 0b01111110
    }

    fn write(&mut self, value: u8) {
        self.0 = value | 0b01111110;
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

                serial.output.push_overwrite(serial.output_byte);
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
            0x07 => self.tac | 0b1111_1000,
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
                self.tac = value | 0b1111_1000;
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
        self.interrupt_flag | 0b1110_0000
    }
    fn write(&mut self, value: u8) {
        self.interrupt_flag = value | 0b1110_0000;
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
    info!("Unimplemented read to IO register 0xFF{address:02X}");
    0xFF
}

#[allow(unused)]
fn unimplemented_io_write(address: u8, value: u8) {
    info!("Unimplemented write to IO register 0xFF{address:02X} with value 0x{value:02X}");
}

#[allow(unused)]
fn unimplemented_mem_read(address: u16) -> u8 {
    info!("Unimplemented read to address 0x{address:04X}");
    0xFF
}

#[allow(unused)]
fn unimplemented_mem_write(address: u16, value: u8) {
    info!("Unimplemented write to to address 0x{address:04X} with value 0x{value:02X}");
}

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
    MBC3(Mbc3),
}

#[delegate_impl]
impl MemoryBankController {
    pub fn read_u8(&self, address: u16) -> u8;
    pub fn write_u8(&mut self, address: u16, value: u8);
    pub fn load_save(&mut self, save_file: impl AsRef<[u8]>);
}

impl MemoryBankController {
    pub fn dump_save(&mut self) -> Option<Vec<u8>> {
        delegate_memory_bank_controller! {
            self.dump_save().map(|data| {
                data.as_ref().to_vec()
            })
        }
    }
}

trait Mapper {
    fn read_u8(&self, address: u16) -> u8;

    fn write_u8(&mut self, address: u16, value: u8);

    fn new(rom: &[u8]) -> Self;

    fn dump_save(&mut self) -> Option<impl AsRef<[u8]>> {
        None as Option<&[u8]>
    }

    fn load_save(&mut self, _save_file: impl AsRef<[u8]>) {}
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

impl Mapper for Mbc0 {
    fn read_u8(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x3FFF => self.rom[address as usize],
            0x4000..=0x7FFF => self.rom2[address as usize - 0x4000],
            0xA000..=0xBFFF => self.external_ram[address as usize - 0xA000],
            _ => unreachable!(),
        }
    }

    fn write_u8(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x3FFF => {
                warn!(
                    "Attempted to write to ROM at address 0x{address:04X} with value 0x{value:02X}"
                );
                // let _ = PLAYBACK_CONTROLLER.0.send(false);
            } //self.rom[address as usize] = value,
            0x4000..=0x7FFF => {
                // TODO: switchable rom banks
                //self.rom_banks[0][address as usize - 0x4000] = value
                warn!(
                    "Attempted to write to ROM at address 0x{address:04X} with value 0x{value:02X}"
                );
                // let _ = PLAYBACK_CONTROLLER.0.send(false);
            }
            0xA000..=0xBFFF => self.external_ram[address as usize - 0xA000] = value,
            _ => unreachable!(),
        }
    }

    fn new(rom: &[u8]) -> Self {
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

impl Mapper for Mbc1 {
    fn read_u8(&self, address: u16) -> u8 {
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
    fn write_u8(&mut self, address: u16, value: u8) {
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

    fn new(rom: &[u8]) -> Self {
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

    fn dump_save(&mut self) -> Option<impl AsRef<[u8]>> {
        Some(self.ram_banks.concat())
    }

    fn load_save(&mut self, save_file: impl AsRef<[u8]>) {
        self.ram_banks = save_file.as_ref().as_chunks::<{ 1024 * 8 }>().0.to_vec();
    }
}

#[derive(Default)]
pub struct Mbc3 {
    #[default(vec![[0; 1024*16]])]
    pub(crate) rom_banks: Vec<Memory16K>,
    #[default(vec![[0; 1024 * 8]])]
    pub(crate) ram_banks: Vec<Memory8K>,

    pub ram_enable: bool,
    pub rom_bank: usize,
    pub ram_bank_and_rtc_register: usize,
    pub rtc_latched: bool,
    pub latch_progress: bool,

    pub seconds_set: Cell<u64>,
    pub minutes_set: Cell<u64>,
    pub hours_set: Cell<u64>,
    pub days_set: Cell<u64>,

    pub seconds_latched: u64,
    pub minutes_latched: u64,
    pub hours_latched: u64,
    pub days_latched: u64,

    #[default(UtcDateTime::now().into())]
    pub time_last_accessed: Cell<UtcDateTime>,

    pub day_overflow: Cell<bool>,
    pub day_overflow_latched: bool,
    pub timer_halt: bool,
}

impl Mapper for Mbc3 {
    fn read_u8(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x3FFF => self.rom_banks[0][address as usize],
            0x4000..=0x7FFF => {
                let bank = self.rom_bank.max(1) % self.rom_banks.len();
                self.rom_banks[bank][address as usize - 0x4000]
            }
            0xA000..=0xBFFF => {
                if self.ram_enable {
                    match self.ram_bank_and_rtc_register {
                        0x00..=0x07 => {
                            if self.ram_bank_and_rtc_register < self.ram_banks.len() {
                                self.ram_banks
                                    [self.ram_bank_and_rtc_register % self.ram_banks.len()]
                                    [address as usize - 0xA000]
                            } else {
                                0xFF
                            }
                        }
                        register @ 0x08..=0x0C => {
                            self.update_time();
                            (if self.rtc_latched {
                                match register {
                                    ..0x08 | 0x0D.. => unreachable!(),
                                    0x08 => self.seconds_latched,
                                    0x09 => self.minutes_latched,
                                    0x0A => self.hours_latched,
                                    0x0B => self.days_latched & 0b1111_1111,
                                    0x0C => {
                                        (self.days_latched >> 8) & 0b1
                                            | ((self.timer_halt as u64) << 6)
                                            | ((self.day_overflow_latched as u64) << 7)
                                    }
                                }
                            } else {
                                match register {
                                    ..0x08 | 0x0D.. => unreachable!(),
                                    0x08 => self.seconds_set.get(),
                                    0x09 => self.minutes_set.get(),
                                    0x0A => self.hours_set.get(),
                                    0x0B => self.days_set.get() & 0b1111_1111,
                                    0x0C => {
                                        (self.days_set.get() >> 8) & 0b1
                                            | ((self.timer_halt as u64) << 6)
                                            | ((self.day_overflow.get() as u64) << 7)
                                    }
                                }
                            }) as u8
                        }
                        0x0D.. => unimplemented_mem_read(address),
                    }
                } else {
                    0xFF
                }
            }
            _ => unreachable!(),
        }
    }
    fn write_u8(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => self.ram_enable = value & 0b1111 == 0xA,
            0x2000..=0x3FFF => self.rom_bank = value as usize & 0b1111111,
            0x4000..=0x5FFF => self.ram_bank_and_rtc_register = value as usize & 0b1111,
            0x6000..=0x7FFF => match value {
                0x00 => self.latch_progress = true,
                0x01 if self.latch_progress => {
                    self.rtc_latched = !self.rtc_latched;
                    if self.rtc_latched {
                        self.update_time();

                        self.seconds_latched = self.seconds_set.get();
                        self.minutes_latched = self.minutes_set.get();
                        self.hours_latched = self.hours_set.get();
                        self.days_latched = self.days_set.get();
                        self.day_overflow_latched = self.day_overflow.get();
                    }
                    self.latch_progress = false;
                }
                _ => self.rtc_latched = false,
            },
            0xA000..=0xBFFF => {
                if self.ram_enable {
                    match self.ram_bank_and_rtc_register {
                        0x00..=0x07 => {
                            if self.ram_bank_and_rtc_register < self.ram_banks.len() {
                                let bank = self.ram_bank_and_rtc_register % self.ram_banks.len();
                                self.ram_banks[bank][address as usize - 0xA000] = value;
                            }
                        }
                        register @ 0x08..=0x0C => {
                            self.update_time();

                            match register {
                                0x08 => self.seconds_set.set(value as u64 % 60),
                                0x09 => self.minutes_set.set(value as u64 % 60),
                                0x0A => self.hours_set.set(value as u64 % 24),
                                0x0B => self
                                    .days_set
                                    .update(|days_set| days_set & !0b1111_1111 | value as u64),
                                0x0C => {
                                    self.days_set.update(|days_set| {
                                        days_set & !(1 << 8) | (value as u64 & 0b1) << 8
                                    });
                                    self.timer_halt = value & (1 << 6) != 0;
                                    self.day_overflow.set(value & (1 << 7) != 0);
                                }
                                ..0x08 | 0x0D.. => unreachable!(),
                            }
                        }
                        0x0D.. => unimplemented_mem_write(address, value),
                    }
                }
            }
            _ => unreachable!("Wrote to address 0x{address:04X}"),
        }
    }

    fn new(rom: &[u8]) -> Self {
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
            0x04 => 16,
            0x05 => 8,
            _ => unimplemented!(),
        };

        Self {
            rom_banks: Vec::from(rom[0..rom_banks * 1024 * 16].as_chunks().0),
            ram_banks: vec![[0; 1024 * 8]; ram_banks],
            ..Default::default()
        }
    }

    fn load_save(&mut self, save_file: impl AsRef<[u8]>) {
        let bytes = save_file.as_ref();
        let (ram, rest) = bytes.as_chunks::<{ 1024 * 8 }>();

        self.ram_banks = ram.to_vec();
        let rtc_data = if rest.len() == 44 {
            let rtc_data: &RtcData<u32> = from_bytes(rest);
            RtcData {
                time_days: rtc_data.time_days,
                time_days_high: rtc_data.time_days_high,
                timestamp: rtc_data.timestamp as u64,
                time_seconds: rtc_data.time_seconds,
                time_minutes: rtc_data.time_minutes,
                time_hours: rtc_data.time_hours,
                latched_time_seconds: rtc_data.latched_time_seconds,
                latched_time_minutes: rtc_data.latched_time_minutes,
                latched_time_hours: rtc_data.latched_time_hours,
                latched_time_days: rtc_data.latched_time_days,
                latched_time_days_high: rtc_data.latched_time_days_high,
            }
        } else {
            *from_bytes(rest)
        };

        self.seconds_set.set(rtc_data.time_seconds as u64);
        self.minutes_set.set(rtc_data.time_minutes as u64);
        self.hours_set.set(rtc_data.time_hours as u64);
        self.days_set
            .set((rtc_data.time_days | (rtc_data.time_days_high & 0b1) << 8) as u64);
        self.day_overflow
            .set(rtc_data.time_days_high & (1 << 7) != 0);

        self.timer_halt = rtc_data.time_days_high & (1 << 6) != 0;

        self.seconds_latched = rtc_data.latched_time_seconds as u64;
        self.minutes_latched = rtc_data.latched_time_minutes as u64;
        self.hours_latched = rtc_data.latched_time_hours as u64;
        self.days_latched =
            (rtc_data.latched_time_days | (rtc_data.latched_time_days_high & 0b1) << 8) as u64;
        self.day_overflow_latched = rtc_data.latched_time_days_high & (1 << 7) != 0;
    }

    fn dump_save(&mut self) -> Option<impl AsRef<[u8]>> {
        self.update_time();
        let mut save = self.ram_banks.concat();
        save.extend_from_slice(bytemuck::bytes_of(&self.rtc_time_stamp()));
        Some(save)
    }
}

impl Mbc3 {
    fn update_time(&self) {
        if !self.timer_halt {
            self.seconds_set.update(|seconds| {
                seconds.wrapping_add(
                    (UtcDateTime::now() - self.time_last_accessed.get())
                        .whole_seconds()
                        .unsigned_abs(),
                ) % 60
            });
            self.minutes_set.update(|minutes| {
                minutes.wrapping_add(
                    (UtcDateTime::now() - self.time_last_accessed.get())
                        .whole_minutes()
                        .unsigned_abs(),
                ) % 60
            });
            self.hours_set.update(|hours| {
                hours.wrapping_add(
                    (UtcDateTime::now() - self.time_last_accessed.get())
                        .whole_hours()
                        .unsigned_abs(),
                ) % 24
            });

            self.days_set.update(|days| {
                let days_set = days.wrapping_add(
                    (UtcDateTime::now() - self.time_last_accessed.get())
                        .whole_days()
                        .unsigned_abs(),
                );
                if days_set >= 512 {
                    self.day_overflow.set(true);
                }
                days_set % 512
            });
        }

        self.time_last_accessed.set(UtcDateTime::now());
    }

    fn rtc_time_stamp(&self) -> RtcData<u64> {
        RtcData {
            time_seconds: self.seconds_set.get() as u32,
            time_minutes: self.minutes_set.get() as u32,
            time_hours: self.hours_set.get() as u32,
            time_days: self.days_set.get() as u32 & 0b1111_1111,
            time_days_high: ((self.days_set.get() >> 8) & 0b1
                | ((self.timer_halt as u64) << 6)
                | ((self.day_overflow.get() as u64) << 7)) as u32,
            latched_time_seconds: self.seconds_latched as u32,
            latched_time_minutes: self.minutes_latched as u32,
            latched_time_hours: self.hours_latched as u32,
            latched_time_days: self.days_latched as u32 & 0b1111_1111,
            latched_time_days_high: ((self.days_latched >> 8) & 0b1
                | ((self.timer_halt as u64) << 6)
                | ((self.day_overflow_latched as u64) << 7))
                as u32,
            timestamp: self.time_last_accessed.get().unix_timestamp() as u64,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Zeroable, Pod)]
#[repr(C, packed)]
struct RtcData<T>
where
    T: Default,
{
    time_seconds: u32,
    time_minutes: u32,
    time_hours: u32,
    time_days: u32,
    time_days_high: u32,
    latched_time_seconds: u32,
    latched_time_minutes: u32,
    latched_time_hours: u32,
    latched_time_days: u32,
    latched_time_days_high: u32,
    timestamp: T,
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

impl MemoryBus {}

#[derive(Default)]
pub struct Context<M: Memory + Default = MemoryBus> {
    pub memory: M,
    pub rom_info: Option<RomInfo>,
}

impl Context<MemoryBus> {
    pub fn load_rom(&mut self, path: impl AsRef<Path>) {
        let rom = if let Some(rom) = unarc_rs::unified::ArchiveFormat::open_path(&path)
            .ok()
            .and_then(|mut archive| {
                archive.set_single_file_name(
                    path.as_ref()
                        .file_prefix()
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
                );
                archive
                    .entries_iter()
                    .map(Result::unwrap)
                    .find(|entry| {
                        entry.file_name().to_lowercase().pipe(|file_name| {
                            file_name.ends_with(".gb") || file_name.ends_with(".gbc")
                        })
                    })
                    .map(|rom_entry| archive.read(&rom_entry).unwrap())
            }) {
            rom
        } else if let Some(rom) = png_achunk::Decoder::from_file(&path)
            .ok()
            .and_then(|mut decoder| decoder.decode_ancillary_chunks().ok())
            .and_then(|chunks| {
                chunks
                    .into_iter()
                    .find(|chunk| chunk.chunk_type.to_ascii() == "gbRM")
                    .map(|chunk| chunk.data)
            })
        {
            rom
        } else {
            //Not an archive, just read as a plain ROM file
            fs::read(&path).unwrap()
        };

        let rom_info = RomInfo::new(&rom, path, self);
        self.memory.load_rom(rom);

        if let Ok(save_file) = fs::read(&rom_info.save_path) {
            self.memory.mapper.load_save(save_file);
        }
        self.rom_info = Some(rom_info);
    }

    pub fn save(&mut self) -> io::Result<()> {
        if let Some(save) = self.memory.mapper.dump_save() {
            fs::write(&self.rom_info.as_ref().unwrap().save_path, save)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RomInfo {
    title: String,
    save_path: PathBuf,
    file_name: OsString,
    header_checksum: u8,
    global_checksum: u16,
    data: Vec<u8>,
}

impl RomInfo {
    fn new(data: impl AsRef<[u8]>, path: impl AsRef<Path>, _context: &Context) -> Self {
        let data = data.as_ref();
        let path = path.as_ref();

        let title = String::from_utf8_lossy(&data[0x134..0x143])
            .trim_end_matches('\0')
            .to_string();

        let file_name = path.file_prefix().unwrap().to_os_string();

        let header_checksum = data[0x014D];

        let global_checksum = u16::from_be_bytes(data[0x014E..=0x014F].try_into().unwrap());

        //TODO: Query settings for save path
        let save_path = path.with_extension("sav");

        RomInfo {
            title,
            save_path,
            file_name,
            header_checksum,
            global_checksum,
            data: data.to_vec(),
        }
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
pub enum MemoryAccessSource {
    #[default]
    Default,
    Oam,
}

pub trait Memory {
    fn read_u8(&self, address: u16) -> u8 {
        self.read_u8_with_source(address, MemoryAccessSource::Default)
    }
    fn write_u8(&mut self, address: u16, value: u8) {
        self.write_u8_with_source(address, value, MemoryAccessSource::Default);
    }

    fn read_u8_with_source(&self, address: u16, source: MemoryAccessSource) -> u8;

    fn write_u8_with_source(&mut self, address: u16, value: u8, source: MemoryAccessSource);

    fn io(&self) -> &impl Io;
    fn io_mut(&mut self) -> &mut impl Io;

    fn ie(&self) -> &u8;
    fn ie_mut(&mut self) -> &mut u8;

    fn load_boot_rom(&mut self, rom: &[u8]);
    fn load_rom(&mut self, rom: impl AsRef<[u8]>);

    fn tick_oam_dma(&mut self);
}

impl Memory for MemoryBus {
    fn read_u8_with_source(&self, address: u16, source: MemoryAccessSource) -> u8 {
        if !matches!(source, MemoryAccessSource::Oam)
            && matches!(self.io.lcd.dma_counter, DmaStatus::Running(_))
            && (matches!(
                (self.io.lcd.dma_source_address, address),
                (0x80..=0x9F, 0x8000..=0x9FFF)
            ) || matches!(
                (self.io.lcd.dma_source_address, address),
                (0x00..=0x7F | 0xC0..=0xFE, 0x0000..=0x7FFF | 0xC000..=0xFEFF)
            ))
        {
            return 0xFF;
        }
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

    fn write_u8_with_source(&mut self, address: u16, value: u8, source: MemoryAccessSource) {
        if !matches!(source, MemoryAccessSource::Oam)
            && matches!(self.io.lcd.dma_counter, DmaStatus::Running(_))
            && (matches!(
                (self.io.lcd.dma_source_address, address),
                (0x80..=0x9F, 0x8000..=0x9FFF)
            ) || matches!(
                (self.io.lcd.dma_source_address, address),
                (0x00..=0x7F | 0xC0..=0xFE, 0x0000..=0x7FFF | 0xC000..=0xFEFF)
            ))
        {
            return;
        }
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

    fn load_boot_rom(&mut self, _rom: &[u8]) {
        // self.rom[..rom.len()].copy_from_slice(rom);
    }

    fn load_rom(&mut self, rom: impl AsRef<[u8]>) {
        let rom = rom.as_ref();
        self.mapper = match rom[0x147] {
            0x00 => Mbc0::new(rom).into(),
            0x01..=0x03 => Mbc1::new(rom).into(),
            0x0F..=0x13 => Mbc3::new(rom).into(),
            _ => unimplemented!(),
        }
    }

    fn tick_oam_dma(&mut self) {
        match self.io.lcd.dma_counter {
            DmaStatus::Queued { address } => {
                self.io.lcd.dma_counter = DmaStatus::Running(Range::from(0..160).into_iter());
                self.io.lcd.dma_source_address = address;
            }
            DmaStatus::Running(ref mut counter) => {
                if let Some(offset) = counter.next() {
                    let len = counter.len();
                    let source_address = self.io.lcd.dma_source_address;
                    let value = self.read_u8_with_source(
                        u16::from_le_bytes([offset, source_address]),
                        MemoryAccessSource::Oam,
                    );
                    self.write_u8_with_source(
                        u16::from_le_bytes([offset, 0xFE]),
                        value,
                        MemoryAccessSource::Oam,
                    );
                    if len == 0 {
                        self.io.lcd.dma_counter = DmaStatus::Done;
                    }
                }
            }
            DmaStatus::Done => {}
        }

        if let Some(address) = self.io.lcd.dma_request.take() {
            self.io.lcd.dma_counter = DmaStatus::Queued { address };
        }
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

    fn read_u8_with_source(&self, _address: u16, _source: MemoryAccessSource) -> u8 {
        todo!()
    }

    fn write_u8_with_source(&mut self, _address: u16, _value: u8, _source: MemoryAccessSource) {
        todo!()
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

    fn load_rom(&mut self, _rom: impl AsRef<[u8]>) {
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

    fn write_u8(&mut self, _address: u8, _value: u8) {
        todo!()
    }

    fn read_u8(&self, _address: u8) -> u8 {
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
