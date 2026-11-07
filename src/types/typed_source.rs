use std::path::PathBuf;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{DefinitionGraph, InferredCopies, LiteralDefaults, TypeChecker, TypedProgram};
use crate::core::{BindingId, ModulePath};
use crate::expand::{Expander, PreludeState, Program};
use crate::language::Language;
use crate::modules::ModuleSources;
use crate::source::SourceMap;

pub struct TypedSource {
    program: Program,
}

impl TypedSource {
    const MODULE: &str = "fixture";

    pub fn expand(text: &str) -> Self {
        let mut sources = SourceMap::default();
        let entry = sources.add(Self::file_name(), text);
        let module_sources = ModuleSources::new(PathBuf::from("."), PathBuf::from("."));
        let program = Expander::new(&mut sources, module_sources)
            .expand(ModulePath::new(Self::MODULE), entry)
            .expect("the source expands");
        Self { program }
    }

    pub fn expand_on_a_loaded_prelude(text: &str) -> Self {
        let state = PreludeState::load(PreludeState::build().save().leak())
            .expect("the saved prelude loads");
        let mut sources = SourceMap::default();
        let entry = sources.add(Self::file_name(), text);
        let module_sources = ModuleSources::new(PathBuf::from("."), PathBuf::from("."));
        let program = Expander::with_prelude(&mut sources, module_sources, state)
            .expand(ModulePath::new(Self::MODULE), entry)
            .expect("the source expands");
        Self { program }
    }

    pub fn enter_at(&mut self, name: &str) {
        let program = &self.program;
        let entry = program
            .entry_module()
            .definitions()
            .map(|(binding, _)| binding)
            .find(|binding| program.bindings.get(*binding).name() == name);
        self.program.entry_function = entry;
    }

    pub fn move_entry_to(&mut self, path: ModulePath) {
        let entry = self.program.entry;
        let module = self
            .program
            .modules
            .iter_mut()
            .find(|module| module.id == entry);
        module.expect("the entry module is expanded").path = path;
    }

    pub fn detach_entry(&mut self) {
        self.program.entry = self.program.modules[0].id;
    }

    pub fn check_also(&mut self, binding: BindingId) {
        self.program.check_also(binding);
    }

    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn typed(&self) -> TypedProgram {
        self.check(&DefinitionGraph::of(&self.program))
    }

    pub fn check(&self, graph: &DefinitionGraph) -> TypedProgram {
        TypeChecker::check(
            &self.program,
            graph,
            &FxHashSet::default(),
            &InferredCopies::default(),
            &FxHashMap::default(),
            LiteralDefaults::default(),
            false,
        )
        .expect("the source checks")
    }

    fn file_name() -> String {
        format!("{}.{}", Self::MODULE, Language::EXTENSION)
    }
}
