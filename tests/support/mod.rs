use std::fs;
use std::path::{Path, PathBuf};

use rslisp::core::{CorePrinter, ModulePath};
use rslisp::diagnostic::{DiagnosticRenderer, Diagnostics};
use rslisp::expand::Expander;
use rslisp::modules::ModuleSources;
use rslisp::reader::Reader;
use rslisp::source::{SourceId, SourceMap};
use rslisp::types::{TypeChecker, TypeDisplay, TypedProgram};

pub struct Fixture {
    sources: SourceMap,
    entry: SourceId,
    path: PathBuf,
}

impl Fixture {
    const FIXTURE_ROOT: &str = "tests/fixtures";

    pub fn load(path: &Path) -> Self {
        let text = fs::read_to_string(path).expect("fixture is readable");
        let mut sources = SourceMap::default();
        let entry = sources.add(Self::display_name(path), text);
        Self {
            sources,
            entry,
            path: path.to_path_buf(),
        }
    }

    pub fn read_report(&self) -> String {
        match Reader::new(self.sources.file(self.entry)).read() {
            Ok(forms) => forms.iter().map(|form| format!("{form}\n")).collect(),
            Err(diagnostics) => self.render(&diagnostics),
        }
    }

    pub fn expand_report(mut self) -> String {
        match self.expand() {
            Ok(program) => CorePrinter::new(&program.bindings).module(program.entry_module()),
            Err(diagnostics) => self.render(&diagnostics),
        }
    }

    pub fn check_report(mut self) -> String {
        match self.expand().and_then(TypeChecker::check) {
            Ok(program) => Self::signatures(&program),
            Err(diagnostics) => self.render(&diagnostics),
        }
    }

    fn expand(&mut self) -> Result<rslisp::expand::Program, Diagnostics> {
        let module_sources = ModuleSources::new(self.directory(), self.display_directory());
        let module_path = ModulePath::new(self.module_name());
        Expander::new(&mut self.sources, module_sources).expand(module_path, self.entry)
    }

    fn signatures(program: &TypedProgram) -> String {
        let display = TypeDisplay::new(&program.bindings, &program.generics);
        program
            .entry_module()
            .definitions
            .iter()
            .map(|definition| {
                let name = program.bindings.get(definition.binding).name();
                format!("(type {name} {})\n", display.scheme(&definition.scheme))
            })
            .collect()
    }

    fn render(&self, diagnostics: &Diagnostics) -> String {
        DiagnosticRenderer::plain(&self.sources).render_all(diagnostics)
    }

    fn directory(&self) -> PathBuf {
        self.path
            .parent()
            .expect("fixtures live in a directory")
            .to_path_buf()
    }

    fn display_directory(&self) -> PathBuf {
        PathBuf::from(Self::display_name(&self.directory()))
    }

    fn module_name(&self) -> String {
        let stem = self.path.file_stem().expect("fixtures have a file name");
        stem.to_string_lossy().into_owned()
    }

    fn display_name(path: &Path) -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(Self::FIXTURE_ROOT);
        path.strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string()
    }
}
