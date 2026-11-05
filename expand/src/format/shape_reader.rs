use std::path::Path;
use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::shapes::{ShapeCatalog, ShapeStatement};
use crate::core::{DefinitionForm, KernelForm, ModulePath};
use crate::modules::{ModuleSource, ModuleSources};
use crate::reader::{Form, Reader};
use crate::source::SourceMap;

pub struct ModuleLookup {
    directories: Vec<ModuleSources>,
}

#[derive(Default)]
pub struct ShapeReader {
    modules: FxHashMap<String, Rc<ModuleShapes>>,
}

#[derive(Default)]
struct ModuleShapes {
    statements: Vec<ShapeStatement>,
    uses: Vec<ModulePath>,
}

struct CatalogWalk<'r> {
    reader: &'r mut ShapeReader,
    lookup: &'r ModuleLookup,
    visited: FxHashSet<String>,
    catalog: ShapeCatalog,
}

impl ModuleLookup {
    pub fn beside(directory: &Path) -> Self {
        Self {
            directories: directory
                .ancestors()
                .map(|ancestor| ModuleSources::new(ancestor.to_path_buf(), ancestor.to_path_buf()))
                .collect(),
        }
    }

    fn find(&self, path: &ModulePath) -> Option<(String, String)> {
        self.directories
            .iter()
            .find_map(|sources| match sources.find(path)? {
                ModuleSource::Text { name, text } => Some((name, text)),
                ModuleSource::Builtin(_) => None,
            })
    }
}

impl ShapeReader {
    pub fn catalog(&mut self, forms: &[Form], lookup: &ModuleLookup) -> ShapeCatalog {
        let file = ModuleShapes::of(forms);
        let mut walk = CatalogWalk {
            reader: self,
            lookup,
            visited: FxHashSet::default(),
            catalog: ShapeCatalog::default(),
        };
        walk.add_module(&ModulePath::new(ModuleSources::PRELUDE));
        walk.add_shapes(&file);
        walk.catalog
    }

    fn module(&mut self, name: String, text: &str) -> Rc<ModuleShapes> {
        Rc::clone(
            self.modules
                .entry(name)
                .or_insert_with_key(|name| Rc::new(ModuleShapes::read(name, text))),
        )
    }
}

impl CatalogWalk<'_> {
    fn add_module(&mut self, path: &ModulePath) {
        let Some((name, text)) = self.lookup.find(path) else {
            return;
        };
        if self.visited.insert(name.clone()) {
            let module = self.reader.module(name, &text);
            self.add_shapes(&module);
        }
    }

    fn add_shapes(&mut self, module: &ModuleShapes) {
        for path in &module.uses {
            self.add_module(path);
        }
        for statement in &module.statements {
            self.catalog.add(statement);
        }
    }
}

impl ModuleShapes {
    fn read(name: &str, text: &str) -> Self {
        let mut sources = SourceMap::default();
        let source = sources.add(name, text);
        Reader::new(sources.file(source))
            .read()
            .map_or_else(|_| Self::default(), |forms| Self::of(&forms))
    }

    fn of(forms: &[Form]) -> Self {
        Self {
            statements: forms.iter().filter_map(ShapeStatement::of).collect(),
            uses: forms.iter().filter_map(Self::used_module).collect(),
        }
    }

    fn used_module(form: &Form) -> Option<ModulePath> {
        let [head, path, ..] = form.as_list()? else {
            return None;
        };
        let use_form = KernelForm::Definition(DefinitionForm::Use).name();
        (head.as_symbol()?.name() == use_form).then_some(())?;
        Some(ModulePath::new(path.as_symbol()?.name()))
    }
}
