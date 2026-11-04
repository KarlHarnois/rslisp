use std::borrow::Cow;

use anstyle::{AnsiColor, Effects, Style};
use rustyline::Helper;
use rustyline::completion::Completer;
use rustyline::highlight::{CmdKind, Highlighter};
use rustyline::hint::Hinter;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};

use super::open_input::OpenInput;
use crate::core::Keyword;
use crate::diagnostic::DiagnosticRenderer;
use crate::reader::{Token, TokenKind, TokenScan};

pub struct InputHelper;

impl InputHelper {
    const STRING_STYLE: Style = AnsiColor::Green.on_default();
    const NUMBER_STYLE: Style = AnsiColor::Yellow.on_default();
    const KEYWORD_STYLE: Style = AnsiColor::Cyan.on_default();
    const COMMENT_STYLE: Style = Style::new().effects(Effects::DIMMED);

    pub fn styled_prompt(prompt: &str) -> String {
        let style = DiagnosticRenderer::GUTTER_STYLE;
        format!("{style}{prompt}{style:#}")
    }

    fn colored_line(line: &str) -> String {
        let mut colored = String::with_capacity(line.len());
        let mut written = 0;
        for token in TokenScan::new(line) {
            colored.push_str(&line[written..token.range.start]);
            Self::push_styled(
                &mut colored,
                Self::style_of(line, &token),
                &line[token.range.clone()],
            );
            written = token.range.end;
        }
        colored.push_str(&line[written..]);
        colored
    }

    fn push_styled(colored: &mut String, style: Style, text: &str) {
        colored.push_str(&format!("{style}{text}{style:#}"));
    }

    fn style_of(line: &str, token: &Token) -> Style {
        match token.kind {
            TokenKind::String => Self::STRING_STYLE,
            TokenKind::Number => Self::NUMBER_STYLE,
            TokenKind::Comment => Self::COMMENT_STYLE,
            TokenKind::Symbol => Self::symbol_style(&line[token.range.clone()]),
            TokenKind::Open | TokenKind::Close | TokenKind::Reserved => Style::new(),
        }
    }

    fn symbol_style(symbol: &str) -> Style {
        match Keyword::field_name(symbol) {
            Some(_) => Self::KEYWORD_STYLE,
            None => Style::new(),
        }
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
        Cow::Owned(Self::colored_line(line))
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

        fn styled(style: Style, text: &str) -> String {
            format!("{style}{text}{style:#}")
        }

        fn continues(input: &str) -> bool {
            matches!(
                InputHelper::validation_of(input),
                ValidationResult::Incomplete
            )
        }
    }

    #[test]
    fn each_kind_of_token_takes_its_color() {
        assert_eq!(
            HelperFixture::colored("(f :x 1 \"s\") ; c"),
            format!(
                "(f {} {} {}) {}",
                HelperFixture::styled(InputHelper::KEYWORD_STYLE, ":x"),
                HelperFixture::styled(InputHelper::NUMBER_STYLE, "1"),
                HelperFixture::styled(InputHelper::STRING_STYLE, "\"s\""),
                HelperFixture::styled(InputHelper::COMMENT_STYLE, "; c"),
            )
        );
    }

    #[test]
    fn a_partial_line_colors_its_unclosed_string() {
        assert_eq!(
            HelperFixture::colored("(defn f \"abc"),
            format!(
                "(defn f {}",
                HelperFixture::styled(InputHelper::STRING_STYLE, "\"abc")
            )
        );
    }

    #[test]
    fn a_lone_keyword_marker_stays_plain() {
        assert_eq!(HelperFixture::colored(": [x]"), ": [x]");
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
