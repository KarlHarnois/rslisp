use std::rc::Rc;

use super::Expander;
use super::context::DefinitionContext;
use super::form_budget::FormBudget;
use super::problems::ExpandProblems;
use super::shape::Shape;
use crate::core::{BindingId, BindingKind, Expr, KernelForm, Lambda, Namespace};
use crate::diagnostic::{Diagnostic, Label};
use crate::interpret::{Closure, Env, Interpreter, Value};
use crate::reader::Form;
use crate::scope::{FastHashMap, Phase, Scope};
use crate::source::Span;

enum MacroState {
    Pending(Form),
    Ready(Rc<Closure>),
    Failed,
}

#[derive(Clone)]
pub struct MacroOrigin {
    pub name: Rc<str>,
    pub defined: Span,
    pub call_site: Span,
}

#[derive(Default)]
pub struct MacroTable {
    states: FastHashMap<BindingId, MacroState>,
}

impl MacroTable {
    pub fn add_pending(&mut self, binding: BindingId, definition: Form) {
        self.states.insert(binding, MacroState::Pending(definition));
    }

    fn pending(&self) -> Vec<(BindingId, Form)> {
        let mut pending: Vec<(BindingId, Form)> = self
            .states
            .iter()
            .filter_map(|(binding, state)| match state {
                MacroState::Pending(definition) => Some((*binding, definition.clone())),
                _ => None,
            })
            .collect();
        pending.sort_by_key(|(binding, _)| *binding);
        pending
    }
}

