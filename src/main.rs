use std::process::ExitCode;

use clap::Parser;
use rslisp::cli::Cli;
use rslisp::counters::CountingAllocator;

#[cfg(feature = "counters")]
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

#[cfg(feature = "counters")]
const COUNTED_ALLOCATOR: Option<&CountingAllocator> = Some(&ALLOCATOR);

#[cfg(not(feature = "counters"))]
const COUNTED_ALLOCATOR: Option<&CountingAllocator> = None;

fn main() -> ExitCode {
    Cli::parse().run(COUNTED_ALLOCATOR)
}
