mod build;
mod build_directory;
mod capped_report;
mod check;
mod counter_report;
mod doc;
mod emit;
mod exit_status;
mod expand;
mod run;
mod signatures;
mod stage_work;
mod terminal;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::build::BuildProfile;
use crate::counters::CountingAllocator;
use crate::language::Language;
use build::BuildCommand;
use check::CheckCommand;
use counter_report::CounterReport;
use doc::{DocCommand, DocEnvironment};
use emit::EmitCommand;
use exit_status::ExitStatus;
use expand::ExpandCommand;
use run::RunCommand;
use signatures::SignaturesCommand;
use terminal::{BuildOptions, Terminal};

#[derive(Parser)]
#[command(name = Language::NAME, version, about = "Compile Lisp source files to Rust")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
    #[arg(long, global = true, hide = true)]
    counters: bool,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Read, expand and check a program, then print its diagnostics")]
    Check { file: PathBuf },
    #[command(about = "Print the inferred signature of each function in a program")]
    Signatures { file: PathBuf },
    #[command(
        name = DocCommand::NAME,
        about = "Print the page of a prelude term, or of a program's term with --file"
    )]
    Doc {
        #[arg(allow_hyphen_values = true)]
        term: String,
        #[arg(long)]
        file: Option<PathBuf>,
    },
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
    pub fn run(self, allocator: Option<&CountingAllocator>) -> ExitCode {
        let mut terminal = Terminal::new();
        let status = self.command.execute(&mut terminal);
        if self.counters {
            let mut report = CounterReport::new(allocator);
            report.add_stage_work(terminal.work());
            report.print();
        }
        status.into()
    }
}

impl Command {
    fn execute(self, terminal: &mut Terminal) -> ExitStatus {
        match self {
            Command::Check { file } => CheckCommand::new(file).run(terminal),
            Command::Signatures { file } => SignaturesCommand::new(file).run(terminal),
            Command::Doc { term, file } => {
                DocCommand::new(DocEnvironment::new(file), term).run(terminal)
            }
            Command::Expand { file } => ExpandCommand::new(file).run(terminal),
            Command::Emit { file } => EmitCommand::new(file).run(terminal),
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
            .run(terminal),
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
            .run(terminal),
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
