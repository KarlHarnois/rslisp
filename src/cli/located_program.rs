use std::path::{Path, PathBuf};

use super::exit_status::ExitStatus;
use super::located_project::LocatedProject;
use super::program_file::ProgramFile;
use super::standard_error::StandardError;
use super::terminal::Terminal;
use crate::language::Language;

pub struct LocatedProgram {
    file: ProgramFile,
    project: Option<LocatedProject>,
}

impl LocatedProgram {
    pub fn locate(file: Option<PathBuf>, terminal: &Terminal) -> Result<Self, ExitStatus> {
        match file {
            Some(file) => Self::of_file(file, terminal),
            None => Self::of_working_directory(terminal),
        }
    }

    fn of_file(file: PathBuf, terminal: &Terminal) -> Result<Self, ExitStatus> {
        let project = LocatedProject::search_from(&ProgramFile::directory_of(&file), terminal)?;
        let file = match &project {
            Some(project) => project.file_within(file),
            None => ProgramFile::beside_itself(file),
        };
        Ok(Self { file, project })
    }

    fn of_working_directory(terminal: &Terminal) -> Result<Self, ExitStatus> {
        let working_directory = Path::new(Terminal::WORKING_DIRECTORY);
        match LocatedProject::search_from(working_directory, terminal)? {
            Some(project) => Ok(Self {
                file: project.entry(),
                project: Some(project),
            }),
            None => Err(Self::report_no_program()),
        }
    }

    pub fn file(&self) -> &ProgramFile {
        &self.file
    }

    pub fn into_file(self) -> ProgramFile {
        self.file
    }

    pub fn project(&self) -> Option<&LocatedProject> {
        self.project.as_ref()
    }

    fn report_no_program() -> ExitStatus {
        StandardError::write_line(format_args!(
            "error: no file given, and no `{}` in this directory or a parent\n\
             help: name a file, or run the command inside a project",
            Language::project_file_name()
        ));
        ExitStatus::UsageError
    }
}
