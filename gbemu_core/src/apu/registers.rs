use core::ops::Index;

use better_default::Default;
use bytemuck::TransparentWrapper;
use strum::FromRepr;

use chapa::{BitEnum, bitfield};

#[bitfield(u8, order = lsb0, width = 4)]
#[derive(Default, Copy, Clone, PartialEq, Debug)]
pub struct ChannelsEnabled {
    #[bits(0)]
    channel1_enabled: bool,
    #[bits(1)]
    channel2_enabled: bool,
    #[bits(2)]
    channel3_enabled: bool,
    #[bits(3)]
    channel4_enabled: bool,
}

impl<T> Index<T> for ChannelsEnabled
where
    usize: From<T>,
{
    type Output = bool;

    #[inline(always)]
    fn index(&self, index: T) -> &Self::Output {
        if match usize::from(index) {
            0 => self.channel1_enabled(),
            1 => self.channel2_enabled(),
            2 => self.channel3_enabled(),
            3 => self.channel4_enabled(),
            _ => panic!("Out of range for bitfield"),
        } {
            &true
        } else {
            &false
        }
    }
}

#[bitfield(u8, order = lsb0)]
#[derive(Default, Copy, Clone, PartialEq, Debug)]
pub struct AudioEnable {
    #[bits(7, default = true)]
    audio_enabled: bool,
    #[bits(4..=6, default = 0b111)]
    _unused: u8,
    #[bits(0..=3, overlay="together")]
    channels: ChannelsEnabled,
    #[bits(0, overlay = "individual", default = true)]
    channel1_enabled: bool,
    #[bits(1, overlay = "individual", default = true)]
    channel2_enabled: bool,
    #[bits(2, overlay = "individual", default = true)]
    channel3_enabled: bool,
    #[bits(3, overlay = "individual", default = true)]
    channel4_enabled: bool,
}

impl AudioEnable {
    #[inline(always)]
    pub fn read(self) -> u8 {
        (self | 0b0111_0000).into()
    }

    #[inline(always)]
    pub fn write(&mut self, value: u8) {
        self.set_audio_enabled(Self::from(value).audio_enabled());
    }

    #[inline(always)]
    pub fn set_channel<T>(&mut self, idx: T, value: bool)
    where
        usize: From<T>,
    {
        match usize::from(idx) {
            0 => self.set_channel1_enabled(value),
            1 => self.set_channel2_enabled(value),
            2 => self.set_channel3_enabled(value),
            3 => self.set_channel4_enabled(value),
            _ => panic!("Out of range for bitfield"),
        }
    }
}

#[bitfield(u8, order = lsb0, width = 5)]
#[derive(Default, Copy, Clone, PartialEq, Debug)]
pub struct ChannelPanning {
    #[bits(0, default = true)]
    right: bool,
    #[bits(1..=3)]
    _padded: u8,
    #[bits(4, default = true)]
    left: bool,
}

#[bitfield(u8, order = lsb0)]
#[derive(Default, Copy, Clone, PartialEq, Debug)]
pub struct AudioPanning {
    #[bits(0..=7, overlay="all", default = 0xFF)]
    _all: u8,
    #[bits(0..=4, overlay="channel1")]
    channel1: ChannelPanning,
    #[bits(1..=5, overlay="channel2")]
    channel2: ChannelPanning,
    #[bits(2..=6, overlay="channel3")]
    channel3: ChannelPanning,
    #[bits(3..=7, overlay="channel4")]
    channel4: ChannelPanning,
}

impl AudioPanning {
    #[inline(always)]
    pub fn read(self) -> u8 {
        self.into()
    }

    #[inline(always)]
    pub fn write(&mut self, value: u8) {
        *self = value.into();
    }

    #[inline(always)]
    pub fn get<T>(&self, idx: T) -> ChannelPanning
    where
        usize: From<T>,
    {
        match usize::from(idx) {
            0 => self.channel1(),
            1 => self.channel2(),
            2 => self.channel3(),
            3 => self.channel4(),
            _ => panic!("Out of range for bitfield"),
        }
    }
}

// #[derive(Default, Debug, TransparentWrapper)]
// #[repr(transparent)]
// pub struct AudioVolume(#[default(0b01110111)] pub(crate) u8);

#[bitfield(u8, order = lsb0)]
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct AudioVolume {
    #[bits(4..=6, overlay="base", default = 0b111)]
    left_volume: u8,
    #[bits(0..=2, overlay="base",default = 0b111)]
    right_volume: u8,
    #[bits(3..=7, overlay="vin")]
    vin_enable: ChannelPanning,
}

impl AudioVolume {
    #[inline(always)]
    pub fn read(self) -> u8 {
        self.into()
    }

