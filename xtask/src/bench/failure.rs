use std::io;
use std::path::Path;

use crate::failure::TaskFailure;

impl TaskFailure {
    pub fn valgrind_missing(error: io::Error) -> Self {
        Self::new(format!(
            "cannot run `valgrind`: {error}. Run the benchmark inside `nix develop` on Linux."
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
}
