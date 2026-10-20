mod exit_status;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::language::Language;
use exit_status::ExitStatus;

#[derive(Parser)]
#[command(name = Language::NAME, version, about = "Compile Lisp source files to Rust")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Read, expand and check a program, then print its diagnostics")]
    Check { file: PathBuf },
    #[command(about = "Print a program as kernel forms after macro expansion")]
    Expand { file: PathBuf },
    #[command(about = "Print the Rust code generated for a program")]
    Emit { file: PathBuf },
    #[command(about = "Compile a program to a native binary")]
    Build {
        file: PathBuf,
        #[arg(long)]
        release: bool,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    #[command(about = "Build a program and run it")]
    Run {
        file: PathBuf,
        #[arg(long)]
        release: bool,
        #[arg(last = true)]
        args: Vec<String>,
    },
}

impl Cli {
    pub fn run(self) -> ExitCode {
        self.command.execute().into()
    }
}

impl Command {
    fn execute(self) -> ExitStatus {
        eprintln!("error: `{}` is not implemented yet", self.name());
        ExitStatus::UsageError
    }

    fn name(&self) -> &'static str {
        match self {
            Command::Check { .. } => "check",
            Command::Expand { .. } => "expand",
            Command::Emit { .. } => "emit",
            Command::Build { .. } => "build",
            Command::Run { .. } => "run",
        }
    }
}