    #[inline(always)]
    pub fn write(&mut self, value: u8) {
        *self = value.into();
    }
}

#[derive(Default, Debug, FromRepr, PartialEq, Eq, Clone, Copy, BitEnum)]
#[repr(u8)]
pub enum SweepDirection {
    #[default]
    #[fallback]
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
#[bitfield(u8, order=lsb0)]
#[derive(Default, Debug, TransparentWrapper, Clone, Copy, PartialEq)]
pub struct ChannelSweep {
    #[bits(7, default = true)]
    _unused: bool,
    #[bits(4..=6)]
    pace: u8,
    #[bits(3, default = SweepDirection::default())]
    direction: SweepDirection,
    #[bits(0..=2)]
    step: u8,
}

impl ChannelSweep {
    #[inline(always)]
    pub fn read(self) -> u8 {
        (self | 0b1000_0000).into()
    }

    #[inline(always)]
    pub fn write(&mut self, value: u8) {
        *self = (value | 0b1000_0000).into();
    }
}
#[derive(Default, Debug, FromRepr, PartialEq, Eq, Clone, Copy, BitEnum)]
#[repr(u8)]
pub enum WaveDuty {
    #[fallback]
    #[default]
    Eighth = 0b00,
    Quarter = 0b01,
    Half = 0b10,
    ThreeQuarter = 0b11,
}

impl WaveDuty {
    #[inline(always)]
    pub fn wave(&self) -> &[u8; 8] {
        match self {
            Self::Eighth => &[0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x0, 0x1],
            Self::Quarter => &[0x0, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x0],
            Self::Half => &[0x0, 0x1, 0x1, 0x1, 0x1, 0x0, 0x0, 0x0],
            Self::ThreeQuarter => &[0x1, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x1],
        }
    }
}

#[bitfield(u8, order=lsb0)]
#[derive(Default, Debug, TransparentWrapper, Clone, Copy, PartialEq)]
pub struct ChannelLengthTimerWithDuty {
    #[bits(6..=7, default = WaveDuty::default())]
    wave_duty: WaveDuty,
    #[bits(0..=5, default = 0b11_1111)]
    length_timer: u8,
}

impl ChannelLengthTimerWithDuty {
    #[inline(always)]
    pub fn read(self) -> u8 {
        (self | 0b0011_1111).into()
    }

    #[inline(always)]
    pub fn write(&mut self, value: u8) {
        *self = value.into();
    }
}

#[derive(Default, Debug, FromRepr, PartialEq, Eq, Clone, Copy, BitEnum)]
#[repr(u8)]
pub enum EnvelopeDirection {
    #[default]
    #[fallback]
    Decrease = 0,
    Increase = 1,
}

impl From<bool> for EnvelopeDirection {
    #[inline(always)]
    fn from(value: bool) -> Self {
        match value {
            false => EnvelopeDirection::Decrease,
            true => EnvelopeDirection::Increase,
        }
    }
}

impl From<EnvelopeDirection> for bool {
    #[inline(always)]
    fn from(value: EnvelopeDirection) -> Self {
        match value {
            EnvelopeDirection::Decrease => false,
            EnvelopeDirection::Increase => true,
        }
    }
}

#[bitfield(u8, order=lsb0)]
#[derive(Default, Debug, TransparentWrapper, Copy, Clone, PartialEq)]
pub struct ChannelVolumeEnvelope {
    #[bits(4..=7, default = 0b1111)]
    initial_volume: u8,
    #[bits(3)]
    envelope_direction: EnvelopeDirection,
    #[bits(0..=2)]
    sweep_pace: u8,
}

impl ChannelVolumeEnvelope {
    #[inline(always)]
    pub fn read(self) -> u8 {
        self.into()
    }
    #[inline(always)]
    pub fn write(&mut self, value: u8) {
        *self = value.into();
    }
}

#[bitfield(u16, order = lsb0)]
#[derive(Default, Debug, TransparentWrapper, Clone, Copy, PartialEq)]
pub struct ChannelPeriodControl {
    #[bits(0..=7, overlay="registers")]
    low: u8,
    #[bits(8..=15, overlay="registers")]
    high: u8,
    #[bits(0..=10, default = 0b111_1111_1111, overlay="default")]
    period: u16,
    #[bits(15, default = true, overlay = "default")]
    trigger: bool,
    #[bits(14, default = false, overlay = "default")]
    length_enable: bool,
    #[bits(11..=13, default = 0b111, overlay="default")]
    _unused: u8,
}

impl ChannelPeriodControl {
    #[inline(always)]
    pub fn read(self) -> u16 {
        (self | 0b1011_1111_1111_1111).into()
    }

