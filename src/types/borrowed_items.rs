use super::checker::TypeChecker;
use super::problems::TypeProblems;
use super::ty::Type;
use super::typed::{TypedExpr, TypedExprKind, TypedLeaf};
use crate::core::BindingId;
use crate::diagnostic::{Diagnostic, DiagnosticCode};

pub(super) struct ArgumentSite<'a> {
    pub callee: Option<&'a TypedExpr>,
    pub position: usize,
}

impl TypeChecker {
    pub(super) fn explain_borrowed_items(
        &self,
        site: &ArgumentSite<'_>,
        collection: &Type,
        diagnostic: Diagnostic,
    ) -> Diagnostic {
        if *diagnostic.code() != DiagnosticCode::MISMATCHED_TYPES {
            return diagnostic;
        }
        match self.borrowing_help(site, collection) {
            Some(help) => diagnostic.with_help(help),
            None => diagnostic,
        }
    }

    fn borrowing_help(&self, site: &ArgumentSite<'_>, collection: &Type) -> Option<String> {
        let callee = site.callee?;
        let function = Self::called_function(callee)?;
        if self.parameter_is_written(function, site.position) {
            return None;
        }
        let signature = self.borrowing_signature(callee, site.position, collection)?;
        let name = self.bindings.get(function).name();
        Some(TypeProblems::borrowed_items_help(name, &signature))
    }

    fn called_function(callee: &TypedExpr) -> Option<BindingId> {
        match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => Some(*binding),
            _ => None,
        }
    }

    fn borrowing_signature(
        &self,
        callee: &TypedExpr,
        position: usize,
        collection: &Type,
    ) -> Option<String> {
        let Type::Function(kind, mut parameters, result) = self.unifier.resolve(&callee.ty) else {
            return None;
        };
        let parameter = parameters.get_mut(position)?;
        let item = self.owned_iterator_item(parameter)?;
        if !self.holds_elements(collection, &item) {
            return None;
        }
        *parameter = self.iterator_of(Type::shared(item))?;
        Some(
            self.display()
                .show(&Type::Function(kind, parameters, result)),
        )
    }

    fn owned_iterator_item(&self, parameter: &Type) -> Option<Type> {
        let Type::Named(binding, arguments) = parameter else {
            return None;
        };
        match arguments.as_slice() {
            [item] if self.rust_types.is_iterator(*binding) && !item.is_reference() => {
                Some(item.clone())
            }
            _ => None,
        }
    }

    fn holds_elements(&self, collection: &Type, item: &Type) -> bool {
        match self.unifier.resolve(collection) {
            Type::Named(binding, elements) if self.rust_types.is_vector(binding) => {
                elements.as_slice() == std::slice::from_ref(item)
            }
            _ => false,
        }
    }

    fn iterator_of(&self, item: Type) -> Option<Type> {
        let iterator = self.rust_types.iterator_binding()?;
        Some(Type::Named(iterator, vec![item]))
    }
}
