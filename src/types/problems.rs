use std::rc::Rc;

use crate::diagnostic::{Diagnostic, DiagnosticCode, Label, Suggestion};
use crate::source::Span;

#[derive(Clone, Copy)]
pub enum MismatchContext {
    Plain,
    IfTest,
    IfBranch { first: Span },
    Signature { signature: Span },
    Annotation { annotation: Span },
}

pub struct TypeProblems;

impl TypeProblems {
    pub fn mismatch(
        expected: &str,
        found: &str,
        span: Span,
        context: MismatchContext,
    ) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::MISMATCHED_TYPES,
            "mismatched types",
            Label::new(span, format!("expected {expected}, found {found}")),
        );
        match context {
            MismatchContext::Plain => diagnostic,
            MismatchContext::IfTest => diagnostic.with_note("the test of `if` must be a `bool`"),
            MismatchContext::IfBranch { first } => diagnostic
                .with_label(Label::new(first, format!("the first branch is {expected}")))
                .with_note("both branches of `if` must have the same type"),
            MismatchContext::Signature { signature } => {
                diagnostic.with_label(Label::new(signature, "expected because of this signature"))
            }
            MismatchContext::Annotation { annotation } => diagnostic.with_label(Label::new(
                annotation,
                "expected because of this annotation",
            )),
        }
    }

    pub fn generic_mismatch(generic: &str, found: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISMATCHED_TYPES,
            "mismatched types",
            Label::new(span, format!("expected `{generic}`, found {found}")),
        )
        .with_note(format!(
            "`{generic}` is a type parameter, so this code must work for every type `{generic}` stands for"
        ))
    }

    pub fn infinite(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::INFINITE_TYPE,
            "this value's type would contain itself",
            Label::new(span, "here"),
        )
        .with_help("a function cannot be called with itself as its own argument type")
    }

    pub fn wrong_argument_count(expected: usize, found: usize, span: Span) -> Diagnostic {
        let arguments = if expected == 1 {
            "argument"
        } else {
            "arguments"
        };
        Diagnostic::error(
            DiagnosticCode::WRONG_ARGUMENT_COUNT,
            format!("this function takes {expected} {arguments} but {found} were given"),
            Label::new(span, format!("expected {expected} {arguments}")),
        )
    }

    pub fn not_a_function(found: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::NOT_A_FUNCTION,
            format!("expected a function, found {found}"),
            Label::new(span, "this is called but is not a function"),
        )
    }

    pub fn missing_trait(
        trait_name: &str,
        found: &str,
        span: Span,
        implementors: Vec<String>,
    ) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISSING_TRAIT,
            format!("`{found}` does not implement `{trait_name}`"),
            Label::new(span, format!("this needs `{trait_name}`")),
        )
        .with_note(format!(
            "`{trait_name}` is implemented for: {}",
            implementors.join(", ")
        ))
    }

    pub fn missing_bound(trait_name: &str, generic: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISSING_TRAIT,
            format!("`{generic}` might not implement `{trait_name}`"),
            Label::new(span, format!("this needs `{trait_name}`")),
        )
        .with_help(format!(
            "add the trait to the signature's `where`: `({generic} {trait_name})`"
        ))
    }

    pub fn out_of_range(literal: &str, type_name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::LITERAL_OUT_OF_RANGE,
            format!("the literal `{literal}` does not fit in `{type_name}`"),
            Label::new(span, format!("out of range for `{type_name}`")),
        )
    }

    pub fn cannot_infer(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::CANNOT_INFER,
            "type annotations needed",
            Label::new(span, "cannot infer the type of this value"),
        )
        .with_help(
            "annotate the binding, as in `(let (name i64) value)`, or add a `type` signature",
        )
    }

    pub fn unknown_field(
        struct_name: &str,
        field: &str,
        span: Span,
        similar: Option<Rc<str>>,
    ) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::UNKNOWN_FIELD,
            format!("`{struct_name}` has no field `{field}`"),
            Label::new(span, "unknown field"),
        );
        match similar {
            Some(similar) => diagnostic.with_suggestion(
                "a field with a similar name exists",
                Suggestion::new(span, similar.to_string()),
            ),
            None => diagnostic,
        }
    }

    pub fn missing_fields(struct_name: &str, missing: &[Rc<str>], span: Span) -> Diagnostic {
        let names: Vec<String> = missing.iter().map(|field| format!("`{field}`")).collect();
        Diagnostic::error(
            DiagnosticCode::MISSING_FIELDS,
            format!("missing fields {} in `{struct_name}`", names.join(", ")),
            Label::new(span, "every field needs a value"),
        )
    }

    pub fn no_fields(found: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::NO_FIELDS,
            format!("`{found}` has no fields"),
            Label::new(span, "only structs have fields"),
        )
    }

    pub fn ambiguous_field(field: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::CANNOT_INFER,
            "type annotations needed",
            Label::new(
                span,
                format!("cannot tell which struct's `{field}` this is"),
            ),
        )
        .with_help("annotate the value with its struct type, as in `(fn ((shape point)) ...)`")
    }

    pub fn compile_time_only(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ONLY,
            format!("`{name}` only exists at compile time"),
            Label::new(span, "used in code that runs in the program"),
        )
        .with_note("definitions that use the `code` module or `quote` can only be used by macros")
    }

    pub fn quote_at_runtime(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ONLY,
            "quoted code only exists at compile time",
            Label::new(span, "used in code that runs in the program"),
        )
        .with_help("use `quote` inside macros, or write a string if you want text")
    }

    pub fn no_type_arguments(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISPLACED_WHERE,
            "this type takes no type arguments",
            Label::new(span, "type arguments given here"),
        )
        .with_note("structs with type parameters are not supported yet")
    }

    pub fn misplaced_where(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISPLACED_WHERE,
            "`where` must wrap a whole signature",
            Label::new(span, "not allowed inside another type"),
        )
    }
}
