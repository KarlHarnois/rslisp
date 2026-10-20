mod context;
mod expression;
mod globals;
mod host;
mod macro_call;
mod module_body;
mod module_loading;
mod names;
mod problems;
mod shape;
mod similar_name;
mod type_expr;

use std::collections::HashMap;

use crate::core::{BindingArena, CoreModule, ModuleId, ModulePath};
use crate::diagnostic::Diagnostics;
use crate::modules::{ModuleRegistry, ModuleSources};
use crate::scope::{BindingTable, Phase, ScopeAllocator};
use crate::source::{SourceId, SourceMap};
use globals::GlobalTable;
use macro_call::MacroTable;

pub struct Program {
    pub modules: Vec<CoreModule>,
    pub entry: ModuleId,
    pub bindings: BindingArena,
}

impl Program {
    pub fn entry_module(&self) -> &CoreModule {
        self.modules
            .iter()
            .find(|module| module.id == self.entry)
            .expect("the entry module is expanded")
    }
}

pub struct Expander<'s> {
    sources: &'s mut SourceMap,
    module_sources: ModuleSources,
    scopes: ScopeAllocator,
    table: BindingTable,
    bindings: BindingArena,
    modules: ModuleRegistry,
    macros: MacroTable,
    globals: GlobalTable,
    expanded: HashMap<ModuleId, CoreModule>,
    order: Vec<ModuleId>,
    diagnostics: Diagnostics,
    phase: Phase,
    expansion_depth: usize,
}

impl<'s> Expander<'s> {
    pub fn new(sources: &'s mut SourceMap, module_sources: ModuleSources) -> Self {
        Self {
            sources,
            module_sources,
            scopes: ScopeAllocator::default(),
            table: BindingTable::default(),
            bindings: BindingArena::default(),
            modules: ModuleRegistry::default(),
            macros: MacroTable::default(),
            globals: GlobalTable::default(),
            expanded: HashMap::new(),
            order: Vec::new(),
            diagnostics: Diagnostics::default(),
            phase: Phase::Runtime,
            expansion_depth: 0,
        }
    }

    pub fn expand(mut self, path: ModulePath, entry: SourceId) -> Result<Program, Diagnostics> {
        let entry = self.load_source_module(path, entry);
        let program = Program {
            modules: self.take_modules(),
            entry,
            bindings: self.bindings,
        };
        self.diagnostics.into_result(program)
    }

    fn take_modules(&mut self) -> Vec<CoreModule> {
        self.order
            .iter()
            .filter_map(|id| self.expanded.remove(id))
            .collect()
    }
}
