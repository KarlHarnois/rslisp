use super::local_borrows::LocalBorrows;
use super::subtree::Subtree;
use crate::types::{Type, TypedBinder, TypedExpr, TypedExprKind, TypedProgram};

pub struct LocalFunctions<'p> {
    program: &'p TypedProgram,
}

impl<'p> LocalFunctions<'p> {
    pub fn new(program: &'p TypedProgram) -> Self {
        Self { program }
    }

    pub fn emits_as_function(&self, binder: &TypedBinder, closure: &TypedExpr) -> bool {
        !binder.annotated
            && self.admits(closure)
            && Self::lent_parameters(closure).iter().any(Option::is_some)
    }

    pub fn lent_parameters(closure: &TypedExpr) -> Vec<Option<&TypedBinder>> {
        let TypedExprKind::Fn(lambda) = &closure.kind else {
            return Vec::new();
        };
        let Some(tail) = LocalBorrows::tail_of(&lambda.body) else {
            return Vec::new();
        };
        lambda
            .params
            .iter()
            .map(|param| {
                LocalBorrows::lent_by(lambda, param.binding)
                    .returned(tail)
                    .map(|_| param)
            })
            .collect()
    }

    fn admits(&self, closure: &TypedExpr) -> bool {
        Subtree::first_capture(closure).is_none()
            && self.fits_signature(&closure.ty)
            && Self::avoids_outer_generics(closure)
    }

    fn fits_signature(&self, signature: &Type) -> bool {
        signature
            .parts()
            .into_iter()
            .all(|part| !part.any_part(&|inner| self.needs_closure(inner)))
    }

    fn needs_closure(&self, ty: &Type) -> bool {
        matches!(ty, Type::Function(..)) || self.is_opaque(ty)
    }

    fn is_opaque(&self, ty: &Type) -> bool {
        let Type::Named(binding, _) = ty else {
            return false;
        };
        self.program
            .rust_types
            .get(*binding)
            .is_some_and(|rust_type| rust_type.is_opaque())
    }

    fn avoids_outer_generics(closure: &TypedExpr) -> bool {
        Subtree::of(closure)
            .iter()
            .all(|expr| !expr.ty.any_part(&|part| matches!(part, Type::Generic(_))))
    }
}
