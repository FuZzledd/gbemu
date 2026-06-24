use core::{array, cmp};
use std::collections::VecDeque;

use crate::{
    apu::registers::ChannelsEnabled,
    context::{Context, Memory, MemoryBus},
};
use crate::{
    apu::registers::EnvelopeDirection,
    context::{Io, TimerRegister},
};
use better_default::Default;
use bitvec::prelude::*;
use blip_buf::BlipBuf;
use bytes::buf;
use crossbeam::channel::{self, Receiver, Sender};
use dasp::{Frame, Sample};
use itertools::{Itertools, izip};
use tracing::instrument;
use uzi::using;
pub(crate) mod registers;

type ExternalChannel<T> = (Sender<T>, Receiver<T>);

fn create_blip_bufs() -> [BlipBuf; 2] {
    array::from_fn(|_| {
        let mut buf = BlipBuf::new(48_000 / 10);
        buf.set_rates(4.194304E+6, 48_000.0);
        buf
    })
}

#[derive(Default)]
pub struct APU {
    pub debug_sender: Option<crossbeam::channel::Sender<f64>>,
    div_apu: u8,
    div_prev: bool,

    cycle_counter: u64,

    channel1: Channel1,
    channel2: Channel2,
    channel3: Channel3,

    clocks: u32,

    #[default(create_blip_bufs())]
    blip_bufs: [BlipBuf; 2],
    prev_frame: [i16; 2],

    #[default(channel::bounded(512))]
    pub output_channel: ExternalChannel<VecDeque<[i16; 2]>>,
}

impl APU {
    fn channels_mut(&mut self) -> [&mut dyn Channel<MemoryBus>; 3] {
        [&mut self.channel1, &mut self.channel2, &mut self.channel3]
    }

    fn channels(&self) -> [&dyn Channel<MemoryBus>; 3] {
        [&self.channel1, &self.channel2, &self.channel3]
    }

    #[instrument(skip_all)]
    pub fn tick(&mut self, ctx: &mut Context<MemoryBus>) {
        let div_bit_5 = ctx.memory.io().timer().div().view_bits::<Lsb0>()[5];

        if !self.div_prev && div_bit_5 {
            self.div_apu = self.div_apu.wrapping_add(1) % 8
        }
        self.div_prev = div_bit_5;

        let audio_volume = ctx.memory.io().audio().nr50();
        let left_volume = (audio_volume.left_volume() + 1) as f64 / 9.0;
        let right_volume = (audio_volume.right_volume() + 1) as f64 / 9.0;

        let audio_panning = *ctx.memory.io().audio().nr51();

        let frame = {
            let div_apu = self.div_apu;
            let cycle_counter = self.cycle_counter;
            let clocks = self.clocks;
            self.channels_mut()
                .into_iter()
                .enumerate()
                .map(|(idx, channel)| {
                    let out = channel.tick(ctx, div_apu, cycle_counter, clocks);
                    [
                        if audio_panning.get(idx).left() {
                            out * left_volume / 4.0
                        } else {
                            0.0
                        },
                        if audio_panning.get(idx).right() {
                            out * right_volume / 4.0
                        } else {
                            0.0
                        },
                    ]
                })
                .fold([0.0, 0.0], |[sum_l, sum_r], [l, r]| [sum_l + l, sum_r + r])
                .map(Sample::to_sample::<i16>)
        };

        for (buf, sample, prev) in izip!(self.blip_bufs.iter_mut(), frame, self.prev_frame) {
            buf.add_delta_fast(self.clocks, sample as i32 - prev as i32);
        }

        self.prev_frame = frame;

        if self.clocks == 0 {
            for buf in self.blip_bufs.iter_mut() {
                buf.end_frame(buf.clocks_needed(512));
            }
            let (mut buf_l, mut buf_r) = ([0; 512], [0; 512]);
            while self.blip_bufs[0].samples_avail() > 0 {
                self.blip_bufs[0].read_samples(&mut buf_l, false);
                self.blip_bufs[1].read_samples(&mut buf_r, false);

                let output_buffer = izip!(buf_l, buf_r).map(|(l, r)| [l, r]).collect();
                let _ = self.output_channel.0.send(output_buffer);
            }
        }

        let audio_control = ctx.memory.io_mut().audio_mut().nr52_mut();
        for i in 0..3 {
            audio_control.set_channel(i, self.channels()[i].enabled());
        }

        self.cycle_counter = self.cycle_counter.wrapping_add(1);
        self.clocks = (self.clocks + 1) % self.blip_bufs[0].clocks_needed(512);
    }
}

