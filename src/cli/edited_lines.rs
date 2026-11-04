use std::io;
use std::rc::Rc;

use anstream::ColorChoice;
use rustyline::config::Config;
use rustyline::error::ReadlineError;
use rustyline::history::FileHistory;
use rustyline::{ColorMode, Editor, EventHandler, KeyCode, KeyEvent, Modifiers};

use super::diagnostic_output::DiagnosticOutput;
use super::enter_key::EnterKey;
use super::input_helper::InputHelper;
use super::prompt_history::PromptHistory;
use super::prompt_lines::EnteredText;

pub type LineEditor = Editor<InputHelper, FileHistory>;

pub struct EditedLines {
    editor: LineEditor,
    history: PromptHistory,
}

impl EditedLines {
    const PROMPT: &str = "> ";
    const HISTORY_SIZE: usize = 1000;
    const ENTER: KeyEvent = KeyEvent(KeyCode::Enter, Modifiers::NONE);

    pub fn new(standard_output: ColorChoice, notes: Rc<dyn DiagnosticOutput>) -> io::Result<Self> {
        let mut editor = Self::config(standard_output)
            .and_then(LineEditor::with_config)
            .map_err(io::Error::other)?;
        editor.set_helper(Some(InputHelper));
        editor.bind_sequence(Self::ENTER, EventHandler::Conditional(Box::new(EnterKey)));
        let history = PromptHistory::open_or_create(&mut editor, notes);
        Ok(Self { editor, history })
    }

    fn config(standard_output: ColorChoice) -> rustyline::Result<Config> {
        let color_mode = match standard_output {
            ColorChoice::Never => ColorMode::Disabled,
            _ => ColorMode::Forced,
        };
        Ok(Config::builder()
            .max_history_size(Self::HISTORY_SIZE)?
            .auto_add_history(true)
            .color_mode(color_mode)
            .build())
    }

    pub fn next_text(&mut self) -> io::Result<Option<EnteredText>> {
        loop {
            match self.editor.readline(Self::PROMPT) {
                Ok(line) => return Ok(Some(self.entered(line))),
                Err(ReadlineError::Interrupted) => {}
                Err(ReadlineError::Eof) => return Ok(None),
                Err(error) => return Err(io::Error::other(error)),
            }
        }
    }

    fn entered(&mut self, line: String) -> EnteredText {
        self.history.save(&mut self.editor);
        EnteredText::Complete(line + "\n")
    }
}
