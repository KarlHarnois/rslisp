use std::path::PathBuf;

use super::exit_status::ExitStatus;
use super::located_program::LocatedProgram;
use super::terminal::Terminal;
use crate::core::CorePrinter;

pub struct ExpandCommand {
    file: Option<PathBuf>,
}

impl ExpandCommand {
    pub fn new(file: Option<PathBuf>) -> Self {
        Self { file }
    }

    pub fn run(self, terminal: &mut Terminal) -> ExitStatus {
        match LocatedProgram::locate(self.file, terminal)
            .and_then(|program| terminal.expand(program.file()))
        {
            Ok(program) => terminal
                .write_output(CorePrinter::new(&program.bindings).module(program.entry_module())),
            Err(status) => status,
        }
    }
}
