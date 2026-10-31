use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::cargo::Cargo;
use crate::failure::TaskFailure;

pub struct CargoWorkspace {
    root: PathBuf,
    targets: Vec<CrateTarget>,
}

impl CargoWorkspace {
    const METADATA: [&'static str; 4] = ["metadata", "--format-version", "1", "--no-deps"];
    const MANIFEST_PATH_FLAG: &'static str = "--manifest-path";
    const MANIFEST: &'static str = "Cargo.toml";
    const TOOLING_PACKAGE: &'static str = env!("CARGO_PKG_NAME");

    pub fn read(root: &Path) -> Result<Self, TaskFailure> {
        let mut metadata = Cargo::command();
        metadata
            .args(Self::METADATA)
            .arg(Self::MANIFEST_PATH_FLAG)
            .arg(root.join(Self::MANIFEST));
        let stdout = Cargo::run(&mut metadata, |stderr| {
            TaskFailure::tool_failed("cargo metadata", stderr)
        })?;
        Self::parse(root, &String::from_utf8_lossy(&stdout))
    }

    pub fn source_directories(&self) -> BTreeSet<PathBuf> {
        self.targets
            .iter()
            .filter_map(|target| target.source_file.parent())
            .map(|directory| self.root.join(directory))
            .collect()
    }

    fn parse(root: &Path, metadata: &str) -> Result<Self, TaskFailure> {
        let targets = serde_json::from_str(metadata)
            .ok()
            .and_then(|metadata| Self::crate_targets(&metadata))
            .ok_or_else(TaskFailure::metadata_malformed)?;
        Ok(Self {
            root: root.to_path_buf(),
            targets,
        })
    }

    fn crate_targets(metadata: &Value) -> Option<Vec<CrateTarget>> {
        let workspace_root = Path::new(metadata["workspace_root"].as_str()?);
        let target_lists = metadata["packages"]
            .as_array()?
            .iter()
            .filter(|package| package["name"] != Self::TOOLING_PACKAGE)
            .map(|package| package["targets"].as_array())
            .collect::<Option<Vec<_>>>()?;
        target_lists
            .into_iter()
            .flatten()
            .filter(|target| !CrateTarget::is_auxiliary(target))
            .map(|target| CrateTarget::read(target, workspace_root))
            .collect()
    }
}

struct CrateTarget {
    source_file: PathBuf,
}

impl CrateTarget {
    const AUXILIARY_KINDS: [&'static str; 4] = ["test", "bench", "example", "custom-build"];

    fn is_auxiliary(target: &Value) -> bool {
        target["kind"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|kind| Self::AUXILIARY_KINDS.contains(&kind))
    }

    fn read(target: &Value, workspace_root: &Path) -> Option<Self> {
        let source_file = Path::new(target["src_path"].as_str()?);
        let within_workspace = source_file
            .strip_prefix(workspace_root)
            .unwrap_or(source_file);
        Some(Self {
            source_file: within_workspace.to_path_buf(),
        })
    }
}

#[cfg(test)]
pub struct WorkspaceFixture;

#[cfg(test)]
impl WorkspaceFixture {
    const METADATA: &'static str = include_str!("../fixtures/workspace/metadata.json");
    const DIRECTORY: &'static str = "fixtures/workspace";

    pub fn workspace() -> CargoWorkspace {
        CargoWorkspace::parse(&Self::root(), Self::METADATA).expect("the fixture metadata parses")
    }

    pub fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(Self::DIRECTORY)
    }

    fn this_repository() -> &'static Path {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        manifest.parent().unwrap_or(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_source_directory_of_each_crate_but_not_of_its_tooling() {
        let root = WorkspaceFixture::root();
        assert_eq!(
            WorkspaceFixture::workspace().source_directories(),
            BTreeSet::from([root.join("src"), root.join("syntax/src")])
        );
    }

    #[test]
    fn reads_the_crates_of_this_repository_without_its_tooling() {
        let directories = CargoWorkspace::read(WorkspaceFixture::this_repository())
            .unwrap()
            .source_directories();
        let tooling = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        assert!(!directories.is_empty());
        assert!(!directories.contains(&tooling));
    }

    #[test]
    fn rejects_metadata_without_packages() {
        let metadata = r#"{ "workspace_root": "/ledger" }"#;
        let failure = CargoWorkspace::parse(&WorkspaceFixture::root(), metadata).err();
        assert!(failure.unwrap().to_string().contains("`cargo metadata`"));
    }

    #[test]
    fn reports_a_directory_without_a_manifest() {
        let failure = CargoWorkspace::read(&WorkspaceFixture::root()).err();
        assert!(
            failure
                .unwrap()
                .to_string()
                .contains("`cargo metadata` failed")
        );
    }
}
