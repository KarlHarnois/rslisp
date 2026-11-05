use std::io;
use std::path::Path;
use std::process::{self, Command};

use super::exit_status::ExitStatus;
use super::standard_error::StandardError;
use crate::build::InterruptSignal;

pub enum FinishedProgram {
    Exited(process::ExitStatus),
    NotStarted,
}

impl FinishedProgram {
    const KILLED_BY_SIGNAL: u8 = 1;
    #[cfg(unix)]
    const SIGPIPE: i32 = 13;

    pub fn run(binary: &Path) -> Self {
        Self::run_with(binary, &[])
    }

    pub fn run_with(binary: &Path, arguments: &[String]) -> Self {
        Self::of(binary, Command::new(binary).args(arguments).status())
    }

    pub fn of(binary: &Path, status: io::Result<process::ExitStatus>) -> Self {
        match status {
            Ok(status) => Self::Exited(status),
            Err(error) => {
                StandardError::write_line(format_args!(
                    "error: cannot run `{}`: {error}",
                    binary.display()
                ));
                Self::NotStarted
            }
        }
    }

    pub fn run_status(&self) -> ExitStatus {
        self.status_or(|_| Self::KILLED_BY_SIGNAL)
    }

    pub fn shell_status(&self) -> ExitStatus {
        if self.was_interrupted() {
            return ExitStatus::Interrupted;
        }
        self.status_or(|status| {
            Self::signal(status)
                .and_then(|signal| u8::try_from(i32::from(ExitStatus::SIGNAL_BASE) + signal).ok())
                .unwrap_or(Self::KILLED_BY_SIGNAL)
        })
    }

    fn was_interrupted(&self) -> bool {
        matches!(self, Self::Exited(status) if InterruptSignal::stopped(*status))
    }

    fn status_or(&self, killed: impl FnOnce(process::ExitStatus) -> u8) -> ExitStatus {
        let Self::Exited(status) = self else {
            return ExitStatus::Environment;
        };
        if Self::stopped_by_a_closed_output(*status) {
            return ExitStatus::Success;
        }
        let code = status.code().and_then(|code| u8::try_from(code).ok());
        ExitStatus::Program(code.unwrap_or_else(|| killed(*status)))
    }

    #[cfg(unix)]
    fn stopped_by_a_closed_output(status: process::ExitStatus) -> bool {
        Self::signal(status) == Some(Self::SIGPIPE)
    }

    #[cfg(not(unix))]
    fn stopped_by_a_closed_output(_status: process::ExitStatus) -> bool {
        false
    }

    #[cfg(unix)]
    fn signal(status: process::ExitStatus) -> Option<i32> {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    }

    #[cfg(not(unix))]
    fn signal(_status: process::ExitStatus) -> Option<i32> {
        None
    }
}
