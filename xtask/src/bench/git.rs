use std::path::Path;
use std::process::{Command, Output};

use crate::failure::TaskFailure;

#[derive(Clone, Copy)]
pub struct Git<'a> {
    workspace: &'a Path,
}

impl<'a> Git<'a> {
    pub const PROGRAM: &'static str = "git";
    const LOCALE_VARIABLE: &'static str = "LC_ALL";
    const UNTRANSLATED_LOCALE: &'static str = "C";

    pub fn new(workspace: &'a Path) -> Self {
        Self { workspace }
    }

    pub fn run(&self, arguments: &[&str]) -> Result<Output, TaskFailure> {
        Command::new(Self::PROGRAM)
            .args(arguments)
            .env(Self::LOCALE_VARIABLE, Self::UNTRANSLATED_LOCALE)
            .current_dir(self.workspace)
            .output()
            .map_err(|error| TaskFailure::tool_missing(Self::PROGRAM, error))
    }
}
