use std::borrow::Cow;

use rustyline::Helper;
use rustyline::completion::Completer;
use rustyline::highlight::{CmdKind, Highlighter};
use rustyline::hint::Hinter;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};

use super::open_input::OpenInput;
use super::token_colors::TokenColors;
use crate::diagnostic::DiagnosticRenderer;

pub struct InputHelper;

impl InputHelper {
    pub fn styled_prompt(prompt: &str) -> String {
        let style = DiagnosticRenderer::GUTTER_STYLE;
        format!("{style}{prompt}{style:#}")
    }

    fn validation_of(input: &str) -> ValidationResult {
        if OpenInput::awaits_more(input) {
            return ValidationResult::Incomplete;
        }
        ValidationResult::Valid(None)
    }
}

impl Highlighter for InputHelper {
    fn highlight<'l>(&self, line: &'l str, _cursor: usize) -> Cow<'l, str> {
        Cow::Owned(TokenColors::paint(line))
    }

    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        Cow::Owned(Self::styled_prompt(prompt))
    }

    fn highlight_char(&self, _line: &str, _cursor: usize, _kind: CmdKind) -> bool {
        true
    }
}

impl Validator for InputHelper {
    fn validate(&self, context: &mut ValidationContext) -> rustyline::Result<ValidationResult> {
        Ok(Self::validation_of(context.input()))
    }
}

impl Completer for InputHelper {
    type Candidate = String;
}

impl Hinter for InputHelper {
    type Hint = String;
}

impl Helper for InputHelper {}

#[cfg(test)]
mod tests {
    use super::*;

    struct HelperFixture;

    impl HelperFixture {
        fn colored(line: &str) -> String {
            InputHelper.highlight(line, 0).into_owned()
        }

        fn continues(input: &str) -> bool {
            matches!(
                InputHelper::validation_of(input),
                ValidationResult::Incomplete
            )
        }
    }

    #[test]
    fn the_typed_line_takes_the_token_colors() {
        assert_eq!(
            HelperFixture::colored("(f :x 1)"),
            TokenColors::paint("(f :x 1)")
        );
    }

    #[test]
    fn the_prompt_takes_the_gutter_style() {
        let style = DiagnosticRenderer::GUTTER_STYLE;
        let helper = InputHelper;
        assert_eq!(
            helper.highlight_prompt("> ", true),
            format!("{style}> {style:#}")
        );
        assert!(helper.highlight_char("(f 1)", 0, CmdKind::Other));
    }

    #[test]
    fn an_unclosed_list_or_string_continues_the_input() {
        assert!(HelperFixture::continues("(defn f (x)"));
        assert!(HelperFixture::continues("(println \"a"));
    }

    #[test]
    fn a_closed_form_or_another_reader_error_ends_the_input() {
        assert!(!HelperFixture::continues("(+ 1 2)"));
        assert!(!HelperFixture::continues(") ("));
        assert!(!HelperFixture::continues("1 ; ("));
    }
}
