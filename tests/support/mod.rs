use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rslisp::build::{BuildError, BuildProfile, RustCompiler};
use rslisp::core::{CorePrinter, ModulePath};
use rslisp::diagnostic::{DiagnosticRenderer, Diagnostics};
use rslisp::emit::{EmittedCrate, RustEmitter};
use rslisp::expand::Expander;
use rslisp::modules::ModuleSources;
use rslisp::ownership::OwnershipChecker;
use rslisp::reader::Reader;
use rslisp::source::{SourceId, SourceMap, Span};
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
        match self.checked() {
            Ok(program) if program.warnings.is_empty() => Self::signatures(&program),
            Ok(program) => format!(
                "{}\n\n{}",
                self.render(&program.warnings),
                Self::signatures(&program)
            ),
            Err(diagnostics) => self.render(&diagnostics),
        }
    }

    pub fn emit_report(mut self) -> String {
        match self.emit() {
            Ok(emitted) => emitted.code,
            Err(diagnostics) => self.render(&diagnostics),
        }
    }

    pub fn run_report(mut self) -> String {
        let emitted = match self.emit() {
            Ok(emitted) => emitted,
            Err(diagnostics) => return self.render(&diagnostics),
        };
        let directory = std::env::temp_dir().join(format!(
            "rslisp-run-{}-{}",
            std::process::id(),
            self.module_name()
        ));
        let binary = directory.join(self.module_name());
        match RustCompiler::new(directory, BuildProfile::Debug).compile(
            &emitted,
            "fixture",
            &binary,
            self.entry_span(),
        ) {
            Ok(()) => Self::stdout_of(&binary),
            Err(BuildError::Program(diagnostics)) => self.render(&diagnostics),
            Err(BuildError::Environment(message)) => {
                panic!("the build environment failed: {message}")
            }
        }
    }

    fn emit(&mut self) -> Result<EmittedCrate, Diagnostics> {
        let program = self.checked()?;
        RustEmitter::emit(&program, self.entry_span())
    }

    fn checked(&mut self) -> Result<TypedProgram, Diagnostics> {
        let mut program = self.expand().and_then(TypeChecker::check)?;
        OwnershipChecker::check(&mut program)?;
        Ok(program)
    }

    fn entry_span(&self) -> Span {
        Span::new(self.entry, 0..0)
    }

    fn stdout_of(binary: &Path) -> String {
        let output = Command::new(binary)
            .output()
            .expect("the compiled fixture runs");
        String::from_utf8(output.stdout).expect("fixtures print text")
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
