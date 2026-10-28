use std::process::{Command, Output};

use super::callgrind::Callgrind;
use super::corpus::Corpus;
use super::git::Git;
use super::header::ResultHeader;
use crate::failure::TaskFailure;

pub struct MeasurementMethod<'a> {
    git: Git<'a>,
}

impl<'a> MeasurementMethod<'a> {
    const REV_PARSE: &'static str = "rev-parse";
    const REVISION: &'static str = "HEAD";
    const RUSTC: &'static str = "rustc";
    const VERSION_FLAG: &'static str = "--version";
    const TOOLCHAIN_LOCK: &'static str = "flake.lock";
    const DEV_SHELL: &'static str = "flake.nix";
    const BENCHMARK_CODE: &'static str = "xtask/src/bench";
    const KEY_SEPARATOR: &'static str = "-";

    pub fn new(git: Git<'a>) -> Self {
        Self { git }
    }

    pub fn keyed_header(&self) -> Result<ResultHeader, TaskFailure> {
        Ok(self.unkeyed_header()?.keyed(self.key()?))
    }

    pub fn unkeyed_header(&self) -> Result<ResultHeader, TaskFailure> {
        Ok(ResultHeader::new(
            self.version_of(Self::RUSTC)?,
            self.version_of(Callgrind::VALGRIND)?,
        ))
    }

    pub fn key(&self) -> Result<String, TaskFailure> {
        let objects = Self::keyed_objects();
        let arguments: Vec<&str> = objects.iter().map(String::as_str).collect();
        let hashes = Self::stdout_of(Git::PROGRAM, self.git.run(&arguments)?)?;
        Ok(hashes
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(Self::KEY_SEPARATOR))
    }

    fn keyed_objects() -> Vec<String> {
        let inputs = [
            Self::TOOLCHAIN_LOCK,
            Self::DEV_SHELL,
            Self::BENCHMARK_CODE,
            Corpus::DIRECTORY,
        ];
        let objects = inputs.map(|path| format!("{}:{path}", Self::REVISION));
        [Self::REV_PARSE.to_string()]
            .into_iter()
            .chain(objects)
            .collect()
    }

    fn version_of(&self, tool: &str) -> Result<String, TaskFailure> {
        let output = Command::new(tool)
            .arg(Self::VERSION_FLAG)
            .output()
            .map_err(|error| TaskFailure::tool_missing(tool, error))?;
        Ok(Self::stdout_of(tool, output)?.trim().to_string())
    }

    fn stdout_of(tool: &str, output: Output) -> Result<String, TaskFailure> {
        if !output.status.success() {
            return Err(TaskFailure::tool_failed(tool, &output.stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
