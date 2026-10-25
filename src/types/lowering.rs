use std::collections::HashMap;

use super::checker::TypeChecker;
use super::problems::TypeProblems;
use super::traits::Implementor;
use super::ty::{Bound, GenericId, GenericParam, Lifetime, Scheme, Type};
use crate::core::ClosureKind;
use crate::core::{BindingId, BindingKind, ReferenceKind, TypeBinder, TypeExpr, TypeExprKind};
use crate::diagnostic::Diagnostic;
use crate::source::Span;

struct LoweredBinders {
    lifetimes: Vec<BindingId>,
    params: Vec<GenericParam>,
    scope: HashMap<BindingId, GenericId>,
}

impl TypeChecker {
    pub(super) fn lower_scheme(&mut self, type_expr: &TypeExpr) -> Result<Scheme, Diagnostic> {
        let TypeExprKind::Where(binders, inner) = type_expr.kind() else {
            return Ok(Scheme::monomorphic(
                self.lower_type(type_expr, &HashMap::new())?,
            ));
        };
        let binders = self.lower_binders(binders)?;
        Ok(Scheme {
            lifetimes: binders.lifetimes,
            params: binders.params,
            body: self.lower_type(inner, &binders.scope)?,
        })
    }

    pub(super) fn lower_implementor(
        &mut self,
        implementor: &TypeExpr,
        trait_name: &str,
    ) -> Result<Implementor, Diagnostic> {
        match implementor.kind() {
            TypeExprKind::Where(binders, inner) => {
                let binders = self.lower_binders(binders)?;
                let (body, argument) = self.lower_implementor_body(inner, &binders.scope)?;
                Self::reject_bare_implementor(&body, implementor.span(), trait_name)?;
                Ok(Implementor {
                    scheme: Scheme {
                        lifetimes: binders.lifetimes,
                        params: binders.params,
                        body,
                    },
                    argument,
                })
            }
            _ => {
                let (body, argument) = self.lower_implementor_body(implementor, &HashMap::new())?;
                Self::reject_bare_implementor(&body, implementor.span(), trait_name)?;
                Ok(Implementor {
                    scheme: Scheme::monomorphic(body),
                    argument,
                })
            }
        }
    }

    fn lower_implementor_body(
        &mut self,
        type_expr: &TypeExpr,
        scope: &HashMap<BindingId, GenericId>,
    ) -> Result<(Type, Option<Type>), Diagnostic> {
        if let TypeExprKind::With(implementor, argument) = type_expr.kind() {
            let body = self.lower_type(implementor, scope)?;
            let argument = self.lower_type(argument, scope)?;
            Ok((body, Some(argument)))
        } else {
            Ok((self.lower_type(type_expr, scope)?, None))
        }
    }

    fn reject_bare_implementor(
        body: &Type,
        span: Span,
        trait_name: &str,
    ) -> Result<(), Diagnostic> {
        if matches!(body, Type::Generic(_)) {
            return Err(TypeProblems::bare_implementor(trait_name, span));
        }
        Ok(())
    }

    fn lower_binders(&mut self, binders: &[TypeBinder]) -> Result<LoweredBinders, Diagnostic> {
        let mut lifetimes = Vec::new();
        let mut params = Vec::new();
        let mut scope = HashMap::new();
        for binder in binders {
            if binder.lifetime {
                if !binder.bounds.is_empty() {
                    return Err(TypeProblems::bounded_type_parameter(
                        self.bindings.get(binder.variable).name(),
                        binder.span,
                    ));
                }
                lifetimes.push(binder.variable);
                continue;
            }
            let name = self.bindings.get(binder.variable).name().to_string();
            let param = self.new_generic(&name, Vec::new());
            scope.insert(binder.variable, param.id);
            params.push(param);
        }
        for (binder, param) in binders
            .iter()
            .filter(|binder| !binder.lifetime)
            .zip(&mut params)
        {
            let bounds = self.lower_bounds(binder, &scope)?;
            self.replace_bounds(param, bounds);
        }
        Ok(LoweredBinders {
            lifetimes,
            params,
            scope,
        })
    }

    fn lower_bounds(
        &mut self,
        binder: &TypeBinder,
        scope: &HashMap<BindingId, GenericId>,
    ) -> Result<Vec<Bound>, Diagnostic> {
        binder
            .bounds
            .iter()
            .map(|bound| {
                let Some(argument) = &bound.argument else {
                    return Ok(Bound::plain(bound.binding));
                };
                self.check_trait_argument(bound.binding, argument.span())?;
                let argument = self.lower_type(argument, scope)?;
                Ok(Bound {
                    binding: bound.binding,
                    argument: Some(argument),
                })
            })
            .collect()
    }

