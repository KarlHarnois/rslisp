use std::fs;
use std::path::Path;

use rslisp::diagnostic::DiagnosticRenderer;
use rslisp::reader::Reader;
use rslisp::source::{SourceId, SourceMap};

pub struct Fixture {
    sources: SourceMap,
    entry: SourceId,
}

impl Fixture {
    const FIXTURE_ROOT: &str = "tests/fixtures";

    pub fn load(path: &Path) -> Self {
        let text = fs::read_to_string(path).expect("fixture is readable");
        let mut sources = SourceMap::default();
        let entry = sources.add(Self::display_name(path), text);
        Self { sources, entry }
    }

    pub fn read_report(&self) -> String {
        match Reader::new(self.sources.file(self.entry)).read() {
            Ok(forms) => forms.iter().map(|form| format!("{form}\n")).collect(),
            Err(diagnostics) => DiagnosticRenderer::plain(&self.sources).render_all(&diagnostics),
        }
    }

    fn display_name(path: &Path) -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(Self::FIXTURE_ROOT);
        path.strip_prefix(root).unwrap_or(path).display().to_string()
    }
}
