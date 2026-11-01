use rustc_hash::FxHashSet;

use super::plan::EmitPlan;
use super::types::RustTypes;
use crate::core::{BindingId, PrimitiveType};
use crate::types::{
    GenericBinding, GenericId, Type, TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf,
    TypedProgram,
};

pub struct UnsizedParameters {
    generic_ids: FxHashSet<GenericId>,
}

impl UnsizedParameters {
    pub fn of(program: &TypedProgram, plan: &EmitPlan<'_>) -> Self {
        let mut instances = Vec::new();
        for module in &program.modules {
            for definition in &module.definitions {
                if plan.item(definition.binding).is_none() {
                    continue;
                }
                Self::definition(plan, definition, &mut instances);
            }
        }
        Self {
            generic_ids: Self::solve(program, instances),
        }
    }

    pub fn contains(&self, generic: GenericId) -> bool {
        self.generic_ids.contains(&generic)
    }

    fn definition(
        plan: &EmitPlan<'_>,
        definition: &TypedDefinition,
        instances: &mut Vec<(GenericId, Type)>,
    ) {
        if matches!(definition.value.kind, TypedExprKind::Fn(_)) {
            Self::expr(plan, &definition.value, instances);
        }
    }

    fn expr(plan: &EmitPlan<'_>, expr: &TypedExpr, instances: &mut Vec<(GenericId, Type)>) {
        if let TypedExprKind::Leaf(TypedLeaf::Global(binding)) = &expr.kind {
            Self::instance(plan, *binding, &expr.ty, instances);
        }
        for child in expr.children() {
            Self::expr(plan, child, instances);
        }
    }

    fn instance(
        plan: &EmitPlan<'_>,
        binding: BindingId,
        actual: &Type,
        instances: &mut Vec<(GenericId, Type)>,
    ) {
        let Some(definition) = plan.definition(binding) else {
            return;
        };
        if !matches!(definition.value.kind, TypedExprKind::Fn(_)) {
            return;
        }
        instances.extend(GenericBinding::of(&definition.scheme.body, actual).into_bound());
    }

    fn solve(program: &TypedProgram, instances: Vec<(GenericId, Type)>) -> FxHashSet<GenericId> {
        let mut found = FxHashSet::default();
        loop {
            let mut changed = false;
            for (generic, actual) in &instances {
                if Self::is_unsized(program, actual, &found) && found.insert(*generic) {
                    changed = true;
                }
            }
            if !changed {
                return found;
            }
        }
    }

    fn is_unsized(program: &TypedProgram, ty: &Type, found: &FxHashSet<GenericId>) -> bool {
        match ty {
            Type::Primitive(PrimitiveType::Str) => true,
            Type::Named(binding, _) => {
                RustTypes::is_rust_type(program, *binding, RustTypes::SLICE_TYPE)
            }
            Type::Generic(generic) => found.contains(generic),
            _ => false,
        }
    }
}
