#![feature(uint_gather_scatter_bits)]

use core::ops::{BitAnd, BitOr, Not, Shl, Shr};

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
