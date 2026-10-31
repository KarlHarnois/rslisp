mod compile_time_check;
mod context;
mod distinct_diagnostics;
mod expression;
mod field_keyword;
mod form_budget;
mod globals;
mod host;
mod introduced;
mod macro_call;
mod macro_function;
mod module_body;
mod module_loading;
mod module_notes;
mod names;
mod problems;
mod recur_check;
mod rest_of_body;
mod rust_type;
mod session;
mod shape;
mod spliced_forms;
mod top_level_form;
mod try_check;
mod type_expr;
mod type_parameters;

use std::collections::HashMap;
use std::rc::Rc;

use crate::core::{BindingArena, CoreModule, ModuleId, ModulePath};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::modules::{ModuleRegistry, ModuleSources};
use crate::reader::Form;
use crate::scope::{BindingTable, FastHashMap, FastHashSet, Phase, Scope, ScopeAllocator};
use crate::source::{SourceId, SourceMap, Span};
use form_budget::FormBudget;
use globals::GlobalTable;
pub use macro_call::MacroOrigin;
use macro_call::MacroTable;
use module_notes::ModuleNotes;
pub use problems::ExpandProblems;
pub use session::ExpansionSession;

pub struct Program {
    pub modules: Vec<CoreModule>,
    pub entry: ModuleId,
    pub bindings: BindingArena,
    pub macro_origins: HashMap<Span, MacroOrigin>,
    pub warnings: Diagnostics,
}

impl Program {
    pub fn entry_module(&self) -> &CoreModule {
        self.modules
            .iter()
            .find(|module| module.id == self.entry)
            .expect("the entry module is expanded")
    }
}

#[derive(Clone)]
pub struct TraitMethods {
    pub rust_name: Rc<str>,
    pub self_variable: crate::core::BindingId,
    pub associated: Vec<(crate::core::BindingId, Rc<str>)>,
    pub methods: Vec<(crate::core::BindingId, crate::core::TypeExpr)>,
    pub defaults: std::collections::HashSet<crate::core::BindingId>,
}

pub struct Expander<'s> {
    sources: &'s mut SourceMap,
    module_sources: ModuleSources,
    scopes: ScopeAllocator,
    local_scopes: FastHashSet<Scope>,
    use_site_scopes: FastHashSet<Scope>,
    table: BindingTable,
    bindings: BindingArena,
    modules: ModuleRegistry,
    macros: MacroTable,
    macro_origins: HashMap<Span, MacroOrigin>,
    introduced_names: FastHashMap<(Span, String), Span>,
    introduction_scopes: FastHashSet<Scope>,
    global_macros: Vec<(crate::core::BindingId, Form)>,
    trait_paths: HashMap<crate::core::BindingId, Rc<str>>,
    enum_variants: HashMap<crate::core::BindingId, Vec<crate::core::BindingId>>,
    trait_methods: HashMap<crate::core::BindingId, TraitMethods>,
    globals: GlobalTable,
    module_notes: ModuleNotes,
    expanded: HashMap<ModuleId, CoreModule>,
    order: Vec<ModuleId>,
    current_module: Option<ModuleId>,
    diagnostics: Diagnostics,
    warnings: Diagnostics,
    phase: Phase,
    expansion_depth: usize,
    nesting_depth: usize,
    form_budget: FormBudget,
    recovered_diagnostics: Vec<Diagnostic>,
}

impl<'s> Expander<'s> {
    pub fn new(sources: &'s mut SourceMap, module_sources: ModuleSources) -> Self {
        Self {
            sources,
            module_sources,
            scopes: ScopeAllocator::default(),
            local_scopes: FastHashSet::default(),
            use_site_scopes: FastHashSet::default(),
            table: BindingTable::default(),
            bindings: BindingArena::default(),
            modules: ModuleRegistry::default(),
            macros: MacroTable::default(),
            macro_origins: HashMap::new(),
            introduced_names: FastHashMap::default(),
            introduction_scopes: FastHashSet::default(),
            global_macros: Vec::new(),
            trait_paths: HashMap::new(),
            enum_variants: HashMap::new(),
            trait_methods: HashMap::new(),
            globals: GlobalTable::default(),
            module_notes: ModuleNotes::default(),
            expanded: HashMap::new(),
            order: Vec::new(),
            current_module: None,
            diagnostics: Diagnostics::default(),
            warnings: Diagnostics::default(),
            phase: Phase::Runtime,
            expansion_depth: 0,
            nesting_depth: 0,
            form_budget: FormBudget::full(),
            recovered_diagnostics: Vec::new(),
        }
    }

    pub fn expand(mut self, path: ModulePath, entry: SourceId) -> Result<Program, Diagnostics> {
        let entry = self.load_source_module(path, entry);
        let diagnostics = self.take_diagnostics();
        let program = Program {
            modules: self.take_modules(),
            entry,
            bindings: self.bindings,
            macro_origins: self.macro_origins,
            warnings: self.warnings,
        };
        diagnostics.into_result(program)
    }

    fn take_diagnostics(&mut self) -> Diagnostics {
        for diagnostic in std::mem::take(&mut self.recovered_diagnostics) {
            self.diagnostics.push(diagnostic);
        }
        std::mem::take(&mut self.diagnostics)
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
