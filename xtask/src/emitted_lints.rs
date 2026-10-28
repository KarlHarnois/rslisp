use std::fs;
use std::path::Path;
use std::process::Command;

use crate::failure::TaskFailure;

pub struct EmittedLints<'a> {
    root: &'a Path,
}

pub struct LintedProgram {
    pub fixture: String,
    pub warnings: String,
}

impl<'a> EmittedLints<'a> {
    const SNAPSHOTS: &'static str = "tests/snapshots";
    const EMIT_PREFIX: &'static str = "stages__emit_fixtures@";
    const SNAPSHOT_SUFFIX: &'static str = ".rslisp.snap";
    const FENCE: &'static str = "---\n";
    const HEADER_FENCES: usize = 2;
    const DIAGNOSTIC_START: &'static str = "error[";
    const WORK_DIRECTORY: &'static str = "target/emitted-lints";
    const CLIPPY: &'static str = "clippy-driver";
    const EDITION: &'static str = "2024";
    const PROGRAM_CHOICES: [&'static str; 10] = [
        "unused_parens",
        "unused_variables",
        "clippy::box_collection",
        "clippy::double_parens",
        "clippy::if_same_then_else",
        "clippy::len_zero",
        "clippy::redundant_closure_call",
        "clippy::unnecessary_fold",
        "clippy::unnecessary_literal_unwrap",
        "clippy::vec_init_then_push",
    ];
    const UNFIXED: [&'static str; 0] = [];

    pub fn new(root: &'a Path) -> Self {
        Self { root }
    }

    pub fn run(&self) -> Result<Vec<LintedProgram>, TaskFailure> {
        let directory = self.root.join(Self::WORK_DIRECTORY);
        fs::create_dir_all(&directory)
            .map_err(|error| TaskFailure::unwritable(&directory, error))?;
        let mut linted = Vec::new();
        for (fixture, program) in self.emitted_programs()? {
            if let Some(warnings) = Self::lint(&directory, &fixture, &program)? {
                linted.push(LintedProgram { fixture, warnings });
            }
        }
        Ok(linted)
    }

    fn emitted_programs(&self) -> Result<Vec<(String, String)>, TaskFailure> {
        let snapshots = self.root.join(Self::SNAPSHOTS);
        let entries =
            fs::read_dir(&snapshots).map_err(|error| TaskFailure::unreadable(&snapshots, error))?;
        let mut programs = Vec::new();
        for entry in entries {
            let path = entry
                .map_err(|error| TaskFailure::unreadable(&snapshots, error))?
                .path();
            let Some(fixture) = Self::emit_fixture(&path) else {
                continue;
            };
            let text =
                fs::read_to_string(&path).map_err(|error| TaskFailure::unreadable(&path, error))?;
            if let Some(program) = Self::program(&text) {
                programs.push((fixture, program.to_string()));
            }
        }
        programs.sort();
        Ok(programs)
    }

    fn emit_fixture(path: &Path) -> Option<String> {
        path.file_name()?
            .to_str()?
            .strip_prefix(Self::EMIT_PREFIX)?
            .strip_suffix(Self::SNAPSHOT_SUFFIX)
            .map(str::to_string)
    }

    fn program(snapshot: &str) -> Option<&str> {
        snapshot
            .splitn(Self::HEADER_FENCES + 1, Self::FENCE)
            .nth(Self::HEADER_FENCES)
            .filter(|body| !body.starts_with(Self::DIAGNOSTIC_START))
    }

    fn lint(directory: &Path, fixture: &str, program: &str) -> Result<Option<String>, TaskFailure> {
        let source = directory.join(format!("{fixture}.rs"));
        fs::write(&source, program).map_err(|error| TaskFailure::unwritable(&source, error))?;
        let output = Self::clippy(directory, &source)
            .output()
            .map_err(|error| TaskFailure::tool_missing(Self::CLIPPY, error))?;
        if output.status.success() {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(&output.stderr).into_owned()))
    }

    fn clippy(directory: &Path, source: &Path) -> Command {
        let mut command = Command::new(Self::CLIPPY);
        command
            .args(["--edition", Self::EDITION, "--crate-type", "bin"])
            .args(["--crate-name", "emitted", "--emit", "metadata"])
            .args(["-D", "warnings"])
            .args(
                Self::PROGRAM_CHOICES
                    .iter()
                    .chain(&Self::UNFIXED)
                    .flat_map(|lint| ["-A", *lint]),
            )
            .arg("--out-dir")
            .arg(directory)
            .arg(source);
        command
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::EmittedLints;

    #[test]
    fn only_emit_snapshots_name_a_fixture() {
        let emitted = Path::new("tests/snapshots/stages__emit_fixtures@person.rslisp.snap");
        let checked = Path::new("tests/snapshots/stages__check_fixtures@person.rslisp.snap");
        assert_eq!(
            EmittedLints::emit_fixture(emitted),
            Some("person".to_string())
        );
        assert_eq!(EmittedLints::emit_fixture(checked), None);
    }

    #[test]
    fn the_program_follows_the_snapshot_header() {
        let snapshot = "---\nsource: tests/stages.rs\n---\nfn main() {}\n";
        assert_eq!(EmittedLints::program(snapshot), Some("fn main() {}\n"));
    }

    #[test]
    fn a_diagnostic_snapshot_holds_no_program() {
        let snapshot = "---\nsource: tests/stages.rs\n---\nerror[L0401]: mismatched types\n";
        assert_eq!(EmittedLints::program(snapshot), None);
    }
}
