use std::collections::HashMap;

use super::checker::TypeChecker;
use super::problems::TypeProblems;
use super::ty::{GenericId, Scheme, Type};
use crate::core::{BindingId, BindingKind, TypeExpr, TypeExprKind};
use crate::diagnostic::Diagnostic;

impl TypeChecker {
    pub(super) fn lower_scheme(&mut self, type_expr: &TypeExpr) -> Result<Scheme, Diagnostic> {
        let TypeExprKind::Where(binders, inner) = type_expr.kind() else {
            return Ok(Scheme::monomorphic(
                self.lower_type(type_expr, &HashMap::new())?,
            ));
        };
        let mut scope = HashMap::new();
        let mut params = Vec::new();
        for binder in binders {
            let name = self.bindings.get(binder.variable).name().to_string();
            let param = self.new_generic(&name, binder.bounds.clone());
            scope.insert(binder.variable, param.id);
            params.push(param);
        }
        Ok(Scheme {
            params,
            body: self.lower_type(inner, &scope)?,
        })
    }

    pub(super) fn lower_type(
        &mut self,
        type_expr: &TypeExpr,
        scope: &HashMap<BindingId, GenericId>,
    ) -> Result<Type, Diagnostic> {
        Ok(match type_expr.kind() {
            TypeExprKind::Primitive(primitive) => Type::Primitive(*primitive),
            TypeExprKind::Named(binding) => self.lower_named(*binding, scope),
            TypeExprKind::Apply(..) => {
                return Err(TypeProblems::no_type_arguments(type_expr.span()));
            }
            TypeExprKind::Function(parameters, result) => Type::Function(
                parameters
                    .iter()
                    .map(|parameter| self.lower_type(parameter, scope))
                    .collect::<Result<Vec<Type>, Diagnostic>>()?,
                Box::new(self.lower_type(result, scope)?),
            ),
            TypeExprKind::Ref(inner) => Type::Ref(Box::new(self.lower_type(inner, scope)?)),
            TypeExprKind::Mut(inner) => Type::Mut(Box::new(self.lower_type(inner, scope)?)),
            TypeExprKind::Hole => self.unifier.fresh(),
            TypeExprKind::Where(..) => return Err(TypeProblems::misplaced_where(type_expr.span())),
        })
    }

    pub(super) fn instantiate(&mut self, scheme: &Scheme) -> Type {
        let replacements: HashMap<GenericId, Type> = scheme
            .params
            .iter()
            .map(|param| (param.id, self.unifier.fresh_bounded(param.bounds.clone())))
            .collect();
        Self::substitute(&scheme.body, &replacements)
    }

    fn lower_named(&mut self, binding: BindingId, scope: &HashMap<BindingId, GenericId>) -> Type {
        match (self.bindings.get(binding).kind(), scope.get(&binding)) {
            (BindingKind::TypeVariable, Some(generic)) => Type::Generic(*generic),
            (BindingKind::TypeVariable, None) => self.unifier.fresh(),
            _ => Type::Struct(binding),
        }
    }

    fn substitute(body: &Type, replacements: &HashMap<GenericId, Type>) -> Type {
        match body {
            Type::Generic(generic) => replacements
                .get(generic)
                .cloned()
                .unwrap_or_else(|| body.clone()),
            Type::Function(parameters, result) => Type::Function(
                parameters
                    .iter()
                    .map(|parameter| Self::substitute(parameter, replacements))
                    .collect(),
                Box::new(Self::substitute(result, replacements)),
            ),
            Type::Ref(inner) => Type::Ref(Box::new(Self::substitute(inner, replacements))),
            Type::Mut(inner) => Type::Mut(Box::new(Self::substitute(inner, replacements))),
            Type::Var(_) | Type::Primitive(_) | Type::Struct(_) => body.clone(),
        }
    }
}
