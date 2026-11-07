use std::path::PathBuf;

use super::exit_status::ExitStatus;
use super::located_program::LocatedProgram;
use super::terminal::Terminal;

pub struct EmitCommand {
    file: Option<PathBuf>,
}

impl EmitCommand {
    pub fn new(file: Option<PathBuf>) -> Self {
        Self { file }
    }

    pub fn run(self, terminal: &mut Terminal) -> ExitStatus {
        match LocatedProgram::locate(self.file, terminal)
            .and_then(|program| terminal.emit(program.file()))
        {
            Ok(emitted) => terminal.write_output(&emitted.code),
            Err(status) => status,
        }
    }
}
