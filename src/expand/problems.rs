use std::rc::Rc;

use crate::core::{KernelForm, KernelType, ModulePath, Namespace};
use crate::diagnostic::{Diagnostic, DiagnosticCode, Label, Suggestion};
use crate::interpret::InterpretProblems;
use crate::source::Span;

pub struct ExpandProblems;

impl ExpandProblems {
    const QUOTE_SHORTHAND: char = '\'';

    pub fn unbound(
        name: &str,
        namespace: Namespace,
        span: Span,
        similar: Option<Rc<str>>,
    ) -> Diagnostic {
        let what = Self::describe(namespace);
        let diagnostic = Diagnostic::error(
            DiagnosticCode::UNBOUND_NAME,
            format!("cannot find {what}`{name}` in this scope"),
            Label::new(span, "not found in this scope"),
        );
        let diagnostic = match similar {
            Some(similar) => diagnostic.with_suggestion(
                "a name with a similar spelling exists",
                Suggestion::new(span, similar.to_string()),
            ),
            None => diagnostic,
        };
        if name.starts_with(Self::QUOTE_SHORTHAND) {
            diagnostic.with_help(format!(
                "there is no quote shorthand; write `(quote {})`",
                &name[1..]
            ))
        } else {
            diagnostic
        }
    }

    pub fn ambiguous(name: &str, span: Span, definitions: Vec<Span>) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::AMBIGUOUS_NAME,
            format!("`{name}` is ambiguous"),
            Label::new(span, "this name could refer to more than one binding"),
        );
        definitions
            .into_iter()
            .fold(diagnostic, |diagnostic, definition| {
                diagnostic.with_label(Label::new(definition, "one candidate is defined here"))
            })
    }

    pub fn duplicate(name: &str, span: Span, previous: Option<Span>) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::DUPLICATE_DEFINITION,
            format!("`{name}` is defined more than once"),
            Label::new(span, "defined again here"),
        );
        match previous {
            Some(previous) => diagnostic.with_label(Label::new(previous, "first defined here")),
            None => diagnostic.with_note(format!(
                "`{name}` is already defined by the prelude or an import"
            )),
        }
    }

    pub fn unknown_module(path: &ModulePath, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNKNOWN_MODULE,
            format!("cannot find module `{path}`"),
            Label::new(span, "no module with this path"),
        )
        .with_help("a module path names a file relative to the entry file, without its extension")
    }

    pub fn missing_export(
        path: &ModulePath,
        name: &str,
        span: Span,
        similar: Option<Rc<str>>,
    ) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::MISSING_EXPORT,
            format!("module `{path}` does not export `{name}`"),
            Label::new(span, "not exported"),
        );
        match similar {
            Some(similar) => diagnostic.with_suggestion(
                "an export with a similar name exists",
                Suggestion::new(span, similar.to_string()),
            ),
            None => diagnostic.with_help(format!("add `{name}` to the `export` list of `{path}`")),
        }
    }

    pub fn import_cycle(path: &ModulePath, span: Span, cycle: Vec<String>) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::IMPORT_CYCLE,
            format!("importing `{path}` creates a cycle"),
            Label::new(span, "this import closes the cycle"),
        )
        .with_note(format!(
            "the modules import each other: {}",
            cycle.join(" -> ")
        ))
        .with_help("move the shared definitions into a module that both can import")
    }

    pub fn not_a_definition(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::NOT_A_DEFINITION,
            "expected a definition at module level",
            Label::new(span, "this is not a definition"),
        )
        .with_help("put expressions inside a function, such as `(defn main () ...)`")
    }

    pub fn module_level_only(kernel: KernelForm, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MODULE_LEVEL_ONLY,
            format!("`{}` can only appear at module level", kernel.name()),
            Label::new(span, "not allowed inside an expression"),
        )
        .with_help("use `let` to bind a name inside a body")
    }

    pub fn let_outside_body(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MODULE_LEVEL_ONLY,
            "`let` can only appear directly in a body",
            Label::new(span, "not allowed here"),
        )
        .with_help("wrap the `let` and the code that uses it in `(do ...)`")
    }

    pub fn macro_outside_def(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MALFORMED_FORM,
            "`macro` must be the value of a module-level `def`",
            Label::new(span, "not allowed here"),
        )
        .with_help("write `(def name (macro (form) body ...))`")
    }

    pub fn empty_call(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::EMPTY_CALL,
            "`()` is not an expression",
            Label::new(span, "an empty list has nothing to call"),
        )
        .with_suggestion(
            "the unit value is written `(do)`",
            Suggestion::new(span, "(do)"),
        )
    }

    pub fn form_as_value(kernel: KernelForm, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::FORM_AS_VALUE,
            format!("`{}` is a form, not a value", kernel.name()),
            Label::new(span, "used as a value here"),
        )
        .with_help(format!("write it as `{}`", kernel.shape()))
    }

    pub fn macro_as_value(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MACRO_AS_VALUE,
            format!("`{name}` is a macro, not a value"),
            Label::new(span, "used as a value here"),
        )
        .with_help("a macro can only be called; pass a function instead")
    }

    pub fn wrong_namespace(name: &str, expected: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::WRONG_NAMESPACE,
            format!("expected {expected}, found `{name}`"),
            Label::new(span, format!("not {expected}")),
        )
    }

    pub fn macro_result(found: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MACRO_RESULT_NOT_FORM,
            format!("this macro returned {found} instead of code"),
            Label::new(span, "in this macro call"),
        )
    }

    pub fn expansion_too_deep(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::EXPANSION_TOO_DEEP,
            "macro expansion did not finish",
            Label::new(span, "expanding this form keeps producing more macro calls"),
        )
        .with_help("check that every recursive macro has a case that stops")
    }

    pub fn compile_time_cycle(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_CYCLE,
            format!("`{name}` depends on itself at compile time"),
            Label::new(span, "needed here while it is still being computed"),
        )
    }

    pub fn not_a_type(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::WRONG_NAMESPACE,
            "expected a type",
            Label::new(span, "this is not a type"),
        )
        .with_help(
            "a type is a name such as `i64`, or a form such as `(fn (i64) i64)` or `(ref str)`",
        )
    }

    pub fn malformed_type(constructor: KernelType, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MALFORMED_FORM,
            format!("malformed `{}` type", constructor.name()),
            Label::new(span, "this does not fit the shape of the type"),
        )
        .with_help(format!("write it as `{}`", constructor.shape()))
    }

    pub fn signature_target(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::WRONG_NAMESPACE,
            format!("`{name}` cannot have a type signature here"),
            Label::new(span, "not a definition of this module"),
        )
        .with_help("a `type` signature must sit in the module whose `def` it describes")
    }

    pub fn broken_definition(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ERROR,
            format!("`{name}` has errors, so macro code cannot use it"),
            Label::new(span, "used by macro code here"),
        )
    }

    pub fn no_compile_time_version(name: &str, used: Span, defined: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::NO_COMPILE_TIME_TWIN,
            format!("`{name}` has no compile-time version"),
            Label::new(used, "used by macro code here"),
        )
        .with_label(Label::new(
            defined,
            format!("`{name}` is Rust code with no twin from the `code` module"),
        ))
        .with_help(InterpretProblems::MISSING_TWIN_HELP)
    }

    pub fn not_at_compile_time(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ERROR,
            format!("`{name}` is not available at compile time"),
            Label::new(span, "used by macro code here"),
        )
    }

    fn describe(namespace: Namespace) -> &'static str {
        match namespace {
            Namespace::Value => "",
            Namespace::Type => "type ",
        }
    }
}
