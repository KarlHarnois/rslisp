use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rustyline::error::ReadlineError;

use super::diagnostic_output::DiagnosticOutput;
use super::edited_lines::LineEditor;
use crate::language::Language;
use crate::source::SourceMap;

pub struct PromptHistory {
    file: Option<PathBuf>,
    notes: Rc<dyn DiagnosticOutput>,
}

impl PromptHistory {
    const STATE_HOME: &str = "XDG_STATE_HOME";
    const HOME: &str = "HOME";
    const STATE_UNDER_HOME: &str = ".local/state";
    const FILE: &str = "history";

    pub fn open_or_create(editor: &mut LineEditor, notes: Rc<dyn DiagnosticOutput>) -> Self {
        let mut history = Self { file: None, notes };
        match Self::state_directory() {
            Some(state) => history.open(editor, &state.join(Language::NAME)),
            None => history.turn_off(&format!(
                "no state directory: set `{}` or `{}`",
                Self::STATE_HOME,
                Self::HOME
            )),
        }
        history
    }

    pub fn save(&mut self, editor: &mut LineEditor) {
        let Some(file) = &self.file else {
            return;
        };
        if let Err(error) = editor.append_history(file) {
            let message = Self::unwritable(file, &error);
            self.turn_off(&message);
        }
    }

    fn state_directory() -> Option<PathBuf> {
        Self::variable_path(Self::STATE_HOME).or_else(|| {
            Self::variable_path(Self::HOME).map(|home| home.join(Self::STATE_UNDER_HOME))
        })
    }

    fn variable_path(name: &str) -> Option<PathBuf> {
        env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }

    fn open(&mut self, editor: &mut LineEditor, directory: &Path) {
        let file = directory.join(Self::FILE);
        if let Err(error) = fs::create_dir_all(directory) {
            return self.turn_off(&Self::unwritable(&file, &error));
        }
        match Self::read(editor, &file) {
            Ok(()) => self.file = Some(file),
            Err(error) => self.turn_off(&format!("cannot read `{}`: {error}", file.display())),
        }
    }

    fn read(editor: &mut LineEditor, file: &Path) -> rustyline::Result<()> {
        match editor.load_history(file) {
            Err(ReadlineError::Io(error)) if error.kind() == ErrorKind::NotFound => Ok(()),
            loaded => loaded,
        }
    }

    fn unwritable(file: &Path, error: &dyn std::fmt::Display) -> String {
        format!("cannot write `{}`: {error}", file.display())
    }

    fn turn_off(&mut self, reason: &str) {
        self.file = None;
        self.notes
            .write_note(&SourceMap::default(), &format!("history is off: {reason}"));
    }
}
