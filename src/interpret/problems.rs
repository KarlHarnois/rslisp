use std::rc::Rc;

use super::Value;
use super::builtins::FailureDescription;
use crate::diagnostic::{Counted, Diagnostic, DiagnosticCode, Label, Suggestion};
use crate::source::Span;

pub struct InterpretProblems;

impl InterpretProblems {
    pub const RETURN_AS_CODE_HELP: &'static str = "to compute this in the program, return it as code from the macro, for example with `quote`";

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
            DiagnosticCode::COMPILE_TIME_ARGUMENT_COUNT,
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
            DiagnosticCode::NOT_CALLABLE,
            format!("{} cannot be called", value.describe()),
            Label::new(span, "called here"),
        )
    }

    pub fn not_a_boolean(value: &Value, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::NOT_A_BOOLEAN,
            format!(
                "the test of `if` must be a boolean, found {}",
                value.describe()
            ),
            Label::new(span, "this test"),
        )
    }

    pub fn out_of_range(function: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::INDEX_OUT_OF_RANGE,
            format!("`{function}` was given an index outside the list"),
            Label::new(span, "index out of range"),
        )
    }

    pub fn arithmetic_failed(function: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::ARITHMETIC_OVERFLOW,
            format!("`{function}` overflowed"),
            Label::new(span, "in this call"),
        )
    }

    pub fn runtime_only(what: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::RUNTIME_ONLY,
            format!("{what} only exist at runtime"),
            Label::new(span, "used at compile time here"),
        )
    }

    pub fn match_only_at_runtime(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MATCH_AT_RUNTIME,
            "a `match` only exists at runtime",
            Label::new(span, "used at compile time here"),
        )
    }

    pub fn program_only(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::RUST_CODE_PROGRAM_ONLY,
            "this Rust code belongs to the program, so macro code cannot run it",
            Label::new(span, "macro code runs this"),
        )
        .with_help(Self::RETURN_AS_CODE_HELP)
    }

    pub fn too_deep(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_RECURSION,
            "compile-time code recursed too deeply",
            Label::new(span, "while evaluating this call"),
        )
    }

    pub fn failed(
        code: DiagnosticCode,
        message: String,
        culprit: Span,
        label: Option<String>,
        earlier: Option<(Span, String)>,
    ) -> Diagnostic {
        let label = label.unwrap_or_else(|| "here".to_owned());
        let failure = Diagnostic::error(code, message, Label::new(culprit, label));
        match earlier {
            Some((span, label)) => failure.with_label(Label::new(span, label)),
            None => failure,
        }
    }

    pub fn missing_earlier_label(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::FAILURE_PART,
            "an `:earlier` part needs an `:earlier-label`",
            Label::new(span, "this `:earlier` part has no `:earlier-label`"),
        )
        .with_help("add `:earlier-label \"text\"` beside `:earlier`")
    }

    pub fn stray_earlier_label(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::FAILURE_PART,
            "an `:earlier-label` part needs an `:earlier`",
            Label::new(span, "this `:earlier-label` part has no `:earlier`"),
        )
        .with_help("add `:earlier` beside `:earlier-label`")
    }

    pub fn malformed_failure_part(span: Span) -> Diagnostic {
        let parts: Vec<String> = FailureDescription::PARTS
            .iter()
            .map(|part| format!("`:{part}`"))
            .collect();
        Diagnostic::error(
            DiagnosticCode::FAILURE_PART,
            "a failure description takes `:part value` pairs",
            Label::new(span, "not a `:part value` pair"),
        )
        .with_help(format!("the parts are {}", parts.join(", ")))
    }

    pub fn unknown_failure_code(name: &str, span: Span, similar: Option<Rc<str>>) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::UNKNOWN_FAILURE_CODE,
            format!("no macro diagnostic is named `{name}`"),
            Label::new(span, "unknown diagnostic"),
        );
        match similar {
            Some(similar) => diagnostic.with_suggestion(
                "a diagnostic with a similar name exists",
                Suggestion::new(span, similar.as_ref()),
            ),
            None => diagnostic,
        }
    }
}
