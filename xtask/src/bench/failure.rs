use std::io;
use std::path::Path;
use std::process::ExitStatus;

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

    pub fn compiler_missing(workspace: &Path) -> Self {
        Self::new(format!(
            "the release build of `{}` produced no binary of its root package",
            workspace.display()
        ))
    }

    pub fn profile_missing(program: &Path, stderr: &[u8]) -> Self {
        Self::new(format!(
            "callgrind wrote no profile for `{}`\n{}",
            program.display(),
            String::from_utf8_lossy(stderr)
        ))
    }

    pub fn run_crashed(program: &Path, status: ExitStatus, stderr: &[u8]) -> Self {
        Self::new(format!(
            "measuring `{}` crashed with {status}\n{}",
            program.display(),
            String::from_utf8_lossy(stderr)
        ))
    }

    pub fn stage_entry_missing(candidates: &[String]) -> Self {
        let entry_point = candidates
            .iter()
            .map(|candidate| format!("`{candidate}`"))
            .collect::<Vec<_>>()
            .join(" or ");
        Self::new(format!(
            "the profile has no call from the command-line module into {entry_point}, \
             so that stage's instructions would count as `other`"
        ))
    }

    pub fn while_measuring(self, program: &Path) -> Self {
        Self::new(format!("while measuring `{}`: {self}", program.display()))
    }

    pub fn malformed_profile(missing: &str) -> Self {
        Self::new(format!("the callgrind profile has no `{missing}` line"))
    }

    pub fn note_missing(commit: &str) -> Self {
        Self::new(format!("`{commit}` has no note of saved results"))
    }

    pub fn note_malformed(commit: &str, line: &str) -> Self {
        Self::new(format!(
            "cannot read the line `{line}` of the note of `{commit}`"
        ))
    }

    pub fn method_differs(commit: &str, noted: Option<&str>, current: &str) -> Self {
        Self::new(format!(
            "the note of `{commit}` was measured by the method `{}`, not by this workspace's `{current}`",
            noted.unwrap_or("unknown")
        ))
    }

    pub fn malformed_results(path: &Path, line: &str) -> Self {
        Self::new(format!(
            "cannot read the line `{line}` of `{}`",
            path.display()
        ))
    }
}