    fn check_trait_argument(&self, binding: BindingId, span: Span) -> Result<(), Diagnostic> {
        if self.trait_takes_argument(binding) {
            return Ok(());
        }
        Err(TypeProblems::trait_takes_no_argument(
            self.bindings.get(binding).name(),
            span,
        ))
    }

    fn replace_bounds(&mut self, param: &mut GenericParam, bounds: Vec<Bound>) {
        param.bounds = bounds.clone();
        self.generics.insert(param.id, param.clone());
        self.unifier.register_generic(param.id, bounds);
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
            TypeExprKind::Function(kind, parameters, result) => {
                self.lower_function(*kind, parameters, result, scope)
            }
            TypeExprKind::Reference(kind, lifetime, inner) => {
                self.lower_reference(*kind, *lifetime, inner, scope)
            }
            TypeExprKind::Hole => Ok(self.unifier.fresh()),
            TypeExprKind::Where(..) => Err(TypeProblems::misplaced_where(type_expr.span())),
            TypeExprKind::With(..) => Err(TypeProblems::misplaced_with(type_expr.span())),
        }
    }

    fn lower_function(
        &mut self,
        kind: ClosureKind,
        parameters: &[TypeExpr],
        result: &TypeExpr,
        scope: &HashMap<BindingId, GenericId>,
    ) -> Result<Type, Diagnostic> {
        let parameters = parameters
            .iter()
            .map(|parameter| self.lower_type(parameter, scope))
            .collect::<Result<Vec<Type>, Diagnostic>>()?;
        let result = self.lower_type(result, scope)?;
        Ok(Type::Function(kind, parameters, Box::new(result)))
    }

    fn lower_reference(
        &mut self,
        kind: ReferenceKind,
        lifetime: Option<BindingId>,
        inner: &TypeExpr,
        scope: &HashMap<BindingId, GenericId>,
    ) -> Result<Type, Diagnostic> {
        let lifetime = lifetime.map_or(Lifetime::Elided, Lifetime::written);
        let inner = Box::new(self.lower_type(inner, scope)?);
        Ok(match kind {
            ReferenceKind::Shared => Type::Ref(lifetime, inner),
            ReferenceKind::Mutable => Type::Mut(lifetime, inner),
        })
    }

    pub(super) fn instantiate(&mut self, scheme: &Scheme) -> Type {
        let fresh: Vec<Type> = scheme.params.iter().map(|_| self.unifier.fresh()).collect();
        let replacements: HashMap<GenericId, Type> = scheme
            .params
            .iter()
            .zip(&fresh)
            .map(|(param, value)| (param.id, value.clone()))
            .collect();
        for (param, value) in scheme.params.iter().zip(&fresh) {
            if let Type::Var(var) = value {
                let bounds = param
                    .bounds
                    .iter()
                    .map(|bound| bound.substituted(&replacements))
                    .collect();
                self.unifier.set_bounds(*var, bounds);
            }
        }
        scheme.body.substitute(&replacements)
    }

    fn lower_named(
        &mut self,
        binding: BindingId,
        arguments: &[TypeExpr],
        span: Span,
        scope: &HashMap<BindingId, GenericId>,
    ) -> Result<Type, Diagnostic> {
        self.check_type_argument_count(binding, arguments.len(), span)?;
        let arguments = arguments
            .iter()
            .map(|argument| self.lower_type(argument, scope))
            .collect::<Result<Vec<Type>, Diagnostic>>()?;
        Ok(
            match (self.bindings.get(binding).kind(), scope.get(&binding)) {
                (BindingKind::TypeVariable, Some(generic)) => Type::Generic(*generic),
                (BindingKind::TypeVariable, None) => self.unifier.fresh(),
                _ => Type::Named(binding, arguments),
            },
        )
    }

    fn check_type_argument_count(
        &self,
        binding: BindingId,
        found: usize,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let expected = self.type_arities.get(&binding).copied().unwrap_or_default();
        if found == expected {
            return Ok(());
        }
        Err(TypeProblems::wrong_type_argument_count(
            self.bindings.get(binding).name(),
            expected,
            found,
            span,
        ))
    }
}
