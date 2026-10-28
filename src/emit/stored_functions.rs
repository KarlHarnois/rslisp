use std::collections::HashSet;

use crate::core::{BindingId, BindingKind, FieldLabel, VariantInfo};
use crate::types::{
    Type, TypedExpr, TypedExprKind, TypedLambda, TypedLeaf, TypedProgram, TypedStructExpr,
};

pub struct StoredFunctions<'a> {
    program: &'a TypedProgram,
    parameters: HashSet<BindingId>,
    stored: HashSet<BindingId>,
}

impl<'a> StoredFunctions<'a> {
    pub fn of(program: &'a TypedProgram, lambda: &TypedLambda) -> Self {
        let mut found = Self {
            program,
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

    pub fn empty(program: &'a TypedProgram) -> Self {
        Self {
            program,
            parameters: HashSet::new(),
            stored: HashSet::new(),
        }
    }

    pub fn stores(&self, binding: BindingId) -> bool {
        self.stored.contains(&binding)
    }

    fn expr(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Struct(
                TypedStructExpr::New(binding, fields) | TypedStructExpr::Update(binding, _, fields),
            ) => self.struct_values(*binding, fields),
            TypedExprKind::Variant(binding, payloads) => self.variant_values(*binding, payloads),
            _ => {}
        }
        for child in expr.children() {
            self.expr(child);
        }
    }

    fn struct_values(&mut self, binding: BindingId, fields: &[(FieldLabel, TypedExpr)]) {
        let program = self.program;
        let definition = program.structs.get(binding);
        for (label, value) in fields {
            let declared = definition.and_then(|definition| definition.field(label.name.as_ref()));
            self.record(value, declared);
        }
    }

    fn variant_values(&mut self, binding: BindingId, payloads: &[TypedExpr]) {
        let program = self.program;
        let BindingKind::Variant(_, VariantInfo { owner, index, .. }) =
            program.bindings.get(binding).kind()
        else {
            return;
        };
        let Some(definition) = program.enums.get(owner) else {
            return;
        };
        let Some(variant) = definition.variants.get(index) else {
            return;
        };
        let declared = variant.payload_types();
        for (position, value) in payloads.iter().enumerate() {
            self.record(value, declared.get(position).copied());
        }
    }

    fn record(&mut self, value: &TypedExpr, declared: Option<&Type>) {
        if declared.is_some_and(|ty| matches!(ty, Type::Function(..)))
            && let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = value.kind
            && self.parameters.contains(&binding)
        {
            self.stored.insert(binding);
        }
    }
}
