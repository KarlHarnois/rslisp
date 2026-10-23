use std::rc::Rc;

use crate::core::{KernelForm, KernelType, ModulePath, Namespace};
use crate::diagnostic::{Counted, Diagnostic, DiagnosticCode, Label, Suggestion};
use crate::interpret::InterpretProblems;
use crate::source::Span;

pub struct ExpandProblems;

impl ExpandProblems {
    const QUOTE_SHORTHAND: char = '\'';
    const CODE_REPLACEMENTS: [(&'static str, &'static str, &'static str); 9] = [
        ("add", "number-add", "code"),
        ("sub", "number-sub", "code"),
        ("lt", "number-lt", "code"),
        ("le", "number-le", "code"),
        ("gt", "number-gt", "code"),
        ("ge", "number-ge", "code"),
        ("eq", "value-eq", "code"),
        ("not", "bool-not", "code"),
        ("map", "map-list", "prelude/macros"),
    ];

    pub fn malformed_type_template(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::BAD_TYPE_TEMPLATE,
            "this type template has a malformed placeholder",
            Label::new(span, "a placeholder is `{`, a number and `}`"),
        )
        .with_help("write a literal brace as `{{` or `}}`")
    }

    pub fn malformed_template(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MALFORMED_TEMPLATE,
            "this Rust template has a malformed placeholder",
            Label::new(span, "a placeholder is `{`, a number and `}`"),
        )
        .with_help("write a literal brace as `{{` or `}}`")
    }

    pub fn borrowed_type_placeholder(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::BAD_TYPE_TEMPLATE,
            "a type template cannot borrow a placeholder",
            Label::new(span, "`{&...}` only makes sense for a value"),
        )
        .with_help("write the placeholder as `{0}`")
    }

    pub fn placeholder_without_parameter(
        index: usize,
        parameters: usize,
        span: Span,
    ) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::BAD_TYPE_TEMPLATE,
            format!("`{{{index}}}` names no type parameter"),
            Label::new(
                span,
                format!(
                    "this type has {}",
                    Counted::noun(parameters, "type parameter")
                ),
            ),
        )
        .with_help(
            "declare the type parameters on the name, as in `(rust-type (vec item) \"Vec<{0}>\")`",
        )
    }

    pub fn unused_type_placeholder(index: usize, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::BAD_TYPE_TEMPLATE,
            "the template never uses this type parameter",
            Label::new(span, format!("its placeholder is `{{{index}}}`")),
        )
        .with_help(format!(
            "use `{{{index}}}` in the template, or remove the parameter"
        ))
    }

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

    pub fn macro_as_value(name: &str, span: Span, function: Option<Rc<str>>) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::MACRO_AS_VALUE,
            format!("`{name}` is a macro, not a value"),
            Label::new(span, "used as a value here"),
        );
        match function {
            Some(function) => diagnostic.with_suggestion(
                format!("a macro can only be called; pass `{function}` instead"),
                Suggestion::new(span, function.to_string()),
            ),
            None => diagnostic.with_help("a macro can only be called; pass a function instead"),
        }
    }

    pub fn wrong_namespace(name: &str, expected: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::WRONG_NAMESPACE,
            format!("expected {expected}, found `{name}`"),
            Label::new(span, format!("not {expected}")),
        )
    }

    pub fn recur_outside_tail(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::RECUR_OUTSIDE_TAIL,
            "`recur` is not in tail position",
            Label::new(span, "this `recur` is not the last thing the function does"),
        )
        .with_help("make `recur` the last form of the function, or call the function by name")
    }

    pub fn recur_argument_count(expected: usize, found: usize, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::RECUR_ARGUMENT_COUNT,
            format!(
                "`recur` passes {}, but its function takes {expected}",
                Counted::noun(found, "argument")
            ),
            Label::new(span, format!("passes {}", Counted::noun(found, "argument"))),
        )
        .with_help(format!(
            "write `recur` with {}, one for each parameter of the function",
            Counted::noun(expected, "argument")
        ))
    }

    pub fn try_outside_fn(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::TRY_OUTSIDE_FN,
            "`try` is not inside a function",
            Label::new(span, "there is no function here for `try` to return from"),
        )
        .with_help("write `try` inside a `fn` body")
    }

    pub fn recur_outside_fn(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::RECUR_OUTSIDE_FN,
            "`recur` is not inside a function",
            Label::new(
                span,
                "there is no function here for `recur` to jump back to",
            ),
        )
        .with_help("write `recur` inside a `fn` body")
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

    pub fn parameters_on_name(kind: &str, replacement: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::TYPE_PARAMETERS_ON_NAME,
            format!("`{kind}` names its type parameters on its name"),
            Label::new(span, "this `where` names them in the body"),
        )
        .with_help(format!("write `{replacement}`"))
    }

    pub fn mixed_variant_payload(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MALFORMED_FORM,
            "a variant's payload is either types or one `struct` body",
            Label::new(span, "this mixes the two"),
        )
        .with_help(
            "write the payload as types, as in `(circle f64)`, or as one struct body, as in `(labeled (struct (text string)))`",
        )
    }

    pub fn not_a_name(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::WRONG_NAMESPACE,
            "expected a name",
            Label::new(span, "this is not a name"),
        )
    }

    pub fn variant_as_value(name: &str, arity: usize, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MALFORMED_FORM,
            format!("`{name}` is a variant constructor, not a value"),
            Label::new(
                span,
                format!("this constructor takes {arity} when it is called"),
            ),
        )
        .with_help(format!("write `({name} value ...)`"))
    }

    pub fn signature_target(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::WRONG_NAMESPACE,
            format!("`{name}` cannot have a type signature here"),
            Label::new(span, "not a definition of this module"),
        )
        .with_help("a `type` signature must sit in the module whose `def` it describes")
    }

    pub fn derive_target(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::WRONG_NAMESPACE,
            format!("`{name}` cannot have a `derive` here"),
            Label::new(span, "not a struct of this module"),
        )
        .with_help("a `derive` must sit in the module whose `struct` it describes")
    }

    pub fn derive_trait(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::WRONG_NAMESPACE,
            format!("`{name}` is not a trait"),
            Label::new(span, "a `derive` lists traits"),
        )
        .with_help("list traits such as `clone`, `debug` or `ord`")
    }

    pub fn broken_definition(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ERROR,
            format!("`{name}` has errors, so macro code cannot use it"),
            Label::new(span, "used by macro code here"),
        )
    }

    pub fn program_only(name: &str, used: Span, defined: Span) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::PROGRAM_ONLY,
            format!("`{name}` belongs to the program, so macro code cannot run it"),
            Label::new(used, "macro code runs this"),
        )
        .with_label(Label::new(
            defined,
            format!("`{name}` is Rust code that runs in the program"),
        ))
        .with_help(InterpretProblems::RETURN_AS_CODE_HELP);
        match Self::code_replacement(name) {
            Some((replacement, module)) => diagnostic.with_help(format!(
                "in macro code, use `{replacement}` from `{module}` instead"
            )),
            None => diagnostic,
        }
    }

    pub fn macro_program_only(name: &str, used: Span, defined: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::PROGRAM_ONLY,
            format!("`{name}` belongs to the program, so macro code cannot run it"),
            Label::new(used, "macro code runs this"),
        )
        .with_label(Label::new(
            defined,
            format!("`{name}` expands to Rust code that runs in the program"),
        ))
        .with_help(InterpretProblems::RETURN_AS_CODE_HELP)
    }

    fn code_replacement(name: &str) -> Option<(&'static str, &'static str)> {
        Self::CODE_REPLACEMENTS
            .iter()
            .find(|(program, _, _)| *program == name)
            .map(|(_, replacement, module)| (*replacement, *module))
    }

    pub fn broken_macro(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ERROR,
            format!("the macro `{name}` has errors, so this call cannot expand"),
            Label::new(span, "this call needs the macro"),
        )
    }

    pub fn not_at_compile_time(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::COMPILE_TIME_ERROR,
            format!("`{name}` is not available at compile time"),
            Label::new(span, "used by macro code here"),
        )
    }

    pub fn second_trait_path(name: &str, existing: &str, path: &str, span: Span) -> Diagnostic {
        let title = if name == existing {
            format!("`{name}` binds `{path}` again")
        } else {
            format!("`{name}` binds a Rust path that `{existing}` already binds")
        };
        let help = if name == existing {
            format!("remove the second `{name}`, or give it a different Rust path")
        } else {
            format!("use `{existing}`, or give `{name}` a different Rust path")
        };
        Diagnostic::warning(
            DiagnosticCode::SECOND_TRAIT_PATH,
            title,
            Label::new(span, format!("this also binds `{path}`")),
        )
        .with_note("two traits that bind one Rust path can disagree about its implementors")
        .with_help(help)
    }

    fn describe(namespace: Namespace) -> &'static str {
        match namespace {
            Namespace::Value => "",
            Namespace::Type => "type ",
        }
    }
}
