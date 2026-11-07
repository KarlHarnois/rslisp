use std::path::PathBuf;

use super::exit_status::ExitStatus;
use super::located_program::LocatedProgram;
use super::terminal::Terminal;

pub struct TypesCommand {
    file: Option<PathBuf>,
}

impl TypesCommand {
    pub fn new(file: Option<PathBuf>) -> Self {
        Self { file }
    }

    pub fn run(self, terminal: &mut Terminal) -> ExitStatus {
        match LocatedProgram::locate(self.file, terminal)
            .and_then(|program| terminal.check(program.file()))
        {
            Ok(program) => terminal.write_output(program.entry_signatures()),
            Err(status) => status,
        }
    }
}
