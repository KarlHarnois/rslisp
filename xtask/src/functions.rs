use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};
use syn::{Block, Ident, ImplItemFn, ItemFn, ItemImpl, ItemMod, ItemTrait, Signature, TraitItemFn};

use crate::complexity::ComplexityCounter;
use crate::failure::TaskFailure;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineSpan {
    pub first: usize,
    pub last: usize,
}

impl LineSpan {
    fn of(signature: &Signature, body: &Block) -> Self {
        Self {
            first: signature.fn_token.span.start().line,
            last: body.brace_token.span.close().start().line,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct SourceFunction {
    pub file: String,
    pub name: String,
    pub lines: LineSpan,
    pub complexity: usize,
}

impl fmt::Display for SourceFunction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{} {}",
            self.file, self.lines.first, self.name
        )
    }
}

pub struct SourceTree {
    root: PathBuf,
    directories: BTreeSet<PathBuf>,
}

impl SourceTree {
    const EXTENSION: &str = "rs";

    pub fn new(root: &Path, directories: BTreeSet<PathBuf>) -> Self {
        Self {
            root: root.to_path_buf(),
            directories,
        }
    }

    pub fn functions(&self) -> Result<Vec<SourceFunction>, TaskFailure> {
        let mut functions = Vec::new();
        for file in self.files()? {
            functions.extend(self.functions_in(&file)?);
        }
        Ok(functions)
    }

    fn files(&self) -> Result<BTreeSet<PathBuf>, TaskFailure> {
        let mut files = BTreeSet::new();
        for directory in &self.directories {
            Self::collect_files(directory, &mut files)
                .map_err(|error| TaskFailure::unreadable(directory, error))?;
        }
        Ok(files)
    }

    fn collect_files(directory: &Path, files: &mut BTreeSet<PathBuf>) -> io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            if path.is_dir() {
                Self::collect_files(&path, files)?;
            } else if path
                .extension()
                .is_some_and(|extension| extension == Self::EXTENSION)
            {
                files.insert(path);
            }
        }
        Ok(())
    }

    fn functions_in(&self, path: &Path) -> Result<Vec<SourceFunction>, TaskFailure> {
        let text =
            fs::read_to_string(path).map_err(|error| TaskFailure::unreadable(path, error))?;
        let syntax =
            syn::parse_file(&text).map_err(|error| TaskFailure::unparsable(path, &error))?;
        Ok(FunctionCollector::collect(self.relative(path), &syntax))
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }
}

pub struct FunctionCollector {
    file: String,
    owner: Option<String>,
    functions: Vec<SourceFunction>,
}

impl FunctionCollector {
    const TEST_MODULE: &str = "tests";

    pub fn collect(file: String, syntax: &syn::File) -> Vec<SourceFunction> {
        let mut collector = Self {
            file,
            owner: None,
            functions: Vec::new(),
        };
        collector.visit_file(syntax);
        collector.functions
    }

    fn record(&mut self, signature: &Signature, body: &Block) {
        self.functions.push(SourceFunction {
            file: self.file.clone(),
            name: self.qualified(&signature.ident),
            lines: LineSpan::of(signature, body),
            complexity: ComplexityCounter::of(body),
        });
    }

    fn qualified(&self, name: &Ident) -> String {
        match &self.owner {
            Some(owner) => format!("{owner}::{name}"),
            None => name.to_string(),
        }
    }

    fn within_owner(&mut self, owner: Option<String>, visit: impl FnOnce(&mut Self)) {
        let outer = std::mem::replace(&mut self.owner, owner);
        visit(self);
        self.owner = outer;
    }

    fn type_name(implementation: &ItemImpl) -> Option<String> {
        match implementation.self_ty.as_ref() {
            syn::Type::Path(path) => path
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string()),
            _ => None,
        }
    }
}

impl<'ast> Visit<'ast> for FunctionCollector {
    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        if module.ident != Self::TEST_MODULE {
            visit::visit_item_mod(self, module);
        }
    }

    fn visit_item_fn(&mut self, function: &'ast ItemFn) {
        self.record(&function.sig, &function.block);
    }

    fn visit_item_impl(&mut self, implementation: &'ast ItemImpl) {
        self.within_owner(Self::type_name(implementation), |collector| {
            visit::visit_item_impl(collector, implementation);
        });
    }

    fn visit_item_trait(&mut self, definition: &'ast ItemTrait) {
        self.within_owner(Some(definition.ident.to_string()), |collector| {
            visit::visit_item_trait(collector, definition);
        });
    }

    fn visit_impl_item_fn(&mut self, method: &'ast ImplItemFn) {
        self.record(&method.sig, &method.block);
    }

    fn visit_trait_item_fn(&mut self, method: &'ast TraitItemFn) {
        if let Some(body) = &method.default {
            self.record(&method.sig, body);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FunctionCollector, LineSpan, SourceTree};
    use crate::workspace::WorkspaceFixture;

    struct SampleFile;

    impl SampleFile {
        const TEXT: &str = "\
fn area(width: f64, height: f64) -> f64 {
    width * height
}

struct Square;

impl Square {
    fn side(&self) -> f64 {
        if true { 1.0 } else { 2.0 }
    }
}

trait Shape {
    fn name(&self) -> String {
        String::new()
    }

    fn sides(&self) -> usize;
}

impl Shape for Square {
    fn sides(&self) -> usize {
        fn nested() -> usize {
            4
        }
        nested()
    }
}

mod tests {
    fn helper() {}
}
";

        fn names() -> Vec<String> {
            Self::collected()
                .into_iter()
                .map(|function| format!("{function} {}", function.complexity))
                .collect()
        }

        fn collected() -> Vec<super::SourceFunction> {
            let syntax = syn::parse_file(Self::TEXT).expect("the sample parses");
            FunctionCollector::collect("src/shapes.rs".to_string(), &syntax)
        }
    }

    #[test]
    fn collects_functions_methods_and_trait_defaults_with_their_owner() {
        assert_eq!(
            SampleFile::names(),
            [
                "src/shapes.rs:1 area 1",
                "src/shapes.rs:8 Square::side 2",
                "src/shapes.rs:14 Shape::name 1",
                "src/shapes.rs:22 Square::sides 1",
            ]
        );
    }

    #[test]
    fn scans_the_sources_of_every_crate_in_the_workspace() {
        let workspace = WorkspaceFixture::workspace();
        let functions = SourceTree::new(&WorkspaceFixture::root(), workspace.source_directories())
            .functions()
            .unwrap();
        let names: Vec<String> = functions.iter().map(ToString::to_string).collect();
        assert_eq!(
            names,
            [
                "src/lib.rs:1 post_entry",
                "src/main.rs:1 main",
                "syntax/src/lib.rs:1 read_entry",
            ]
        );
    }

    #[test]
    fn a_function_spans_its_signature_to_its_closing_brace() {
        let side = &SampleFile::collected()[1];
        assert_eq!(side.lines, LineSpan { first: 8, last: 10 });
    }
}
