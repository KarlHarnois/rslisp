use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::cargo::Cargo;
use crate::failure::TaskFailure;

#[derive(Debug, PartialEq, Eq)]
pub struct BuiltCompiler {
    pub executable: PathBuf,
    pub crate_name: String,
}

pub struct CompilerBuild<'a> {
    workspace: &'a Path,
}

impl<'a> CompilerBuild<'a> {
    const RELEASE_BUILD: [&'static str; 5] = [
        "build",
        "--release",
        "--quiet",
        "--message-format=json",
        "--target-dir",
    ];
    const TARGET_DIRECTORY: &'static str = "target";
    const MANIFEST: &'static str = "Cargo.toml";

    pub fn new(workspace: &'a Path) -> Self {
        Self { workspace }
    }

    pub fn run(&self) -> Result<BuiltCompiler, TaskFailure> {
        let output = Cargo::command()
            .args(Self::RELEASE_BUILD)
            .arg(self.workspace.join(Self::TARGET_DIRECTORY))
            .current_dir(self.workspace)
            .output()
            .map_err(|error| TaskFailure::tool_missing("cargo", error))?;
        if !output.status.success() {
            return Err(TaskFailure::build_failed("the compiler", &output.stderr));
        }
        self.read_artifacts(&String::from_utf8_lossy(&output.stdout))
            .ok_or_else(|| TaskFailure::compiler_missing(self.workspace))
    }

    fn read_artifacts(&self, messages: &str) -> Option<BuiltCompiler> {
        let manifest = self.workspace.join(Self::MANIFEST);
        let artifacts: Vec<Artifact> = messages
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .map(Artifact::new)
            .filter(|artifact| artifact.belongs_to(&manifest))
            .collect();
        let binary = artifacts.iter().find_map(Artifact::executable)?;
        let crate_target = artifacts
            .iter()
            .find(|artifact| artifact.is_library())
            .or_else(|| {
                artifacts
                    .iter()
                    .find(|artifact| artifact.executable().is_some())
            })?;
        Some(BuiltCompiler {
            executable: binary,
            crate_name: crate_target.crate_name()?,
        })
    }
}

struct Artifact {
    message: Value,
}

impl Artifact {
    const ARTIFACT_REASON: &'static str = "compiler-artifact";
    const LIBRARY_KIND: &'static str = "lib";
    const BINARY_KIND: &'static str = "bin";

    fn new(message: Value) -> Self {
        Self { message }
    }

    fn belongs_to(&self, manifest: &Path) -> bool {
        self.message["reason"] == Self::ARTIFACT_REASON
            && self.message["manifest_path"].as_str().map(Path::new) == Some(manifest)
    }

    fn executable(&self) -> Option<PathBuf> {
        let is_binary = self.has_kind(Self::BINARY_KIND);
        let executable = self.message["executable"].as_str().map(PathBuf::from);
        executable.filter(|_| is_binary)
    }

    fn is_library(&self) -> bool {
        self.has_kind(Self::LIBRARY_KIND)
    }

    fn crate_name(&self) -> Option<String> {
        let name = self.message["target"]["name"].as_str()?;
        Some(name.replace('-', "_"))
    }

    fn has_kind(&self, kind: &str) -> bool {
        self.message["target"]["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|candidate| candidate == kind))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MESSAGES: &str = include_str!("../../fixtures/bench/cargo-build.json");

    #[test]
    fn takes_the_binary_and_library_crate_of_the_checkout_package() {
        let built = CompilerBuild::new(Path::new("/checkout")).read_artifacts(MESSAGES);
        assert_eq!(
            built,
            Some(BuiltCompiler {
                executable: PathBuf::from("/checkout/target/release/ledger-lang"),
                crate_name: "ledger_lang".to_string(),
            })
        );
    }

    #[test]
    fn finds_no_compiler_outside_the_checkout() {
        let built = CompilerBuild::new(Path::new("/elsewhere")).read_artifacts(MESSAGES);
        assert_eq!(built, None);
    }
}
