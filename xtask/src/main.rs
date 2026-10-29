mod badge;
mod bench;
mod cargo;
mod checks;
mod complexity;
mod coverage;
mod crap;
mod emitted_lints;
mod failure;
mod functions;
mod gate;
mod instrumented_tests;
mod language;
mod lcov;
mod release;

use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use bench::BenchTask;
use gate::QualityGate;
use release::{NextReleaseTask, ReleaseNotesTask, SetVersionTask};

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
    #[command(
        subcommand,
        about = "Measure the compiler's front end under callgrind, or compare two measurements"
    )]
    Bench(BenchTask),
    #[command(
        about = "Print the tag that follows the latest release, bumped by the pull requests' labels"
    )]
    NextRelease(NextReleaseTask),
    #[command(about = "Write a version into the crate's manifest and its lock file entry")]
    SetVersion(SetVersionTask),
    #[command(
        about = "Print the release notes: the pull requests with a release label first, then the rest"
    )]
    ReleaseNotes(ReleaseNotesTask),
}

impl Task {
    fn run(self) -> ExitCode {
        let manifest_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_directory.parent().unwrap_or(manifest_directory);
        match self {
            Task::Coverage => QualityGate::new(workspace_root).run(),
            Task::Bench(task) => task.run(workspace_root),
            Task::NextRelease(release) => release.run(),
            Task::SetVersion(version) => version.run(workspace_root),
            Task::ReleaseNotes(notes) => notes.run(),
        }
    }
}

fn main() -> ExitCode {
    Tasks::parse().task.run()
}
