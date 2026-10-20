use std::process::ExitCode;

use clap::Parser;
use rslisp::cli::Cli;

fn main() -> ExitCode {
    Cli::parse().run()
}
