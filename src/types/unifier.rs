use std::collections::HashMap;

use super::ty::{Bound, GenericId, Type, TypeVar};
use crate::core::PrimitiveType;
use crate::source::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiteralKind {
    Integer,
    Float,
}

impl LiteralKind {
    fn accepts(self, primitive: PrimitiveType) -> bool {
        match self {
            LiteralKind::Integer => primitive.is_integer() || primitive.is_float(),
            LiteralKind::Float => primitive.is_float(),
        }
    }

    fn merge(left: Option<LiteralKind>, right: Option<LiteralKind>) -> Option<LiteralKind> {
        match (left, right) {
            (Some(LiteralKind::Float), _) | (_, Some(LiteralKind::Float)) => {
                Some(LiteralKind::Float)
            }
            (Some(LiteralKind::Integer), _) | (_, Some(LiteralKind::Integer)) => {
                Some(LiteralKind::Integer)
            }
            (None, None) => None,
        }
    }
}

pub enum UnifyError {
    Mismatch,
    Infinite,
    NotNumeric,
}

pub struct Obligation {
    pub bound: Bound,
    pub implementor: Type,
    pub span: Span,
}

#[derive(Default)]
struct VarState {
    binding: Option<Type>,
    literal: Option<LiteralKind>,
    bounds: Vec<Bound>,
}

#[derive(Default)]
pub struct Unifier {
    vars: Vec<VarState>,
    obligations: Vec<Obligation>,
    generic_bounds: HashMap<GenericId, Vec<Bound>>,
}

impl Unifier {
    pub fn fresh(&mut self) -> Type {
        self.fresh_with(None, Vec::new())
    }

    pub fn fresh_literal(&mut self, kind: LiteralKind) -> Type {
        self.fresh_with(Some(kind), Vec::new())
    }

    pub fn register_generic(&mut self, generic: GenericId, bounds: Vec<Bound>) {
        self.generic_bounds.insert(generic, bounds);
    }

