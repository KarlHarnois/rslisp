use crate::diagnostic::{Diagnostic, DiagnosticCode, Label};
use crate::source::Span;

pub struct EmitProblems;

pub struct UntiedNames {
    pub method: String,
    pub implementor: String,
    pub parameter: String,
}

impl EmitProblems {
    pub fn missing_main(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISSING_MAIN,
            "the entry file has no `main` function",
            Label::new(span, "a program starts at `main`"),
        )
        .with_help("add `(def main (fn () ...))` to the file you build")
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

    pub fn implementor_argument_in_result(names: &UntiedNames, span: Span) -> Diagnostic {
        Self::argument_in_result(names, span).with_help(format!(
            "write `self` in place of `{}` for this parameter in the trait",
            names.implementor
        ))
    }

    pub fn borrowed_argument_in_result(names: &UntiedNames, span: Span) -> Diagnostic {
        Self::argument_in_result(names, span).with_help(format!(
            "build the `{}` in a function outside the trait",
            names.implementor
        ))
    }

    fn argument_in_result(names: &UntiedNames, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNTIED_IMPLEMENTOR_BORROW,
            format!(
                "`{}` returns a `{}`, which cannot hold the borrow in `{}`",
                names.method, names.implementor, names.parameter
            ),
            Label::new(span, "this borrow reaches the result"),
        )
        .with_note(format!(
            "a trait signature cannot name the lifetime that `{}` holds",
            names.implementor
        ))
    }

    pub fn implementor_borrow_in_static_result(names: &UntiedNames, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNTIED_IMPLEMENTOR_BORROW,
            format!(
                "`{}` returns a borrow that cannot come from `{}`",
                names.method, names.parameter
            ),
            Label::new(span, "the result borrows from this value"),
        )
        .with_note(
            "no parameter of the trait's signature holds a borrow, so the result takes the `'static` lifetime",
        )
        .with_help("take this parameter as `(ref self)` in the trait, so the result can borrow from it")
    }

    pub fn bad_template(problem: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::BAD_TEMPLATE,
            format!("invalid Rust template: {problem}"),
            Label::new(span, "in this template"),
        )
    }
}
