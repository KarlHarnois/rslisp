use std::rc::Rc;

use super::parameter_calls::ParameterCall;
use crate::core::{FieldLabel, Keyword};
use crate::diagnostic::{Counted, Diagnostic, DiagnosticCode, Label, Suggestion};
use crate::source::Span;

#[derive(Clone)]
pub enum MismatchContext {
    Plain,
    IfTest {
        expanded_from: Option<Rc<str>>,
    },
    IfBranch {
        first: Span,
        expanded_from: Option<Rc<str>>,
    },
    Signature {
        signature: Span,
    },
    Annotation {
        annotation: Span,
    },
}

pub struct TypeProblems;

#[derive(Default)]
pub struct ImplementorList {
    pub entries: Vec<String>,
    pub groups: Vec<ImplementorGroup>,
}

pub struct ProjectionNames<'a> {
    pub projection: &'a str,
    pub trait_name: &'a str,
    pub owner: &'a str,
}

impl ProjectionNames<'_> {
    fn needs(&self) -> String {
        format!(
            "`({} {})` needs `{}` to implement `{}`",
            self.projection, self.owner, self.owner, self.trait_name
        )
    }
}

pub struct ImplementorGroup {
    pub constructor: String,
    pub parameter: String,
    pub arguments: Vec<String>,
}

impl TypeProblems {
    fn expansion_name(expanded_from: Option<Rc<str>>) -> String {
        expanded_from.map_or_else(|| "if".to_string(), |name| name.to_string())
    }

    fn if_test_note(expanded_from: Option<Rc<str>>) -> String {
        format!(
            "the test of `{}` must be a `bool`",
            Self::expansion_name(expanded_from)
        )
    }

    fn if_branch_note(expanded_from: Option<Rc<str>>) -> String {
        format!(
            "both branches of `{}` must have the same type",
            Self::expansion_name(expanded_from)
        )
    }

