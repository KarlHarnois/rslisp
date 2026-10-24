use super::functions::ParamMode;
use crate::core::ReferenceKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, Label, Suggestion};
use crate::source::Span;

pub struct OwnershipProblems;

impl OwnershipProblems {
    pub fn use_after_move(name: &str, used: Span, moved: Span) -> Diagnostic {
        Self::moved_value(DiagnosticCode::USE_AFTER_MOVE, name, used, moved)
    }

    fn moved_value(code: DiagnosticCode, name: &str, used: Span, moved: Span) -> Diagnostic {
        Diagnostic::error(
            code,
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

    pub fn mut_parameter_move_help(diagnostic: Diagnostic) -> Diagnostic {
        diagnostic
            .with_help("use the value before it moves, because a `mut` borrow cannot be cloned")
    }

    pub fn changed_value_help(diagnostic: Diagnostic) -> Diagnostic {
        diagnostic.with_help(
            "change the value in place instead of moving it, so both uses see the change",
        )
    }

    pub fn recursive_parameter_help(diagnostic: Diagnostic) -> Diagnostic {
        diagnostic.with_help("write the parameter as a borrow in a `type` signature: `(ref ...)`")
    }

    pub fn missing_copy_bound(
        name: &str,
        parameter: &str,
        copy_trait: &str,
        used: Span,
        moved: Span,
    ) -> Diagnostic {
        Self::moved_value(DiagnosticCode::MISSING_COPY_BOUND, name, used, moved)
            .with_note(format!(
                "the signature writes `{parameter}` without `{copy_trait}`, and a written signature is exact"
            ))
            .with_help(format!(
                "add a `{copy_trait}` bound to the signature: `({parameter} {copy_trait})`"
            ))
    }

    pub fn written_mode_mismatch(
        parameter: &str,
        kind: ReferenceKind,
        mode: ParamMode,
        span: Span,
    ) -> Diagnostic {
        let taken = match mode {
            ParamMode::Copy | ParamMode::Own => "a copy",
            ParamMode::Ref => "a reference",
            ParamMode::Mut => "a mutable reference",
        };
        let (title, passed, help) = match kind {
            ReferenceKind::Shared => (
                format!("`{parameter}` is taken as {taken}, so this borrow cannot pass"),
                "this passes a borrow",
                "pass the value without `ref`, or write the parameter's type",
            ),
            ReferenceKind::Mutable => (
                format!("`{parameter}` is taken as {taken}, so this mutable borrow cannot pass"),
                "this passes a mutable borrow",
                "pass the value without `mut`, or write the parameter's type",
            ),
        };
        Diagnostic::error(
            DiagnosticCode::WRITTEN_MODE_MISMATCH,
            title,
            Label::new(span, passed),
        )
        .with_note("the callee's body decides whether the parameter takes a borrow")
        .with_help(help)
    }

    pub fn closure_move_help(diagnostic: Diagnostic) -> Diagnostic {
        diagnostic.with_help("this closure moves a captured value, so it cannot be copied")
    }

    pub fn closure_moves_capture(name: &str, moved: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::CLOSURE_MOVES_CAPTURE,
            "this closure moves a captured value",
            Label::new(moved, format!("`{name}` moves here")),
        )
        .with_help(format!(
            "clone the value with `(clone {name})`, or return a borrow instead"
        ))
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

    pub fn function_value_mismatch(
        expected: &str,
        found: &str,
        span: Span,
        parameter: usize,
    ) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::FUNCTION_VALUE_MISMATCH,
            "mismatched function type",
            Label::new(span, format!("expected `{expected}`, found `{found}`")),
        )
        .with_note(format!("no adjustment makes parameter {parameter} match"))
        .with_help(
            "change the parameter's ownership in a `type` signature, or pass a function that matches",
        )
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
