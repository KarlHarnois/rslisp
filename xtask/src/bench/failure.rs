use std::fmt;
use std::io;
use std::path::Path;
use std::process::ExitCode;

#[derive(Debug)]
pub struct BenchFailure {
    message: String,
}

impl BenchFailure {
    pub fn tool_missing(tool: &str, error: io::Error) -> Self {
        Self::new(format!(
            "cannot run `{tool}`: {error}. Run the benchmark inside `nix develop` on Linux."
        ))
    }

    pub fn build_failed(target: &str, stderr: &[u8]) -> Self {
        Self::new(format!(
            "cannot build {target}\n{}",
            String::from_utf8_lossy(stderr)
        ))
    }

    pub fn profile_missing(program: &Path, stderr: &[u8]) -> Self {
        Self::new(format!(
            "callgrind wrote no profile for `{}`\n{}",
            program.display(),
            String::from_utf8_lossy(stderr)
        ))
    }

    pub fn malformed_profile(missing: &str) -> Self {
        Self::new(format!("the callgrind profile has no `{missing}` line"))
    }

    pub fn malformed_results(path: &Path, line: &str) -> Self {
        Self::new(format!(
            "cannot read the line `{line}` of `{}`",
            path.display()
        ))
    }

    pub fn unreadable(path: &Path, error: io::Error) -> Self {
        Self::new(format!("cannot read `{}`: {error}", path.display()))
    }

    pub fn unwritable(path: &Path, error: io::Error) -> Self {
        Self::new(format!("cannot write `{}`: {error}", path.display()))
    }

    pub fn report(&self) -> ExitCode {
        eprintln!("error: {self}");
        ExitCode::FAILURE
    }

    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for BenchFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
