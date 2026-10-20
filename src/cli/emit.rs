use std::path::PathBuf;

use super::exit_status::ExitStatus;
use super::terminal::Terminal;

pub struct EmitCommand {
    file: PathBuf,
}

impl EmitCommand {
    pub fn new(file: PathBuf) -> Self {
        Self { file }
    }

    pub fn run(self) -> ExitStatus {
        match Terminal::new().emit(&self.file) {
            Ok(emitted) => {
                print!("{}", emitted.code);
                ExitStatus::Success
            }
            Err(status) => status,
        }
    }
}