    pub fn try_needs_value(found: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::TRY_NOT_RESULT_OR_OPTION,
            "`try` needs a result or an option",
            Label::new(span, format!("this is {found}")),
        )
        .with_help("write `try` around a result or an option value")
    }

    pub fn try_needs_unknown_recursive(
        called: &str,
        group: &[String],
        hides_prelude: bool,
        span: Span,
    ) -> Diagnostic {
        let names: Vec<String> = group.iter().map(|name| format!("`{name}`")).collect();
        let diagnostic = Diagnostic::error(
            DiagnosticCode::TRY_NOT_RESULT_OR_OPTION,
            "`try` needs a result or an option",
            Label::new(span, "the type of this call is not known yet"),
        )
        .with_help(format!(
            "write a `type` signature for one of: {}",
            names.join(", ")
        ));
        if hides_prelude {
            diagnostic.with_note(format!(
                "this `{called}` is the program's own, and it hides the prelude's `{called}`"
            ))
        } else {
            diagnostic
        }
    }

    pub fn try_cannot_return(needed: &str, found: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::TRY_TARGET,
            "`try` cannot return from this function",
            Label::new(span, format!("this function returns {found}")),
        )
        .with_note(format!("`try` here needs a function that returns {needed}"))
        .with_help(
            "make the function return the value `try` needs, or handle this value without `try`",
        )
    }

    pub fn try_ambiguous_conversion(
        source: &str,
        target: &str,
        variants: &[(String, String)],
        span: Span,
    ) -> Diagnostic {
        let mut diagnostic = Diagnostic::error(
            DiagnosticCode::TRY_NO_CONVERSION,
            "`try` cannot choose an error variant",
            Label::new(span, format!("this error is {source}")),
        )
        .with_note(format!(
            "this function returns a result whose error is {target}"
        ))
        .with_note("`try` converts an error through one variant of an error enum that holds it");
        for (name, payload) in variants {
            diagnostic = diagnostic.with_note(format!("variant `{name}` holds {payload}"));
        }
        diagnostic
            .with_help("keep one variant for each error type, or convert the error with `map-err`")
    }

    pub fn try_no_conversion(source: &str, target: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::TRY_NO_CONVERSION,
            "`try` cannot convert this error",
            Label::new(span, format!("this error is {source}")),
        )
        .with_note(format!(
            "this function returns a result whose error is {target}"
        ))
        .with_note("`try` converts an error through one variant of an error enum that holds it")
        .with_help(
            "write the function's error type as an enum with one variant for each error it tries, or convert the error with `map-err`",
        )
    }

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
            MismatchContext::IfTest { expanded_from } => {
                diagnostic.with_note(Self::if_test_note(expanded_from))
            }
            MismatchContext::IfBranch {
                first,
                expanded_from,
            } => diagnostic
                .with_label(Label::new(first, format!("the first branch is {expected}")))
                .with_note(Self::if_branch_note(expanded_from)),
            MismatchContext::Signature { signature } => {
                diagnostic.with_label(Label::new(signature, "expected because of this signature"))
            }
            MismatchContext::Annotation { annotation } => diagnostic.with_label(Label::new(
                annotation,
                "expected because of this annotation",
            )),
        }
    }

    pub fn parameter_call(
        diagnostic: Diagnostic,
        function: &str,
        call: &ParameterCall,
    ) -> Diagnostic {
        let parameter = &call.parameter;
        let diagnostic = diagnostic.with_label(Label::new(
            call.span,
            format!(
                "`{function}` calls its parameter `{parameter}` here, so it expects a function"
            ),
        ));
        if !call.shadows_function {
            return diagnostic;
        }
        Self::parameter_shadows_function(diagnostic, call)
    }

    fn parameter_shadows_function(diagnostic: Diagnostic, call: &ParameterCall) -> Diagnostic {
        let parameter = &call.parameter;
        if call.arguments > 0 {
            return diagnostic.with_help(format!(
                "the parameter `{parameter}` shadows the function `{parameter}`, so to call the function, rename the parameter"
            ));
        }
        diagnostic.with_suggestion(
            format!(
                "the parameter `{parameter}` shadows the function `{parameter}`, so to use the parameter's value, write it without parentheses"
            ),
            Suggestion::new(call.span, parameter.to_string()),
        )
    }

    pub fn iter_needs_collection(found: &str, span: Span) -> Diagnostic {
        Self::mismatch(
            "a vector, a slice or a map",
            found,
            span,
            MismatchContext::Plain,
        )
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

    pub fn literal_default_note(
        parameter: &str,
        default: &str,
        function: &str,
        calls: &[String],
    ) -> String {
        format!(
            "`{parameter}` defaulted to `{default}`, because `{function}` is called with {}",
            Self::literal_calls(calls)
        )
    }

    fn literal_calls(calls: &[String]) -> String {
        calls
            .iter()
            .map(|call| format!("`{call}`"))
            .collect::<Vec<_>>()
            .join(" and with ")
    }

    pub fn literal_default_help(function: &str, signature: &str) -> String {
        format!(
            "give `{function}` one parameter type with a `type` signature, such as `(type {function} {signature})`, and pass that type at every call"
        )
    }

    pub fn borrowed_items_help(function: &str, signature: &str) -> String {
        format!(
            "`{function}` takes owned items, but the argument yields borrowed items: write `(type {function} {signature})` to take borrowed items"
        )
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
        let expected = Counted::noun(expected, "argument");
        Diagnostic::error(
            DiagnosticCode::WRONG_ARGUMENT_COUNT,
            format!(
                "this function takes {expected} but {found} {} given",
                Counted::was_or_were(found)
            ),
            Label::new(span, format!("expected {expected}")),
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
        implementors: ImplementorList,
    ) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::MISSING_TRAIT,
            format!("`{found}` does not implement `{trait_name}`"),
            Label::new(span, format!("this needs `{trait_name}`")),
        )
        .with_note(format!(
            "`{trait_name}` is implemented for: {}",
            implementors.entries.join(", ")
        ));
        implementors
            .groups
            .into_iter()
            .fold(diagnostic, |diagnostic, group| {
                diagnostic.with_note(Self::implementor_group_note(trait_name, &group))
            })
    }

    fn implementor_group_note(trait_name: &str, group: &ImplementorGroup) -> String {
        format!(
            "`{trait_name}` is implemented for `({} {})` for each `{}` in: {}",
            group.constructor,
            group.parameter,
            group.parameter,
            group.arguments.join(", ")
        )
    }

    pub fn missing_associated_value(trait_name: &str, projection: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISSING_ASSOCIATED_VALUE,
            format!("this entry of `{trait_name}` does not give `{projection}`"),
            Label::new(
                span,
                format!("`{trait_name}` declares the associated type `{projection}`"),
            ),
        )
        .with_help(format!(
            "end the entry with `({projection} type)`, as in `(where () i64 ({projection} type))`"
        ))
    }

    pub fn undeclared_associated_value(
        trait_name: &str,
        projection: &str,
        span: Span,
    ) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNDECLARED_ASSOCIATED_VALUE,
            format!("`{trait_name}` does not declare `{projection}`"),
            Label::new(
                span,
                format!("`{projection}` is not an associated type of `{trait_name}`"),
            ),
        )
        .with_help(format!(
            "remove the clause, or declare it with `(associated ({projection} owner) \"Name\")`"
        ))
    }

    pub fn projection_without_implementor(names: &ProjectionNames<'_>, span: Span) -> Diagnostic {
        let ProjectionNames {
            projection,
            trait_name,
            owner,
        } = names;
        Diagnostic::error(
            DiagnosticCode::PROJECTION_WITHOUT_IMPLEMENTOR,
            format!("`{owner}` does not implement `{trait_name}`"),
            Label::new(span, names.needs()),
        )
        .with_note(format!(
            "`{projection}` is an associated type of `{trait_name}`"
        ))
    }

    pub fn projection_without_bound(names: &ProjectionNames<'_>, span: Span) -> Diagnostic {
        let ProjectionNames {
            trait_name, owner, ..
        } = names;
        Diagnostic::error(
            DiagnosticCode::PROJECTION_WITHOUT_BOUND,
            format!("`{owner}` might not implement `{trait_name}`"),
            Label::new(span, names.needs()),
        )
        .with_help(format!(
            "add the trait to the `where` that binds `{owner}`: `({owner} {trait_name})`"
        ))
    }

    pub fn foreign_implementor(trait_name: &str, found: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::FOREIGN_IMPLEMENTOR,
            format!("`{found}` cannot implement `{trait_name}`"),
            Label::new(
                span,
                format!("only a struct or enum of this program can implement `{trait_name}`"),
            ),
        )
        .with_note("Rust implements a trait only for a type in the crate that defines the trait or the type")
        .with_help("implement it for a struct or enum this program defines")
    }

    pub fn missing_supertrait(
        trait_name: &str,
        supertrait_name: &str,
        implementor: &str,
        span: Span,
        help: Option<Rc<str>>,
    ) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::SUPERTRAIT,
            format!(
                "`{implementor}` does not implement `{supertrait_name}`, a supertrait of `{trait_name}`"
            ),
            Label::new(
                span,
                format!("`{implementor}` does not implement `{supertrait_name}`"),
            ),
        );
        match help {
            Some(help) => diagnostic.with_help(help.to_string()),
            None => {
                diagnostic.with_help(format!("implement `{supertrait_name}` for `{implementor}`"))
            }
        }
    }

    pub fn missing_trait_on_struct(trait_name: &str, found: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISSING_TRAIT,
            format!("`{found}` does not implement `{trait_name}`"),
            Label::new(span, format!("this needs `{trait_name}`")),
        )
    }

    pub fn missing_trait_on_box(trait_name: &str, found: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISSING_TRAIT,
            format!("`{found}` does not implement `{trait_name}`"),
            Label::new(span, format!("this needs `{trait_name}`")),
        )
        .with_help(format!(
            "write the type `(box {trait_name})` where the box is made, such as `(let (shapes (vec (box {trait_name}))) ...)`"
        ))
    }

    pub fn order_not_inferred_note(trait_name: &str) -> String {
        format!(
            "`{trait_name}` is never inferred for a struct, so the struct needs a written `derive`"
        )
    }

    pub fn order_derive_help(struct_name: &str, derives: &[String], fields: &[Rc<str>]) -> String {
        format!(
            "add `(derive {struct_name} {})` to compare fields in this order: {}",
            derives.join(" "),
            Self::field_order(fields)
        )
    }

    pub fn order_blocked_note(
        struct_name: &str,
        field: &str,
        field_type: &str,
        trait_name: &str,
    ) -> String {
        format!("`{struct_name}` cannot derive `{trait_name}`: field `{field}` is `{field_type}`")
    }

    pub fn sort_by_key_help(field: Option<&str>) -> String {
        match field {
            Some(field) => format!(
                "to sort by one field instead, use `sort-by-key!` with the field keyword `{}{field}`",
                Keyword::MARKER
            ),
            None => "to sort by one field instead, use `sort-by-key!`".to_string(),
        }
    }

    fn field_order(fields: &[Rc<str>]) -> String {
        let names: Vec<String> = fields.iter().map(|field| format!("`{field}`")).collect();
        match names.split_last() {
            None => "no fields".to_string(),
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{}, then {last}", rest.join(", ")),
        }
    }

    pub fn blocked_derive_note(
        struct_name: &str,
        trait_name: &str,
        field: &str,
        field_type: &str,
    ) -> String {
        format!("`{struct_name}` cannot derive `{trait_name}`: field `{field}` is `{field_type}`")
    }

    pub fn missing_bound(trait_name: &str, generic: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISSING_BOUND,
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

    pub fn literal_of_any_type(literal: &str, parameter: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::LITERAL_TYPE_PARAMETER,
            format!("`{literal}` cannot have type `{parameter}`"),
            Label::new(span, format!("this literal has type `{parameter}`")),
        )
        .with_note(format!(
            "`{parameter}` has no trait, so it can be any type, including `string`"
        ))
        .with_help(format!(
            "give `{parameter}` a numeric trait in the signature"
        ))
    }

    pub fn literal_misfit(
        literal: &str,
        parameter: &str,
        misfit: &str,
        admissible: &[String],
        span: Span,
    ) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::LITERAL_MISFIT,
            format!("`{literal}` does not fit every type `{parameter}` can be"),
            Label::new(
                span,
                format!("`{parameter}` can be `{misfit}`, which cannot hold this value"),
            ),
        )
        .with_note(format!(
            "`{parameter}` can be any of: {}",
            admissible.join(", ")
        ))
        .with_help(format!(
            "narrow `{parameter}` with a trait whose types all hold this value"
        ))
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

    pub fn ambiguous_variant(name: &str, enums: &[String], span: Span) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::AMBIGUOUS_VARIANT,
            "cannot tell which enum this pattern matches",
            Label::new(span, "this variant name fits more than one enum"),
        )
        .with_note(format!(
            "this variant name fits {}",
            enums
                .iter()
                .map(|enum_name| format!("`{enum_name}`"))
                .collect::<Vec<_>>()
                .join(" and ")
        ));
        match enums.first() {
            Some(enum_name) if !name.is_empty() => diagnostic.with_help(format!(
                "qualify one pattern with its enum's name, as in `({enum_name}/{name} ...)`"
            )),
            _ => diagnostic.with_help("qualify one pattern with its enum's name"),
        }
    }

    pub fn variant_arm_value(name: &str, variant: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::VARIANT_ARM_VALUE,
            format!("`{name}` names a variant in this arm, not a value"),
            Label::new(span, "names the variant, not a value"),
        )
        .with_help(format!("write `{variant}` for the variant's value"))
    }

    pub fn unknown_variant(
        enum_name: &str,
        variant: &str,
        variants: &[Rc<str>],
        span: Span,
    ) -> Diagnostic {
        let names: Vec<String> = variants.iter().map(|name| format!("`{name}`")).collect();
        Diagnostic::error(
            DiagnosticCode::UNKNOWN_VARIANT,
            format!("`{enum_name}` has no variant `{variant}`"),
            Label::new(span, format!("not a variant of `{enum_name}`")),
        )
        .with_help(format!("write one of: {}", names.join(", ")))
    }

    pub fn non_exhaustive_match(cases: &[String], span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::NON_EXHAUSTIVE_MATCH,
            "non-exhaustive `match`",
            Label::new(span, "this `match` does not cover every value"),
        )
        .with_note(format!(
            "missing {}: {}",
            Counted::noun(cases.len(), "case"),
            cases
                .iter()
                .map(|case| format!("`{case}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .with_help("add an arm for each missing case")
    }

    pub fn unreachable_arm(span: Span, covered_by: Option<Span>) -> Diagnostic {
        let diagnostic = Diagnostic::warning(
            DiagnosticCode::UNREACHABLE_ARM,
            "unreachable `match` arm",
            Label::new(span, "this arm can never match"),
        );
        match covered_by {
            Some(covered_by) => diagnostic.with_label(Label::new(covered_by, "this arm covers it")),
            None => {
                diagnostic.with_note("the earlier arms together cover every value this arm matches")
            }
        }
    }

    pub fn pattern_count(name: &str, expected: usize, found: usize, span: Span) -> Diagnostic {
        let expected = Counted::noun(expected, "part");
        Diagnostic::error(
            DiagnosticCode::WRONG_PATTERN_COUNT,
            format!(
                "`{name}` takes {expected} in a pattern but {found} {} given",
                Counted::was_or_were(found)
            ),
            Label::new(span, "this pattern has the wrong number of parts"),
        )
    }

    pub fn unknown_field(
        struct_name: &str,
        label: &FieldLabel,
        similar: Option<Rc<str>>,
    ) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::UNKNOWN_FIELD,
            format!(
                "`{struct_name}` has no field `{}`",
                label.spelled(&label.name)
            ),
            Label::new(label.span, "unknown field"),
        );
        match similar {
            Some(similar) => diagnostic.with_suggestion(
                "a field with a similar name exists",
                Suggestion::new(label.span, label.spelled(&similar)),
            ),
            None => diagnostic,
        }
    }

    pub fn main_returns_value(shown: &str, returned: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MAIN_RETURNS_VALUE,
            format!("`main` returns a value of type `{shown}`"),
            Label::new(returned, format!("this `{shown}` is returned from `main`")),
        )
        .with_note("`main` returns `unit` or a `result`")
        .with_help(
            "end the body of `main` with a print, such as `(println value)`, or return a `result`",
        )
    }

    pub fn selfless_method_call(
        trait_name: &str,
        method: &str,
        replacement: &str,
        call: Span,
        signature: Span,
    ) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::SELFLESS_METHOD_CALL,
            format!("cannot choose which implementor of `{trait_name}` runs `{method}`"),
            Label::new(
                call,
                format!("no type here picks an implementor of `{trait_name}`"),
            ),
        )
        .with_label(Label::new(
            signature,
            "this signature never mentions `self`",
        ))
        .with_note(
            "a call picks an implementor from the type of a parameter or result that is `self`",
        )
        .with_help(format!(
            "add `(ref self)` as the first parameter of `{method}`, as in `{replacement}`"
        ))
    }

    pub fn repeated_field(struct_name: &str, label: &FieldLabel, first: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::REPEATED_FIELD,
            format!(
                "`{}` is given twice in `{struct_name}`",
                label.spelled(&label.name)
            ),
            Label::new(label.span, "given again here"),
        )
        .with_label(Label::new(first, "first given here"))
        .with_help(format!(
            "remove the second `{}` and its value",
            label.spelled(&label.name)
        ))
    }

    pub fn missing_fields(struct_name: &str, missing: &[Rc<str>], span: Span) -> Diagnostic {
        let noun = if missing.len() == 1 {
            "field"
        } else {
            "fields"
        };
        let names: Vec<String> = missing.iter().map(|field| format!("`{field}`")).collect();
        Diagnostic::error(
            DiagnosticCode::MISSING_FIELDS,
            format!("missing {noun} {} in `{struct_name}`", names.join(", ")),
            Label::new(span, "each missing field needs a value"),
        )
    }

    pub fn no_fields(found: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::NO_FIELDS,
            format!("`{found}` has no fields"),
            Label::new(span, "only structs have fields"),
        )
    }

    pub fn borrowed_update_base(struct_name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::NO_FIELDS,
            format!("cannot update a borrowed `{struct_name}`"),
            Label::new(span, format!("this `{struct_name}` is borrowed")),
        )
        .with_help(format!(
            "bind an owned `{struct_name}` first, as in `(let owned (clone base))`, and update `owned`"
        ))
    }

    pub fn struct_base_help() -> String {
        format!(
            "a struct is not a collection, so update a field with a keyword key, such as `{}field`",
            Keyword::MARKER
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
        .with_help("name the struct in a signature, as in `(fn ((ref point)) _)`")
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
            DiagnosticCode::QUOTE_AT_RUNTIME,
            "quoted code only exists at compile time",
            Label::new(span, "used in code that runs in the program"),
        )
        .with_help("use `quote` inside macros, or write a string if you want text")
    }

    pub fn wrong_type_argument_count(
        name: &str,
        expected: usize,
        found: usize,
        span: Span,
    ) -> Diagnostic {
        let expected = Counted::noun(expected, "type argument");
        Diagnostic::error(
            DiagnosticCode::WRONG_TYPE_ARGUMENT_COUNT,
            format!(
                "`{name}` takes {expected} but {found} {} given",
                Counted::was_or_were(found)
            ),
            Label::new(span, format!("expected {expected}")),
        )
    }

    pub fn bare_implementor(trait_name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::BARE_IMPLEMENTOR,
            "a type parameter alone cannot implement a trait",
            Label::new(span, "this would match every type"),
        )
        .with_help(format!(
            "wrap the parameter in a type, as in `(where ((a {trait_name})) (ref a))`"
        ))
    }

    pub fn unsized_implementor(implementor: &str, method: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNSIZABLE_IMPLEMENTOR,
            format!("`{implementor}` has no size, so `{method}` cannot take `self` by value"),
            Label::new(span, format!("`{implementor}` has no size")),
        )
        .with_help("take the receiver by reference, as in `(fn ((ref self)) ...)`")
    }

    pub fn unused_type_parameter(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNUSED_TYPE_PARAMETER,
            format!("no field uses the type parameter `{name}`"),
            Label::new(span, "declared here"),
        )
        .with_help(format!("use `{name}` in a field's type, or remove it"))
    }

    pub fn unused_enum_parameter(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNUSED_ENUM_PARAMETER,
            format!("no payload uses the type parameter `{name}`"),
            Label::new(span, "declared here"),
        )
        .with_help(format!("use `{name}` in a variant payload, or remove it"))
    }

    pub fn recursively_used_enum_parameter(
        name: &str,
        enum_name: &str,
        binder: Span,
        recursive_use: Span,
    ) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::RECURSIVELY_USED_TYPE_PARAMETER,
            format!("type parameter `{name}` is only used recursively"),
            Label::new(binder, "declared here"),
        )
        .with_label(Label::new(recursive_use, "first recursive use"))
        .with_help(format!(
            "use `{name}` in a payload outside `{enum_name}`, or remove it"
        ))
    }

    pub fn bounded_type_parameter(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::BOUNDED_TYPE_PARAMETER,
            format!("the type parameter `{name}` cannot require traits here"),
            Label::new(span, "traits given here"),
        )
        .with_help(format!(
            "write `{name}` alone, and require the traits in the `where` of each function that needs them"
        ))
    }

    pub fn mut_borrow_of_copy(span: Span, place: Option<String>) -> Diagnostic {
        let help = match place {
            Some(place) => format!("pass the field as a place: `{place}`"),
            None => "pass a local or a field, because a call returns a copy".to_string(),
        };
        Diagnostic::error(
            DiagnosticCode::MUT_BORROW_OF_COPY,
            "this argument is a copy, so changing it changes nothing",
            Label::new(span, "this is a copy, not a place"),
        )
        .with_help(help)
    }

    pub fn trait_claim_on_struct(struct_name: &str, trait_name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::TRAIT_CLAIM_ON_STRUCT,
            format!("`{struct_name}` does not derive `{trait_name}`"),
            Label::new(span, format!("this struct does not derive `{trait_name}`")),
        )
        .with_help(format!(
            "remove `{struct_name}` from this `rust-trait`, or list a type that Rust implements `{trait_name}` for"
        ))
    }

    pub fn not_derivable(trait_name: &str, derivable: &[String], span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::NOT_DERIVABLE,
            format!("`{trait_name}` cannot be derived"),
            Label::new(span, "not a derivable trait"),
        )
        .with_help(format!("a `derive` lists any of: {}", derivable.join(", ")))
    }

    pub fn blocked_enum_derive(
        enum_name: &str,
        trait_name: &str,
        variant: &str,
        payload_type: &str,
        span: Span,
    ) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::BLOCKED_DERIVE,
            format!("`{enum_name}` cannot derive `{trait_name}`"),
            Label::new(
                span,
                format!("variant `{variant}` holds `{payload_type}`, which is not `{trait_name}`"),
            ),
        )
        .with_help(format!(
            "change the payload of variant `{variant}`, or leave this trait out of the `derive`"
        ))
    }

    pub fn blocked_enum_derive_note(
        enum_name: &str,
        trait_name: &str,
        variant: &str,
        payload_type: &str,
    ) -> String {
        format!(
            "`{enum_name}` cannot derive `{trait_name}`: variant `{variant}` holds `{payload_type}`"
        )
    }

    pub fn order_not_inferred_enum_note(trait_name: &str) -> String {
        format!(
            "`{trait_name}` is never inferred for an enum, so the enum needs a written `derive`"
        )
    }

    pub fn enum_order_help(enum_name: &str, derives: &[String], variants: &[Rc<str>]) -> String {
        format!(
            "add `(derive {enum_name} {})` to compare variants in declared order: {}",
            derives.join(" "),
            Self::field_order(variants)
        )
    }

    pub fn add_to_enum_derive_help(trait_name: &str) -> String {
        format!("add `{trait_name}` to the enum's `derive`")
    }

    pub fn order_blocked_enum_note(
        enum_name: &str,
        variant: &str,
        payload_type: &str,
        trait_name: &str,
    ) -> String {
        format!(
            "`{enum_name}` cannot derive `{trait_name}`: variant `{variant}` holds `{payload_type}`"
        )
    }

    pub fn blocked_derive(
        struct_name: &str,
        trait_name: &str,
        field: &str,
        field_type: &str,
        span: Span,
    ) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::BLOCKED_DERIVE,
            format!("`{struct_name}` cannot derive `{trait_name}`"),
            Label::new(
                span,
                format!("field `{field}` is `{field_type}`, which is not `{trait_name}`"),
            ),
        )
        .with_help(format!(
            "change the type of field `{field}`, or leave this trait out of the `derive`"
        ))
    }

    pub fn derive_on_rust_type(enum_name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::DERIVE_ON_RUST_TYPE,
            format!("`{enum_name}` is a Rust type, so it has no `derive`"),
            Label::new(span, format!("`{enum_name}` names a Rust type")),
        )
        .with_help("the traits of a Rust type come from its `rust-trait` entries")
    }

    pub fn duplicate_derive(struct_name: &str, span: Span, first: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::DUPLICATE_DERIVE,
            format!("`{struct_name}` has a second `derive`"),
            Label::new(span, "second `derive`"),
        )
        .with_label(Label::new(first, "first `derive`"))
        .with_help("merge the two trait lists into one `derive`")
    }

    pub fn derive_leaves_out(span: Span, trait_name: &str) -> Label {
        Label::new(span, format!("this `derive` leaves out `{trait_name}`"))
    }

    pub fn add_to_derive_help(trait_name: &str) -> String {
        format!("add `{trait_name}` to the struct's `derive`")
    }

    pub fn trait_takes_no_argument(trait_name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::TRAIT_NO_SECOND_TYPE,
            format!("`{trait_name}` takes no second type"),
            Label::new(span, "this gives it a second type"),
        )
        .with_help(format!("write `{trait_name}` alone, without a second type"))
    }

    pub fn misplaced_with(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISPLACED_WITH,
            "`with` is only allowed in a `rust-trait`",
            Label::new(span, "not allowed here"),
        )
        .with_help("write `(with type type)` as an entry of `rust-trait`")
    }

    pub fn not_boxable(trait_name: &str, blocker: String, help: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::OBJECT_SAFETY,
            format!("`{trait_name}` cannot be a trait object"),
            Label::new(span, blocker),
        )
        .with_help(help)
    }

    pub fn trait_outside_box(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::TRAIT_OUTSIDE_BOX,
            "a trait is a type only inside `(box ...)`",
            Label::new(span, format!("`{name}` is a trait")),
        )
        .with_help(format!("write `(box {name})` to box it as a trait object"))
    }

    pub fn misplaced_where(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISPLACED_WHERE,
            "`where` must wrap a whole signature",
            Label::new(span, "not allowed inside another type"),
        )
    }
}
