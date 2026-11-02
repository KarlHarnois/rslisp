use std::path::PathBuf;

use super::exit_status::ExitStatus;
use super::terminal::Terminal;
use crate::core::CorePrinter;

pub struct ExpandCommand {
    file: PathBuf,
}

impl ExpandCommand {
    pub fn new(file: PathBuf) -> Self {
        Self { file }
    }

    pub fn run(self, terminal: &mut Terminal) -> ExitStatus {
        match terminal.expand(&self.file) {
            Ok(program) => {
                print!(
                    "{}",
                    CorePrinter::new(&program.bindings).module(program.entry_module())
                );
                ExitStatus::Success
            }
            Err(status) => status,
        }
    }
}
