mod headers;
mod reports;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rslisp::build::{BuildError, BuildProfile, EmittedFormatting, RustCompiler};
use rslisp::core::{
    BindingArena, CorePrinter, ExprKind, Item, ItemKind, Leaf, Literal, ModulePath,
};
use rslisp::diagnostic::{DiagnosticRenderer, Diagnostics};
use rslisp::emit::{EmittedCrate, RustEmitter};
use rslisp::expand::{Expander, PreludeState};
use rslisp::modules::ModuleSources;
use rslisp::reader::Reader;
use rslisp::source::{SourceId, SourceMap, Span};
use rslisp::types::TypedProgram;

pub use headers::HeaderAgreement;
pub use reports::FixtureReports;

pub struct Fixture {
    sources: SourceMap,
    entry: SourceId,
    path: PathBuf,
    prelude: Option<PreludeState>,
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
            prelude: None,
        }
    }

    pub fn with_saved_prelude(self, prelude: PreludeState) -> Self {
        Self {
            prelude: Some(prelude),
            ..self
        }
    }

    pub fn stage_report(self) -> String {
        match self.stage_name().as_str() {
            "expand" => self.expand_report(),
            "check" => self.check_report(),
            _ => self.emit_report(),
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

    pub fn string_definitions(mut self) -> HashMap<String, String> {
        let program = self.expand().unwrap_or_else(|diagnostics| {
            panic!("the fixture expands:\n{}", self.render(&diagnostics))
        });
        program
            .entry_module()
            .items
            .iter()
            .filter_map(|item| Self::string_definition(&program.bindings, item))
            .collect()
    }

    fn string_definition(bindings: &BindingArena, item: &Item) -> Option<(String, String)> {
        let ItemKind::Def(binding, value) = &item.kind else {
            return None;
        };
        let ExprKind::Leaf(Leaf::Literal(Literal::String(text))) = value.kind() else {
            return None;
        };
        Some((bindings.get(*binding).name().to_string(), text.to_string()))
    }

    pub fn check_report(mut self) -> String {
        match self.checked() {
            Ok(program) if program.warnings.is_empty() => program.entry_signatures(),
            Ok(program) => format!(
                "{}\n\n{}",
                self.render(&program.warnings),
                program.entry_signatures()
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
        let directory = self.build_directory();
        let binary = directory.join(self.module_name());
        match RustCompiler::new(directory, BuildProfile::Debug).compile(
            &emitted,
            "fixture",
            &binary,
            self.entry_span(),
        ) {
            Ok(()) => Self::report_of(&binary, &self.arguments(), &self.directory()),
            Err(BuildError::Program(diagnostics)) => self.render(&diagnostics),
            Err(BuildError::Environment(message)) => {
                panic!("the build environment failed: {message}")
            }
        }
    }

    fn build_directory(&self) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rslisp-run-{}-{}-{}",
            std::process::id(),
            self.stage_name(),
            self.module_name()
        ))
    }

    fn stage_name(&self) -> String {
        let stage = self.directory();
        let name = stage
            .file_name()
            .expect("fixtures live in a stage directory");
        name.to_string_lossy().into_owned()
    }

    fn arguments(&self) -> Vec<String> {
        let arguments = self.path.with_extension("arguments");
        match fs::read_to_string(arguments) {
            Ok(text) => text.lines().map(str::to_string).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn emit(&mut self) -> Result<EmittedCrate, Diagnostics> {
        let program = self.checked()?;
        let emitted = RustEmitter::emit(&program, self.entry_span())?;
        Ok(EmittedFormatting::within(EmittedFormatting::TIME_LIMIT)
            .apply(&emitted)
            .expect("rustfmt formats the emitted Rust"))
    }

    fn checked(&mut self) -> Result<TypedProgram, Diagnostics> {
        let program = self.expand()?;
        rslisp::check::ProgramChecker::check(&program)
    }

    fn entry_span(&self) -> Span {
        Span::new(self.entry, 0..0)
    }

    fn report_of(binary: &Path, arguments: &[String], directory: &Path) -> String {
        let output = Command::new(binary)
            .args(arguments)
            .current_dir(directory)
            .output()
            .expect("the compiled fixture runs");
        let stdout = String::from_utf8(output.stdout).expect("fixtures print text");
        let stderr = String::from_utf8(output.stderr).expect("fixtures print text");
        if stderr.is_empty() && output.status.success() {
            stdout
        } else {
            let code = output
                .status
                .code()
                .map_or_else(|| "a signal".to_string(), |code| code.to_string());
            format!("{stdout}stderr:\n{stderr}exit code: {code}\n")
        }
    }

    fn expand(&mut self) -> Result<rslisp::expand::Program, Diagnostics> {
        let module_sources = ModuleSources::new(self.directory(), self.display_directory());
        let module_path = ModulePath::new(self.module_name());
        let expander = match self.prelude.take() {
            Some(prelude) => Expander::with_prelude(&mut self.sources, module_sources, prelude),
            None => Expander::new(&mut self.sources, module_sources),
        };
        expander.expand(module_path, self.entry)
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
