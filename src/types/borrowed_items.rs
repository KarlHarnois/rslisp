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
        found: &Type,
        diagnostic: Diagnostic,
    ) -> Diagnostic {
        if *diagnostic.code() != DiagnosticCode::MISMATCHED_TYPES {
            return diagnostic;
        }
        match self.borrowing_help(site, found) {
            Some(help) => diagnostic.with_help(help),
            None => diagnostic,
        }
    }

    fn borrowing_help(&self, site: &ArgumentSite<'_>, found: &Type) -> Option<String> {
        let callee = site.callee?;
        let function = Self::called_function(callee)?;
        if self.parameter_is_written(function, site.position) {
            return None;
        }
        let signature = self.borrowing_signature(callee, function, site.position, found)?;
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
        function: BindingId,
        position: usize,
        found: &Type,
    ) -> Option<String> {
        let Type::Function(kind, mut parameters, result) = self.unifier.resolve(&callee.ty) else {
            return None;
        };
        let item = self.owned_iterator_item(parameters.get(position)?)?;
        if !self.yields_borrowed(found, &item) {
            return None;
        }
        let borrowed = self.iterator_of(Type::shared(item.clone()))?;
        self.borrow_owned_items(function, &mut parameters, &item, &borrowed);
        Some(
            self.display()
                .show(&Type::Function(kind, parameters, result)),
        )
    }

    fn borrow_owned_items(
        &self,
        function: BindingId,
        parameters: &mut [Type],
        item: &Type,
        borrowed: &Type,
    ) {
        for (position, parameter) in parameters.iter_mut().enumerate() {
            if !self.parameter_is_written(function, position)
                && self.owned_iterator_item(parameter).as_ref() == Some(item)
            {
                *parameter = borrowed.clone();
            }
        }
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

    fn yields_borrowed(&self, found: &Type, item: &Type) -> bool {
        match self.unifier.resolve(found) {
            Type::Named(binding, arguments) if self.rust_types.is_iterator(binding) => {
                matches!(arguments.as_slice(), [Type::Ref(_, borrowed)] if **borrowed == *item)
            }
            _ => false,
        }
    }

    fn iterator_of(&self, item: Type) -> Option<Type> {
        let iterator = self.rust_types.iterator_binding()?;
        Some(Type::Named(iterator, vec![item]))
    }
}
