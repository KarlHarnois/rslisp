use std::io::{self, BufRead, IsTerminal, Write};

use crate::diagnostic::DiagnosticCode;
use crate::reader::Reader;
use crate::source::SourceMap;

pub enum EnteredText {
    Complete(String),
    Unfinished(String),
}

pub struct PromptLines<R> {
    input: R,
    shows_prompts: bool,
}

impl PromptLines<io::StdinLock<'static>> {
    pub fn standard_input() -> Self {
        let input = io::stdin();
        let shows_prompts = input.is_terminal();
        Self {
            input: input.lock(),
            shows_prompts,
        }
    }
}

impl<R: BufRead> PromptLines<R> {
    const PROMPT: &str = "> ";
    const CONTINUATION: &str = "  ";
    const CONTINUING_CODES: [DiagnosticCode; 2] = [
        DiagnosticCode::UNCLOSED_LIST,
        DiagnosticCode::UNTERMINATED_STRING,
    ];

    pub fn next_text(&mut self) -> io::Result<Option<EnteredText>> {
        let mut text = String::new();
        self.show(Self::PROMPT)?;
        while self.input.read_line(&mut text)? > 0 {
            if !Self::awaits_more(&text) {
                return Ok(Some(EnteredText::Complete(text)));
            }
            self.show(Self::CONTINUATION)?;
        }
        Ok((!text.is_empty()).then_some(EnteredText::Unfinished(text)))
    }

    fn awaits_more(text: &str) -> bool {
        let mut scratch = SourceMap::default();
        let source = scratch.add("", text);
        match Reader::new(scratch.file(source)).read() {
            Ok(_) => false,
            Err(diagnostics) => diagnostics
                .in_source_order()
                .iter()
                .all(|diagnostic| Self::continues(diagnostic.code())),
        }
    }

    fn continues(code: Option<&DiagnosticCode>) -> bool {
        code.is_some_and(|code| Self::CONTINUING_CODES.contains(code))
    }

    fn show(&self, prompt: &str) -> io::Result<()> {
        if !self.shows_prompts {
            return Ok(());
        }
        let mut output = io::stdout().lock();
        write!(output, "{prompt}")?;
        output.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LinesFixture;

    impl LinesFixture {
        fn texts(input: &str) -> Vec<String> {
            let mut lines = PromptLines {
                input: input.as_bytes(),
                shows_prompts: false,
            };
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
