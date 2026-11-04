use std::io;
use std::process::ExitCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitStatus {
    Success,
    ProgramErrors,
    UsageError,
    Environment,
    Program(u8),
}

impl ExitStatus {
    pub fn code(self) -> u8 {
        match self {
            ExitStatus::Success => 0,
            ExitStatus::ProgramErrors => 1,
            ExitStatus::UsageError | ExitStatus::Environment => 2,
            ExitStatus::Program(code) => code,
        }
    }

    pub fn report_unreadable_input(error: &io::Error) -> Self {
        eprintln!("error: cannot read standard input: {error}");
        Self::UsageError
    }
}

impl From<ExitStatus> for ExitCode {
    fn from(status: ExitStatus) -> Self {
        ExitCode::from(status.code())
    }
}
