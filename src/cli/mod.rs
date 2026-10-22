mod build;
mod check;
mod emit;
mod exit_status;
mod expand;
mod run;
mod signatures;
mod terminal;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::build::BuildProfile;
use crate::language::Language;
use build::BuildCommand;
use check::CheckCommand;
use emit::EmitCommand;
use exit_status::ExitStatus;
use expand::ExpandCommand;
use run::RunCommand;
use signatures::SignaturesCommand;
use terminal::BuildOptions;

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
    #[command(about = "Print the inferred signature of each function in a program")]
    Signatures { file: PathBuf },
    #[command(about = "Print a program as kernel forms after macro expansion")]
    Expand { file: PathBuf },
    #[command(about = "Print the Rust code generated for a program")]
    Emit { file: PathBuf },
    #[command(about = "Compile a program to a native binary")]
    Build {
        file: PathBuf,
        #[command(flatten)]
        release: ReleaseFlag,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    #[command(about = "Build a program and run it")]
    Run {
        file: PathBuf,
        #[command(flatten)]
        release: ReleaseFlag,
        #[arg(last = true)]
        args: Vec<String>,
    },
}

#[derive(Args)]
struct ReleaseFlag {
    #[arg(long)]
    release: bool,
}

impl Cli {
    pub fn run(self) -> ExitCode {
        self.command.execute().into()
    }
}

impl Command {
    fn execute(self) -> ExitStatus {
        match self {
            Command::Check { file } => CheckCommand::new(file).run(),
            Command::Signatures { file } => SignaturesCommand::new(file).run(),
            Command::Expand { file } => ExpandCommand::new(file).run(),
            Command::Emit { file } => EmitCommand::new(file).run(),
            Command::Build {
                file,
                release,
                output,
            } => BuildCommand::new(
                file,
                BuildOptions {
                    profile: release.profile(),
                    output,
                },
            )
            .run(),
            Command::Run {
                file,
                release,
                args,
            } => RunCommand::new(
                file,
                BuildOptions {
                    profile: release.profile(),
                    output: None,
                },
                args,
            )
            .run(),
        }
    }
}

impl ReleaseFlag {
    fn profile(&self) -> BuildProfile {
        if self.release {
            BuildProfile::Release
        } else {
            BuildProfile::Debug
        }
    }
}
