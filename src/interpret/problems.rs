use super::Value;
use crate::diagnostic::{Counted, Diagnostic, DiagnosticCode, Label};
use crate::source::Span;

pub struct InterpretProblems;

impl InterpretProblems {
    pub fn wrong_argument(
        function: &str,
        position: usize,
        expected: &str,
        found: &Value,
        span: Span,
    ) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ERROR,
            format!(
                "`{function}` expects {expected} as argument {position}, found {}",
                found.describe()
            ),
            Label::new(span, format!("{} given here", found.describe())),
        )
    }

    pub fn wrong_argument_count(
        function: &str,
        expected: usize,
        found: usize,
        span: Span,
    ) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ERROR,
            format!(
                "`{function}` takes {} but {found} {} given",
                Counted::noun(expected, "argument"),
                Counted::was_or_were(found)
            ),
            Label::new(span, format!("{} given", Counted::noun(found, "argument"))),
        )
    }

    pub fn not_callable(value: &Value, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ERROR,
            format!("{} cannot be called", value.describe()),
            Label::new(span, "called here"),
        )
    }

    pub fn not_a_boolean(value: &Value, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ERROR,
            format!(
                "the test of `if` must be a boolean, found {}",
                value.describe()
            ),
            Label::new(span, "this test"),
        )
    }

    pub fn out_of_range(function: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ERROR,
            format!("`{function}` was given an index outside the list"),
            Label::new(span, "index out of range"),
        )
    }

    pub fn arithmetic_failed(function: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ERROR,
            format!("`{function}` overflowed or divided by zero"),
            Label::new(span, "in this call"),
        )
    }

    pub fn runtime_only(what: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ERROR,
            format!("{what} only exist at runtime"),
            Label::new(span, "used at compile time here"),
        )
    }

    pub fn too_deep(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ERROR,
            "compile-time code recursed too deeply",
            Label::new(span, "while evaluating this call"),
        )
    }

    pub fn failed(message: String, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MACRO_FAILED,
            message,
            Label::new(span, "here"),
        )
    }
}
