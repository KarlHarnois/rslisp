use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::{DefinitionGraph, InferredCopies, LiteralDefaults, TypeChecker, TypedProgram};
use crate::core::ModulePath;
use crate::expand::{Expander, Program};
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
            &HashSet::new(),
            &InferredCopies::default(),
            &HashMap::new(),
            LiteralDefaults::default(),
            false,
        )
        .expect("the source checks")
    }

    fn file_name() -> String {
        format!("{}.{}", Self::MODULE, Language::EXTENSION)
    }
}
