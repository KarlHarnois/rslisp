use super::form_budget::FormBudget;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BudgetId(usize);

pub struct BudgetArena {
    budgets: Vec<FormBudget>,
    spending: BudgetId,
}

impl BudgetArena {
    pub fn open(&mut self) -> BudgetId {
        let id = BudgetId(self.budgets.len());
        self.budgets.push(FormBudget::full());
        id
    }

    pub fn spending(&self) -> BudgetId {
        self.spending
    }

    pub fn spend_from(&mut self, budget: BudgetId) {
        self.spending = budget;
    }

    pub fn current(&mut self) -> &mut FormBudget {
        &mut self.budgets[self.spending.0]
    }
}

impl Default for BudgetArena {
    fn default() -> Self {
        Self {
            budgets: vec![FormBudget::full()],
            spending: BudgetId(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spends_from_the_budget_it_switched_to() {
        let mut arena = BudgetArena::default();
        let opened = arena.open();
        arena.spend_from(opened);
        assert_eq!(arena.spending(), opened);
    }

    #[test]
    fn keeps_each_opened_budget_apart() {
        let mut arena = BudgetArena::default();
        let exhausted = arena.open();
        let untouched = arena.open();
        arena.spend_from(exhausted);
        arena.current().exhaust();
        arena.spend_from(untouched);
        assert!(!arena.current().is_exhausted());
        arena.spend_from(exhausted);
        assert!(arena.current().is_exhausted());
    }
}
