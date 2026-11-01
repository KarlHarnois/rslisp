use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::generic_positions::GenericPositions;
use super::plan::EmitPlan;
use crate::core::BindingId;

pub(super) struct GenericCallees<'a> {
    plan: &'a EmitPlan<'a>,
    positions: RefCell<HashMap<BindingId, Option<Rc<GenericPositions>>>>,
}

impl<'a> GenericCallees<'a> {
    pub(super) fn new(plan: &'a EmitPlan<'a>) -> Self {
        Self {
            plan,
            positions: RefCell::new(HashMap::new()),
        }
    }

    pub(super) fn positions_of(&self, callee: BindingId) -> Option<Rc<GenericPositions>> {
        let scheme = &self.plan.definition(callee)?.scheme;
        if scheme.params.is_empty() {
            return None;
        }
        self.positions
            .borrow_mut()
            .entry(callee)
            .or_insert_with(|| {
                GenericPositions::of_function(&scheme.params, &scheme.body).map(Rc::new)
            })
            .clone()
    }
}