    #[inline(always)]
    pub fn write(&mut self, value: u16) {
        *self = value.into()
    }
}

#[bitfield(u8, order = lsb0)]
#[derive(Default, Debug, TransparentWrapper, Clone, Copy, PartialEq)]
pub struct ChannelDacEnable {
    #[bits(7, default = true)]
    enable: bool,
    #[bits(0..=6, default = 0xFF)]
    _unused: u8,
}
impl ChannelDacEnable {
    #[inline(always)]
    pub fn read(self) -> u8 {
        (self | 0b0111_1111).into()
    }

    #[inline(always)]
    pub fn write(&mut self, value: u8) {
        *self = (value | 0b0111_1111).into();
    }
}
#[bitfield(u8, order = lsb0)]
#[derive(Default, Debug, TransparentWrapper, Clone, Copy, PartialEq)]
pub struct ChannelLengthTimer {
    #[bits(0..=7, default = 0xFF)]
    length_timer: u8,
}
impl ChannelLengthTimer {
    #[inline(always)]
    pub fn read(self) -> u8 {
        0b11111111
    }

    #[inline(always)]
    pub fn write(&mut self, value: u8) {
        *self = value.into();
    }
}

#[bitfield(u8, order = lsb0)]
#[derive(Default, Debug, TransparentWrapper, Clone, Copy, PartialEq)]
pub struct ChannelLengthTimerShort {
    #[bits(0..=5, default = 0xFF)]
    length_timer: u8,
    #[bits(6..=7, default = 0xFF)]
    _unused: u8,
}
impl ChannelLengthTimerShort {
    #[inline(always)]
    pub fn read(self) -> u8 {
        0b1111_1111
    }

    #[inline(always)]
    pub fn write(&mut self, value: u8) {
        *self = (value | 0b1100_0000).into();
    }
}

#[bitfield(u8, order = lsb0)]
#[derive(Default, Debug, TransparentWrapper, Clone, Copy, PartialEq)]
pub struct ChannelVolume {
    #[bits(0..=4, default = 0xFF)]
    _unused: u8,
    #[bits(5..=6)]
    volume: u8,
    #[bits(7, default = true)]
    _unused2: bool,
}
impl ChannelVolume {
    #[inline(always)]
    pub fn read(self) -> u8 {
        (self | 0b10011111).into()
    }

    #[inline(always)]
    pub fn write(&mut self, value: u8) {
        *self = (value | 0b10011111).into();
    }
}

#[derive(Default, Debug, FromRepr, PartialEq, Eq, Clone, Copy, BitEnum)]
#[repr(u8)]
pub enum LfsrWidth {
    #[default]
    #[fallback]
    Fifteen = 0,
    Seven = 1,
}

impl From<bool> for LfsrWidth {
    #[inline(always)]
    fn from(value: bool) -> Self {
        match value {
            false => LfsrWidth::Fifteen,
            true => LfsrWidth::Seven,
        }
    }
}

impl From<LfsrWidth> for bool {
    #[inline(always)]
    fn from(value: LfsrWidth) -> Self {
        match value {
            LfsrWidth::Fifteen => false,
            LfsrWidth::Seven => true,
        }
    }
}

#[bitfield(u8, order=lsb0)]
#[derive(Default, Clone, Copy, Debug, PartialEq, TransparentWrapper)]
pub struct ClockDivider {
    #[bits(0..=2, default=0b111)]
    value: u8,
}

impl ClockDivider {
    pub fn get(self) -> f64 {
        (self.value() as f64).max(0.5)
    }
}

#[bitfield(u8, order=lsb0)]
#[derive(Default, Debug, TransparentWrapper, Copy, Clone, PartialEq)]
pub struct ChannelFrequencyRandomness {
    #[bits(4..=7, default = 0xFF)]
    clock_shift: u8,
    #[bits(3, default = LfsrWidth::Seven)]
    lfsr_width: LfsrWidth,
    #[bits(0..=2, default = ClockDivider::default())]
    clock_divider: ClockDivider,
}
impl ChannelFrequencyRandomness {
    #[inline(always)]
    pub fn read(self) -> u8 {
        self.into()
    }

    #[inline(always)]
    pub fn write(&mut self, value: u8) {
        *self = value.into();
    }
}
#[bitfield(u8, order = lsb0)]
#[derive(Default, Debug, TransparentWrapper, Clone, Copy, PartialEq)]
pub struct ChannelControl {
    #[bits(7, default = true)]
    trigger: bool,
    #[bits(6, default = true)]
    length_enable: bool,
    #[bits(0..=5, default = 0xFF)]
    _unused: u8,
}

impl ChannelControl {
    #[inline(always)]
    pub fn read(self) -> u8 {
        (self | 0b10111111).into()
    }
    #[inline(always)]
    pub fn write(&mut self, value: u8) {
        *self = (value | 0b00111111).into();
    }
}
