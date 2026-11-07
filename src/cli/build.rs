use std::path::PathBuf;

use super::exit_status::ExitStatus;
use super::located_program::LocatedProgram;
use super::terminal::{BuildOptions, Terminal};

pub struct BuildCommand {
    file: Option<PathBuf>,
    options: BuildOptions,
}

impl BuildCommand {
    pub fn new(file: Option<PathBuf>, options: BuildOptions) -> Self {
        Self { file, options }
    }

    pub fn run(self, terminal: &mut Terminal) -> ExitStatus {
        match LocatedProgram::locate(self.file, terminal)
            .and_then(|program| terminal.build(&program, &self.options))
        {
            Ok(_) => ExitStatus::Success,
            Err(status) => status,
        }
    }
}
