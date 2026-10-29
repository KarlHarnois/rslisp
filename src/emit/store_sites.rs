use super::result_borrows::ResultBorrows;
use super::stored_arguments::{Store, StoredArguments};
use crate::types::{TypedArgument, TypedExpr, TypedExprKind, TypedLambda, TypedLeaf};

pub(super) struct StoreSite<'e> {
    pub target: &'e TypedExpr,
    pub value: &'e TypedExpr,
}

pub(super) struct StoreSites<'r, 'a, 'e> {
    rules: &'r ResultBorrows<'a>,
    sites: Vec<StoreSite<'e>>,
}

impl<'r, 'a, 'e> StoreSites<'r, 'a, 'e> {
    pub(super) fn of(rules: &'r ResultBorrows<'a>, lambda: &'e TypedLambda) -> Vec<StoreSite<'e>> {
        let mut found = Self {
            rules,
            sites: Vec::new(),
        };
        for expr in lambda.body.expressions() {
            found.expr(expr);
        }
        found.sites
    }

    fn expr(&mut self, expr: &'e TypedExpr) {
        if let TypedExprKind::Call(callee, arguments) = &expr.kind {
            self.call(callee, arguments);
        }
        for child in expr.children() {
            self.expr(child);
        }
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &'e [TypedArgument]) {
        for store in self.stores_of(callee) {
            self.site(&store, arguments);
        }
    }

    fn stores_of(&self, callee: &TypedExpr) -> Vec<Store> {
        let mut stores = StoredArguments::of(&callee.ty).stores(self.rules.program());
        if let TypedExprKind::Leaf(TypedLeaf::Global(binding)) = callee.kind {
            stores.extend(
                self.rules
                    .definition_stores(binding)
                    .positions
                    .iter()
                    .cloned(),
            );
        }
        stores
    }

    fn site(&mut self, store: &Store, arguments: &'e [TypedArgument]) {
        let (Some(target), Some(value)) = (arguments.get(store.target), arguments.get(store.value))
        else {
            return;
        };
        if self.rules.carries_borrow(&value.value.ty) {
            self.sites.push(StoreSite {
                target: &target.value,
                value: &value.value,
            });
        }
    }
}
