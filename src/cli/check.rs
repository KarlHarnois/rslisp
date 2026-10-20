use std::path::PathBuf;

use super::exit_status::ExitStatus;
use super::terminal::Terminal;
use crate::reader::Reader;
use crate::source::SourceId;

pub struct CheckCommand {
    file: PathBuf,
    terminal: Terminal,
}

impl CheckCommand {
    pub fn new(file: PathBuf) -> Self {
        Self {
            file,
            terminal: Terminal::new(),
        }
    }

    pub fn run(mut self) -> ExitStatus {
        match self.terminal.load(&self.file) {
            Ok(entry) => self.check(entry),
            Err(status) => status,
        }
    }

    fn check(&self, entry: SourceId) -> ExitStatus {
        let read = Reader::new(self.terminal.sources().file(entry)).read();
        self.terminal.report(&read.err().unwrap_or_default())
    }
}
