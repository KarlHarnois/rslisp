use std::path::PathBuf;
use std::rc::Rc;

use super::Project;
use super::dependency_reader::{DependencyReader, Section};
use super::fields::{FieldScanner, Fields};
use super::problems::ProjectProblems;
use crate::diagnostic::{Diagnostics, SimilarName};
use crate::language::Language;
use crate::reader::Form;
use crate::source::Span;

pub struct ProjectReader {
    problems: Diagnostics,
}

struct ProjectForm<'f> {
    head: &'f Form,
    name: Option<&'f Form>,
    body: &'f [Form],
}

impl ProjectReader {
    const HEAD: &str = "project";
    const KEYS: &[&str] = &["version", "entry", "lib", "edition"];
    const DEFAULT_VERSION: &str = "0.1.0";
    const DEFAULT_ENTRY: &str = "src/main";
    const DEFAULT_LIBRARY_ROOT: &str = "src/lib";

    pub fn read(forms: &[Form], file: Span) -> Result<Project, Diagnostics> {
        let mut reader = Self {
            problems: Diagnostics::default(),
        };
        let project = reader.project(forms, file);
        match project {
            Some(project) => reader.problems.into_result(project),
            None => Err(reader.problems),
        }
    }

    fn project(&mut self, forms: &[Form], file: Span) -> Option<Project> {
        let form = self.project_form(forms, file)?;
        let name = self.name(&form);
        let scanned = FieldScanner::new(Self::KEYS, &mut self.problems).scan(form.body);
        let mut sections = SectionTable::default();
        scanned
            .others
            .iter()
            .for_each(|other| self.read_section(other, &mut sections));
        let project = self.with_fields(name?, &scanned.fields, sections);
        Some(project)
    }

    fn project_form<'f>(&mut self, forms: &'f [Form], file: Span) -> Option<ProjectForm<'f>> {
        let Some(first) = forms.first() else {
            self.problems
                .push(ProjectProblems::missing_project_form(file.start()));
            return None;
        };
        if let Some(extra) = forms.get(1) {
            self.problems.push(ProjectProblems::extra_project_form(
                extra.span(),
                first.span(),
            ));
        }
        let form = ProjectForm::of(first);
        if form.is_none() {
            self.problems
                .push(ProjectProblems::missing_project_form(first.span()));
        }
        form
    }

    fn name(&mut self, form: &ProjectForm<'_>) -> Option<String> {
        let name = form.name.and_then(DependencyReader::bare_name);
        if name.is_none() {
            let span = form.name.unwrap_or(form.head).span();
            self.problems
                .push(ProjectProblems::missing_project_name(span));
        }
        name.map(str::to_string)
    }

    fn read_section(&mut self, form: &Form, sections: &mut SectionTable) {
        let Some((head, entries)) = Self::section_parts(form) else {
            return self
                .problems
                .push(ProjectProblems::unexpected_form(form.span()));
        };
        let Some(section) = self.section_named(head) else {
            return;
        };
        if let Some(first) = sections.heads.iter().find(|(seen, _)| *seen == section) {
            return self.problems.push(ProjectProblems::duplicate_entry(
                section.name(),
                head.span(),
                first.1,
            ));
        }
        sections.heads.push((section, head.span()));
        DependencyReader::new(section, &mut self.problems).read(entries, sections);
    }

    fn section_parts(form: &Form) -> Option<(&Form, &[Form])> {
        let (head, entries) = form.as_list()?.split_first()?;
        head.as_symbol()?;
        Some((head, entries))
    }

    fn section_named(&mut self, head: &Form) -> Option<Section> {
        let name = head
            .as_symbol()
            .map(|symbol| symbol.name())
            .unwrap_or_default();
        let section = Section::named(name);
        if section.is_none() {
            let candidates: Vec<Rc<str>> = Section::ALL
                .iter()
                .map(|section| Rc::from(section.name()))
                .collect();
            let closest = SimilarName::closest(name, &candidates);
            self.problems.push(ProjectProblems::unknown_section(
                name,
                head.span(),
                closest.as_deref(),
            ));
        }
        section
    }

    fn with_fields(
        &mut self,
        name: String,
        fields: &Fields<'_>,
        sections: SectionTable,
    ) -> Project {
        Project {
            name,
            version: fields
                .string("version", &mut self.problems)
                .unwrap_or_else(|| Self::DEFAULT_VERSION.to_string()),
            entry: self.module_file(fields, "entry", Self::DEFAULT_ENTRY),
            library_root: self.module_file(fields, "lib", Self::DEFAULT_LIBRARY_ROOT),
            edition: self.edition(fields),
            libraries: sections.libraries,
            dependencies: sections.dependencies,
            dev_dependencies: sections.dev_dependencies,
        }
    }

    fn module_file(&mut self, fields: &Fields<'_>, key: &str, default_module: &str) -> PathBuf {
        fields
            .string(key, &mut self.problems)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("{default_module}.{}", Language::EXTENSION)))
    }

    fn edition(&mut self, fields: &Fields<'_>) -> i128 {
        let Some((edition, span)) = fields.whole_number("edition", &mut self.problems) else {
            return Language::EDITION;
        };
        if edition != Language::EDITION {
            self.problems
                .push(ProjectProblems::unknown_edition(&edition.to_string(), span));
        }
        edition
    }
}

impl<'f> ProjectForm<'f> {
    fn of(form: &'f Form) -> Option<Self> {
        let (head, parts) = form.as_list()?.split_first()?;
        if head.as_symbol()?.name() != ProjectReader::HEAD {
            return None;
        }
        let name = parts
            .first()
            .filter(|first| FieldScanner::key_of(first).is_none());
        let body = &parts[usize::from(name.is_some())..];
        Some(Self { head, name, body })
    }
}

#[derive(Default)]
pub struct SectionTable {
    heads: Vec<(Section, Span)>,
    pub libraries: Vec<super::LibraryDependency>,
    pub dependencies: Vec<super::CrateDependency>,
    pub dev_dependencies: Vec<super::CrateDependency>,
}
