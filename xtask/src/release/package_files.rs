use super::release_version::ReleaseVersion;
use super::toml_text::TomlText;
use crate::failure::TaskFailure;

pub struct PackageFiles {
    manifest: TomlText,
    lock: TomlText,
}

impl PackageFiles {
    const PACKAGE_TABLE: &'static str = "[package]";
    const NAME_KEY: &'static str = "name";
    const VERSION_KEY: &'static str = "version";
    const MANIFEST_NAME: &'static str = "the manifest";
    const LOCK_NAME: &'static str = "the lock file";

    pub fn new(manifest: TomlText, lock: TomlText) -> Self {
        Self { manifest, lock }
    }

    pub fn manifest(&self) -> &str {
        self.manifest.as_str()
    }

    pub fn lock(&self) -> &str {
        self.lock.as_str()
    }

    pub fn versioned(&self, version: &ReleaseVersion) -> Result<Self, TaskFailure> {
        let version = version.to_string();
        let lock_entry = self.lock_entry()?;
        let manifest = self
            .manifest
            .with_string_value(Self::PACKAGE_TABLE, Self::VERSION_KEY, &version)
            .ok_or_else(|| {
                TaskFailure::manifest_key_missing(
                    Self::MANIFEST_NAME,
                    Self::PACKAGE_TABLE,
                    Self::VERSION_KEY,
                )
            })?;
        let lock = self
            .lock
            .with_string_value(&lock_entry, Self::VERSION_KEY, &version)
            .ok_or_else(|| {
                TaskFailure::manifest_key_missing(Self::LOCK_NAME, &lock_entry, Self::VERSION_KEY)
            })?;
        Ok(Self::new(manifest, lock))
    }

    fn lock_entry(&self) -> Result<String, TaskFailure> {
        let name = self
            .manifest
            .string_value(Self::PACKAGE_TABLE, Self::NAME_KEY)
            .ok_or_else(|| {
                TaskFailure::manifest_key_missing(
                    Self::MANIFEST_NAME,
                    Self::PACKAGE_TABLE,
                    Self::NAME_KEY,
                )
            })?;
        Ok(format!("{} = \"{name}\"", Self::NAME_KEY))
    }
}

#[cfg(test)]
mod tests {
    use super::PackageFiles;
    use crate::release::release_version::ReleaseVersion;
    use crate::release::toml_text::TomlText;

    struct PackageFilesFixture;

    impl PackageFilesFixture {
        const MANIFEST: &'static str = include_str!("../../fixtures/release/manifest.toml");
        const LOCK: &'static str = include_str!("../../fixtures/release/lock.toml");
        const VERSIONED_MANIFEST: &'static str =
            include_str!("../../fixtures/release/versioned-manifest.toml");
        const VERSIONED_LOCK: &'static str =
            include_str!("../../fixtures/release/versioned-lock.toml");

        fn versioned(version: &str) -> PackageFiles {
            let files = PackageFiles::new(
                TomlText::new(Self::MANIFEST.to_string()),
                TomlText::new(Self::LOCK.to_string()),
            );
            files
                .versioned(&ReleaseVersion::parse(version).unwrap())
                .unwrap()
        }
    }

    #[test]
    fn the_manifest_changes_only_the_package_version() {
        assert_eq!(
            PackageFilesFixture::versioned("0.1.1").manifest(),
            PackageFilesFixture::VERSIONED_MANIFEST
        );
    }

    #[test]
    fn the_lock_file_changes_only_the_package_entry() {
        assert_eq!(
            PackageFilesFixture::versioned("0.1.1").lock(),
            PackageFilesFixture::VERSIONED_LOCK
        );
    }
}
