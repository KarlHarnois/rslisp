use std::collections::HashMap;

use super::checker::TypeChecker;
use super::problems::TypeProblems;
use super::ty::{GenericId, Scheme, Type};
use crate::core::{BindingId, BindingKind, ReferenceKind, TypeExpr, TypeExprKind};
use crate::diagnostic::Diagnostic;
use crate::source::Span;

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
        match type_expr.kind() {
            TypeExprKind::Primitive(primitive) => Ok(Type::Primitive(*primitive)),
            TypeExprKind::Named(binding, arguments) => {
                self.lower_named(*binding, arguments, type_expr.span(), scope)
            }
            TypeExprKind::Function(parameters, result) => {
                self.lower_function(parameters, result, scope)
            }
            TypeExprKind::Reference(kind, inner) => self.lower_reference(*kind, inner, scope),
            TypeExprKind::Hole => Ok(self.unifier.fresh()),
            TypeExprKind::Where(..) => Err(TypeProblems::misplaced_where(type_expr.span())),
        }
    }

    fn lower_function(
        &mut self,
        parameters: &[TypeExpr],
        result: &TypeExpr,
        scope: &HashMap<BindingId, GenericId>,
    ) -> Result<Type, Diagnostic> {
        let parameters = parameters
            .iter()
            .map(|parameter| self.lower_type(parameter, scope))
            .collect::<Result<Vec<Type>, Diagnostic>>()?;
        let result = self.lower_type(result, scope)?;
        Ok(Type::Function(parameters, Box::new(result)))
    }

    fn lower_reference(
        &mut self,
        kind: ReferenceKind,
        inner: &TypeExpr,
        scope: &HashMap<BindingId, GenericId>,
    ) -> Result<Type, Diagnostic> {
        let inner = Box::new(self.lower_type(inner, scope)?);
        Ok(match kind {
            ReferenceKind::Shared => Type::Ref(inner),
            ReferenceKind::Mutable => Type::Mut(inner),
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

    fn lower_named(
        &mut self,
        binding: BindingId,
        arguments: &[TypeExpr],
        span: Span,
        scope: &HashMap<BindingId, GenericId>,
    ) -> Result<Type, Diagnostic> {
        if !arguments.is_empty() {
            return Err(TypeProblems::no_type_arguments(span));
        }
        Ok(
            match (self.bindings.get(binding).kind(), scope.get(&binding)) {
                (BindingKind::TypeVariable, Some(generic)) => Type::Generic(*generic),
                (BindingKind::TypeVariable, None) => self.unifier.fresh(),
                _ => Type::Struct(binding),
            },
        )
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
