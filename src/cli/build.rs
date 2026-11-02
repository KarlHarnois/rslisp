use std::path::PathBuf;

use super::exit_status::ExitStatus;
use super::terminal::{BuildOptions, Terminal};

pub struct BuildCommand {
    file: PathBuf,
    options: BuildOptions,
}

impl BuildCommand {
    pub fn new(file: PathBuf, options: BuildOptions) -> Self {
        Self { file, options }
    }

    pub fn run(self, terminal: &mut Terminal) -> ExitStatus {
        match terminal.build(&self.file, &self.options) {
            Ok(_) => ExitStatus::Success,
            Err(status) => status,
        }
    }
}
