pub trait Signed: Number + Neg<Output = Self> {}

impl<T> Signed for T where T: Number + Neg<Output = T> {}
