use super::LibrarySource;
use crate::diagnostic::{Diagnostic, DiagnosticCode, Label, Suggestion};
use crate::language::Language;
use crate::source::Span;

pub struct ProjectProblems;

impl ProjectProblems {
    const PROJECT_EXAMPLE: &str = "(project my-program)";

    pub fn missing_project_form(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISSING_PROJECT_FORM,
            format!(
                "`{}` must hold a `project` form",
                Language::project_file_name()
            ),
            Label::new(span, "expected `(project <name> ...)` here"),
        )
        .with_help(format!("start the file with `{}`", Self::PROJECT_EXAMPLE))
    }

    pub fn extra_project_form(extra: Span, project: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::EXTRA_PROJECT_FORM,
            format!(
                "`{}` holds a form after its `project` form",
                Language::project_file_name()
            ),
            Label::new(extra, "this form is outside the `project` form"),
        )
        .with_label(Label::new(project, "the `project` form ends here"))
        .with_help("move this form inside the `project` form, or remove it")
    }

    pub fn missing_project_name(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISSING_PROJECT_NAME,
            "the project has no name",
            Label::new(span, "expected a name after `project`"),
        )
        .with_help(format!("name the project, as `{}`", Self::PROJECT_EXAMPLE))
    }

    pub fn unknown_key(key: &str, span: Span, closest: Option<&str>) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::UNKNOWN_PROJECT_KEY,
            format!("unknown key `:{key}`"),
            Label::new(span, "not a key of this form"),
        );
        match closest {
            Some(closest) => diagnostic.with_suggestion(
                "a key with a similar name exists",
                Suggestion::new(span, format!(":{closest}")),
            ),
            None => diagnostic,
        }
    }

    pub fn missing_value(key: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISSING_PROJECT_VALUE,
            format!("`:{key}` has no value"),
            Label::new(span, "expected a value after this key"),
        )
        .with_help(format!("write the value right after `:{key}`"))
    }

    pub fn mismatched_value(key: &str, expected: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISMATCHED_PROJECT_VALUE,
            format!("`:{key}` takes {expected}"),
            Label::new(span, format!("expected {expected}")),
        )
    }

    pub fn unquoted_feature(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MISMATCHED_PROJECT_VALUE,
            "a feature is a string",
            Label::new(span, "expected a string"),
        )
        .with_suggestion(
            "quote the feature's name",
            Suggestion::new(span, format!("\"{name}\"")),
        )
    }

    pub fn unknown_section(section: &str, span: Span, closest: Option<&str>) -> Diagnostic {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::UNKNOWN_PROJECT_SECTION,
            format!("unknown section `{section}`"),
            Label::new(span, "not a section of a project"),
        );
        match closest {
            Some(closest) => diagnostic.with_suggestion(
                "a section with a similar name exists",
                Suggestion::new(span, closest),
            ),
            None => diagnostic
                .with_help("the sections are `libraries`, `dependencies` and `dev-dependencies`"),
        }
    }

    pub fn unexpected_form(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNEXPECTED_PROJECT_FORM,
            "expected a key or a section",
            Label::new(span, "this is neither a `:key value` pair nor a section"),
        )
        .with_help("give a value after a key, as `:entry \"src/main.rslisp\"`")
    }

    pub fn unexpected_entry_form(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNEXPECTED_PROJECT_FORM,
            "expected a key",
            Label::new(span, "this is not a `:key value` pair"),
        )
        .with_help("an entry is a name, then an optional version, then `:key value` pairs")
    }

    pub fn duplicate_entry(name: &str, span: Span, first: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::DUPLICATE_PROJECT_ENTRY,
            format!("`{name}` is given more than once"),
            Label::new(span, "given again here"),
        )
        .with_label(Label::new(first, "first given here"))
        .with_help("remove one of them")
    }

    pub fn malformed_dependency(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::MALFORMED_DEPENDENCY,
            "expected a dependency",
            Label::new(span, "a dependency is a list that starts with a name"),
        )
        .with_help("write a dependency as `(serde \"1.0\")` or `(hints :path \"../hints\")`")
    }

    pub fn versioned_library(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::VERSIONED_LIBRARY,
            format!("library `{name}` cannot take a version"),
            Label::new(span, "a library is found by its source, not its version"),
        )
        .with_help("remove the version")
    }

    pub fn library_without_source(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::LIBRARY_WITHOUT_SOURCE,
            format!("library `{name}` has no source"),
            Label::new(span, "this library needs `:path` or `:git`"),
        )
        .with_help(format!(
            "write `({name} :path \"../{name}\")`, or `:git` with a `:rev`"
        ))
    }

    pub fn conflicting_sources(name: &str, span: Span, other: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::CONFLICTING_LIBRARY_SOURCES,
            format!("library `{name}` has two sources"),
            Label::new(span, "this source"),
        )
        .with_label(Label::new(other, "conflicts with this one"))
        .with_help("keep `:path` alone, or `:git` with `:rev`")
    }

    pub fn git_without_revision(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::GIT_LIBRARY_WITHOUT_REVISION,
            format!("git library `{name}` has no `:rev`"),
            Label::new(span, "this repository needs a commit"),
        )
        .with_help(format!(
            "add `:rev` with the commit's full {}-character hash",
            LibrarySource::FULL_COMMIT_LENGTH
        ))
    }

    pub fn short_revision(span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::SHORT_REVISION,
            "`:rev` must name a full commit",
            Label::new(
                span,
                format!(
                    "expected {} hexadecimal digits",
                    LibrarySource::FULL_COMMIT_LENGTH
                ),
            ),
        )
        .with_note("git fetches a commit only by its full hash")
        .with_help("write the commit's full hash, as `git rev-parse <commit>` prints it")
    }

    pub fn unversioned_crate(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNVERSIONED_CRATE,
            format!("crate `{name}` has no version"),
            Label::new(span, "this crate needs a version or a `:path`"),
        )
        .with_help(format!(
            "write the version after the name, as `({name} \"1.0\")`"
        ))
    }

    pub fn unknown_edition(written: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            DiagnosticCode::UNKNOWN_EDITION,
            format!("edition {written} does not exist"),
            Label::new(span, "not an edition of this compiler"),
        )
        .with_suggestion(
            format!("the only edition is {}", Language::EDITION),
            Suggestion::new(span, Language::EDITION.to_string()),
        )
    }
}
