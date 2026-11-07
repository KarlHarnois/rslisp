mod dependency_reader;
mod fields;
mod problems;
mod reader;

use std::path::PathBuf;

pub use problems::ProjectProblems;
pub use reader::ProjectReader;

#[derive(Debug)]
pub struct Project {
    pub name: String,
    pub version: String,
    pub entry: PathBuf,
    pub library_root: PathBuf,
    pub edition: i128,
    pub libraries: Vec<LibraryDependency>,
    pub dependencies: Vec<CrateDependency>,
    pub dev_dependencies: Vec<CrateDependency>,
}

#[derive(Debug)]
pub struct LibraryDependency {
    pub name: String,
    pub source: LibrarySource,
}

#[derive(Debug)]
pub enum LibrarySource {
    Path(PathBuf),
    Git { url: String, commit: String },
}

impl LibrarySource {
    pub const FULL_COMMIT_LENGTH: usize = 40;
}

#[derive(Debug)]
pub struct CrateDependency {
    pub name: String,
    pub version: Option<String>,
    pub features: Vec<String>,
    pub path: Option<PathBuf>,
}
