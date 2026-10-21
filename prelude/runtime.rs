use std::ops::{Add, Div, Mul, Neg, Rem, Sub};

pub trait Number:
    Copy + PartialOrd + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> + Div<Output = Self> + Rem<Output = Self>
{
    fn from_literal(value: i128) -> Self;

    fn from_float_literal(value: f64) -> Self;
}

pub trait Signed: Number + Neg<Output = Self> {}

pub trait Float: Signed {}

pub trait Integer: Number {}

macro_rules! numbers {
    ($($number:ty),*) => {
        $(
            impl Number for $number {
                fn from_literal(value: i128) -> Self {
                    value as Self
                }

                fn from_float_literal(value: f64) -> Self {
                    value as Self
                }
            }
        )*
    };
}

numbers!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64);

impl<T> Signed for T where T: Number + Neg<Output = T> {}

impl Float for f32 {}

impl Float for f64 {}

macro_rules! integers {
    ($($integer:ty),*) => {
        $(
            impl Integer for $integer {}
        )*
    };
}

integers!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);
