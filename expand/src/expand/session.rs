use super::{Expander, Program};
use crate::core::{BindingId, Expr, ModuleId, ModulePath};
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

    pub fn binding(&self, term: &Form) -> Option<BindingId> {
        let scoped = self.in_entry_scope(term.clone());
        self.expander.term_binding(scoped.as_symbol()?)
    }

    pub fn finish(self) -> Result<Program, Diagnostics> {
        self.expander.into_program(self.entry)
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::core::ItemKind;
    use crate::modules::ModuleSources;
    use crate::reader::{FormKind, Symbol};
    use crate::source::{SourceMap, Span};

    struct SessionFixture {
        sources: SourceMap,
        entry: SourceId,
    }

    impl SessionFixture {
        const ENTRY: &str = "entry";
        const PROGRAM: &str = "(defn twice (x) (* 2 x))";
        const DEFINED_NAME: &str = "twice";
        const UNBOUND_NAME: &str = "thrice";

        fn new() -> Self {
            let mut sources = SourceMap::default();
            let entry = sources.add(Self::ENTRY, Self::PROGRAM);
            Self { sources, entry }
        }

        fn session(&mut self) -> ExpansionSession<'_> {
            let entry = self.entry;
            self.expander()
                .open(ModulePath::new(Self::ENTRY), entry)
                .unwrap_or_else(|_| panic!("the entry expands"))
        }

        fn expanded(&mut self) -> Program {
            let entry = self.entry;
            self.expander()
                .expand(ModulePath::new(Self::ENTRY), entry)
                .unwrap_or_else(|_| panic!("the entry expands"))
        }

        fn expander(&mut self) -> Expander<'_> {
            Expander::new(
                &mut self.sources,
                ModuleSources::new(PathBuf::new(), PathBuf::new()),
            )
        }

        fn symbol(&self, name: &str) -> Form {
            Form::new(FormKind::Symbol(Symbol::new(name)), self.span())
        }

        fn list(&self, name: &str) -> Form {
            Form::new(FormKind::List(vec![self.symbol(name)]), self.span())
        }

        fn span(&self) -> Span {
            Span::new(self.entry, 0..0)
        }

        fn finished(session: ExpansionSession<'_>) -> Program {
            session
                .finish()
                .unwrap_or_else(|_| panic!("the session finishes"))
        }

        fn defined_binding(program: &Program) -> Option<BindingId> {
            program
                .entry_module()
                .items
                .iter()
                .find_map(|item| match item.kind {
                    ItemKind::Def(binding, _) => Some(binding),
                    _ => None,
                })
        }

        fn entry_module_text(program: &Program) -> String {
            format!("{:?}", program.entry_module())
        }
    }

    #[test]
    fn a_session_resolves_a_name_to_the_definition_it_finishes_with() {
        let mut fixture = SessionFixture::new();
        let name = fixture.symbol(SessionFixture::DEFINED_NAME);
        let session = fixture.session();
        let binding = session.binding(&name);
        let program = SessionFixture::finished(session);
        assert!(binding.is_some());
        assert_eq!(binding, SessionFixture::defined_binding(&program));
    }

    #[test]
    fn a_session_resolves_no_binding_for_an_unbound_name() {
        let mut fixture = SessionFixture::new();
        let name = fixture.symbol(SessionFixture::UNBOUND_NAME);
        assert_eq!(fixture.session().binding(&name), None);
    }

    #[test]
    fn a_session_resolves_no_binding_for_a_list() {
        let mut fixture = SessionFixture::new();
        let list = fixture.list(SessionFixture::DEFINED_NAME);
        assert_eq!(fixture.session().binding(&list), None);
    }

    #[test]
    fn a_finished_session_holds_the_program_that_expansion_returns() {
        let finished = SessionFixture::finished(SessionFixture::new().session());
        let expanded = SessionFixture::new().expanded();
        assert_eq!(
            SessionFixture::entry_module_text(&finished),
            SessionFixture::entry_module_text(&expanded)
        );
    }
}
