use std::path::{Path, PathBuf};
use std::process::Command;

use super::exit_status::ExitStatus;
use super::terminal::{BuildOptions, Terminal};

pub struct RunCommand {
    file: PathBuf,
    options: BuildOptions,
    arguments: Vec<String>,
}

impl RunCommand {
    const KILLED_BY_SIGNAL: u8 = 1;

    pub fn new(file: PathBuf, options: BuildOptions, arguments: Vec<String>) -> Self {
        Self {
            file,
            options,
            arguments,
        }
    }

    pub fn run(self) -> ExitStatus {
        match Terminal::new().build(&self.file, &self.options) {
            Ok(binary) => self.execute(&binary),
            Err(status) => status,
        }
    }

    fn execute(&self, binary: &Path) -> ExitStatus {
        match Command::new(binary).args(&self.arguments).status() {
            Ok(status) => ExitStatus::Program(
                status
                    .code()
                    .and_then(|code| u8::try_from(code).ok())
                    .unwrap_or(Self::KILLED_BY_SIGNAL),
            ),
            Err(error) => {
                eprintln!("error: cannot run `{}`: {error}", binary.display());
                ExitStatus::UsageError
            }
        }
    }
}
