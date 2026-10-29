use std::fs;
use std::path::{Path, PathBuf};

use super::package_files::PackageFiles;
use super::release_version::ReleaseVersion;
use super::toml_text::TomlText;
use crate::failure::TaskFailure;

pub struct CrateVersion {
    manifest: PathBuf,
    lock: PathBuf,
}

impl CrateVersion {
    const MANIFEST: &'static str = "Cargo.toml";
    const LOCK: &'static str = "Cargo.lock";

    pub fn new(root: &Path) -> Self {
        Self {
            manifest: root.join(Self::MANIFEST),
            lock: root.join(Self::LOCK),
        }
    }

    pub fn set(&self, version: &ReleaseVersion) -> Result<(), TaskFailure> {
        let files = PackageFiles::new(Self::read(&self.manifest)?, Self::read(&self.lock)?);
        let versioned = files.versioned(version)?;
        Self::write(&self.manifest, versioned.manifest())?;
        Self::write(&self.lock, versioned.lock())
    }

    fn read(path: &Path) -> Result<TomlText, TaskFailure> {
        fs::read_to_string(path)
            .map(TomlText::new)
            .map_err(|error| TaskFailure::unreadable(path, error))
    }

    fn write(path: &Path, text: &str) -> Result<(), TaskFailure> {
        fs::write(path, text).map_err(|error| TaskFailure::unwritable(path, error))
    }
}
