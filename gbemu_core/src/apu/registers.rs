use better_default::Default;
use bitvec::prelude::*;
use bytemuck::TransparentWrapper;
use std::ops::{Deref, DerefMut};
use strum::FromRepr;

#[derive(Default, Debug, TransparentWrapper)]
#[repr(transparent)]
pub struct AudioEnable(#[default(0b11111111)] pub(crate) u8);

impl AudioEnable {
    pub fn audio_enabled(&self) -> bool {
        self.0.view_bits::<Lsb0>()[7]
    }
    pub fn set_audio_enabled(&mut self, enabled: bool) {
        self.0.view_bits_mut::<Lsb0>().set(7, enabled);
    }

    pub fn channel_1_enabled(&self) -> bool {
        self.0.view_bits::<Lsb0>()[0]
    }
    pub fn channel_2_enabled(&self) -> bool {
        self.0.view_bits::<Lsb0>()[1]
    }
    pub fn channel_3_enabled(&self) -> bool {
        self.0.view_bits::<Lsb0>()[2]
    }
    pub fn channel_4_enabled(&self) -> bool {
        self.0.view_bits::<Lsb0>()[3]
    }

    pub fn read(&self) -> u8 {
        self.0 | 0b0111_0000
    }

    pub fn write(&mut self, value: u8) {
        self.0
            .view_bits_mut::<Lsb0>()
            .set(7, value.view_bits::<Lsb0>()[7]);
    }

    pub fn set_channel_1_enabled(&mut self, value: bool) {
        self.0.view_bits_mut::<Lsb0>().set(0, value)
    }

    pub fn set_channel_2_enabled(&mut self, value: bool) {
        self.0.view_bits_mut::<Lsb0>().set(1, value)
    }

    pub fn set_channel_3_enabled(&mut self, value: bool) {
        self.0.view_bits_mut::<Lsb0>().set(2, value)
    }

    pub fn set_channel_4_enabled(&mut self, value: bool) {
        self.0.view_bits_mut::<Lsb0>().set(3, value)
    }
}

#[derive(Default, Debug, TransparentWrapper)]
#[repr(transparent)]
pub struct AudioPanning(#[default(0b11111111)] pub(crate) u8);

impl AudioPanning {
    pub fn read(&self) -> u8 {
        self.0
    }

    pub fn write(&mut self, value: u8) {
        self.0 = value;
    }

    pub fn channel_4_left(&self) -> bool {
        self.0.view_bits::<Lsb0>()[7]
    }

    pub fn set_channel_4_left(&mut self, enabled: bool) {
        self.0.view_bits_mut::<Lsb0>().set(7, enabled);
    }

    pub fn channel_3_left(&self) -> bool {
        self.0.view_bits::<Lsb0>()[6]
    }

    pub fn set_channel_3_left(&mut self, enabled: bool) {
        self.0.view_bits_mut::<Lsb0>().set(6, enabled);
    }

    pub fn channel_2_left(&self) -> bool {
        self.0.view_bits::<Lsb0>()[5]
    }

    pub fn set_channel_2_left(&mut self, enabled: bool) {
        self.0.view_bits_mut::<Lsb0>().set(5, enabled);
    }

    pub fn channel_1_left(&self) -> bool {
        self.0.view_bits::<Lsb0>()[4]
    }

    pub fn set_channel_1_left(&mut self, enabled: bool) {
        self.0.view_bits_mut::<Lsb0>().set(4, enabled);
    }

    pub fn channel_4_right(&self) -> bool {
        self.0.view_bits::<Lsb0>()[3]
    }

    pub fn set_channel_4_right(&mut self, enabled: bool) {
        self.0.view_bits_mut::<Lsb0>().set(3, enabled);
    }

    pub fn channel_3_right(&self) -> bool {
        self.0.view_bits::<Lsb0>()[2]
    }
    pub fn set_channel_3_right(&mut self, enabled: bool) {
        self.0.view_bits_mut::<Lsb0>().set(2, enabled);
    }

    pub fn channel_2_right(&self) -> bool {
        self.0.view_bits::<Lsb0>()[1]
    }

    pub fn set_channel_2_right(&mut self, enabled: bool) {
        self.0.view_bits_mut::<Lsb0>().set(1, enabled);
    }

    pub fn channel_1_right(&self) -> bool {
        self.0.view_bits::<Lsb0>()[0]
    }

    pub fn set_channel_1_right(&mut self, enabled: bool) {
        self.0.view_bits_mut::<Lsb0>().set(0, enabled);
    }
}

#[derive(Default, Debug, TransparentWrapper)]
#[repr(transparent)]
pub struct AudioVolume(#[default(0b01110111)] pub(crate) u8);

impl AudioVolume {
    pub fn read(&self) -> u8 {
        self.0
    }

    pub fn write(&mut self, value: u8) {
        self.0 = value;
    }

    pub fn vin_left(&self) -> bool {
        self.0.view_bits::<Lsb0>()[7]
    }
    pub fn set_vin_left(&mut self, enabled: bool) {
        self.0.view_bits_mut::<Lsb0>().set(7, enabled);
    }

    pub fn left_volume(&self) -> u8 {
        self.0.view_bits::<Lsb0>()[4..=6].load_le()
    }

    pub fn set_left_volume(&mut self, volume: u8) {
        self.0.view_bits_mut::<Lsb0>()[4..=6].store_le(volume);
    }

    pub fn vin_right(&self) -> bool {
        self.0.view_bits::<Lsb0>()[3]
    }

    pub fn set_vin_right(&mut self, enabled: bool) {
        self.0.view_bits_mut::<Lsb0>().set(3, enabled);
    }

    pub fn right_volume(&self) -> u8 {
        self.0.view_bits::<Lsb0>()[0..=2].load_le()
    }
    pub fn set_right_volume(&mut self, volume: u8) {
        self.0.view_bits_mut::<Lsb0>().store_le(volume);
    }
}

#[derive(Default, Debug, FromRepr, PartialEq, Eq, Clone, Copy)]
#[repr(u8)]
pub enum SweepDirection {
    #[default]
    Addition = 0,
    Subtraction = 1,
}

impl From<bool> for SweepDirection {
    fn from(value: bool) -> Self {
        match value {
            false => SweepDirection::Addition,
            true => SweepDirection::Subtraction,
        }
    }
}

impl From<SweepDirection> for bool {
    fn from(value: SweepDirection) -> Self {
        match value {
            SweepDirection::Addition => false,
            SweepDirection::Subtraction => true,
        }
    }
}

#[derive(Default, Debug, TransparentWrapper)]
#[repr(transparent)]
pub struct ChannelSweep(#[default(0b1000_0000)] pub(crate) u8);

impl ChannelSweep {
    pub fn read(&self) -> u8 {
        self.0 | 0b1000_0000
    }

    pub fn write(&mut self, value: u8) {
        self.0 = value | 0b1000_0000;
    }

    pub fn pace(&self) -> u8 {
        self.0.view_bits::<Lsb0>()[4..=6].load_le()
    }
    pub fn set_pace(&mut self, value: u8) {
        self.0.view_bits_mut::<Lsb0>()[4..=6].store_le(value);
    }

    pub fn direction(&self) -> SweepDirection {
        self.0.view_bits::<Lsb0>()[3].into()
    }

    pub fn set_direction(&mut self, value: SweepDirection) {
        self.0.view_bits_mut::<Lsb0>().set(3, value.into());
    }

    pub fn step(&self) -> u8 {
        self.0.view_bits::<Lsb0>()[0..=2].load_le()
    }
    pub fn set_step(&mut self, value: u8) {
        self.0.view_bits_mut::<Lsb0>()[0..=2].store_le(value);
    }
}
#[derive(Default, Debug, FromRepr, PartialEq, Eq, Clone, Copy)]
#[repr(u8)]
pub enum WaveDuty {
    #[default]
    Eighth = 0b00,
    Quarter = 0b01,
    Half = 0b10,
    ThreeQuarter = 0b11,
}

impl WaveDuty {
    pub fn wave(&self) -> [u8; 8] {
        match self {
            Self::Eighth => [0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x0, 0x1],
            Self::Quarter => [0x0, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x0],
            Self::Half => [0x0, 0x1, 0x1, 0x1, 0x1, 0x0, 0x0, 0x0],
            Self::ThreeQuarter => [0x1, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x1],
        }
    }
}

#[derive(Default, Debug, TransparentWrapper)]
#[repr(transparent)]
pub struct ChannelLengthTimerWithDuty(#[default(0b00111111)] pub(crate) u8);

impl ChannelLengthTimerWithDuty {
    pub fn wave_duty(&self) -> WaveDuty {
        WaveDuty::from_repr(self.0.view_bits::<Lsb0>()[6..=7].load_le()).unwrap_or(WaveDuty::Eighth)
    }

    pub fn set_wave_duty(&mut self, value: WaveDuty) {
        self.0.view_bits_mut::<Lsb0>()[6..=7].store_le(value as u8);
    }

    pub fn length_timer(&self) -> u8 {
        self.0.view_bits::<Lsb0>()[0..=5].load_le()
    }
    pub fn set_length_timer(&mut self, value: u8) {
        self.0.view_bits_mut::<Lsb0>()[0..=5].store_le(value);
    }

    pub fn read(&self) -> u8 {
        self.0 | 0b0011_1111
    }

    pub fn write(&mut self, value: u8) {
        self.0 = value;
    }
}

#[derive(Default, Debug, FromRepr, PartialEq, Eq, Clone, Copy)]
#[repr(u8)]
pub enum EnvelopeDirection {
    #[default]
    Decrease = 0,
    Increase = 1,
}

impl From<bool> for EnvelopeDirection {
    fn from(value: bool) -> Self {
        match value {
            false => EnvelopeDirection::Decrease,
            true => EnvelopeDirection::Increase,
        }
    }
}

impl From<EnvelopeDirection> for bool {
    fn from(value: EnvelopeDirection) -> Self {
        match value {
            EnvelopeDirection::Decrease => false,
            EnvelopeDirection::Increase => true,
        }
    }
}

#[derive(Default, Debug, TransparentWrapper)]
#[repr(transparent)]
pub struct ChannelVolumeEnvelope(#[default(0b11110000)] pub(crate) u8);

impl ChannelVolumeEnvelope {
    pub fn read(&self) -> u8 {
        self.0
    }

    pub fn write(&mut self, value: u8) {
        self.0 = value;
    }

    pub fn initial_volume(&self) -> u8 {
        self.0.view_bits::<Lsb0>()[4..=7].load_le()
    }
    pub fn set_initial_volume(&mut self, value: u8) {
        self.0.view_bits_mut::<Lsb0>()[4..=7].store_le(value);
    }

    pub fn envelope_direction(&self) -> EnvelopeDirection {
        self.0.view_bits::<Lsb0>()[3].into()
    }

    pub fn set_envelope_direction(&mut self, value: EnvelopeDirection) {
        self.0.view_bits_mut::<Lsb0>().set(3, value.into());
    }

    pub fn sweep_pace(&self) -> u8 {
        self.0.view_bits::<Lsb0>()[0..=2].load_le()
    }
    pub fn set_sweep_pace(&mut self, value: u8) {
        self.0.view_bits_mut::<Lsb0>()[0..=2].store_le(value);
    }
}

#[derive(Default, Debug, TransparentWrapper)]
#[repr(transparent)]
pub struct ChannelPeriodControl(#[default(0b01111111_11111111)] pub(crate) u16);
impl ChannelPeriodControl {
    pub fn read(&self) -> u16 {
        self.0 | 0b10111111_11111111
    }

    pub fn write_low(&mut self, value: u8) {
        self.0.view_bits_mut::<Lsb0>()[0..=7].store_be(value);
    }

    pub fn write_high(&mut self, value: u8) {
        self.0.view_bits_mut::<Lsb0>()[8..=15].store_be(value);
    }

    pub fn period(&self) -> u16 {
        self.0.view_bits::<Lsb0>()[0..=10].load_be()
    }
    pub fn set_period(&mut self, value: u16) {
        self.0.view_bits_mut::<Lsb0>()[0..=10].store_be(value);
    }

    pub fn length_enable(&self) -> bool {
        self.0.view_bits::<Lsb0>()[14]
    }
    pub fn set_length_enable(&mut self, value: bool) {
        self.0.view_bits_mut::<Lsb0>().set(14, value)
    }

    pub fn trigger(&self) -> bool {
        self.0.view_bits::<Lsb0>()[15]
    }
    pub fn set_trigger(&mut self, value: bool) {
        self.0.view_bits_mut::<Lsb0>().set(15, value)
    }
}

#[derive(Default, Debug, TransparentWrapper)]
#[repr(transparent)]
pub struct ChannelDacEnable(#[default(0b11111111)] pub(crate) u8);
impl ChannelDacEnable {
    pub fn read(&self) -> u8 {
        self.0 | 0b0111_1111
    }

    pub fn enable(&self) -> bool {
        self.0.view_bits::<Lsb0>()[7]
    }
    pub fn set_enable(&mut self, value: bool) {
        self.0.view_bits_mut::<Lsb0>().set(7, value);
    }

    pub fn write(&mut self, value: u8) {
        self.0 = value | 0b0111_1111
    }
}

#[derive(Default, Debug, TransparentWrapper)]
#[repr(transparent)]
pub struct ChannelLengthTimer(#[default(0b11111111)] pub(crate) u8);
impl ChannelLengthTimer {
    pub fn read(&self) -> u8 {
        0b11111111
    }

    pub fn set_length_timer(&mut self, value: u8) {
        self.0 = value;
    }

    pub(crate) fn length_timer(&self) -> u8 {
        self.0
    }

    pub fn write(&mut self, value: u8) {
        self.0 = value;
    }
}

#[derive(Default, Debug, TransparentWrapper)]
#[repr(transparent)]
pub struct ChannelVolume(#[default(0b10111111)] pub(crate) u8);
impl ChannelVolume {
    pub fn read(&self) -> u8 {
        self.0 | 0b10011111
    }

    pub fn write(&mut self, value: u8) {
        self.0 = value | 0b10011111;
    }

    pub fn volume(&self) -> u8 {
        self.0.view_bits::<Lsb0>()[5..=6].load_le()
    }
    pub fn set_volume(&mut self, value: u8) {
        self.0.view_bits_mut::<Lsb0>()[5..=6].store_le(value);
    }
}

#[derive(Default, Debug, FromRepr, PartialEq, Eq, Clone, Copy)]
#[repr(u8)]
enum LfsrWidth {
    #[default]
    Fifteen = 0,
    Seven = 1,
}

impl From<bool> for LfsrWidth {
    fn from(value: bool) -> Self {
        match value {
            false => LfsrWidth::Fifteen,
            true => LfsrWidth::Seven,
        }
    }
}

impl From<LfsrWidth> for bool {
    fn from(value: LfsrWidth) -> Self {
        match value {
            LfsrWidth::Fifteen => false,
            LfsrWidth::Seven => true,
        }
    }
}

#[derive(Default, Debug, TransparentWrapper)]
#[repr(transparent)]
pub struct ChannelFrequencyRandomness(#[default(0b11111111)] pub(crate) u8);
impl ChannelFrequencyRandomness {
    pub fn read(&self) -> u8 {
        self.0
    }

    pub fn clock_shift(&self) -> u8 {
        self.0.view_bits::<Lsb0>()[4..=7].load_le()
    }
    pub fn set_clock_shift(&mut self, value: u8) {
        self.0.view_bits_mut::<Lsb0>()[4..=7].store_le(value);
    }

    pub fn lfsr_width(&self) -> LfsrWidth {
        self.0.view_bits::<Lsb0>()[3].into()
    }

    pub fn set_lfsr_width(&mut self, value: LfsrWidth) {
        self.0.view_bits_mut::<Lsb0>().set(3, value.into());
    }

    pub fn clock_divider(&self) -> u8 {
        self.0.view_bits::<Lsb0>()[0..=2].load_le()
    }

    pub fn set_clock_divider(&mut self, value: u8) {
        self.0.view_bits_mut::<Lsb0>()[0..=2].store_le(value);
    }
}

#[derive(Default, Debug, TransparentWrapper)]
#[repr(transparent)]
pub struct ChannelControl(#[default(0b11111111)] pub(crate) u8);
impl ChannelControl {
    pub fn read(&self) -> u8 {
        self.0 | 0b10111111
    }

    pub fn length_enable(&self) -> bool {
        self.0.view_bits::<Lsb0>()[6]
    }
    pub fn set_length_enable(&mut self, value: bool) {
        self.0.view_bits_mut::<Lsb0>().set(6, value)
    }
}