fn to_i32(value: u8) -> i32 {
    -2000 * value as i32 + 16000
}
fn dac(value: u8) -> f64 {
    -(value as f64) / 15.0 + 0.5
}

trait Channel<M: Default + Memory> {
    fn tick(&mut self, ctx: &mut Context<M>, div_apu: u8, cycle_count: u64, _clocks: u32) -> f64;

    #[allow(clippy::collapsible_if)]
    fn div_apu_tick(&mut self, ctx: &mut Context<M>, div_apu: u8);

    fn trigger(&mut self, ctx: &mut Context<M>);

    fn enabled(&self) -> bool;
}

#[derive(Default)]
struct Channel1 {
    div_apu_prev: u8,

    sweep_pace: u8,
    sweep_direction: registers::SweepDirection,
    sweep_individual_step: u8,
    sweep_timer: u8,

    sweep_enabled: bool,

    duty_step: u8,
    duty_cycle: registers::WaveDuty,

    length_timer: u8,

    envelope_direction: registers::EnvelopeDirection,
    envelope_sweep_pace: u8,
    envelope_timer: u8,

    period: u16,
    length_enable: bool,
    period_divider: u16,

    enable: bool,
    volume: u8,

    capacitor: f64,
}

impl Channel<MemoryBus> for Channel1 {
    fn tick(
        &mut self,
        ctx: &mut Context<MemoryBus>,
        div_apu: u8,
        cycle_count: u64,
        _clocks: u32,
    ) -> f64 {
        if ctx.memory.io().audio().nr10().pace() == 0 {
            self.sweep_pace = 0;
        }
        self.sweep_direction = ctx.memory.io().audio().nr10().direction();
        self.sweep_individual_step = ctx.memory.io().audio().nr10().step();
        self.duty_cycle = ctx.memory.io().audio().nr11().wave_duty();
        self.length_enable = ctx.memory.io().audio().nr13_14().length_enable();
        let triggered = ctx.memory.io().audio().nr13_14().trigger();

        if triggered {
            ctx.memory
                .io_mut()
                .audio_mut()
                .nr13_14_mut()
                .set_trigger(false);
            self.trigger(ctx);
        }

        let dac_on = if ctx.memory.io().audio().nr12().initial_volume() == 0
            && ctx.memory.io().audio().nr12().envelope_direction() == EnvelopeDirection::Decrease
        {
            self.enable = false;
            false
        } else {
            true
        };

        self.div_apu_tick(ctx, div_apu);

        if cycle_count.is_multiple_of(4) {
            if self.period_divider == 0x7FF {
                self.period = ctx.memory.io().audio().nr13_14().period();
                self.period_divider = self.period;
                self.duty_step = (self.duty_step + 1) % 8;
            } else {
                self.period_divider += 1
            }
        }

        let raw_output = if self.enable {
            self.duty_cycle.wave()[self.duty_step as usize] * self.volume
        } else {
            0x0
        };

        if dac_on {
            let input = dac(raw_output);
            let out = input - self.capacitor;
            self.capacitor = input - out * 0.99958;
            out
        } else {
            0.0
        }
    }

    #[allow(clippy::collapsible_if)]
    fn div_apu_tick(&mut self, ctx: &mut Context<MemoryBus>, div_apu: u8) {
        if div_apu == self.div_apu_prev {
            return;
        }
        //64 Hz
        if div_apu.is_multiple_of(8) {
            if self.envelope_sweep_pace != 0 {
                self.envelope_timer = self.envelope_timer.wrapping_add(1);
                if self.envelope_timer >= self.envelope_sweep_pace {
                    match self.envelope_direction {
                        registers::EnvelopeDirection::Decrease => {
                            self.volume = self.volume.saturating_sub(1)
                        }
                        registers::EnvelopeDirection::Increase => {
                            self.volume = cmp::min(self.volume + 1, 0xF)
                        }
                    }
                }
            }
        }

        //128 Hz
        if div_apu.is_multiple_of(4) && self.sweep_enabled {
            if self.sweep_pace > 0 {
                self.sweep_timer += 1;
            }
            if self.sweep_pace > 0 && self.sweep_timer == self.sweep_pace {
                self.sweep_timer = 0;
                self.calculate_sweep(ctx, true);
                self.sweep_pace = ctx.memory.io().audio().nr10().pace();
            } else {
                self.calculate_sweep(ctx, false);
            }
        }

        //256 Hz
        if div_apu.is_multiple_of(2) {
            if self.length_enable {
                self.length_timer = self.length_timer.wrapping_add(1);
                if self.length_timer == 64 {
                    self.enable = false;
                }
            }
        }

        self.div_apu_prev = div_apu;
    }

