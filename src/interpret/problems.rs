use super::Value;
use crate::diagnostic::{Counted, Diagnostic, DiagnosticCode, Label};
use crate::source::Span;

pub struct InterpretProblems;

impl InterpretProblems {
    pub const RETURN_AS_CODE_HELP: &'static str = "to compute this in the program, return it as code from the macro, for example with `template`";

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

    pub fn builtin_argument_count(
        builtin: &str,
        expected: usize,
        found: usize,
        span: Span,
    ) -> Diagnostic {
        Self::argument_count(&format!("`{builtin}`"), expected, found, span)
    }

    pub fn closure_argument_count(expected: usize, found: usize, span: Span) -> Diagnostic {
        Self::argument_count("this function", expected, found, span)
    }

    fn argument_count(callee: &str, expected: usize, found: usize, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ERROR,
            format!(
                "{callee} takes {} but {found} {} given",
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
            format!("`{function}` overflowed"),
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

    pub fn program_only(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::PROGRAM_ONLY,
            "this Rust code belongs to the program, so macro code cannot run it",
            Label::new(span, "macro code runs this"),
        )
        .with_help(Self::RETURN_AS_CODE_HELP)
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
