use std::path::PathBuf;

use super::fields::{FieldScanner, Fields};
use super::problems::ProjectProblems;
use super::reader::SectionTable;
use super::{CrateDependency, LibraryDependency, LibrarySource};
use crate::diagnostic::Diagnostics;
use crate::reader::{Form, FormKind};
use crate::source::Span;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Libraries,
    Dependencies,
    DevDependencies,
}

pub struct DependencyReader<'p> {
    section: Section,
    problems: &'p mut Diagnostics,
    names: Vec<(String, Span)>,
}

struct EntryParts<'f> {
    name: &'f str,
    name_span: Span,
    version: Option<Version<'f>>,
    fields: Fields<'f>,
}

struct Version<'f> {
    text: &'f str,
    span: Span,
}

impl Section {
    pub const ALL: [Section; 3] = [
        Section::Libraries,
        Section::Dependencies,
        Section::DevDependencies,
    ];
    const LIBRARY_KEYS: &[&str] = &["path", "git", "rev"];
    const CRATE_KEYS: &[&str] = &["features", "path"];

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|section| section.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            Section::Libraries => "libraries",
            Section::Dependencies => "dependencies",
            Section::DevDependencies => "dev-dependencies",
        }
    }

    fn keys(self) -> &'static [&'static str] {
        match self {
            Section::Libraries => Self::LIBRARY_KEYS,
            Section::Dependencies | Section::DevDependencies => Self::CRATE_KEYS,
        }
    }
}

impl<'p> DependencyReader<'p> {
    pub fn new(section: Section, problems: &'p mut Diagnostics) -> Self {
        Self {
            section,
            problems,
            names: Vec::new(),
        }
    }

    pub fn bare_name(form: &Form) -> Option<&str> {
        let name = form.as_symbol()?.name();
        FieldScanner::key_of(form).is_none().then_some(name)
    }

    pub fn read(mut self, entries: &[Form], table: &mut SectionTable) {
        for entry in entries {
            if let Some(parts) = self.entry_parts(entry) {
                self.add_entry(&parts, table);
            }
        }
    }

    fn entry_parts<'f>(&mut self, entry: &'f Form) -> Option<EntryParts<'f>> {
        let Some((name, rest)) = Self::named_entry(entry) else {
            self.problems
                .push(ProjectProblems::malformed_dependency(entry.span()));
            return None;
        };
        let (version, keyed) = Self::split_version(rest);
        let scanned = FieldScanner::new(self.section.keys(), self.problems).scan(keyed);
        for other in scanned.others {
            self.problems
                .push(ProjectProblems::unexpected_entry_form(other.span()));
        }
        Some(EntryParts {
            name: Self::bare_name(name)?,
            name_span: name.span(),
            version,
            fields: scanned.fields,
        })
    }

    fn named_entry(entry: &Form) -> Option<(&Form, &[Form])> {
        let (name, rest) = entry.as_list()?.split_first()?;
        Self::bare_name(name)?;
        Some((name, rest))
    }

    fn split_version(forms: &[Form]) -> (Option<Version<'_>>, &[Form]) {
        if let Some((first, rest)) = forms.split_first()
            && let FormKind::String(text) = first.kind()
        {
            return (
                Some(Version {
                    text,
                    span: first.span(),
                }),
                rest,
            );
        }
        (None, forms)
    }

    fn add_entry(&mut self, parts: &EntryParts<'_>, table: &mut SectionTable) {
        if let Some((_, first)) = self.names.iter().find(|(name, _)| name == parts.name) {
            return self.problems.push(ProjectProblems::duplicate_entry(
                parts.name,
                parts.name_span,
                *first,
            ));
        }
        self.names.push((parts.name.to_string(), parts.name_span));
        match self.section {
            Section::Libraries => table.libraries.extend(self.library(parts)),
            Section::Dependencies => table.dependencies.push(self.crate_dependency(parts)),
            Section::DevDependencies => table.dev_dependencies.push(self.crate_dependency(parts)),
        }
    }

    fn library(&mut self, parts: &EntryParts<'_>) -> Option<LibraryDependency> {
        if let Some(version) = &parts.version {
            self.problems
                .push(ProjectProblems::versioned_library(parts.name, version.span));
        }
        Some(LibraryDependency {
            name: parts.name.to_string(),
            source: self.library_source(parts)?,
        })
    }

    fn library_source(&mut self, parts: &EntryParts<'_>) -> Option<LibrarySource> {
        let fields = &parts.fields;
        let path = fields.get("path");
        let repository = fields.get("git").or_else(|| fields.get("rev"));
        match (path, repository) {
            (Some(path), Some(repository)) => {
                self.problems.push(ProjectProblems::conflicting_sources(
                    parts.name,
                    repository.keyword.span(),
                    path.keyword.span(),
                ));
                None
            }
            (Some(_), None) => fields
                .string("path", self.problems)
                .map(|path| LibrarySource::Path(PathBuf::from(path))),
            (None, Some(_)) => self.git_source(parts),
            (None, None) => {
                self.problems.push(ProjectProblems::library_without_source(
                    parts.name,
                    parts.name_span,
                ));
                None
            }
        }
    }

    fn git_source(&mut self, parts: &EntryParts<'_>) -> Option<LibrarySource> {
        let git = parts.fields.get("git")?;
        if parts.fields.get("rev").is_none() {
            self.problems.push(ProjectProblems::git_without_revision(
                parts.name,
                git.keyword.span(),
            ));
            return None;
        }
        let url = parts.fields.string("git", self.problems);
        let commit = self.full_commit(&parts.fields);
        Some(LibrarySource::Git {
            url: url?,
            commit: commit?,
        })
    }

    fn full_commit(&mut self, fields: &Fields<'_>) -> Option<String> {
        let commit = fields.string("rev", self.problems)?;
        if Self::is_full_commit(&commit) {
            return Some(commit);
        }
        let span = fields.get("rev")?.value.span();
        self.problems.push(ProjectProblems::short_revision(span));
        None
    }

    fn is_full_commit(commit: &str) -> bool {
        commit.len() == LibrarySource::FULL_COMMIT_LENGTH
            && commit
                .chars()
                .all(|character| character.is_ascii_hexdigit())
    }

    fn crate_dependency(&mut self, parts: &EntryParts<'_>) -> CrateDependency {
        if parts.version.is_none() && parts.fields.get("path").is_none() {
            self.problems.push(ProjectProblems::unversioned_crate(
                parts.name,
                parts.name_span,
            ));
        }
        CrateDependency {
            name: parts.name.to_string(),
            version: parts
                .version
                .as_ref()
                .map(|version| version.text.to_string()),
            features: parts.fields.strings("features", self.problems),
            path: parts
                .fields
                .string("path", self.problems)
                .map(PathBuf::from),
        }
    }
}