    fn trigger(&mut self, ctx: &mut Context<MemoryBus>) {
        self.enable = true;
        self.period = ctx.memory.io().audio().nr13_14().period();
        self.period_divider = self.period;
        self.volume = ctx.memory.io().audio().nr12().initial_volume();
        self.envelope_timer = 0;
        self.envelope_direction = ctx.memory.io().audio().nr12().envelope_direction();
        self.envelope_sweep_pace = ctx.memory.io().audio().nr12().sweep_pace();
        self.sweep_pace = ctx.memory.io().audio().nr10().pace();
        self.sweep_timer = 0;
        if self.length_timer >= 64 {
            self.length_timer = ctx.memory.io().audio().nr11().length_timer()
        }

        self.sweep_enabled = self.sweep_pace | self.sweep_individual_step != 0;
        if self.sweep_individual_step != 0 {
            self.calculate_sweep(ctx, true);
        }
    }

    fn enabled(&self) -> bool {
        self.enable
    }
}

impl Channel1 {
    fn calculate_sweep(&mut self, ctx: &mut Context<MemoryBus>, write_back: bool) {
        let frequency = {
            let modifier = self.period >> self.sweep_individual_step;
            match self.sweep_direction {
                registers::SweepDirection::Addition => self.period + modifier,
                registers::SweepDirection::Subtraction => self.period - modifier,
            }
        };
        if frequency > 0x7FF {
            self.enable = false
        } else if write_back {
            self.period = frequency & 0x7FF;
            ctx.memory
                .io_mut()
                .audio_mut()
                .nr13_14_mut()
                .set_period(frequency & 0x7FF);
            self.calculate_sweep(ctx, false);
        }
    }
}

#[derive(Default)]
struct Channel2 {
    div_apu_prev: u8,

    duty_step: u8,
    duty_cycle: registers::WaveDuty,

    length_timer: u8,

    envelope_direction: registers::EnvelopeDirection,
    envelope_sweep_pace: u8,
    envelope_timer: u8,

    period: u16,
    length_enable: bool,
    period_divider: u16,

    enable: bool,
    volume: u8,

    capacitor: f64,
}

impl Channel<MemoryBus> for Channel2 {
    fn tick(
        &mut self,
        ctx: &mut Context<MemoryBus>,
        div_apu: u8,
        cycle_count: u64,
        _clocks: u32,
    ) -> f64 {
        self.duty_cycle = ctx.memory.io().audio().nr21().wave_duty();
        self.length_enable = ctx.memory.io().audio().nr23_24().length_enable();
        let triggered = ctx.memory.io().audio().nr23_24().trigger();

        if triggered {
            ctx.memory
                .io_mut()
                .audio_mut()
                .nr23_24_mut()
                .set_trigger(false);
            self.trigger(ctx);
        }

        let dac_on = if ctx.memory.io().audio().nr22().initial_volume() == 0
            && ctx.memory.io().audio().nr22().envelope_direction() == EnvelopeDirection::Decrease
        {
            self.enable = false;
            false
        } else {
            true
        };

        self.div_apu_tick(ctx, div_apu);

        if cycle_count.is_multiple_of(4) {
            if self.period_divider == 0x7FF {
                self.period = ctx.memory.io().audio().nr23_24().period();
                self.period_divider = self.period;
                self.duty_step = (self.duty_step + 1) % 8;
            } else {
                self.period_divider += 1
            }
        }

        let raw_output = if self.enable {
            self.duty_cycle.wave()[self.duty_step as usize] * self.volume
        } else {
            0x0
        };

        if dac_on {
            let input = dac(raw_output);
            let out = input - self.capacitor;
            self.capacitor = input - out * 0.99958;
            out
        } else {
            0.0
        }
    }

