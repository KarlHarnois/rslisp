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
        .with_suggestion(
            "clone the value if both places need it",
            Suggestion::new(moved, format!("(clone {name})")),
        )
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
}
