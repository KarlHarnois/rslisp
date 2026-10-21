use super::Expander;
use super::problems::ExpandProblems;
use crate::core::{
    BindingId, BindingKind, KernelType, Namespace, ReferenceKind, TypeBinder, TypeExpr,
    TypeExprKind,
};
use crate::diagnostic::Diagnostic;
use crate::reader::{Form, FormKind, Symbol};
use crate::source::Span;

impl Expander<'_> {
    const FUNCTION_TYPE_PARTS: usize = 3;
    const WRAPPER_TYPE_PARTS: usize = 2;
    const WHERE_PARTS: usize = 3;

    pub(super) fn lower_type(&mut self, form: &Form) -> Result<TypeExpr, Diagnostic> {
        let span = form.span();
        let kind = match form.kind() {
            FormKind::Symbol(symbol) => self.lower_type_name(symbol, span)?,
            FormKind::List(items) => self.lower_type_application(form, items)?,
            FormKind::String(_) | FormKind::Number(_) => {
                return Err(ExpandProblems::not_a_type(span));
            }
        };
        Ok(TypeExpr::new(kind, span))
    }

    fn lower_type_name(&self, symbol: &Symbol, span: Span) -> Result<TypeExprKind, Diagnostic> {
        let binding = self.resolve(symbol, Namespace::Type, span)?;
        match self.kind(binding) {
            BindingKind::KernelType(KernelType::Primitive(primitive)) => {
                Ok(TypeExprKind::Primitive(primitive))
            }
            BindingKind::KernelType(KernelType::Hole) => Ok(TypeExprKind::Hole),
            BindingKind::KernelType(constructor) => {
                Err(ExpandProblems::malformed_type(constructor, span))
            }
            BindingKind::Struct(_) | BindingKind::TypeVariable => {
                Ok(TypeExprKind::Named(binding, Vec::new()))
            }
            _ => Err(ExpandProblems::wrong_namespace(
                symbol.name(),
                "a type",
                span,
            )),
        }
    }

    fn lower_type_application(
        &mut self,
        form: &Form,
        items: &[Form],
    ) -> Result<TypeExprKind, Diagnostic> {
        let (head, binding) = self.type_constructor(form, items)?;
        match self.kind(binding) {
            BindingKind::KernelType(KernelType::Function) => self.lower_function_type(form, items),
            BindingKind::KernelType(KernelType::Reference(kind)) => {
                self.lower_reference_type(kind, form, items)
            }
            BindingKind::KernelType(KernelType::Where) => self.lower_where(form, items),
            BindingKind::Struct(_) => self.lower_struct_application(binding, items),
            _ => Err(ExpandProblems::wrong_namespace(
                &head.to_string(),
                "a type constructor",
                head.span(),
            )),
        }
    }

    fn type_constructor<'f>(
        &self,
        form: &Form,
        items: &'f [Form],
    ) -> Result<(&'f Form, BindingId), Diagnostic> {
        let head = items
            .first()
            .ok_or_else(|| ExpandProblems::not_a_type(form.span()))?;
        let symbol = head
            .as_symbol()
            .ok_or_else(|| ExpandProblems::not_a_type(head.span()))?;
        let binding = self.resolve(symbol, Namespace::Type, head.span())?;
        Ok((head, binding))
    }

    fn lower_struct_application(
        &mut self,
        binding: BindingId,
        items: &[Form],
    ) -> Result<TypeExprKind, Diagnostic> {
        let arguments = items[1..]
            .iter()
            .map(|argument| self.lower_type(argument))
            .collect::<Result<Vec<TypeExpr>, Diagnostic>>()?;
        Ok(TypeExprKind::Named(binding, arguments))
    }

    fn lower_function_type(
        &mut self,
        form: &Form,
        items: &[Form],
    ) -> Result<TypeExprKind, Diagnostic> {
        let malformed = || ExpandProblems::malformed_type(KernelType::Function, form.span());
        if items.len() != Self::FUNCTION_TYPE_PARTS {
            return Err(malformed());
        }
        let parameters = items[1].as_list().ok_or_else(malformed)?;
        let parameters = parameters
            .iter()
            .map(|parameter| self.lower_type(parameter))
            .collect::<Result<Vec<TypeExpr>, Diagnostic>>()?;
        let result = self.lower_type(&items[2])?;
        Ok(TypeExprKind::Function(parameters, Box::new(result)))
    }

    fn lower_reference_type(
        &mut self,
        kind: ReferenceKind,
        form: &Form,
        items: &[Form],
    ) -> Result<TypeExprKind, Diagnostic> {
        if items.len() != Self::WRAPPER_TYPE_PARTS {
            return Err(ExpandProblems::malformed_type(
                KernelType::Reference(kind),
                form.span(),
            ));
        }
        let inner = self.lower_type(&items[1])?;
        Ok(TypeExprKind::Reference(kind, Box::new(inner)))
    }

    fn lower_where(&mut self, form: &Form, items: &[Form]) -> Result<TypeExprKind, Diagnostic> {
        let malformed = || ExpandProblems::malformed_type(KernelType::Where, form.span());
        if items.len() != Self::WHERE_PARTS {
            return Err(malformed());
        }
        let scope = self.fresh_local_scope();
        let mut binders = items[1].as_list().ok_or_else(malformed)?.to_vec();
        let mut body = items[2].clone();
        binders
            .iter_mut()
            .for_each(|binder| binder.add_scope(scope));
        body.add_scope(scope);
        let binders = binders
            .iter()
            .map(|binder| self.lower_type_binder(binder))
            .collect::<Result<Vec<TypeBinder>, Diagnostic>>()?;
        Ok(TypeExprKind::Where(
            binders,
            Box::new(self.lower_type(&body)?),
        ))
    }

    fn lower_type_binder(&mut self, binder: &Form) -> Result<TypeBinder, Diagnostic> {
        let (name, bounds) = match binder.as_list() {
            Some([name, bounds @ ..]) => (name, bounds),
            _ => (binder, &[][..]),
        };
        let bounds = bounds
            .iter()
            .map(|bound| self.lower_bound(bound))
            .collect::<Result<Vec<BindingId>, Diagnostic>>()?;
        let variable = self.define_local(name, Namespace::Type, BindingKind::TypeVariable, None)?;
        Ok(TypeBinder {
            variable,
            bounds,
            span: binder.span(),
        })
    }

    fn lower_bound(&self, bound: &Form) -> Result<BindingId, Diagnostic> {
        let symbol = bound.as_symbol().ok_or_else(|| {
            ExpandProblems::wrong_namespace(&bound.to_string(), "a trait", bound.span())
        })?;
        let binding = self.resolve(symbol, Namespace::Type, bound.span())?;
        match self.kind(binding) {
            BindingKind::Trait(_) => Ok(binding),
            _ => Err(ExpandProblems::wrong_namespace(
                symbol.name(),
                "a trait",
                bound.span(),
            )),
        }
    }
}
