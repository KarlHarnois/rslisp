use std::rc::Rc;

use super::functions::ParamMode;
use crate::core::ReferenceKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, Label, Suggestion};
use crate::source::Span;
use crate::types::ClosureKind;

#[derive(Clone)]
pub enum FunctionStorage {
    Field(Rc<str>),
    Payload(Rc<str>),
}

pub enum DroppedValue {
    Local(Span),
    Temporary(Span),
    FunctionParameter(Span),
    ClosureParameter(Span),
}

pub enum BorrowedStorage {
    Field {
        field: String,
        owner: String,
        owned: String,
        declared: Span,
    },
    Payload {
        variant: String,
        owner: String,
        owned: String,
        declared: Span,
    },
    NamedPayload {
        field: String,
        variant: String,
        owner: String,
        owned: String,
        declared: Span,
    },
}

impl BorrowedStorage {
    fn declared(&self) -> Span {
        match self {
            Self::Field { declared, .. }
            | Self::Payload { declared, .. }
            | Self::NamedPayload { declared, .. } => *declared,
        }
    }
}

struct MoveLabels {
    used: &'static str,
    moved: &'static str,
}

impl MoveLabels {
    const SAME_PASS: Self = Self {
        used: "value used here after it moved",
        moved: "value moved here",
    };
    const EARLIER_PASS: Self = Self {
        used: "value used here after an earlier pass of the loop moved it",
        moved: "value moved here, in an earlier pass of the loop",
    };
}

pub struct OwnershipProblems;

impl OwnershipProblems {
    pub fn use_after_move(name: &str, used: Span, moved: Span) -> Diagnostic {
        Self::moved_value(
            DiagnosticCode::USE_AFTER_MOVE,
            name,
            used,
            moved,
            &MoveLabels::SAME_PASS,
        )
    }

    pub fn use_after_earlier_pass_move(name: &str, used: Span, moved: Span) -> Diagnostic {
        Self::moved_value(
            DiagnosticCode::USE_AFTER_MOVE,
            name,
            used,
            moved,
            &MoveLabels::EARLIER_PASS,
        )
    }

