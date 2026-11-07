use std::path::{Path, PathBuf};

use super::program_file::ProgramFile;
use super::terminal::ModuleOfSources;
use crate::source::SourceId;

pub struct PromptModule {
    anchor: SourceId,
    root: PathBuf,
}

impl PromptModule {
    pub const PATH: &str = "<prompt>";

    pub fn new(anchor: SourceId, root: PathBuf) -> Self {
        Self { anchor, root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn of(&self, inputs: Vec<SourceId>) -> ModuleOfSources {
        let mut sources = vec![self.anchor];
        sources.extend(inputs);
        ModuleOfSources {
            file: ProgramFile::within(PathBuf::from(Self::PATH), self.root.clone()),
            sources,
        }
    }
}
