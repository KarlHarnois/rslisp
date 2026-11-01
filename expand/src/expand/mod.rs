mod budget_arena;
mod compile_time_check;
mod context;
mod defining_call;
mod distinct_diagnostics;
mod expression;
mod field_keyword;
mod form_budget;
mod globals;
mod host;
mod introduced;
mod macro_call;
mod macro_function;
mod misplaced_definition;
mod module_body;
mod module_loading;
mod module_notes;
mod names;
mod prelude_state;
mod problems;
mod recur_check;
mod rest_of_body;
mod rust_type;
mod session;
mod shape;
mod sourced_items;
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
use budget_arena::BudgetArena;
use globals::GlobalTable;
pub use macro_call::MacroOrigin;
use macro_call::MacroTable;
use module_notes::ModuleNotes;
pub use prelude_state::{PreludeState, UnreadablePrelude};
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

#[derive(Clone, serde::Serialize, serde::Deserialize)]
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
    modules: ModuleRegistry,
    tables: ExpansionTables,
    current_module: Option<ModuleId>,
    diagnostics: Diagnostics,
    warnings: Diagnostics,
    phase: Phase,
    expansion_depth: usize,
    nesting_depth: usize,
    form_budgets: BudgetArena,
    recovered_diagnostics: Vec<Diagnostic>,
    saved_prelude: Option<PreludeState>,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct ExpansionTables {
    scopes: ScopeAllocator,
    local_scopes: FastHashSet<Scope>,
    use_site_scopes: FastHashSet<Scope>,
    table: BindingTable,
    bindings: BindingArena,
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
}

impl<'s> Expander<'s> {
    pub fn new(sources: &'s mut SourceMap, module_sources: ModuleSources) -> Self {
        Self {
            sources,
            module_sources,
            modules: ModuleRegistry::default(),
            tables: ExpansionTables::default(),
            current_module: None,
            diagnostics: Diagnostics::default(),
            warnings: Diagnostics::default(),
            phase: Phase::Runtime,
            expansion_depth: 0,
            nesting_depth: 0,
            form_budgets: BudgetArena::default(),
            recovered_diagnostics: Vec::new(),
            saved_prelude: None,
        }
    }

    pub fn with_prelude(
        sources: &'s mut SourceMap,
        module_sources: ModuleSources,
        prelude: PreludeState,
    ) -> Self {
        Self {
            saved_prelude: Some(prelude),
            ..Self::new(sources, module_sources)
        }
    }

    pub fn expand(mut self, path: ModulePath, entry: SourceId) -> Result<Program, Diagnostics> {
        let entry = self.load_source_module(path, entry);
        let diagnostics = self.take_diagnostics();
        let program = Program {
            modules: self.take_modules(),
            entry,
            bindings: self.tables.bindings,
            macro_origins: self.tables.macro_origins,
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
        let scope = self.tables.scopes.fresh();
        self.tables.local_scopes.insert(scope);
        scope
    }

    fn quoted(&self, datum: &Form) -> Form {
        let mut quoted = datum.clone();
        quoted.retain_scopes(&|scope| {
            !self.tables.local_scopes.contains(&scope)
                && !self.tables.use_site_scopes.contains(&scope)
        });
        quoted
    }

    fn take_modules(&mut self) -> Vec<CoreModule> {
        self.tables
            .order
            .iter()
            .filter_map(|id| self.tables.expanded.remove(id))
            .collect()
    }
}
