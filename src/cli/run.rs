use std::path::PathBuf;

use super::exit_status::ExitStatus;
use super::finished_program::FinishedProgram;
use super::located_program::LocatedProgram;
use super::terminal::{BuildOptions, Terminal};

pub struct RunCommand {
    file: Option<PathBuf>,
    options: BuildOptions,
    arguments: Vec<String>,
}

impl RunCommand {
    pub fn new(file: Option<PathBuf>, options: BuildOptions, arguments: Vec<String>) -> Self {
        Self {
            file,
            options,
            arguments,
        }
    }

    pub fn run(self, terminal: &mut Terminal) -> ExitStatus {
        match LocatedProgram::locate(self.file, terminal)
            .and_then(|program| terminal.build(&program, &self.options))
        {
            Ok(binary) => FinishedProgram::run_with(&binary, &self.arguments).run_status(),
            Err(status) => status,
        }
    }
}