    fn moved_value(
        code: DiagnosticCode,
        name: &str,
        used: Span,
        moved: Span,
        labels: &MoveLabels,
    ) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            code,
            format!("use of moved value `{name}`"),
            Label::new(used, labels.used),
        );
        if used == moved {
            return diagnostic;
        }
        diagnostic.with_label(Label::new(moved, labels.moved))
    }

    pub fn clone_suggestion(diagnostic: Diagnostic, name: &str, moved: Span) -> Diagnostic {
        diagnostic.with_suggestion(
            "clone the value if both places need it",
            Suggestion::new(moved, format!("(clone {name})")),
        )
    }

    pub fn pipeline_help(diagnostic: Diagnostic, name: &str) -> Diagnostic {
        Self::pipeline_wording(diagnostic, &format!("`{name}`"), name)
    }

    fn pipeline_wording(diagnostic: Diagnostic, subject: &str, collected: &str) -> Diagnostic {
        diagnostic
            .with_note(format!("{subject} is a lazy pipeline, so it runs once"))
            .with_help(format!(
                "collect it if both places need its items, as in `(collect {collected})`, or write a function that rebuilds it"
            ))
    }

    pub fn mut_parameter_move_help(diagnostic: Diagnostic) -> Diagnostic {
        diagnostic
            .with_help("use the value before it moves, because a `mut` borrow cannot be cloned")
    }

    pub fn changed_value_help(diagnostic: Diagnostic) -> Diagnostic {
        diagnostic.with_help(
            "change the value in place instead of moving it, so both uses see the change",
        )
    }

    pub fn recursive_parameter_help(diagnostic: Diagnostic) -> Diagnostic {
        diagnostic.with_help("write the parameter as a borrow in a `type` signature: `(ref ...)`")
    }

    pub fn missing_copy_bound(
        name: &str,
        parameter: &str,
        copy_trait: &str,
        used: Span,
        moved: Span,
    ) -> Diagnostic {
        Self::moved_value(
            DiagnosticCode::MISSING_COPY_BOUND,
            name,
            used,
            moved,
            &MoveLabels::SAME_PASS,
        )
            .with_note(format!(
                "the signature writes `{parameter}` without `{copy_trait}`, and a written signature is exact"
            ))
            .with_help(format!(
                "add a `{copy_trait}` bound to the signature: `({parameter} {copy_trait})`"
            ))
    }

    pub fn written_mode_mismatch(
        parameter: &str,
        kind: ReferenceKind,
        mode: ParamMode,
        span: Span,
    ) -> Diagnostic {
        let taken = match mode {
            ParamMode::Copy | ParamMode::Own => "a copy",
            ParamMode::Ref => "a reference",
            ParamMode::Mut => "a mutable reference",
        };
        let (title, passed, help) = match kind {
            ReferenceKind::Shared => (
                format!("`{parameter}` is taken as {taken}, so this borrow cannot pass"),
                "this passes a borrow",
                "pass the value without `ref`, or write the parameter's type",
            ),
            ReferenceKind::Mutable => (
                format!("`{parameter}` is taken as {taken}, so this mutable borrow cannot pass"),
                "this passes a mutable borrow",
                "pass the value without `mut`, or write the parameter's type",
            ),
        };
        Diagnostic::error(
            DiagnosticCode::WRITTEN_MODE_MISMATCH,
            title,
            Label::new(span, passed),
        )
        .with_note("the callee's body decides whether the parameter takes a borrow")
        .with_help(help)
    }

    pub fn closure_move_help(diagnostic: Diagnostic) -> Diagnostic {
        diagnostic.with_help("this closure moves a captured value, so it cannot be copied")
    }

    pub fn capturing_function_field(
        closure: Span,
        capture: &str,
        storage: &FunctionStorage,
        declared: Span,
    ) -> Diagnostic {
        let (place, holder) = match storage {
            FunctionStorage::Field(field) => (format!("the field `{field}`"), "field"),
            FunctionStorage::Payload(variant) => (format!("the `{variant}` payload"), "payload"),
        };
        Diagnostic::error(
            DiagnosticCode::CAPTURING_FUNCTION_FIELD,
            format!("cannot store a capturing closure in {place}"),
            Label::new(closure, format!("this closure captures `{capture}`")),
        )
        .with_label(Label::new(declared, "declared with a function type here"))
        .with_help(format!(
            "a {holder} declared with a function type holds only a named function or a closure that captures nothing"
        ))
    }

    pub fn closure_kind_mismatch(
        actual: ClosureKind,
        required: ClosureKind,
        capture: Option<&str>,
        callee: Option<&str>,
        passed: Span,
        capture_span: Option<Span>,
    ) -> Diagnostic {
        let (primary, secondary) = Self::closure_kind_labels(actual, capture, passed, capture_span);
        let diagnostic = Diagnostic::error(
            DiagnosticCode::CLOSURE_KIND_MISMATCH,
            format!(
                "this closure is `{}`, but the parameter accepts `{}`",
                actual.keyword(),
                required.keyword()
            ),
            primary,
        )
        .with_help(Self::closure_kind_help(actual, required, capture, callee));
        match secondary {
            Some(label) => diagnostic.with_label(label),
            None => diagnostic,
        }
    }

    pub fn closure_kind_mismatch_call(
        actual: ClosureKind,
        required: ClosureKind,
        name: &str,
        kind: ClosureKind,
        callee: Option<&str>,
        passed: Span,
        capture_span: Span,
    ) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::CLOSURE_KIND_MISMATCH,
            format!(
                "this closure is `{}`, but the parameter accepts `{}`",
                actual.keyword(),
                required.keyword()
            ),
            Label::new(capture_span, format!("calls `{name}` here")),
        )
        .with_label(Label::new(passed, "this closure is passed here"))
        .with_help(Self::called_capture_help(required, name, kind, callee))
    }

    fn called_capture_help(
        required: ClosureKind,
        name: &str,
        kind: ClosureKind,
        callee: Option<&str>,
    ) -> String {
        let callee = callee.unwrap_or("this function");
        format!(
            "`{callee}` calls this closure as `{}`, but it calls `{name}`, which is `{}`",
            required.keyword(),
            kind.keyword()
        )
    }

    fn closure_kind_labels(
        actual: ClosureKind,
        capture: Option<&str>,
        passed: Span,
        capture_span: Option<Span>,
    ) -> (Label, Option<Label>) {
        let action = if actual == ClosureKind::FnOnce {
            "moves"
        } else {
            "changes"
        };
        match (capture, capture_span) {
            (Some(name), Some(span)) => (
                Label::new(span, format!("`{name}` {action} here")),
                Some(Label::new(passed, "this closure is passed here")),
            ),
            (Some(name), None) => (Label::new(passed, format!("`{name}` {action} here")), None),
            (None, _) => (
                Label::new(
                    passed,
                    "this closure is stronger than the parameter accepts",
                ),
                None,
            ),
        }
    }

    fn closure_kind_help(
        actual: ClosureKind,
        required: ClosureKind,
        capture: Option<&str>,
        callee: Option<&str>,
    ) -> String {
        if actual == ClosureKind::FnMut {
            let callee = callee.unwrap_or("this function");
            return match capture {
                Some(name) => format!(
                    "`{callee}` calls this closure as `{}`, but it changes `{name}`",
                    required.keyword()
                ),
                None => format!(
                    "`{callee}` calls this closure as `{}`, but it changes a captured value",
                    required.keyword()
                ),
            };
        }
        match capture {
            Some(name) => format!("clone `{name}`, or borrow it with `(ref {name})`"),
            None => "clone the closure's captures, or borrow them instead".to_string(),
        }
    }

    pub fn local_borrow(borrowed: Span, dropped: &DroppedValue) -> Diagnostic {
        Self::returned_local_borrow(borrowed, dropped).with_help(match dropped {
            DroppedValue::Local(_) => {
                "return an owned value with `(clone ...)`, or borrow from a parameter"
            }
            DroppedValue::Temporary(_) => {
                "return an owned value, or borrow from a parameter or a top-level `def`"
            }
            DroppedValue::FunctionParameter(_) => {
                "return an owned value with `(clone ...)`, or give this parameter a borrowed type"
            }
            DroppedValue::ClosureParameter(_) => {
                "return an owned value with `(clone ...)`, or capture the value instead of passing it"
            }
        })
    }

    pub fn stored_local_borrow(
        borrowed: Span,
        dropped: &DroppedValue,
        storage: &BorrowedStorage,
    ) -> Diagnostic {
        Self::returned_local_borrow(borrowed, dropped)
            .with_help(Self::owned_storage(storage))
            .with_label(Label::new(storage.declared(), "declared as a borrow here"))
    }

    fn owned_storage(storage: &BorrowedStorage) -> String {
        match storage {
            BorrowedStorage::Field {
                field,
                owner,
                owned,
                ..
            } => format!(
                "the field `{field}` of `{owner}` borrows its data, so give it the owned type `{owned}`"
            ),
            BorrowedStorage::Payload {
                variant,
                owner,
                owned,
                ..
            } => format!(
                "the `{variant}` payload of `{owner}` borrows its data, so give it the owned type `{owned}`"
            ),
            BorrowedStorage::NamedPayload {
                field,
                variant,
                owner,
                owned,
                ..
            } => format!(
                "the field `{field}` of `{owner}/{variant}` borrows its data, so give it the owned type `{owned}`"
            ),
        }
    }

    pub fn lent_local_borrow(
        borrowed: Span,
        dropped: &DroppedValue,
        lent: Span,
        closure: &str,
    ) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::LENT_LOCAL_BORROW,
            format!("cannot pass a borrow of a local value to `{closure}`"),
            Label::new(borrowed, "this borrows a local value"),
        )
        .with_label(Label::new(
            lent,
            format!("the result of `{closure}` borrows this parameter, so it needs a borrow that outlives the function"),
        ))
        .with_label(Self::dropped_label(dropped))
        .with_help(format!(
            "define `{closure}` with `defn`, where its result can borrow from its argument"
        ))
    }

    fn returned_local_borrow(borrowed: Span, dropped: &DroppedValue) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::LOCAL_BORROW,
            "cannot return a borrow of a local value",
            Label::new(borrowed, "this borrows a local value"),
        )
        .with_label(Self::dropped_label(dropped))
    }

    fn dropped_label(dropped: &DroppedValue) -> Label {
        match dropped {
            DroppedValue::Local(defined) => {
                Label::new(*defined, "this local is dropped when the function returns")
            }
            DroppedValue::Temporary(built) => Label::new(
                *built,
                "this temporary value is dropped when the function returns",
            ),
            DroppedValue::FunctionParameter(defined) | DroppedValue::ClosureParameter(defined) => {
                Label::new(
                    *defined,
                    "this parameter owns its value, which is dropped when the function returns",
                )
            }
        }
    }

    pub fn moved_definition(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::NEEDS_OWNERSHIP,
            format!("cannot move `{name}` out of a top-level definition"),
            Label::new(span, "a top-level definition can only be borrowed"),
        )
        .with_suggestion(
            "clone the value to own a copy",
            Suggestion::new(span, format!("(clone {name})")),
        )
    }

    pub fn needs_ownership(span: Span) -> Diagnostic {
        Self::borrowed_argument(span).with_help("pass a copy with `(clone ...)`")
    }

    pub fn needs_pipeline_ownership(span: Span) -> Diagnostic {
        Self::pipeline_wording(Self::borrowed_argument(span), "this value", "...")
    }

    fn borrowed_argument(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::NEEDS_OWNERSHIP,
            "this value is borrowed, but the call needs to own it",
            Label::new(span, "only a borrow is available here"),
        )
    }

    pub fn function_value_mismatch(
        expected: &str,
        found: &str,
        span: Span,
        parameter: usize,
    ) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::FUNCTION_VALUE_MISMATCH,
            "mismatched function type",
            Label::new(span, format!("expected `{expected}`, found `{found}`")),
        )
        .with_note(format!("no adjustment makes parameter {parameter} match"))
        .with_help(
            "change the parameter's ownership in a `type` signature, or pass a function that matches",
        )
    }

    pub fn missing_bang(name: &str, span: Span) -> Diagnostic {
        Diagnostic::warning(
            DiagnosticCode::MISSING_BANG,
            format!("`{name}` changes an argument, so its name should end with `!`"),
            Label::new(span, "this function changes an argument"),
        )
        .with_help(format!("rename it `{name}!`"))
    }
}
