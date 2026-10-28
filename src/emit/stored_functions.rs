use std::collections::HashSet;

use crate::core::BindingId;
use crate::types::{Type, TypedExpr, TypedExprKind, TypedLambda, TypedLeaf, TypedStructExpr};

#[derive(Default)]
pub struct StoredFunctions {
    parameters: HashSet<BindingId>,
    stored: HashSet<BindingId>,
}

impl StoredFunctions {
    pub fn of(lambda: &TypedLambda) -> Self {
        let mut found = Self {
            parameters: lambda
                .params
                .iter()
                .filter(|param| matches!(param.ty, Type::Function(..)))
                .map(|param| param.binding)
                .collect(),
            stored: HashSet::new(),
        };
        for expr in lambda.body.expressions() {
            found.expr(expr);
        }
        found
    }

    pub fn stores(&self, binding: BindingId) -> bool {
        self.stored.contains(&binding)
    }

    fn expr(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Struct(
                TypedStructExpr::New(_, fields) | TypedStructExpr::Update(_, _, fields),
            ) => self.values(fields.iter().map(|(_, value)| value)),
            TypedExprKind::Variant(_, payloads) => self.values(payloads.iter()),
            _ => {}
        }
        for child in expr.children() {
            self.expr(child);
        }
    }

    fn values<'b>(&mut self, values: impl Iterator<Item = &'b TypedExpr>) {
        for value in values {
            if let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = value.kind
                && self.parameters.contains(&binding)
            {
                self.stored.insert(binding);
            }
        }
    }
}
