use crate::diagnostic::{Diagnostic, DiagnosticCode, Label, Suggestion};
use crate::source::Span;

pub struct OwnershipProblems;

impl OwnershipProblems {
    pub fn use_after_move(name: &str, used: Span, moved: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::USE_AFTER_MOVE,
            format!("use of moved value `{name}`"),
            Label::new(used, "value used here after it moved"),
        )
        .with_label(Label::new(moved, "value moved here"))
    }

    pub fn clone_suggestion(diagnostic: Diagnostic, name: &str, moved: Span) -> Diagnostic {
        diagnostic.with_suggestion(
            "clone the value if both places need it",
            Suggestion::new(moved, format!("(clone {name})")),
        )
    }

    pub fn closure_move_help(diagnostic: Diagnostic) -> Diagnostic {
        diagnostic.with_help("this closure moves a captured value, so it can only be used once")
    }

    pub fn ambiguous_borrow(function: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::AMBIGUOUS_BORROW,
            format!("`{function}` returns a borrow, but it borrows several parameters"),
            Label::new(span, "cannot tell which parameter the result borrows from"),
        )
        .with_help(
            "return an owned value with `(clone ...)`; lifetime annotations are not supported yet",
        )
    }

    pub fn needs_ownership(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::NEEDS_OWNERSHIP,
            "this value is borrowed, but the call needs to own it",
            Label::new(span, "only a borrow is available here"),
        )
        .with_help("pass a copy with `(clone ...)`")
    }

    pub fn missing_bang(name: &str, span: Span) -> Diagnostic {
        Diagnostic::warning(
            DiagnosticCode::MISSING_BANG,
            format!("`{name}` changes an argument, so its name should end with `!`"),
            Label::new(span, "this function changes an argument"),
        )
        .with_help(format!("rename it `{name}!`"))
    }
}
