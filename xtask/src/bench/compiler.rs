use std::path::{Path, PathBuf};
use std::process::Command;

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
    target_directory: &'a Path,
}

impl<'a> CompilerBuild<'a> {
    const RELEASE_BUILD: [&'static str; 4] =
        ["build", "--release", "--quiet", "--message-format=json"];
    const UNPARTITIONED_CODEGEN: [&'static str; 2] =
        ["--config", "profile.release.codegen-units=1"];
    const RELEASE_CLEAN: [&'static str; 3] = ["clean", "--release", "--quiet"];
    const PACKAGE_FLAG: &'static str = "--package";
    const PACKAGE_ID_COMMAND: &'static str = "pkgid";
    const TARGET_DIRECTORY_FLAG: &'static str = "--target-dir";
    const MANIFEST: &'static str = "Cargo.toml";

    pub fn new(workspace: &'a Path, target_directory: &'a Path) -> Self {
        Self {
            workspace,
            target_directory,
        }
    }

    pub fn run(&self) -> Result<BuiltCompiler, TaskFailure> {
        self.clean_root_package()?;
        let messages = self.build_release()?;
        self.read_artifacts(&messages)
            .ok_or_else(|| TaskFailure::compiler_missing(self.workspace))
    }

    fn clean_root_package(&self) -> Result<(), TaskFailure> {
        let spec = self.read_root_package_spec()?;
        let arguments = Self::RELEASE_CLEAN
            .into_iter()
            .chain([Self::PACKAGE_FLAG, spec.as_str()]);
        let mut clean = self.cargo_in_target(arguments);
        Self::run_cargo(&mut clean, |stderr| {
            TaskFailure::tool_failed("cargo clean", stderr)
        })?;
        Ok(())
    }

    fn read_root_package_spec(&self) -> Result<String, TaskFailure> {
        let mut pkgid = self.cargo_in_checkout();
        pkgid.arg(Self::PACKAGE_ID_COMMAND);
        let stdout = Self::run_cargo(&mut pkgid, |stderr| {
            TaskFailure::tool_failed("cargo pkgid", stderr)
        })?;
        Ok(Self::package_spec(&stdout))
    }

    fn package_spec(stdout: &[u8]) -> String {
        String::from_utf8_lossy(stdout).trim().to_string()
    }

    fn build_release(&self) -> Result<String, TaskFailure> {
        let mut build = self.release_build();
        let stdout = Self::run_cargo(&mut build, |stderr| {
            TaskFailure::build_failed("the compiler", stderr)
        })?;
        Ok(String::from_utf8_lossy(&stdout).into_owned())
    }

    fn release_build(&self) -> Command {
        let arguments = Self::RELEASE_BUILD
            .into_iter()
            .chain(Self::UNPARTITIONED_CODEGEN);
        self.cargo_in_target(arguments)
    }

    fn cargo_in_target<'b>(&self, arguments: impl IntoIterator<Item = &'b str>) -> Command {
        let mut command = self.cargo_in_checkout();
        command
            .args(arguments)
            .arg(Self::TARGET_DIRECTORY_FLAG)
            .arg(self.target_directory);
        command
    }

    fn cargo_in_checkout(&self) -> Command {
        let mut command = Cargo::command();
        command.current_dir(self.workspace);
        command
    }

    fn run_cargo(
        command: &mut Command,
        failure: impl FnOnce(&[u8]) -> TaskFailure,
    ) -> Result<Vec<u8>, TaskFailure> {
        let output = command
            .output()
            .map_err(|error| TaskFailure::tool_missing("cargo", error))?;
        if !output.status.success() {
            return Err(failure(&output.stderr));
        }
        Ok(output.stdout)
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
        let built = CompilerBuild::new(Path::new("/checkout"), Path::new("/checkout/target"))
            .read_artifacts(MESSAGES);
        assert_eq!(
            built,
            Some(BuiltCompiler {
                executable: PathBuf::from("/checkout/target/release/ledger-lang"),
                crate_name: "ledger_lang".to_string(),
            })
        );
    }

    #[test]
    fn reads_the_package_spec_without_its_line_break() {
        let stdout = b"path+file:///checkout#ledger-lang@0.1.0\n";
        assert_eq!(
            CompilerBuild::package_spec(stdout),
            "path+file:///checkout#ledger-lang@0.1.0"
        );
    }

    #[test]
    fn builds_the_compiler_in_one_codegen_unit() {
        let build = CompilerBuild::new(Path::new("/checkout"), Path::new("/checkout/target"))
            .release_build();
        let arguments: Vec<_> = build.get_args().collect();
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["--config", "profile.release.codegen-units=1"])
        );
    }

    #[test]
    fn finds_no_compiler_outside_the_checkout() {
        let built = CompilerBuild::new(Path::new("/elsewhere"), Path::new("/elsewhere/target"))
            .read_artifacts(MESSAGES);
        assert_eq!(built, None);
    }
}