    #[allow(clippy::collapsible_if)]
    fn div_apu_tick(&mut self, _ctx: &mut Context<MemoryBus>, div_apu: u8) {
        if div_apu == self.div_apu_prev {
            return;
        }
        //64 Hz
        if div_apu.is_multiple_of(8) {
            if self.envelope_sweep_pace != 0 {
                self.envelope_timer = self.envelope_timer.wrapping_add(1);
                if self.envelope_timer >= self.envelope_sweep_pace {
                    match self.envelope_direction {
                        registers::EnvelopeDirection::Decrease => {
                            self.volume = self.volume.saturating_sub(1)
                        }
                        registers::EnvelopeDirection::Increase => {
                            self.volume = cmp::min(self.volume + 1, 0xF)
                        }
                    }
                }
            }
        }

        //256 Hz
        if div_apu.is_multiple_of(2) {
            if self.length_enable {
                self.length_timer = self.length_timer.wrapping_add(1);
                if self.length_timer == 64 {
                    self.enable = false;
                }
            }
        }

        self.div_apu_prev = div_apu;
    }

    fn trigger(&mut self, ctx: &mut Context<MemoryBus>) {
        self.enable = true;
        self.period = ctx.memory.io().audio().nr23_24().period();
        self.period_divider = self.period;
        self.volume = ctx.memory.io().audio().nr22().initial_volume();
        self.envelope_timer = 0;
        self.envelope_direction = ctx.memory.io().audio().nr22().envelope_direction();
        self.envelope_sweep_pace = ctx.memory.io().audio().nr22().sweep_pace();

        if self.length_timer >= 64 {
            self.length_timer = ctx.memory.io().audio().nr21().length_timer()
        }
    }

    fn enabled(&self) -> bool {
        self.enable
    }
}

#[derive(Default)]
struct Channel3 {
    div_apu_prev: u8,

    duty_step: u8,

    length_timer: u16,

    period: u16,
    length_enable: bool,
    period_divider: u16,

    enable: bool,
    volume: u8,
    prev_output: i32,

    #[default({
        let mut buf = BlipBuf::new(48000 / 10);
        buf.set_rates(4.194304E+6, 48000.0);
        buf
    })]
    buffer: BlipBuf,
    sample: u8,
    capacitor: f64,
}

impl Channel<MemoryBus> for Channel3 {
    fn tick(
        &mut self,
        ctx: &mut Context<MemoryBus>,
        div_apu: u8,
        cycle_count: u64,
        _clocks: u32,
    ) -> f64 {
        self.length_enable = ctx.memory.io().audio().nr33_34().length_enable();
        let triggered = ctx.memory.io().audio().nr33_34().trigger();

        if triggered {
            ctx.memory
                .io_mut()
                .audio_mut()
                .nr33_34_mut()
                .set_trigger(false);
            self.trigger(ctx);
        }

        let dac_on = ctx.memory.io().audio().nr30().enable();
        if !dac_on {
            self.enable = false;
            false
        } else {
            true
        };

        self.div_apu_tick(ctx, div_apu);

        if cycle_count.is_multiple_of(2) {
            if self.period_divider == 0x7FF {
                self.period = ctx.memory.io().audio().nr33_34().period();
                self.period_divider = self.period;
                self.duty_step = (self.duty_step + 1) % 32;
            } else {
                self.period_divider += 1
            }
        }

        let raw_output = if self.enable {
            match self.volume {
                0 => 0,
                1 => self.sample,
                2 => self.sample >> 1,
                3 => self.sample >> 2,
                _ => unreachable!(),
            }
        } else {
            0
        };

        self.sample = {
            let wave = ctx.memory.io().audio().wave_pattern_ram();
            if self.duty_step.is_multiple_of(2) {
                self.period = ctx.memory.io().audio().nr33_34().period();
                wave[self.duty_step as usize / 2] >> 4
            } else {
                wave[self.duty_step as usize / 2] & 0b1111
            }
        };

        if dac_on {
            let input = dac(raw_output);
            let out = input - self.capacitor;
            self.capacitor = input - out * 0.99958;
            out
        } else {
            0.0
        }
    }

