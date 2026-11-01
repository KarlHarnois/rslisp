use crate::interpret::Value;
use crate::reader::Form;
use crate::scope::Scope;

#[derive(Clone)]
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

    pub fn admitted_form(&self, value: &Value) -> Option<Form> {
        let mut allowance = self.remaining;
        value.to_form_within(&mut allowance)
    }

    pub fn flip_scope(&mut self, form: &mut Form, scope: Scope) -> bool {
        form.flip_scope_within(scope, &mut self.remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpret::CodeList;
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

        fn admits_value(&self, value: &Value) -> bool {
            self.budget.admitted_form(value).is_some()
        }

        fn list_of(forms: usize) -> Form {
            let items = vec![Self::leaf(); forms - 1];
            Form::new(FormKind::List(items), Self::span())
        }

        fn code_list(items: Vec<Value>) -> Value {
            Value::List(CodeList::new(items, Self::span()))
        }

        fn leaf() -> Form {
            Form::new(FormKind::Symbol(Symbol::new("leaf")), Self::span())
        }

        fn span() -> Span {
            Span::new(SourceId::new(0), 0..0)
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

    #[test]
    fn admits_a_converted_list_of_exactly_the_limit() {
        let leaves = vec![Value::Form(BudgetFixture::leaf()); FormBudget::EXPANSION_LIMIT - 1];
        let list = BudgetFixture::code_list(leaves);
        assert!(BudgetFixture::new().admits_value(&list));
    }

    #[test]
    fn rejects_a_converted_list_whose_leaves_pass_the_limit() {
        let leaves = vec![Value::Form(BudgetFixture::leaf()); FormBudget::EXPANSION_LIMIT];
        let list = BudgetFixture::code_list(leaves);
        assert!(!BudgetFixture::new().admits_value(&list));
    }

    #[test]
    fn rejects_a_converted_list_whose_inner_lists_pass_the_limit() {
        let empty_lists = vec![BudgetFixture::code_list(Vec::new()); FormBudget::EXPANSION_LIMIT];
        let list = BudgetFixture::code_list(empty_lists);
        assert!(!BudgetFixture::new().admits_value(&list));
    }
}