impl Expander<'_> {
    const MAX_EXPANSION_DEPTH: usize = 256;
    const MACRO_PARAMETERS: usize = 1;

    pub(super) fn expand_head(
        &mut self,
        form: Form,
        mut context: Option<&mut DefinitionContext>,
    ) -> Result<Form, Diagnostic> {
        let mut form = form;
        let mut expansions = 0;
        while let Some(binding) = self.macro_head(&form) {
            if expansions == Self::MAX_EXPANSION_DEPTH {
                return Err(ExpandProblems::expansion_too_deep(form.span()));
            }
            form = self.apply_macro(binding, form, context.as_deref_mut())?;
            expansions += 1;
        }
        Ok(form)
    }

    pub(super) fn lower_expansion(&mut self, expanded: &Form) -> Result<Expr, Diagnostic> {
        self.expansion_depth += 1;
        let lowered = self.lower_expr(expanded);
        self.expansion_depth -= 1;
        lowered
    }

    pub(super) fn macro_head(&self, form: &Form) -> Option<BindingId> {
        let head = form.as_list()?.first()?.as_symbol()?;
        let binding = self.try_resolve(head, Namespace::Value)?;
        (matches!(self.kind(binding), BindingKind::Macro(_))).then_some(binding)
    }

    pub(super) fn apply_macro(
        &mut self,
        binding: BindingId,
        form: Form,
        context: Option<&mut DefinitionContext>,
    ) -> Result<Form, Diagnostic> {
        let call_site = form.span();
        let expanded = self.run_macro(binding, form, context)?;
        Ok(self.place_expansion(binding, call_site, expanded))
    }

    pub(super) fn value_expansion(&mut self, binding: BindingId, form: Form) -> Option<Form> {
        let call_site = form.span();
        let expanded = self.trial_expansion(binding, form)?;
        Some(self.place_expansion(binding, call_site, expanded))
    }

    fn place_expansion(&mut self, binding: BindingId, call_site: Span, mut form: Form) -> Form {
        let origin = self.macro_origin(binding, call_site);
        self.record_introduced_names(&form, &origin);
        form.relocate_outside(call_site);
        self.macro_origins.entry(call_site).or_insert(origin);
        form
    }

    pub(super) fn discarded_expansion(&mut self, binding: BindingId, form: Form) -> Option<Form> {
        let checkpoint = self.module_notes.checkpoint();
        let budget = self.form_budget.clone();
        let expansion = self.trial_expansion(binding, form);
        self.module_notes.roll_back(checkpoint);
        self.form_budget = budget;
        expansion
    }

    fn trial_expansion(&mut self, binding: BindingId, form: Form) -> Option<Form> {
        let diagnostics = self.diagnostics.len();
        let recovered = self.recovered_diagnostics.len();
        let budget_before = self.form_budget.clone();
        let result = self.run_macro(binding, form, None).ok();
        self.diagnostics.truncate(diagnostics);
        self.recovered_diagnostics.truncate(recovered);
        if result.is_none() {
            self.form_budget = budget_before;
        }
        result
    }

    fn run_macro(
        &mut self,
        binding: BindingId,
        form: Form,
        context: Option<&mut DefinitionContext>,
    ) -> Result<Form, Diagnostic> {
        let checkpoint = self.module_notes.checkpoint();
        let expansion = self.macro_expansion(binding, form, context);
        if expansion.is_err() {
            self.module_notes.roll_back(checkpoint);
        }
        expansion
    }

    fn macro_expansion(
        &mut self,
        binding: BindingId,
        form: Form,
        context: Option<&mut DefinitionContext>,
    ) -> Result<Form, Diagnostic> {
        let call_site = form.span();
        if self.expansion_depth >= Self::MAX_EXPANSION_DEPTH {
            return Err(ExpandProblems::expansion_too_deep(call_site));
        }
        let closure = self.macro_closure(binding, call_site)?;
        let (input, introduction) = self.prepare_input(form, context);
        self.expansion_depth += 1;
        let result = Interpreter::new(self, call_site).call(
            &Value::Closure(closure),
            vec![Value::Form(input)],
            call_site,
        );
        self.expansion_depth -= 1;
        let output = result.map_err(|diagnostic| Self::point_at_call(diagnostic, call_site))?;
        let mut form = self.output_form(binding, &output, call_site)?;
        self.introduce_within_budget(binding, &mut form, introduction, call_site)?;
        Ok(form)
    }

    fn output_form(
        &self,
        binding: BindingId,
        output: &Value,
        call_site: Span,
    ) -> Result<Form, Diagnostic> {
        if !output.is_code() {
            return Err(ExpandProblems::macro_result(output.describe(), call_site));
        }
        self.form_budget
            .admitted_form(output)
            .ok_or_else(|| self.expansion_too_large(binding, call_site))
    }

    fn introduce_within_budget(
        &mut self,
        binding: BindingId,
        form: &mut Form,
        introduction: Scope,
        call_site: Span,
    ) -> Result<(), Diagnostic> {
        if self.form_budget.flip_scope(form, introduction) {
            return Ok(());
        }
        Err(self.expansion_too_large(binding, call_site))
    }

    fn expansion_too_large(&self, binding: BindingId, call_site: Span) -> Diagnostic {
        let name = self.bindings.get(binding).name();
        ExpandProblems::expansion_too_large(name, FormBudget::EXPANSION_LIMIT, call_site)
    }

    fn macro_origin(&self, binding: BindingId, call_site: Span) -> MacroOrigin {
        let info = self.bindings.get(binding);
        MacroOrigin {
            name: Rc::from(info.name()),
            defined: info.span().unwrap_or(call_site),
            call_site,
        }
    }

    fn prepare_input(
        &mut self,
        mut form: Form,
        context: Option<&mut DefinitionContext>,
    ) -> (Form, Scope) {
        let introduction = self.scopes.fresh();
        self.introduction_scopes.insert(introduction);
        form.flip_scope(introduction);
        if let Some(context) = context {
            let use_site = self.scopes.fresh();
            self.use_site_scopes.insert(use_site);
            form.add_scope(use_site);
            context.record_use_site(use_site);
        }
        (form, introduction)
    }

    pub(super) fn macro_origin_at(&self, span: Span) -> Option<&MacroOrigin> {
        self.macro_origins
            .values()
            .filter(|origin| origin.call_site.contains(span))
            .min_by_key(|origin| origin.call_site.range().len())
    }

    pub(super) fn introduced_name(&self, call_site: Span, name: &str) -> Option<Span> {
        self.introduced_names
            .get(&(call_site, name.to_string()))
            .copied()
    }

    fn record_introduced_names(&mut self, form: &Form, origin: &MacroOrigin) {
        if let Some(symbol) = form.as_symbol()
            && !origin.call_site.contains(form.span())
        {
            self.introduced_names
                .entry((origin.call_site, symbol.name().to_string()))
                .or_insert(form.span());
        }
        if let Some(items) = form.as_list() {
            for item in items {
                self.record_introduced_names(item, origin);
            }
        }
    }

    fn point_at_call(diagnostic: Diagnostic, call_site: Span) -> Diagnostic {
        if call_site.contains(diagnostic.primary().span()) {
            diagnostic
        } else {
            diagnostic.with_label(Label::new(call_site, "in this macro call"))
        }
    }

    fn macro_closure(&mut self, binding: BindingId, span: Span) -> Result<Rc<Closure>, Diagnostic> {
        let name = self.bindings.get(binding).name().to_string();
        match self.macros.states.remove(&binding) {
            Some(MacroState::Ready(closure)) => {
                self.macros
                    .states
                    .insert(binding, MacroState::Ready(Rc::clone(&closure)));
                Ok(closure)
            }
            Some(MacroState::Pending(definition)) => {
                self.within_top_level_form(|expander| expander.compile_macro(binding, &definition))
            }
            Some(MacroState::Failed) => {
                self.macros.states.insert(binding, MacroState::Failed);
                Err(ExpandProblems::broken_macro(&name, span))
            }
            None => Err(ExpandProblems::compile_time_cycle(&name, span)),
        }
    }

    pub(super) fn compile_pending_macros(&mut self) {
        for (binding, definition) in self.macros.pending() {
            let compiled =
                self.within_top_level_form(|expander| expander.compile_macro(binding, &definition));
            if let Err(diagnostic) = compiled {
                self.diagnostics.push(diagnostic);
            }
        }
    }

    fn compile_macro(
        &mut self,
        binding: BindingId,
        definition: &Form,
    ) -> Result<Rc<Closure>, Diagnostic> {
        let compiled = self.lower_macro_lambda(definition).and_then(|lambda| {
            self.check_compile_time(&lambda)?;
            Ok(Rc::new(Closure {
                lambda: Rc::new(lambda),
                env: Env::default(),
            }))
        });
        let state = match &compiled {
            Ok(closure) => MacroState::Ready(Rc::clone(closure)),
            Err(_) => MacroState::Failed,
        };
        self.macros.states.insert(binding, state);
        compiled
    }

    fn lower_macro_lambda(&mut self, definition: &Form) -> Result<Lambda, Diagnostic> {
        let shape = Shape::new(KernelForm::Macro, definition);
        let items = shape.at_least(2)?;
        if shape.list(&items[1])?.len() != Self::MACRO_PARAMETERS {
            return Err(shape.malformed(&items[1]));
        }
        let outer_phase = std::mem::replace(&mut self.phase, Phase::CompileTime);
        let lambda = self.lower_lambda(&shape, &items[1], &items[2..]);
        self.phase = outer_phase;
        lambda
    }
}