    #[allow(clippy::collapsible_if)]
    fn div_apu_tick(&mut self, _ctx: &mut Context<MemoryBus>, div_apu: u8) {
        if div_apu == self.div_apu_prev {
            return;
        }

        //256 Hz
        if div_apu.is_multiple_of(2) {
            if self.length_enable {
                self.length_timer = self.length_timer.wrapping_add(1);
                if self.length_timer == 256 {
                    self.enable = false;
                }
            }
        }

        self.div_apu_prev = div_apu;
    }

    fn trigger(&mut self, ctx: &mut Context<MemoryBus>) {
        self.enable = true;
        self.period = ctx.memory.io().audio().nr33_34().period();
        self.period_divider = self.period;
        self.volume = ctx.memory.io().audio().nr32().volume();

        self.duty_step = 0;

        if self.length_timer >= 256 {
            self.length_timer = ctx.memory.io().audio().nr31().length_timer() as u16;
        }
    }

    fn enabled(&self) -> bool {
        self.enable
    }
}

#[derive(Default, Debug)]
pub struct AudioRegisters {
    pub(crate) nr10: registers::ChannelSweep,
    pub(crate) nr11: registers::ChannelLengthTimerWithDuty,
    pub(crate) nr12: registers::ChannelVolumeEnvelope,
    pub(crate) nr13_14: registers::ChannelPeriodControl,
    pub(crate) nr21: registers::ChannelLengthTimerWithDuty,
    pub(crate) nr22: registers::ChannelVolumeEnvelope,
    pub(crate) nr23_24: registers::ChannelPeriodControl,
    pub(crate) nr30: registers::ChannelDacEnable,
    pub(crate) nr31: registers::ChannelLengthTimer,
    pub(crate) nr32: registers::ChannelVolume,
    pub(crate) nr33_34: registers::ChannelPeriodControl,
    pub(crate) nr41: registers::ChannelLengthTimer,
    pub(crate) nr42: registers::ChannelVolumeEnvelope,
    pub(crate) nr43: registers::ChannelFrequencyRandomness,
    pub(crate) nr44: registers::ChannelControl,
    pub(crate) nr50: registers::AudioVolume,
    pub(crate) nr51: registers::AudioPanning,
    pub(crate) nr52: registers::AudioEnable,
    wave_pattern_ram: [u8; 16],
}

