use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::exit_status::ExitStatus;
use super::program_file::ProgramFile;
use super::terminal::{ModuleOfSources, Terminal};
use crate::language::Language;

pub struct TestEntries {
    path: PathBuf,
}

impl TestEntries {
    const DIRECTORY_ENTRY: &'static str = "<tests>";

    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self, terminal: &mut Terminal) -> Result<ModuleOfSources, ExitStatus> {
        if self.path.is_dir() {
            self.load_directory(terminal)
        } else {
            Ok(ModuleOfSources {
                file: ProgramFile::beside_itself(self.path.clone()),
                sources: vec![terminal.load(&self.path)?],
            })
        }
    }

    fn load_directory(&self, terminal: &mut Terminal) -> Result<ModuleOfSources, ExitStatus> {
        let mut modules = Vec::new();
        self.collect_modules(&self.path, &mut modules)
            .map_err(|error| Terminal::report_unreadable(&self.path, &error))?;
        modules.sort();
        let uses: Vec<String> = modules
            .iter()
            .map(|module| format!("(use {module})"))
            .collect();
        let source = terminal.add_named_source(Self::DIRECTORY_ENTRY, &uses.join("\n"));
        Ok(ModuleOfSources {
            file: ProgramFile::within(self.path.join(Self::DIRECTORY_ENTRY), self.path.clone()),
            sources: vec![source],
        })
    }

    fn collect_modules(&self, directory: &Path, modules: &mut Vec<String>) -> io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            if path.is_dir() {
                self.collect_modules(&path, modules)?;
            } else if let Some(module) = self.module_path(&path) {
                modules.push(module);
            }
        }
        Ok(())
    }

    fn module_path(&self, file: &Path) -> Option<String> {
        if file.extension()? != Language::EXTENSION || Self::is_project_file(file) {
            return None;
        }
        ProgramFile::module_name(file.strip_prefix(&self.path).ok()?)
    }

    fn is_project_file(file: &Path) -> bool {
        file.file_name()
            .is_some_and(|name| *name == *Language::project_file_name())
    }
}
