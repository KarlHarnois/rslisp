use super::Expander;
use super::problems::ExpandProblems;
use crate::core::{
    BindingId, BindingKind, KernelType, Namespace, ReferenceKind, TypeBinder, TypeBound, TypeExpr,
    TypeExprKind,
};
use crate::diagnostic::Diagnostic;
use crate::reader::{Form, FormKind, Symbol};
use crate::source::Span;

impl Expander<'_> {
    const FUNCTION_TYPE_PARTS: usize = 3;
    const WRAPPER_TYPE_PARTS: usize = 2;
    const WHERE_PARTS: usize = 3;
    const WITH_PARTS: usize = 3;

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
            BindingKind::Struct(_) | BindingKind::RustType(_) | BindingKind::TypeVariable => {
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
            BindingKind::KernelType(KernelType::With) => self.lower_with_type(form, items),
            BindingKind::Struct(_) | BindingKind::RustType(_) => {
                self.lower_named_application(binding, items)
            }
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

    fn lower_named_application(
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
        let parameterized = self.lower_parameters(items, malformed)?;
        Ok(TypeExprKind::Where(
            parameterized.binders,
            Box::new(self.lower_type(&parameterized.body[0])?),
        ))
    }

    fn lower_with_type(&mut self, form: &Form, items: &[Form]) -> Result<TypeExprKind, Diagnostic> {
        if items.len() != Self::WITH_PARTS {
            return Err(ExpandProblems::malformed_type(
                KernelType::With,
                form.span(),
            ));
        }
        let implementor = self.lower_type(&items[1])?;
        let argument = self.lower_type(&items[2])?;
        Ok(TypeExprKind::With(
            Box::new(implementor),
            Box::new(argument),
        ))
    }

    pub(super) fn define_binder_variable(
        &mut self,
        binder: &Form,
    ) -> Result<BindingId, Diagnostic> {
        let name = match binder.as_list() {
            Some([name, ..]) => name,
            _ => binder,
        };
        self.define_local(name, Namespace::Type, BindingKind::TypeVariable, None)
    }

    pub(super) fn lower_type_binder(
        &mut self,
        binder: &Form,
        variable: BindingId,
    ) -> Result<TypeBinder, Diagnostic> {
        let bounds = match binder.as_list() {
            Some([_, bounds @ ..]) => bounds,
            _ => &[][..],
        };
        let bounds = bounds
            .iter()
            .map(|bound| self.lower_bound(bound))
            .collect::<Result<Vec<TypeBound>, Diagnostic>>()?;
        Ok(TypeBinder {
            variable,
            bounds,
            span: binder.span(),
        })
    }

    fn lower_bound(&mut self, bound: &Form) -> Result<TypeBound, Diagnostic> {
        let (name, argument) = match bound.as_list() {
            Some([name, argument]) => (name, Some(argument)),
            _ => (bound, None),
        };
        let symbol = name.as_symbol().ok_or_else(|| {
            ExpandProblems::wrong_namespace(&bound.to_string(), "a trait", bound.span())
        })?;
        let binding = self.resolve(symbol, Namespace::Type, bound.span())?;
        if !matches!(self.kind(binding), BindingKind::Trait(_)) {
            return Err(ExpandProblems::wrong_namespace(
                symbol.name(),
                "a trait",
                bound.span(),
            ));
        }
        let argument = argument
            .map(|argument| self.lower_type(argument))
            .transpose()?;
        Ok(TypeBound { binding, argument })
    }
}
