use std::fs;
use std::path::{Path, PathBuf};

use crate::failure::TaskFailure;

pub struct Corpus {
    directory: PathBuf,
    extension: String,
}

impl Corpus {
    pub const DIRECTORY: &'static str = "tests/fixtures/corpus";

    pub fn new(directory: PathBuf, extension: String) -> Self {
        Self {
            directory,
            extension,
        }
    }

    pub fn programs(&self) -> Result<Vec<PathBuf>, TaskFailure> {
        let entries = fs::read_dir(&self.directory)
            .map_err(|error| TaskFailure::unreadable(&self.directory, error))?;
        let paths = entries
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|error| TaskFailure::unreadable(&self.directory, error))
            })
            .collect::<Result<Vec<PathBuf>, TaskFailure>>()?;
        let mut programs: Vec<PathBuf> = paths
            .into_iter()
            .filter(|path| self.is_program(path))
            .collect();
        programs.sort();
        Ok(programs)
    }

    fn is_program(&self, path: &Path) -> bool {
        path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == self.extension.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::LanguageSource;
    use crate::workspace::WorkspaceFixture;

    #[test]
    fn lists_the_entry_programs_but_not_their_modules() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let extension = LanguageSource::find(&WorkspaceFixture::this_repository())
            .unwrap()
            .constant("EXTENSION")
            .unwrap();
        let names: Vec<String> = Corpus::new(manifest.join("fixtures/bench/corpus"), extension)
            .programs()
            .unwrap()
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["inventory.rslisp", "invoices.rslisp"]);
    }
}
