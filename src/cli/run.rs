use std::path::{Path, PathBuf};
use std::process::{self, Command};

use super::exit_status::ExitStatus;
use super::terminal::{BuildOptions, Terminal};

pub struct RunCommand {
    file: PathBuf,
    options: BuildOptions,
    arguments: Vec<String>,
}

impl RunCommand {
    const KILLED_BY_SIGNAL: u8 = 1;
    #[cfg(unix)]
    const SIGPIPE: i32 = 13;

    pub fn new(file: PathBuf, options: BuildOptions, arguments: Vec<String>) -> Self {
        Self {
            file,
            options,
            arguments,
        }
    }

    pub fn run(self, terminal: &mut Terminal) -> ExitStatus {
        match terminal.build(&self.file, &self.options) {
            Ok(binary) => self.execute(&binary),
            Err(status) => status,
        }
    }

    fn execute(&self, binary: &Path) -> ExitStatus {
        match Command::new(binary).args(&self.arguments).status() {
            Ok(status) => Self::exit_status_of(status),
            Err(error) => {
                eprintln!("error: cannot run `{}`: {error}", binary.display());
                ExitStatus::UsageError
            }
        }
    }

    fn exit_status_of(status: process::ExitStatus) -> ExitStatus {
        if Self::stopped_by_a_closed_output(status) {
            return ExitStatus::Success;
        }
        ExitStatus::Program(
            status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .unwrap_or(Self::KILLED_BY_SIGNAL),
        )
    }

    #[cfg(unix)]
    fn stopped_by_a_closed_output(status: process::ExitStatus) -> bool {
        use std::os::unix::process::ExitStatusExt;
        status.signal() == Some(Self::SIGPIPE)
    }

    #[cfg(not(unix))]
    fn stopped_by_a_closed_output(_status: process::ExitStatus) -> bool {
        false
    }
}
