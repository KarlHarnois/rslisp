use crate::reader::Form;
use crate::scope::Scope;

#[derive(Default)]
pub struct DefinitionContext {
    use_site_scopes: Vec<Scope>,
}

impl DefinitionContext {
    pub fn record_use_site(&mut self, scope: Scope) {
        self.use_site_scopes.push(scope);
    }

    pub fn strip_use_sites(&self, binder: &mut Form) {
        self.use_site_scopes
            .iter()
            .for_each(|scope| binder.remove_scope(*scope));
    }
}
