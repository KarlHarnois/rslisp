use std::path::PathBuf;

use super::exit_status::ExitStatus;
use super::terminal::Terminal;

pub struct CheckCommand {
    file: PathBuf,
}

impl CheckCommand {
    pub fn new(file: PathBuf) -> Self {
        Self { file }
    }

    pub fn run(self) -> ExitStatus {
        match Terminal::new().expand(&self.file) {
            Ok(_) => ExitStatus::Success,
            Err(status) => status,
        }
    }
}
