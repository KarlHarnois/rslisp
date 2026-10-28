use std::iter;
use std::path::Path;
use std::process::Command;

use super::callgrind::Callgrind;
use super::corpus::Corpus;
use super::header::ResultHeader;
use crate::failure::TaskFailure;

pub struct MeasurementMethod<'a> {
    workspace: &'a Path,
}

impl<'a> MeasurementMethod<'a> {
    const GIT: &'static str = "git";
    const REV_PARSE: &'static str = "rev-parse";
    const REVISION: &'static str = "HEAD";
    const RUSTC: &'static str = "rustc";
    const VERSION_FLAG: &'static str = "--version";
    const TOOLCHAIN_LOCK: &'static str = "flake.lock";
    const BENCHMARK_CODE: &'static str = "xtask/src/bench";
    const KEY_SEPARATOR: &'static str = "-";

    pub fn new(workspace: &'a Path) -> Self {
        Self { workspace }
    }

    pub fn header(&self) -> Result<ResultHeader, TaskFailure> {
        Ok(ResultHeader::new(
            self.key()?,
            self.version_of(Self::RUSTC)?,
            self.version_of(Callgrind::VALGRIND)?,
        ))
    }

    pub fn key(&self) -> Result<String, TaskFailure> {
        let objects = self.run(Self::GIT, &Self::keyed_objects())?;
        Ok(objects
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(Self::KEY_SEPARATOR))
    }

    fn keyed_objects() -> Vec<String> {
        let inputs = [
            Self::TOOLCHAIN_LOCK,
            Self::BENCHMARK_CODE,
            Corpus::DIRECTORY,
        ];
        let objects = inputs.map(|path| format!("{}:{path}", Self::REVISION));
        iter::once(Self::REV_PARSE.to_string())
            .chain(objects)
            .collect()
    }

    fn version_of(&self, tool: &str) -> Result<String, TaskFailure> {
        Ok(self.run(tool, &[Self::VERSION_FLAG])?.trim().to_string())
    }

    fn run(&self, tool: &str, arguments: &[impl AsRef<str>]) -> Result<String, TaskFailure> {
        let output = Command::new(tool)
            .args(arguments.iter().map(AsRef::as_ref))
            .current_dir(self.workspace)
            .output()
            .map_err(|error| TaskFailure::tool_missing(tool, error))?;
        if !output.status.success() {
            return Err(TaskFailure::tool_failed(tool, &output.stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
