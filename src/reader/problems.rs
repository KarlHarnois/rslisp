use super::syntax::Syntax;
use crate::diagnostic::{Diagnostic, DiagnosticCode, Label, Suggestion};
use crate::source::Span;

pub struct ReadProblems;

impl ReadProblems {
    const NUMBER_EXAMPLES: &str = "`42`, `-7`, `1_000`, `2.5` or `1e9`";
    const ESCAPES: &str = "`\\n`, `\\r`, `\\t`, `\\0`, `\\\\`, `\\\"`, `\\'` and `\\u{...}`";

    pub fn unclosed_list(open: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNCLOSED_LIST,
            "this list is never closed",
            Label::new(open, "this `(` has no matching `)`"),
        )
        .with_help("add a `)` where the list ends")
    }

    pub fn unmatched_close(close: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNMATCHED_CLOSE,
            "unexpected `)`",
            Label::new(close, "this `)` has no matching `(`"),
        )
        .with_suggestion("remove the extra `)`", Suggestion::new(close, ""))
    }

    pub fn unterminated_string(quote: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNTERMINATED_STRING,
            "this string is never closed",
            Label::new(quote, "this `\"` has no closing `\"`"),
        )
        .with_help("add a `\"` where the string ends")
    }

    pub fn unknown_escape(escape: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNKNOWN_ESCAPE,
            "unknown escape in string",
            Label::new(escape, "unknown escape"),
        )
        .with_help(format!("the escapes are {}", Self::ESCAPES))
    }

    pub fn malformed_number(text: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MALFORMED_NUMBER,
            format!("`{text}` is not a valid number"),
            Label::new(span, "this starts like a number"),
        )
        .with_note("a name cannot start with a digit, or with a sign followed by a digit")
        .with_help(format!("numbers look like {}", Self::NUMBER_EXAMPLES))
    }

    pub fn number_too_large(text: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::NUMBER_TOO_LARGE,
            format!("the number `{text}` is too large"),
            Label::new(span, "this does not fit in 128 bits"),
        )
    }

    pub fn list_too_deep(open: Span, limit: usize) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::LIST_TOO_DEEP,
            "this list nests too deeply",
            Label::new(
                open,
                format!("this `(` opens a list inside {limit} other lists"),
            ),
        )
        .with_note(format!("lists nest at most {limit} deep"))
        .with_help("move the inner forms into a function of their own")
    }

    pub fn reserved_character(character: char, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::RESERVED_CHARACTER,
            format!("`{character}` is not part of the syntax"),
            Label::new(span, "not allowed here"),
        )
        .with_suggestion(
            "every list uses `(` and `)`",
            Suggestion::new(span, Self::paren_for(character).to_string()),
        )
    }

    fn paren_for(character: char) -> char {
        if character == '[' || character == '{' {
            Syntax::OPEN
        } else {
            Syntax::CLOSE
        }
    }
}
