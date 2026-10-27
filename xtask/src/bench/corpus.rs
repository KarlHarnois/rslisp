use std::fs;
use std::path::{Path, PathBuf};

use rslisp::language::Language;

use super::failure::BenchFailure;

pub struct Corpus {
    directory: PathBuf,
}

impl Corpus {
    pub const DIRECTORY: &'static str = "tests/fixtures/corpus";

    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn programs(&self) -> Result<Vec<PathBuf>, BenchFailure> {
        let entries = fs::read_dir(&self.directory)
            .map_err(|error| BenchFailure::unreadable(&self.directory, error))?;
        let mut programs: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| Self::is_program(path))
            .collect();
        programs.sort();
        Ok(programs)
    }

    fn is_program(path: &Path) -> bool {
        path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == Language::EXTENSION)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_the_entry_programs_but_not_their_modules() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/bench/corpus");
        let names: Vec<String> = Corpus::new(directory)
            .programs()
            .unwrap()
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["inventory.rslisp", "invoices.rslisp"]);
    }
}
