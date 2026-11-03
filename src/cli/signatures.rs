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

    pub fn run(self, terminal: &mut Terminal) -> ExitStatus {
        match terminal.check(&self.file) {
            Ok(program) => terminal.write_output(program.entry_signatures()),
            Err(status) => status,
        }
    }
}