impl AudioRegister for AudioRegisters {
    fn nr10_mut(&mut self) -> &mut registers::ChannelSweep {
        &mut self.nr10
    }
    fn nr11_mut(&mut self) -> &mut registers::ChannelLengthTimerWithDuty {
        &mut self.nr11
    }
    fn nr12_mut(&mut self) -> &mut registers::ChannelVolumeEnvelope {
        &mut self.nr12
    }
    fn nr13_14_mut(&mut self) -> &mut registers::ChannelPeriodControl {
        &mut self.nr13_14
    }
    fn nr21_mut(&mut self) -> &mut registers::ChannelLengthTimerWithDuty {
        &mut self.nr21
    }
    fn nr22_mut(&mut self) -> &mut registers::ChannelVolumeEnvelope {
        &mut self.nr22
    }
    fn nr23_24_mut(&mut self) -> &mut registers::ChannelPeriodControl {
        &mut self.nr23_24
    }
    fn nr30_mut(&mut self) -> &mut registers::ChannelDacEnable {
        &mut self.nr30
    }
    fn nr31_mut(&mut self) -> &mut registers::ChannelLengthTimer {
        &mut self.nr31
    }
    fn nr32_mut(&mut self) -> &mut registers::ChannelVolume {
        &mut self.nr32
    }
    fn nr33_34_mut(&mut self) -> &mut registers::ChannelPeriodControl {
        &mut self.nr33_34
    }
    fn nr41_mut(&mut self) -> &mut registers::ChannelLengthTimer {
        &mut self.nr41
    }
    fn nr42_mut(&mut self) -> &mut registers::ChannelVolumeEnvelope {
        &mut self.nr42
    }
    fn nr43_mut(&mut self) -> &mut registers::ChannelFrequencyRandomness {
        &mut self.nr43
    }
    fn nr44_mut(&mut self) -> &mut registers::ChannelControl {
        &mut self.nr44
    }
    fn nr50_mut(&mut self) -> &mut registers::AudioVolume {
        &mut self.nr50
    }
    fn nr51_mut(&mut self) -> &mut registers::AudioPanning {
        &mut self.nr51
    }
    fn nr52_mut(&mut self) -> &mut registers::AudioEnable {
        &mut self.nr52
    }
    fn nr10(&self) -> &registers::ChannelSweep {
        &self.nr10
    }
    fn nr11(&self) -> &registers::ChannelLengthTimerWithDuty {
        &self.nr11
    }
    fn nr12(&self) -> &registers::ChannelVolumeEnvelope {
        &self.nr12
    }
    fn nr13_14(&self) -> &registers::ChannelPeriodControl {
        &self.nr13_14
    }
    fn nr21(&self) -> &registers::ChannelLengthTimerWithDuty {
        &self.nr21
    }
    fn nr22(&self) -> &registers::ChannelVolumeEnvelope {
        &self.nr22
    }
    fn nr23_24(&self) -> &registers::ChannelPeriodControl {
        &self.nr23_24
    }
    fn nr30(&self) -> &registers::ChannelDacEnable {
        &self.nr30
    }
    fn nr31(&self) -> &registers::ChannelLengthTimer {
        &self.nr31
    }
    fn nr32(&self) -> &registers::ChannelVolume {
        &self.nr32
    }
    fn nr33_34(&self) -> &registers::ChannelPeriodControl {
        &self.nr33_34
    }
    fn nr41(&self) -> &registers::ChannelLengthTimer {
        &self.nr41
    }
    fn nr42(&self) -> &registers::ChannelVolumeEnvelope {
        &self.nr42
    }
    fn nr43(&self) -> &registers::ChannelFrequencyRandomness {
        &self.nr43
    }
    fn nr44(&self) -> &registers::ChannelControl {
        &self.nr44
    }
    fn nr50(&self) -> &registers::AudioVolume {
        &self.nr50
    }
    fn nr51(&self) -> &registers::AudioPanning {
        &self.nr51
    }
    fn nr52(&self) -> &registers::AudioEnable {
        &self.nr52
    }
    fn wave_pattern_ram(&self) -> &[u8; 16] {
        &self.wave_pattern_ram
    }
    fn wave_pattern_ram_mut(&mut self) -> &mut [u8; 16] {
        &mut self.wave_pattern_ram
    }

    fn read_u8(&self, address: u8) -> u8 {
        match address {
            0x00..0x10 => unreachable!(),
            0x10 => self.nr10().read(),
            0x11 => self.nr11().read(),
            0x12 => self.nr12().read(),
            0x13 => self.nr13_14().read().to_le_bytes()[0],
            0x14 => self.nr13_14().read().to_le_bytes()[1],
            0x15 => unimplemented_audio_read(address),
            0x16 => self.nr21().read(),
            0x17 => self.nr22().read(),
            0x18 => self.nr23_24().read().to_le_bytes()[0],
            0x19 => self.nr23_24().read().to_le_bytes()[1],
            0x1A => self.nr30().read(),
            0x1B => self.nr31().read(),
            0x1C => self.nr32().read(),
            0x1D => self.nr33_34().read().to_le_bytes()[0],
            0x1E => self.nr33_34().read().to_le_bytes()[1],
            0x1F => unimplemented_audio_read(address),
            0x20..=0x23 => unimplemented_audio_read(address),
            0x24 => self.nr50.read(),
            0x25 => self.nr51.read(),
            0x26 => self.nr52.read(),
            0x27..0x30 => unimplemented_audio_read(address),
            0x30..=0x3F => self.wave_pattern_ram()[address as usize - 0x30],
            0x40.. => unreachable!(),
        }
    }
    fn write_u8(&mut self, address: u8, value: u8) {
        match address {
            0x00..0x10 => unreachable!(),
            0x10 => self.nr10_mut().write(value),
            0x11 => self.nr11_mut().write(value),
            0x12 => self.nr12_mut().write(value),
            0x13 => self.nr13_14_mut().set_low(value),
            0x14 => self.nr13_14_mut().set_high(value),
            0x15 => unimplemented_audio_write(address, value),
            0x16 => self.nr21_mut().write(value),
            0x17 => self.nr22_mut().write(value),
            0x18 => self.nr23_24_mut().set_low(value),
            0x19 => self.nr23_24_mut().set_high(value),
            0x1A => self.nr30_mut().write(value),
            0x1B => self.nr31_mut().write(value),
            0x1C => self.nr32_mut().write(value),
            0x1D => self.nr33_34_mut().set_low(value),
            0x1E => self.nr33_34_mut().set_high(value),
            0x1F => unimplemented_audio_write(address, value),
            0x20..=0x23 => unimplemented_audio_write(address, value),
            0x24 => self.nr50_mut().write(value),
            0x25 => self.nr51_mut().write(value),
            0x26 => self.nr52_mut().write(value),
            0x27..0x30 => unimplemented_audio_write(address, value),
            0x30..=0x3F => self.wave_pattern_ram_mut()[address as usize - 0x30] = value,
            0x40.. => unreachable!(),
        }
    }
}

