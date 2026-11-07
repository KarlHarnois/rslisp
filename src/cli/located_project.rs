use std::path::{Path, PathBuf};

use super::exit_status::ExitStatus;
use super::program_file::ProgramFile;
use super::terminal::Terminal;
use crate::language::Language;
use crate::project::Project;

pub struct LocatedProject {
    root: PathBuf,
    project: Project,
}

impl LocatedProject {
    pub fn search_from(directory: &Path, terminal: &Terminal) -> Result<Option<Self>, ExitStatus> {
        let Some(root) = Self::nearest_root(directory)? else {
            return Ok(None);
        };
        let project = terminal
            .scratch()
            .load_project(&root.join(Language::project_file_name()))?;
        Ok(Some(Self { root, project }))
    }

    fn nearest_root(directory: &Path) -> Result<Option<PathBuf>, ExitStatus> {
        let start = Terminal::canonical(directory)?;
        let file_name = Language::project_file_name();
        let found = start
            .ancestors()
            .find(|ancestor| ancestor.join(&file_name).is_file());
        match found {
            Some(root) => Ok(Some(Self::shown(root)?)),
            None => Ok(None),
        }
    }

    fn shown(root: &Path) -> Result<PathBuf, ExitStatus> {
        let working_directory = Terminal::canonical(Path::new(Terminal::WORKING_DIRECTORY))?;
        Ok(root
            .strip_prefix(&working_directory)
            .unwrap_or(root)
            .to_path_buf())
    }

    pub fn name(&self) -> &str {
        &self.project.name
    }

    pub fn entry(&self) -> ProgramFile {
        let entry = self.root.join(&self.project.entry);
        self.file_within(entry)
    }

    pub fn file_within(&self, path: PathBuf) -> ProgramFile {
        ProgramFile::inside_or_beside(path, self.source_root())
    }

    pub fn source_root(&self) -> PathBuf {
        ProgramFile::directory_of(&self.root.join(&self.project.entry))
    }

    pub fn build_directory(&self) -> PathBuf {
        self.root
            .join(Terminal::BUILD_DIRECTORY)
            .join(&self.project.name)
    }
}
