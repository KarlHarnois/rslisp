use crate::reader::Form;
use crate::scope::Scope;

pub struct FormBudget {
    remaining: usize,
}

impl FormBudget {
    pub const EXPANSION_LIMIT: usize = 131_072;

    pub fn full() -> Self {
        Self {
            remaining: Self::EXPANSION_LIMIT,
        }
    }

    pub fn flip_scope(&mut self, form: &mut Form, scope: Scope) -> bool {
        form.flip_scope_within(scope, &mut self.remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::{FormKind, Symbol};
    use crate::scope::ScopeAllocator;
    use crate::source::{SourceId, Span};

    struct BudgetFixture {
        budget: FormBudget,
        scopes: ScopeAllocator,
    }

    impl BudgetFixture {
        fn new() -> Self {
            Self {
                budget: FormBudget::full(),
                scopes: ScopeAllocator::default(),
            }
        }

        fn admits_list_of(&mut self, forms: usize) -> bool {
            let mut list = Self::list_of(forms);
            let scope = self.scopes.fresh();
            self.budget.flip_scope(&mut list, scope)
        }

        fn list_of(forms: usize) -> Form {
            let span = Span::new(SourceId::new(0), 0..0);
            let leaf = Form::new(FormKind::Symbol(Symbol::new("leaf")), span);
            let items = vec![leaf; forms - 1];
            Form::new(FormKind::List(items), span)
        }
    }

    #[test]
    fn admits_an_expansion_of_exactly_the_limit() {
        let mut fixture = BudgetFixture::new();
        assert!(fixture.admits_list_of(FormBudget::EXPANSION_LIMIT));
    }

    #[test]
    fn rejects_an_expansion_one_form_past_the_limit() {
        let mut fixture = BudgetFixture::new();
        assert!(!fixture.admits_list_of(FormBudget::EXPANSION_LIMIT + 1));
    }

    #[test]
    fn counts_the_forms_of_every_expansion_against_one_limit() {
        let mut fixture = BudgetFixture::new();
        let half = FormBudget::EXPANSION_LIMIT / 2;
        assert!(fixture.admits_list_of(half));
        assert!(fixture.admits_list_of(half));
        assert!(!fixture.admits_list_of(1));
    }
}
