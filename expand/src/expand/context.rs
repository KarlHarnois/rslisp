use crate::reader::Form;
use crate::scope::Scope;

#[derive(Default)]
pub struct DefinitionContext {
    ascending_use_site_scopes: Vec<Scope>,
}

impl DefinitionContext {
    pub fn record_fresh_use_site(&mut self, scope: Scope) {
        debug_assert!(self.ascending_use_site_scopes.last() < Some(&scope));
        self.ascending_use_site_scopes.push(scope);
    }

    pub fn strip_use_sites(&self, binder: &mut Form) {
        if !self.ascending_use_site_scopes.is_empty() {
            binder.retain_scopes(&|scope| !self.records_use_site(scope));
        }
    }

    fn records_use_site(&self, scope: Scope) -> bool {
        self.ascending_use_site_scopes.binary_search(&scope).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::DefinitionContext;
    use crate::reader::{Form, FormKind, Symbol};
    use crate::scope::{Scope, ScopeAllocator, ScopeSet};
    use crate::source::{SourceId, Span};

    struct ContextFixture {
        scopes: ScopeAllocator,
        context: DefinitionContext,
    }

    impl ContextFixture {
        fn new() -> Self {
            Self {
                scopes: ScopeAllocator::default(),
                context: DefinitionContext::default(),
            }
        }

        fn use_site(&mut self) -> Scope {
            let scope = self.scopes.fresh();
            self.context.record_fresh_use_site(scope);
            scope
        }

        fn stripped_binder(&self, scopes: &[Scope]) -> Form {
            let mut set = ScopeSet::default();
            scopes.iter().for_each(|scope| set.add(*scope));
            let symbol = Symbol::with_scopes("binder", set);
            let mut binder = Form::new(FormKind::Symbol(symbol), Span::new(SourceId::new(0), 0..0));
            self.context.strip_use_sites(&mut binder);
            binder
        }

        fn carries(binder: &Form, scope: Scope) -> bool {
            binder
                .as_symbol()
                .is_some_and(|symbol| symbol.scopes().contains(scope))
        }
    }

    #[test]
    fn strips_a_recorded_use_site_from_a_binder() {
        let mut fixture = ContextFixture::new();
        let module = fixture.scopes.fresh();
        let use_site = fixture.use_site();
        let binder = fixture.stripped_binder(&[module, use_site]);
        assert!(!ContextFixture::carries(&binder, use_site));
    }

    #[test]
    fn keeps_the_scopes_that_no_use_site_recorded() {
        let mut fixture = ContextFixture::new();
        let module = fixture.scopes.fresh();
        let use_site = fixture.use_site();
        let binder = fixture.stripped_binder(&[module, use_site]);
        assert!(ContextFixture::carries(&binder, module));
    }
}
