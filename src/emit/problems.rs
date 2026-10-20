use crate::diagnostic::{Diagnostic, DiagnosticCode, Label};
use crate::source::Span;

pub struct EmitProblems;

impl EmitProblems {
    pub fn missing_main(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISSING_MAIN,
            "the entry file has no `main` function",
            Label::new(span, "a program starts at `main`"),
        )
        .with_help("add `(defn main () ...)` to the file you build")
    }

    pub fn name_collision(
        rust_name: &str,
        first: &str,
        second: &str,
        span: Span,
        other: Option<Span>,
    ) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::NAME_COLLISION,
            format!("`{first}` and `{second}` both become `{rust_name}` in Rust"),
            Label::new(span, format!("this becomes `{rust_name}`")),
        )
        .with_help("rename one of them");
        match other {
            Some(other) => diagnostic.with_label(Label::new(
                other,
                format!("this also becomes `{rust_name}`"),
            )),
            None => diagnostic,
        }
    }

    pub fn bad_template(problem: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::BAD_TEMPLATE,
            format!("invalid Rust template: {problem}"),
            Label::new(span, "in this template"),
        )
    }
}
