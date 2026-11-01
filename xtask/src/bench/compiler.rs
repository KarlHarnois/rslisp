use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::cargo::Cargo;
use crate::failure::TaskFailure;
use crate::workspace::CargoWorkspace;

#[derive(Debug, PartialEq, Eq)]
pub struct BuiltCompiler {
    pub executable: PathBuf,
    pub crate_names: BTreeSet<String>,
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
    const TARGET_DIRECTORY_FLAG: &'static str = "--target-dir";
    const MANIFEST: &'static str = "Cargo.toml";

    pub fn new(workspace: &'a Path, target_directory: &'a Path) -> Self {
        Self {
            workspace,
            target_directory,
        }
    }

    pub fn run(&self) -> Result<BuiltCompiler, TaskFailure> {
        let workspace = CargoWorkspace::read(self.workspace)?;
        self.clean_workspace_packages(&workspace.package_names())?;
        let messages = self.build_release()?;
        let executable = self
            .read_executable(&messages)
            .ok_or_else(|| TaskFailure::compiler_missing(self.workspace))?;
        Ok(BuiltCompiler {
            executable,
            crate_names: workspace.crate_names(),
        })
    }

    fn clean_workspace_packages(
        &self,
        package_names: &BTreeSet<String>,
    ) -> Result<(), TaskFailure> {
        let mut clean = self.release_clean(package_names);
        Cargo::run(&mut clean, |stderr| {
            TaskFailure::tool_failed("cargo clean", stderr)
        })?;
        Ok(())
    }

    fn release_clean(&self, package_names: &BTreeSet<String>) -> Command {
        let packages = package_names
            .iter()
            .flat_map(|name| [Self::PACKAGE_FLAG, name.as_str()]);
        self.cargo_in_target(Self::RELEASE_CLEAN.into_iter().chain(packages))
    }

    fn build_release(&self) -> Result<String, TaskFailure> {
        let mut build = self.release_build();
        let stdout = Cargo::run(&mut build, |stderr| {
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

    fn read_executable(&self, messages: &str) -> Option<PathBuf> {
        let manifest = self.workspace.join(Self::MANIFEST);
        messages
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .map(Artifact::new)
            .filter(|artifact| artifact.belongs_to(&manifest))
            .find_map(|artifact| artifact.executable())
    }
}

struct Artifact {
    message: Value,
}

impl Artifact {
    const ARTIFACT_REASON: &'static str = "compiler-artifact";
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

    fn has_kind(&self, kind: &str) -> bool {
        self.message["target"]["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|candidate| candidate == kind))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::WorkspaceFixture;

    const MESSAGES: &str = include_str!("../../fixtures/bench/cargo-build.json");

    #[test]
    fn takes_the_binary_of_the_checkout_package() {
        let executable = CompilerBuild::new(Path::new("/checkout"), Path::new("/checkout/target"))
            .read_executable(MESSAGES);
        assert_eq!(
            executable,
            Some(PathBuf::from("/checkout/target/release/ledger-lang"))
        );
    }

    #[test]
    fn cleans_every_package_of_the_workspace() {
        let package_names = WorkspaceFixture::workspace().package_names();
        let clean = CompilerBuild::new(Path::new("/checkout"), Path::new("/checkout/target"))
            .release_clean(&package_names);
        let arguments: Vec<_> = clean.get_args().collect();
        for package in ["ledger-lang", "ledger-syntax"] {
            assert!(
                arguments
                    .windows(2)
                    .any(|pair| pair == ["--package", package])
            );
        }
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
        let executable =
            CompilerBuild::new(Path::new("/elsewhere"), Path::new("/elsewhere/target"))
                .read_executable(MESSAGES);
        assert_eq!(executable, None);
    }
}
