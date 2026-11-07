use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use knave::format::{FormatFailure, ModuleLookup, SourceFormatter};
use knave::language::Language;
use knave::source::SourceMap;

pub struct ShapeLookup;

pub struct FormatFailures {
    pub failed: Vec<String>,
    pub formatted: usize,
    pub elapsed: Duration,
}

struct RepositorySources {
    root: PathBuf,
}

impl ShapeLookup {
    pub fn beside(path: &Path) -> ModuleLookup {
        ModuleLookup::beside(path.parent().expect("a source file lives in a directory"))
    }
}

impl FormatFailures {
    const SKIPPED_DIRECTORIES: [&str; 2] = ["target", ".git"];

    pub fn of_repository() -> Self {
        let sources = RepositorySources {
            root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        };
        let started = Instant::now();
        let mut formatter = SourceFormatter::default();
        let paths = sources.files(&sources.root);
        let failed = paths
            .iter()
            .filter_map(|path| sources.failure(&mut formatter, path))
            .collect();
        Self {
            failed,
            formatted: paths.len(),
            elapsed: started.elapsed(),
        }
    }
}

impl RepositorySources {
    fn files(&self, directory: &Path) -> Vec<PathBuf> {
        let mut entries: Vec<PathBuf> = fs::read_dir(directory)
            .expect("the repository directory is readable")
            .map(|entry| entry.expect("a directory entry is readable").path())
            .collect();
        entries.sort();
        entries
            .into_iter()
            .flat_map(|entry| self.files_at(entry))
            .collect()
    }

    fn files_at(&self, entry: PathBuf) -> Vec<PathBuf> {
        if entry.is_dir() && !Self::is_skipped(&entry) {
            self.files(&entry)
        } else if entry
            .extension()
            .is_some_and(|extension| extension == Language::EXTENSION)
        {
            vec![entry]
        } else {
            Vec::new()
        }
    }

    fn is_skipped(directory: &Path) -> bool {
        directory.file_name().is_some_and(|name| {
            FormatFailures::SKIPPED_DIRECTORIES
                .iter()
                .any(|skipped| name == *skipped)
        })
    }

    fn failure(&self, formatter: &mut SourceFormatter, path: &Path) -> Option<String> {
        let text = fs::read_to_string(path).expect("a repository source is readable");
        let mut sources = SourceMap::default();
        let source = sources.add(path.display().to_string(), text);
        match formatter.format(sources.file(source), &ShapeLookup::beside(path)) {
            Ok(_) | Err(FormatFailure::Unreadable(_)) => None,
            Err(failure) => Some(format!(
                "{}: {failure:?}",
                path.strip_prefix(&self.root).unwrap_or(path).display()
            )),
        }
    }
}
