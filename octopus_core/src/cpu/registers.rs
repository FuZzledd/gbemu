use core::ops::Deref;

use core::ops::DerefMut;

#[derive(Default, Debug)]
pub struct Registers {
    pub sp: u16,
    pub a: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub f: FlagsRegister,
    pub h: u8,
    pub l: u8,
}

impl Registers {
    pub(crate) fn af(&self) -> u16 {
        (self.a as u16) << 8 | *self.f as u16
    }
    pub(crate) fn set_af(&mut self, value: u16) {
        self.a = ((value & 0xFF00) >> 8) as u8;
        *self.f = (value & 0xF0) as u8;
    }

    pub(crate) fn bc(&self) -> u16 {
        (self.b as u16) << 8 | self.c as u16
    }
    pub(crate) fn set_bc(&mut self, value: u16) {
        self.b = ((value & 0xFF00) >> 8) as u8;
        self.c = (value & 0xFF) as u8;
    }
    pub(crate) fn de(&self) -> u16 {
        (self.d as u16) << 8 | self.e as u16
    }
    pub(crate) fn set_de(&mut self, value: u16) {
        self.d = ((value & 0xFF00) >> 8) as u8;
        self.e = (value & 0xFF) as u8;
    }

    pub(crate) fn hl(&self) -> u16 {
        (self.h as u16) << 8 | self.l as u16
    }
    pub(crate) fn set_hl(&mut self, value: u16) {
        self.h = ((value & 0xFF00) >> 8) as u8;
        self.l = (value & 0xFF) as u8;
    }
}

#[repr(transparent)]
#[derive(Default, Debug, Copy, Clone)]
pub struct FlagsRegister(u8);

impl FlagsRegister {
    pub(crate) fn new(value: u8) -> Self {
        Self(value)
    }

    pub(crate) fn zero(&self) -> bool {
        (self.0 >> 7) & 1 != 0
    }
    pub(crate) fn subtract(&self) -> bool {
        (self.0 >> 6) & 1 != 0
    }
    pub(crate) fn half_carry(&self) -> bool {
        (self.0 >> 5) & 1 != 0
    }
    pub(crate) fn carry(&self) -> bool {
        (self.0 >> 4) & 1 != 0
    }

    pub(crate) fn set_zero(&mut self, value: bool) -> &mut Self {
        self.0 &= !(1 << 7);
        self.0 |= u8::from(value) << 7;
        self
    }
    pub(crate) fn set_subtract(&mut self, value: bool) -> &mut Self {
        self.0 &= !(1 << 6);
        self.0 |= u8::from(value) << 6;
        self
    }
    pub(crate) fn set_half_carry(&mut self, value: bool) -> &mut Self {
        self.0 &= !(1 << 5);
        self.0 |= u8::from(value) << 5;
        self
    }
    pub(crate) fn set_carry(&mut self, value: bool) -> &mut Self {
        self.0 &= !(1 << 4);
        self.0 |= u8::from(value) << 4;
        self
    }
}

impl From<u8> for FlagsRegister {
    fn from(value: u8) -> Self {
        Self::new(value)
    }
}

impl DerefMut for FlagsRegister {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Deref for FlagsRegister {
    type Target = u8;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
