mod budget_arena;
mod classified_form;
mod compile_time_check;
mod context;
mod defining_call;
mod distinct_diagnostics;
mod evaluated_forms;
mod expression;
mod field_keyword;
mod form_budget;
mod generated_entry;
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
mod result_printer;
mod rust_clauses;
mod rust_type;
mod session;
mod shape;
mod sourced_items;
mod spliced_forms;
mod structural_clause;
mod top_level_form;
mod try_check;
mod type_expr;
mod type_parameters;

use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::core::{BindingArena, BindingId, CoreModule, ModuleId, ModulePath};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::modules::{ModuleRegistry, ModuleSources};
use crate::reader::Form;
use crate::scope::{BindingTable, Phase, Scope, ScopeAllocator};
use crate::source::{SourceId, SourceMap, Span};
use budget_arena::BudgetArena;
pub use classified_form::ClassifiedForm;
pub use evaluated_forms::BoundName;
use evaluated_forms::EvaluatedForms;
pub use generated_entry::{GeneratedEntry, WrittenForm};
use globals::{GlobalTable, LoweredGlobals};
pub use macro_call::MacroOrigin;
use macro_call::MacroTable;
use module_notes::ModuleNotes;
pub use prelude_state::{PreludeState, UnreadablePrelude};
pub use problems::ExpandProblems;
pub use result_printer::{GeneratedFunction, ResultPrinter};
pub use session::ExpansionSession;

pub struct Program {
    pub modules: Vec<CoreModule>,
    pub entry: ModuleId,
    pub entry_function: Option<BindingId>,
    pub evaluated_forms: Vec<WrittenForm>,
    pub bindings: BindingArena,
    pub macro_origins: FxHashMap<Span, MacroOrigin>,
    pub macro_helpers: FxHashSet<BindingId>,
    pub warnings: Diagnostics,
    pub expansion: ExpansionWork,
}

#[derive(Clone, Copy, Default)]
pub struct ExpansionWork {
    pub lowered_forms: usize,
}

impl Program {
    pub(super) const WRITTEN_ENTRY_FUNCTION: &str = "main";

    pub fn entry_module(&self) -> &CoreModule {
        self.modules
            .iter()
            .find(|module| module.id == self.entry)
            .expect("the entry module is expanded")
    }

    fn written_entry_function(&self) -> Option<BindingId> {
        self.entry_module()
            .definitions()
            .map(|(binding, _)| binding)
            .find(|binding| self.bindings.get(*binding).name() == Self::WRITTEN_ENTRY_FUNCTION)
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct TraitMethods {
    pub rust_name: Rc<str>,
    pub self_variable: crate::core::BindingId,
    pub associated: Vec<(crate::core::BindingId, Rc<str>)>,
    pub methods: Vec<(crate::core::BindingId, crate::core::TypeExpr)>,
    pub defaults: rustc_hash::FxHashSet<crate::core::BindingId>,
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
    expansion: ExpansionWork,
    evaluated_forms: EvaluatedForms,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct ExpansionTables {
    scopes: ScopeAllocator,
    local_scopes: FxHashSet<Scope>,
    use_site_scopes: FxHashSet<Scope>,
    table: BindingTable,
    bindings: BindingArena,
    macros: MacroTable,
    macro_origins: FxHashMap<Span, MacroOrigin>,
    introduced_names: FxHashMap<(Span, String), Span>,
    introduction_scopes: FxHashSet<Scope>,
    global_macros: Vec<(crate::core::BindingId, Form)>,
    trait_paths: FxHashMap<crate::core::BindingId, Rc<str>>,
    structural_paths: FxHashMap<Rc<str>, (crate::core::BindingId, Span)>,
    enum_variants: FxHashMap<crate::core::BindingId, Vec<crate::core::BindingId>>,
    trait_methods: FxHashMap<crate::core::BindingId, TraitMethods>,
    globals: GlobalTable,
    module_notes: ModuleNotes,
    expanded: FxHashMap<ModuleId, CoreModule>,
    order: Vec<ModuleId>,
}

impl ExpansionTables {
    fn take_lowered_globals(&mut self) -> LoweredGlobals {
        let definitions = self.expanded.values().flat_map(CoreModule::definitions);
        self.globals.take_lowered(definitions)
    }

    fn restore_lowered_globals(&mut self, lowered: &LoweredGlobals) {
        let definitions = self.expanded.values().flat_map(CoreModule::definitions);
        self.globals.restore_lowered(lowered, definitions);
    }
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
            expansion: ExpansionWork::default(),
            evaluated_forms: EvaluatedForms::default(),
        }
    }

    pub fn evaluating(self, spans: impl IntoIterator<Item = Span>) -> Self {
        Self {
            evaluated_forms: EvaluatedForms::new(spans),
            ..self
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

    #[inline(never)]
    pub fn expand(mut self, path: ModulePath, entry: SourceId) -> Result<Program, Diagnostics> {
        let entry = self.load_source_module(path, &[entry]);
        self.into_program(entry)
    }

    fn into_program(mut self, entry: ModuleId) -> Result<Program, Diagnostics> {
        let diagnostics = self.take_diagnostics();
        let mut program = Program {
            modules: self.take_modules(),
            entry,
            entry_function: None,
            evaluated_forms: Vec::new(),
            bindings: self.tables.bindings,
            macro_helpers: self.tables.globals.macro_helpers(),
            macro_origins: self.tables.macro_origins,
            warnings: self.warnings,
            expansion: self.expansion,
        };
        program.entry_function = program.written_entry_function();
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
