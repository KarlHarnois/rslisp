pub struct DeepRecursion;

impl DeepRecursion {
    const RED_ZONE: usize = 64 * 1024;
    const GROWTH: usize = 1024 * 1024;

    pub fn guard<R>(work: impl FnOnce() -> R) -> R {
        stacker::maybe_grow(Self::RED_ZONE, Self::GROWTH, work)
    }
}
