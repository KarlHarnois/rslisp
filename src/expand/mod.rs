mod compile_time_check;
mod context;
mod expression;
mod globals;
mod host;
mod macro_call;
mod module_body;
mod module_loading;
mod names;
mod problems;
mod recur_check;
mod rust_type;
mod shape;
mod type_expr;
mod type_parameters;

use std::collections::{HashMap, HashSet};

use crate::core::{BindingArena, CoreModule, ModuleId, ModulePath};
use crate::diagnostic::Diagnostics;
use crate::modules::{ModuleRegistry, ModuleSources};
use crate::reader::Form;
use crate::scope::{BindingTable, Phase, Scope, ScopeAllocator};
use crate::source::{SourceId, SourceMap, Span};
use globals::GlobalTable;
use macro_call::{MacroOrigin, MacroTable};

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
    local_scopes: HashSet<Scope>,
    use_site_scopes: HashSet<Scope>,
    table: BindingTable,
    bindings: BindingArena,
    modules: ModuleRegistry,
    macros: MacroTable,
    macro_origins: HashMap<Span, MacroOrigin>,
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
            local_scopes: HashSet::new(),
            use_site_scopes: HashSet::new(),
            table: BindingTable::default(),
            bindings: BindingArena::default(),
            modules: ModuleRegistry::default(),
            macros: MacroTable::default(),
            macro_origins: HashMap::new(),
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

    fn fresh_local_scope(&mut self) -> Scope {
        let scope = self.scopes.fresh();
        self.local_scopes.insert(scope);
        scope
    }

    fn quoted(&self, datum: &Form) -> Form {
        let mut quoted = datum.clone();
        quoted.retain_scopes(&|scope| {
            !self.local_scopes.contains(&scope) && !self.use_site_scopes.contains(&scope)
        });
        quoted
    }

    fn take_modules(&mut self) -> Vec<CoreModule> {
        self.order
            .iter()
            .filter_map(|id| self.expanded.remove(id))
            .collect()
    }
}