#[allow(unused)]
fn unimplemented_audio_read(address: u8) -> u8 {
    0xFF
}

#[allow(unused)]
fn unimplemented_audio_write(address: u8, value: u8) {}

pub trait AudioRegister {
    fn nr10_mut(&mut self) -> &mut registers::ChannelSweep;
    fn nr11_mut(&mut self) -> &mut registers::ChannelLengthTimerWithDuty;
    fn nr12_mut(&mut self) -> &mut registers::ChannelVolumeEnvelope;
    fn nr13_14_mut(&mut self) -> &mut registers::ChannelPeriodControl;
    fn nr21_mut(&mut self) -> &mut registers::ChannelLengthTimerWithDuty;
    fn nr22_mut(&mut self) -> &mut registers::ChannelVolumeEnvelope;
    fn nr23_24_mut(&mut self) -> &mut registers::ChannelPeriodControl;
    fn nr30_mut(&mut self) -> &mut registers::ChannelDacEnable;
    fn nr31_mut(&mut self) -> &mut registers::ChannelLengthTimer;
    fn nr32_mut(&mut self) -> &mut registers::ChannelVolume;
    fn nr33_34_mut(&mut self) -> &mut registers::ChannelPeriodControl;
    fn nr41_mut(&mut self) -> &mut registers::ChannelLengthTimer;
    fn nr42_mut(&mut self) -> &mut registers::ChannelVolumeEnvelope;
    fn nr43_mut(&mut self) -> &mut registers::ChannelFrequencyRandomness;
    fn nr44_mut(&mut self) -> &mut registers::ChannelControl;
    fn nr50_mut(&mut self) -> &mut registers::AudioVolume;
    fn nr51_mut(&mut self) -> &mut registers::AudioPanning;
    fn nr52_mut(&mut self) -> &mut registers::AudioEnable;
    fn nr10(&self) -> &registers::ChannelSweep;
    fn nr11(&self) -> &registers::ChannelLengthTimerWithDuty;
    fn nr12(&self) -> &registers::ChannelVolumeEnvelope;
    fn nr13_14(&self) -> &registers::ChannelPeriodControl;
    fn nr21(&self) -> &registers::ChannelLengthTimerWithDuty;
    fn nr22(&self) -> &registers::ChannelVolumeEnvelope;
    fn nr23_24(&self) -> &registers::ChannelPeriodControl;
    fn nr30(&self) -> &registers::ChannelDacEnable;
    fn nr31(&self) -> &registers::ChannelLengthTimer;
    fn nr32(&self) -> &registers::ChannelVolume;
    fn nr33_34(&self) -> &registers::ChannelPeriodControl;
    fn nr41(&self) -> &registers::ChannelLengthTimer;
    fn nr42(&self) -> &registers::ChannelVolumeEnvelope;
    fn nr43(&self) -> &registers::ChannelFrequencyRandomness;
    fn nr44(&self) -> &registers::ChannelControl;
    fn nr50(&self) -> &registers::AudioVolume;
    fn nr51(&self) -> &registers::AudioPanning;
    fn nr52(&self) -> &registers::AudioEnable;
    fn wave_pattern_ram(&self) -> &[u8; 16];
    fn wave_pattern_ram_mut(&mut self) -> &mut [u8; 16];

    fn write_u8(&mut self, address: u8, value: u8);

    fn read_u8(&self, address: u8) -> u8;
}
