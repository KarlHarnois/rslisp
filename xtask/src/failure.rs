use std::fmt;
use std::io;
use std::path::Path;
use std::process::{ExitCode, Output};

#[derive(Debug)]
pub struct TaskFailure {
    message: String,
}

impl TaskFailure {
    pub fn new(message: String) -> Self {
        Self { message }
    }

    pub fn tool_missing(tool: &str, error: io::Error) -> Self {
        Self::new(format!(
            "cannot run `{tool}`: {error}. Run the task inside `nix develop`."
        ))
    }

    pub fn tests_failed(output: &Output) -> Self {
        Self::new(format!(
            "the instrumented test run failed\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }

    pub fn unreadable(path: &Path, error: io::Error) -> Self {
        Self::new(format!("cannot read `{}`: {error}", path.display()))
    }

    pub fn unwritable(path: &Path, error: io::Error) -> Self {
        Self::new(format!("cannot write `{}`: {error}", path.display()))
    }

    pub fn unparsable(path: &Path, error: &syn::Error) -> Self {
        Self::new(format!("cannot parse `{}`: {error}", path.display()))
    }

    pub fn constant_missing(path: &Path, owner: &str, name: &str) -> Self {
        Self::new(format!(
            "`{}` defines no string constant `{owner}::{name}`",
            path.display()
        ))
    }

    pub fn malformed_tag(tag: &str) -> Self {
        Self::new(format!(
            "cannot read the release tag {tag:?}: expected v<major>.<minor>.<patch>"
        ))
    }

    pub fn malformed_version(version: &str) -> Self {
        Self::new(format!(
            "cannot read the version {version:?}: expected <major>.<minor>.<patch>"
        ))
    }

    pub fn manifest_key_missing(file: &str, anchor: &str, key: &str) -> Self {
        Self::new(format!("{file} has no `{key}` under `{anchor}`"))
    }

    pub fn pulls_unparsable(error: &serde_json::Error) -> Self {
        Self::new(format!("cannot parse the pull requests: {error}"))
    }

    pub fn pulls_not_a_list() -> Self {
        Self::new("the pull requests are not a JSON list".to_string())
    }

    pub fn pull_malformed(pull: &serde_json::Value) -> Self {
        Self::new(format!(
            "the pull request {pull} lacks a number, a title or a list of labels"
        ))
    }

    pub fn report(&self) -> ExitCode {
        eprintln!("error: {self}");
        ExitCode::FAILURE
    }
}

impl fmt::Display for TaskFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
