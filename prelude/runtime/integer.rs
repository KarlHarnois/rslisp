pub trait Integer: Number {}

macro_rules! integers {
    ($($integer:ty),*) => {
        $(
            impl Integer for $integer {}
        )*
    };
}

integers!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);
