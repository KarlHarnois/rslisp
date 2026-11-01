use super::Expander;
use crate::core::{Expr, ModuleId, ModulePath};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::reader::Form;
use crate::source::SourceId;

pub struct ExpansionSession<'s> {
    expander: Expander<'s>,
    entry: ModuleId,
}

impl<'s> Expander<'s> {
    pub fn open(
        mut self,
        path: ModulePath,
        entry: SourceId,
    ) -> Result<ExpansionSession<'s>, Diagnostics> {
        let entry = self.load_source_module(path, entry);
        self.take_diagnostics().into_result(ExpansionSession {
            expander: self,
            entry,
        })
    }
}

impl ExpansionSession<'_> {
    pub fn add_source(&mut self, name: &str, text: &str) -> SourceId {
        self.expander.sources.add(name, text)
    }

    pub fn expression(&mut self, form: Form) -> Result<Expr, Diagnostics> {
        let scoped = self.in_entry_scope(form);
        let lowered = self.lower_in_entry(&scoped);
        self.collect_diagnostics(lowered)
    }

    fn in_entry_scope(&self, mut form: Form) -> Form {
        let module = self.expander.modules.get(self.entry);
        form.add_scope(module.base_scope());
        form.add_scope(module.body_scope());
        form
    }

    fn lower_in_entry(&mut self, form: &Form) -> Result<Expr, Diagnostic> {
        let outer_module = self.expander.current_module.replace(self.entry);
        let lowered = self.expander.lower_expr(form);
        self.expander.current_module = outer_module;
        lowered
    }

    fn collect_diagnostics(
        &mut self,
        lowered: Result<Expr, Diagnostic>,
    ) -> Result<Expr, Diagnostics> {
        let mut diagnostics = self.expander.take_diagnostics();
        match lowered {
            Ok(expression) => diagnostics.into_result(expression),
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                Err(diagnostics)
            }
        }
    }
}