    pub fn generic_bounds(&self, generic: GenericId) -> &[Bound] {
        self.generic_bounds
            .get(&generic)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn literal_kind(&self, var: TypeVar) -> Option<LiteralKind> {
        self.vars[var.index()].literal
    }

    pub fn bounds(&self, var: TypeVar) -> &[Bound] {
        &self.vars[var.index()].bounds
    }

    pub fn set_bounds(&mut self, var: TypeVar, bounds: Vec<Bound>) {
        self.vars[var.index()].bounds = bounds;
    }

    pub fn add_bound(&mut self, var: TypeVar, bound: Bound) {
        let bounds = &mut self.vars[var.index()].bounds;
        if !bounds.contains(&bound) {
            bounds.push(bound);
        }
    }

    pub fn bind(&mut self, var: TypeVar, value: Type) {
        self.vars[var.index()].binding = Some(value);
    }

    pub fn defer(&mut self, obligation: Obligation) {
        self.obligations.push(obligation);
    }

    pub fn take_obligations(&mut self) -> Vec<Obligation> {
        std::mem::take(&mut self.obligations)
    }

    pub fn resolve(&self, value: &Type) -> Type {
        match self.shallow(value) {
            Type::Named(binding, arguments) => Type::Named(binding, self.resolve_all(&arguments)),
            Type::Function(kind, parameters, result) => Type::Function(
                kind,
                self.resolve_all(&parameters),
                Box::new(self.resolve(&result)),
            ),
            Type::Ref(lifetime, inner) => Type::Ref(lifetime, Box::new(self.resolve(&inner))),
            Type::Mut(lifetime, inner) => Type::Mut(lifetime, Box::new(self.resolve(&inner))),
            other => other,
        }
    }

    fn resolve_all(&self, values: &[Type]) -> Vec<Type> {
        values.iter().map(|value| self.resolve(value)).collect()
    }

    pub fn shallow(&self, value: &Type) -> Type {
        let mut current = value.clone();
        while let Type::Var(var) = current {
            match &self.vars[var.index()].binding {
                Some(bound) => current = bound.clone(),
                None => return current,
            }
        }
        current
    }

    pub fn unify(&mut self, expected: &Type, found: &Type, span: Span) -> Result<(), UnifyError> {
        match (self.shallow(expected), self.shallow(found)) {
            (Type::Var(left), Type::Var(right)) => {
                self.unify_vars(left, right);
                Ok(())
            }
            (Type::Var(var), other) | (other, Type::Var(var)) => {
                self.bind_checked(var, other, span)
            }
            (
                Type::Function(_, left_parameters, left_result),
                Type::Function(_, right_parameters, right_result),
            ) => self.unify_functions(
                &left_parameters,
                &left_result,
                &right_parameters,
                &right_result,
                span,
            ),
            (Type::Ref(_, left), Type::Ref(_, right))
            | (Type::Mut(_, left), Type::Mut(_, right)) => self.unify(&left, &right, span),
            (left, right) => self.unify_rigid(&left, &right, span),
        }
    }

    fn unify_rigid(&mut self, left: &Type, right: &Type, span: Span) -> Result<(), UnifyError> {
        match (left, right) {
            (Type::Named(left, left_arguments), Type::Named(right, right_arguments))
                if left == right =>
            {
                self.unify_pairwise(left_arguments, right_arguments, span)
            }
            _ if left == right => Ok(()),
            _ => Err(UnifyError::Mismatch),
        }
    }

    fn unify_pairwise(
        &mut self,
        left: &[Type],
        right: &[Type],
        span: Span,
    ) -> Result<(), UnifyError> {
        if left.len() != right.len() {
            return Err(UnifyError::Mismatch);
        }
        for (left_part, right_part) in left.iter().zip(right) {
            self.unify(left_part, right_part, span)?;
        }
        Ok(())
    }

    fn unify_vars(&mut self, left: TypeVar, right: TypeVar) {
        if left != right {
            self.merge_vars(left, right);
        }
    }

    fn unify_functions(
        &mut self,
        left: &[Type],
        left_result: &Type,
        right: &[Type],
        right_result: &Type,
        span: Span,
    ) -> Result<(), UnifyError> {
        self.unify_pairwise(left, right, span)?;
        self.unify(left_result, right_result, span)
    }

    fn fresh_with(&mut self, literal: Option<LiteralKind>, bounds: Vec<Bound>) -> Type {
        let var = TypeVar::new(self.vars.len());
        self.vars.push(VarState {
            binding: None,
            literal,
            bounds,
        });
        Type::Var(var)
    }

    fn merge_vars(&mut self, from: TypeVar, into: TypeVar) {
        let moved_bounds = std::mem::take(&mut self.vars[from.index()].bounds);
        let literal = LiteralKind::merge(
            self.vars[from.index()].literal,
            self.vars[into.index()].literal,
        );
        self.vars[into.index()].literal = literal;
        moved_bounds
            .into_iter()
            .for_each(|bound| self.add_bound(into, bound));
        self.bind(from, Type::Var(into));
    }

    fn bind_checked(&mut self, var: TypeVar, value: Type, span: Span) -> Result<(), UnifyError> {
        if value.contains_var(var) {
            return Err(UnifyError::Infinite);
        }
        if let Some(literal) = self.vars[var.index()].literal {
            self.check_literal(literal, &value)?;
        }
        for bound in self.vars[var.index()].bounds.clone() {
            self.obligations.push(Obligation {
                bound,
                implementor: value.clone(),
                span,
            });
        }
        self.bind(var, value);
        Ok(())
    }

    fn check_literal(&self, literal: LiteralKind, value: &Type) -> Result<(), UnifyError> {
        match value {
            Type::Primitive(primitive) if literal.accepts(*primitive) => Ok(()),
            Type::Generic(_) => Ok(()),
            _ => Err(UnifyError::NotNumeric),
        }
    }
}
