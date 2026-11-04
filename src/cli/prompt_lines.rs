use std::env;
use std::io::{self, BufRead, IsTerminal, Write};
use std::rc::Rc;

use anstream::{AutoStream, ColorChoice};

use super::diagnostic_output::DiagnosticOutput;
use super::edited_lines::EditedLines;
use super::input_helper::InputHelper;
use super::open_input::OpenInput;

pub enum EnteredText {
    Complete(String),
    Unfinished(String),
}

pub enum PromptLines {
    Piped(PipedLines<io::StdinLock<'static>>),
    Edited(Box<EditedLines>),
}

pub struct PipedLines<R> {
    input: R,
    prompt: Option<String>,
}

struct UnsupportedTerminal;

impl PromptLines {
    pub fn standard_input(notes: Rc<dyn DiagnosticOutput>) -> io::Result<Self> {
        let input = io::stdin();
        if !input.is_terminal() {
            return Ok(Self::Piped(PipedLines::new(input.lock(), None)));
        }
        let standard_output = AutoStream::choice(&io::stdout());
        if UnsupportedTerminal::is_current() {
            let prompt = PipedLines::<io::StdinLock>::prompt_for(standard_output);
            return Ok(Self::Piped(PipedLines::new(input.lock(), Some(prompt))));
        }
        Ok(Self::Edited(Box::new(EditedLines::new(
            standard_output,
            notes,
        )?)))
    }

    pub fn next_text(&mut self) -> io::Result<Option<EnteredText>> {
        match self {
            Self::Piped(lines) => lines.next_text(),
            Self::Edited(lines) => lines.next_text(),
        }
    }
}

impl<R: BufRead> PipedLines<R> {
    const PROMPT: &str = "> ";
    const CONTINUATION: &str = "  ";

    fn new(input: R, prompt: Option<String>) -> Self {
        Self { input, prompt }
    }

    pub fn next_text(&mut self) -> io::Result<Option<EnteredText>> {
        let mut text = String::new();
        self.show_prompt()?;
        while self.input.read_line(&mut text)? > 0 {
            if !OpenInput::awaits_more(&text) {
                return Ok(Some(EnteredText::Complete(text)));
            }
            self.show_continuation()?;
        }
        Ok((!text.is_empty()).then_some(EnteredText::Unfinished(text)))
    }

    fn prompt_for(standard_output: ColorChoice) -> String {
        match standard_output {
            ColorChoice::Never => Self::PROMPT.to_string(),
            _ => InputHelper::styled_prompt(Self::PROMPT),
        }
    }

    fn show_prompt(&self) -> io::Result<()> {
        match &self.prompt {
            Some(prompt) => Self::show(prompt),
            None => Ok(()),
        }
    }

    fn show_continuation(&self) -> io::Result<()> {
        match &self.prompt {
            Some(_) => Self::show(Self::CONTINUATION),
            None => Ok(()),
        }
    }

    fn show(text: &str) -> io::Result<()> {
        let mut output = io::stdout().lock();
        write!(output, "{text}")?;
        output.flush()
    }
}

impl UnsupportedTerminal {
    const VARIABLE: &str = "TERM";
    const NAMES: [&str; 3] = ["dumb", "cons25", "emacs"];

    fn is_current() -> bool {
        env::var(Self::VARIABLE).is_ok_and(|term| {
            Self::NAMES
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&term))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LinesFixture;

    impl LinesFixture {
        fn texts(input: &str) -> Vec<String> {
            let mut lines = PipedLines::new(input.as_bytes(), None);
            let mut texts = Vec::new();
            while let Some(text) = lines.next_text().expect("a byte slice reads") {
                texts.push(Self::describe(text));
            }
            texts
        }

        fn describe(text: EnteredText) -> String {
            match text {
                EnteredText::Complete(text) => format!("complete {text:?}"),
                EnteredText::Unfinished(text) => format!("unfinished {text:?}"),
            }
        }
    }

    #[test]
    fn each_line_of_closed_forms_is_one_text() {
        assert_eq!(
            LinesFixture::texts("(+ 1 2)\n(+ 3 4) 5\n"),
            ["complete \"(+ 1 2)\\n\"", "complete \"(+ 3 4) 5\\n\""]
        );
    }

    #[test]
    fn an_unclosed_list_reads_the_next_line() {
        assert_eq!(
            LinesFixture::texts("(+ 1\n 2)\n"),
            ["complete \"(+ 1\\n 2)\\n\""]
        );
    }

    #[test]
    fn an_unclosed_string_reads_the_next_line() {
        assert_eq!(
            LinesFixture::texts("(println \"a\nb\")\n"),
            ["complete \"(println \\\"a\\nb\\\")\\n\""]
        );
    }

    #[test]
    fn a_parenthesis_inside_a_comment_reads_no_further() {
        assert_eq!(
            LinesFixture::texts("1 ; (\n2\n"),
            ["complete \"1 ; (\\n\"", "complete \"2\\n\""]
        );
    }

    #[test]
    fn an_extra_close_ends_the_text_at_once() {
        assert_eq!(
            LinesFixture::texts("(+ 1 2))\n(\n"),
            ["complete \"(+ 1 2))\\n\"", "unfinished \"(\\n\""]
        );
    }

    #[test]
    fn an_extra_close_beside_an_unclosed_list_ends_the_text_at_once() {
        assert_eq!(LinesFixture::texts(") (\n"), ["complete \") (\\n\""]);
    }

    #[test]
    fn the_end_of_input_inside_a_list_leaves_the_text_unfinished() {
        assert_eq!(LinesFixture::texts("(+ 1"), ["unfinished \"(+ 1\""]);
    }
}
