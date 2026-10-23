use std::rc::Rc;

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

    pub fn missing_trait_on_struct(trait_name: &str, found: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISSING_TRAIT,
            format!("`{found}` does not implement `{trait_name}`"),
            Label::new(span, format!("this needs `{trait_name}`")),
        )
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

    pub fn sort_by_key_help(struct_name: &str, field: Option<&str>) -> String {
        match field {
            Some(field) => format!(
                "to sort by one field instead, use `sort-by-key!` with the accessor `{struct_name}-{field}`"
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

    pub fn print_struct_help() -> &'static str {
        "print a struct's data form with `prn`"
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
            DiagnosticCode::LITERAL_TYPE_PARAMETER,
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
            DiagnosticCode::COMPILE_TIME_ONLY,
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
            DiagnosticCode::UNUSED_TYPE_PARAMETER,
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

    pub fn borrowed_struct_field(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::BORROWED_STRUCT_FIELD,
            "a struct field cannot hold a borrow",
            Label::new(span, "this field type borrows"),
        )
        .with_note("milestone 11 adds written lifetimes, so a struct field cannot borrow yet")
        .with_help("store an owned value, such as `string`, instead of a borrow")
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

    pub fn nested_boxed_pattern(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::NESTED_BOXED_PATTERN,
            "a pattern cannot look inside an inferred box yet",
            Label::new(span, "this pattern looks inside a box"),
        )
        .with_help("bind a name here and `match` on it in the arm")
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

    pub fn print_enum_help() -> &'static str {
        "print an enum's data form with `prn`"
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

    pub fn misplaced_where(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISPLACED_WHERE,
            "`where` must wrap a whole signature",
            Label::new(span, "not allowed inside another type"),
        )
    }
}
