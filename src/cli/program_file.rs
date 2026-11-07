use std::fs;
use std::path::{Path, PathBuf};

use crate::core::ModulePath;

#[derive(Clone)]
pub struct ProgramFile {
    path: PathBuf,
    root: PathBuf,
    module: ModulePath,
}

impl ProgramFile {
    const CURRENT_DIRECTORY: &str = ".";
    const MODULE_SEPARATOR: &str = "/";

    pub fn beside_itself(path: PathBuf) -> Self {
        let root = Self::directory_of(&path);
        Self::within(path, root)
    }

    pub fn within(path: PathBuf, root: PathBuf) -> Self {
        let module = Self::relative_module(&path, &root).unwrap_or_else(|| Self::file_stem(&path));
        Self {
            path,
            root,
            module: ModulePath::new(module),
        }
    }

    pub fn inside_or_beside(path: PathBuf, root: PathBuf) -> Self {
        match Self::relative_module(&path, &root) {
            Some(module) => Self {
                path,
                root,
                module: ModulePath::new(module),
            },
            None => Self::beside_itself(path),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn module_path(&self) -> ModulePath {
        self.module.clone()
    }

    pub fn module_name(relative: &Path) -> Option<String> {
        let without_extension = relative.with_extension("");
        let parts: Vec<&str> = without_extension
            .components()
            .map(|component| component.as_os_str().to_str())
            .collect::<Option<_>>()?;
        Some(parts.join(Self::MODULE_SEPARATOR))
    }

    pub fn directory_of(path: &Path) -> PathBuf {
        path.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    fn relative_module(path: &Path, root: &Path) -> Option<String> {
        let directory = Self::canonical(&Self::directory_of(path))?;
        let relative_directory = directory.strip_prefix(Self::canonical(root)?).ok()?;
        Self::module_name(&relative_directory.join(path.file_name()?))
    }

    fn canonical(directory: &Path) -> Option<PathBuf> {
        fs::canonicalize(Path::new(Self::CURRENT_DIRECTORY).join(directory)).ok()
    }

    fn file_stem(path: &Path) -> String {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}
