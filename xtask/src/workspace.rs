use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::cargo::Cargo;
use crate::failure::TaskFailure;

pub struct CargoWorkspace {
    root: PathBuf,
    packages: Vec<MemberPackage>,
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

    pub fn package_names(&self) -> BTreeSet<String> {
        self.packages
            .iter()
            .map(|package| package.name.clone())
            .collect()
    }

    pub fn source_directories(&self) -> BTreeSet<PathBuf> {
        self.targets()
            .filter_map(|target| target.source_file.parent())
            .map(|directory| self.root.join(directory))
            .collect()
    }

    pub fn crate_names(&self) -> BTreeSet<String> {
        self.targets()
            .map(|target| target.crate_name.clone())
            .collect()
    }

    fn targets(&self) -> impl Iterator<Item = &CrateTarget> {
        self.packages.iter().flat_map(|package| &package.targets)
    }

    fn parse(root: &Path, metadata: &str) -> Result<Self, TaskFailure> {
        let packages = serde_json::from_str(metadata)
            .ok()
            .and_then(|metadata| Self::member_packages(&metadata))
            .ok_or_else(TaskFailure::metadata_malformed)?;
        if let Some(package) = packages
            .iter()
            .find(|package| package.keeps_sources_at_root())
        {
            return Err(TaskFailure::sources_at_package_root(&package.name));
        }
        Ok(Self {
            root: root.to_path_buf(),
            packages,
        })
    }

    fn member_packages(metadata: &Value) -> Option<Vec<MemberPackage>> {
        let workspace_root = WorkspaceRoot::new(metadata["workspace_root"].as_str()?);
        metadata["packages"]
            .as_array()?
            .iter()
            .filter(|package| package["name"] != Self::TOOLING_PACKAGE)
            .map(|package| MemberPackage::read(package, &workspace_root))
            .collect()
    }
}

struct WorkspaceRoot<'a> {
    path: &'a Path,
}

impl<'a> WorkspaceRoot<'a> {
    fn new(path: &'a str) -> Self {
        Self {
            path: Path::new(path),
        }
    }

    fn within(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        path.strip_prefix(self.path).unwrap_or(path).to_path_buf()
    }
}

struct MemberPackage {
    name: String,
    manifest: PathBuf,
    targets: Vec<CrateTarget>,
}

impl MemberPackage {
    fn read(package: &Value, workspace_root: &WorkspaceRoot) -> Option<Self> {
        let targets = package["targets"]
            .as_array()?
            .iter()
            .filter(|target| !CrateTarget::is_auxiliary(target))
            .map(|target| CrateTarget::read(target, workspace_root))
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            name: package["name"].as_str()?.to_string(),
            manifest: workspace_root.within(package["manifest_path"].as_str()?),
            targets,
        })
    }

    fn keeps_sources_at_root(&self) -> bool {
        let package_root = self.manifest.parent();
        self.targets
            .iter()
            .any(|target| target.source_file.parent() == package_root)
    }
}

struct CrateTarget {
    crate_name: String,
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

    fn read(target: &Value, workspace_root: &WorkspaceRoot) -> Option<Self> {
        Some(Self {
            crate_name: target["name"].as_str()?.replace('-', "_"),
            source_file: workspace_root.within(target["src_path"].as_str()?),
        })
    }
}

#[cfg(test)]
pub struct WorkspaceFixture;

#[cfg(test)]
impl WorkspaceFixture {
    const METADATA: &'static str = include_str!("../fixtures/workspace/metadata.json");
    const METADATA_WITHOUT_CRATES: &'static str =
        r#"{ "packages": [], "workspace_root": "/ledger" }"#;
    const DIRECTORY: &'static str = "fixtures/workspace";

    pub fn workspace() -> CargoWorkspace {
        Self::parsed(Self::METADATA)
    }

    pub fn without_crates() -> CargoWorkspace {
        Self::parsed(Self::METADATA_WITHOUT_CRATES)
    }

    pub fn this_repository() -> CargoWorkspace {
        CargoWorkspace::read(Self::repository_root()).expect("this repository's metadata reads")
    }

    pub fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(Self::DIRECTORY)
    }

    fn parsed(metadata: &str) -> CargoWorkspace {
        CargoWorkspace::parse(&Self::root(), metadata).expect("the fixture metadata parses")
    }

    fn metadata_without(owner: &str, field: &str) -> String {
        let mut metadata = Self::editable_metadata();
        metadata
            .pointer_mut(owner)
            .and_then(Value::as_object_mut)
            .and_then(|object| object.remove(field))
            .expect("the fixture metadata holds the field");
        metadata.to_string()
    }

    fn metadata_with(field: &str, text: &str) -> String {
        let mut metadata = Self::editable_metadata();
        *metadata
            .pointer_mut(field)
            .expect("the fixture metadata holds the field") = Value::from(text);
        metadata.to_string()
    }

    fn editable_metadata() -> Value {
        serde_json::from_str(Self::METADATA).expect("the fixture metadata is JSON")
    }

    fn repository_root() -> &'static Path {
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
    fn names_each_package_but_not_its_tooling() {
        assert_eq!(
            WorkspaceFixture::workspace().package_names(),
            BTreeSet::from(["ledger-lang".to_string(), "ledger-syntax".to_string()])
        );
    }

    #[test]
    fn names_each_crate_as_its_symbols_do_but_not_its_tooling() {
        assert_eq!(
            WorkspaceFixture::workspace().crate_names(),
            BTreeSet::from(["ledger_lang".to_string(), "ledger_syntax".to_string()])
        );
    }

    #[test]
    fn reads_the_crates_of_this_repository_without_its_tooling() {
        let directories = WorkspaceFixture::this_repository().source_directories();
        let tooling = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        assert!(!directories.is_empty());
        assert!(!directories.contains(&tooling));
    }

    #[test]
    fn rejects_metadata_that_lacks_a_field() {
        const WORKSPACE: &str = "";
        const MEMBER: &str = "/packages/1";
        const MEMBER_TARGET: &str = "/packages/1/targets/0";
        for (owner, field) in [
            (WORKSPACE, "workspace_root"),
            (WORKSPACE, "packages"),
            (MEMBER, "name"),
            (MEMBER, "manifest_path"),
            (MEMBER, "targets"),
            (MEMBER_TARGET, "name"),
            (MEMBER_TARGET, "src_path"),
        ] {
            let metadata = WorkspaceFixture::metadata_without(owner, field);
            let failure = CargoWorkspace::parse(&WorkspaceFixture::root(), &metadata).err();
            assert!(
                failure.is_some_and(|failure| failure.to_string().contains("`cargo metadata`")),
                "metadata without `{owner}/{field}` parsed"
            );
        }
    }

    #[test]
    fn refuses_a_crate_whose_sources_sit_at_its_package_root() {
        for (source_file, path, package) in [
            (
                "/packages/1/targets/0/src_path",
                "/ledger/syntax/lib.rs",
                "ledger-syntax",
            ),
            (
                "/packages/2/targets/1/src_path",
                "/ledger/main.rs",
                "ledger-lang",
            ),
        ] {
            let metadata = WorkspaceFixture::metadata_with(source_file, path);
            let failure = CargoWorkspace::parse(&WorkspaceFixture::root(), &metadata).err();
            assert!(
                failure.is_some_and(|failure| failure.to_string().contains(&format!(
                    "the package `{package}` keeps a crate's source file at its root"
                ))),
                "a source file at `{path}` was accepted"
            );
        }
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
