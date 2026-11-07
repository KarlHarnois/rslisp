use std::process::ExitCode;

use knave::cli::Cli;
use knave::counters::CountingAllocator;

#[cfg(feature = "counters")]
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

#[cfg(feature = "counters")]
const COUNTED_ALLOCATOR: Option<&CountingAllocator> = Some(&ALLOCATOR);

#[cfg(not(feature = "counters"))]
const COUNTED_ALLOCATOR: Option<&CountingAllocator> = None;

fn main() -> ExitCode {
    Cli::parse_arguments().run(COUNTED_ALLOCATOR)
}
