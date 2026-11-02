use std::process::ExitCode;

use clap::Parser;
use rslisp::cli::Cli;
#[cfg(feature = "counters")]
use rslisp::counters::CountingAllocator;

#[cfg(feature = "counters")]
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

fn main() -> ExitCode {
    Cli::parse().run()
}
