use std::collections::{HashMap, HashSet};

use super::plan::EmitPlan;
use super::types::RustTypes;
use crate::core::{BindingId, PrimitiveType};
use crate::types::{
    GenericId, Type, TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf, TypedProgram,
};

pub struct UnsizedParameters {
    generic_ids: HashSet<GenericId>,
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
        let mut bound = HashMap::new();
        if Self::bind(&definition.scheme.body, actual, &mut bound) {
            instances.extend(bound);
        }
    }

    fn bind(scheme: &Type, actual: &Type, bound: &mut HashMap<GenericId, Type>) -> bool {
        match scheme {
            Type::Generic(generic) => {
                bound.insert(*generic, actual.clone());
                true
            }
            Type::Ref(inner) | Type::Mut(inner) => Self::bind_reference(inner, actual, bound),
            _ => Self::bind_compound(scheme, actual, bound),
        }
    }

    fn bind_reference(inner: &Type, actual: &Type, bound: &mut HashMap<GenericId, Type>) -> bool {
        let actual = match actual {
            Type::Ref(actual) | Type::Mut(actual) => actual,
            actual => actual,
        };
        Self::bind(inner, actual, bound)
    }

    fn bind_compound(scheme: &Type, actual: &Type, bound: &mut HashMap<GenericId, Type>) -> bool {
        match (scheme, actual) {
            (Type::Named(scheme, scheme_parts), Type::Named(actual, actual_parts))
                if scheme == actual && scheme_parts.len() == actual_parts.len() =>
            {
                Self::bind_all(scheme_parts, actual_parts, bound)
            }
            (
                Type::Function(_, scheme_parameters, scheme_result),
                Type::Function(_, actual_parameters, actual_result),
            ) if scheme_parameters.len() == actual_parameters.len() => {
                Self::bind_all(scheme_parameters, actual_parameters, bound)
                    && Self::bind(scheme_result, actual_result, bound)
            }
            (Type::Primitive(scheme), Type::Primitive(actual)) => scheme == actual,
            _ => false,
        }
    }

    fn bind_all(scheme: &[Type], actual: &[Type], bound: &mut HashMap<GenericId, Type>) -> bool {
        scheme
            .iter()
            .zip(actual)
            .all(|(scheme, actual)| Self::bind(scheme, actual, bound))
    }

    fn solve(program: &TypedProgram, instances: Vec<(GenericId, Type)>) -> HashSet<GenericId> {
        let mut found = HashSet::new();
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

    fn is_unsized(program: &TypedProgram, ty: &Type, found: &HashSet<GenericId>) -> bool {
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
