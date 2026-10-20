use std::process::ExitCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitStatus {
    Success,
    ProgramErrors,
    UsageError,
    Program(u8),
}

impl From<ExitStatus> for ExitCode {
    fn from(status: ExitStatus) -> Self {
        match status {
            ExitStatus::Success => ExitCode::SUCCESS,
            ExitStatus::ProgramErrors => ExitCode::from(1),
            ExitStatus::UsageError => ExitCode::from(2),
            ExitStatus::Program(code) => ExitCode::from(code),
        }
    }
}
