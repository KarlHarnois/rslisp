use std::ops::{Add, Div, Mul, Neg, Rem, Sub};

pub trait Number:
    Copy + PartialOrd + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> + Div<Output = Self> + Rem<Output = Self>
{
}

impl<T> Number for T where
    T: Copy + PartialOrd + Add<Output = T> + Sub<Output = T> + Mul<Output = T> + Div<Output = T> + Rem<Output = T>
{
}

pub trait Signed: Number + Neg<Output = Self> {}

impl<T> Signed for T where T: Number + Neg<Output = T> {}
