use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::language::Language;

pub struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    const ATTEMPTS: usize = 16;

    pub fn create() -> io::Result<Self> {
        let names = (0..Self::ATTEMPTS).map(Self::unique_name);
        Self::create_in(&std::env::temp_dir(), names)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn create_in(parent: &Path, names: impl Iterator<Item = String>) -> io::Result<Self> {
        let mut last_error = io::Error::from(io::ErrorKind::AlreadyExists);
        for name in names {
            let path = parent.join(name);
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => last_error = error,
                Err(error) => return Err(error),
            }
        }
        Err(last_error)
    }

    fn unique_name(attempt: usize) -> String {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!(
            "{}-eval-{}-{since_epoch}-{attempt}",
            Language::NAME,
            process::id()
        )
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::TemporaryDirectory;
    use crate::language::Language;

    struct ParentFixture {
        parent: PathBuf,
    }

    impl ParentFixture {
        const TAKEN: &str = "taken";
        const FREE: &str = "free";

        fn new(test_name: &str) -> Self {
            let parent = std::env::temp_dir().join(format!(
                "{}-temporary-{}-{test_name}",
                Language::NAME,
                std::process::id()
            ));
            fs::create_dir_all(parent.join(Self::TAKEN)).expect("the parent is writable");
            Self { parent }
        }

        fn create(&self, names: &[&str]) -> std::io::Result<TemporaryDirectory> {
            let names = names.iter().map(|name| name.to_string());
            TemporaryDirectory::create_in(&self.parent, names)
        }
    }

    impl Drop for ParentFixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.parent).ok();
        }
    }

    #[test]
    fn a_taken_name_moves_on_to_the_next_name() {
        let fixture = ParentFixture::new("next");
        let created = fixture
            .create(&[ParentFixture::TAKEN, ParentFixture::FREE])
            .expect("the free name is created");
        assert_eq!(created.path(), fixture.parent.join(ParentFixture::FREE));
    }

    #[test]
    fn a_directory_that_exists_is_never_reused() {
        let fixture = ParentFixture::new("taken");
        let error = fixture.create(&[ParentFixture::TAKEN]).err();
        assert_eq!(
            error.map(|error| error.kind()),
            Some(std::io::ErrorKind::AlreadyExists)
        );
    }

    #[test]
    fn a_missing_parent_fails_at_once() {
        let fixture = ParentFixture::new("missing");
        let missing = fixture.parent.join("missing");
        let error = TemporaryDirectory::create_in(&missing, ["a".to_string()].into_iter()).err();
        assert_eq!(
            error.map(|error| error.kind()),
            Some(std::io::ErrorKind::NotFound)
        );
    }

    #[test]
    fn the_directory_is_removed_when_it_drops() {
        let fixture = ParentFixture::new("removed");
        let path = fixture
            .create(&[ParentFixture::FREE])
            .expect("the name is free")
            .path()
            .to_path_buf();
        assert!(!path.exists());
    }
}
