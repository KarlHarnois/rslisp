use std::path::PathBuf;

use super::exit_status::ExitStatus;
use super::terminal::Terminal;

pub struct SignaturesCommand {
    file: PathBuf,
}

impl SignaturesCommand {
    pub fn new(file: PathBuf) -> Self {
        Self { file }
    }

    pub fn run(self) -> ExitStatus {
        match Terminal::new().check(&self.file) {
            Ok(program) => {
                print!("{}", program.entry_signatures());
                ExitStatus::Success
            }
            Err(status) => status,
        }
    }
}
