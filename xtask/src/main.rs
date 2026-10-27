mod badge;
mod checks;
mod complexity;
mod coverage;
mod crap;
mod emitted_lints;
mod failure;
mod functions;
mod gate;
mod instrumented_tests;
mod lcov;
mod platform;

use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use gate::QualityGate;
use platform::Platform;

#[derive(Parser)]
#[command(about = "Development tasks for this repository")]
struct Tasks {
    #[command(subcommand)]
    task: Task,
}

#[derive(Subcommand)]
enum Task {
    #[command(
        about = "Run the tests with coverage, then check the coverage floor, the CRAP limit, the README badge and the emitted Rust's lints"
    )]
    Coverage,
}

impl Task {
    fn run(self) -> ExitCode {
        let manifest_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_directory.parent().unwrap_or(manifest_directory);
        match self {
            Task::Coverage => QualityGate::new(workspace_root, Platform::current()).run(),
        }
    }
}

fn main() -> ExitCode {
    Tasks::parse().task.run()
}
