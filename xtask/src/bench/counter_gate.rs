pub struct CounterGate;

impl CounterGate {
    pub const GATED_COUNTERS: [&'static str; 2] =
        ["allocator.allocated_bytes", "allocator.peak_live_bytes"];
    pub const TOLERANCE_PERCENT: u64 = 1;
}
