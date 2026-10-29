use std::collections::HashMap;

use super::checker::TypeChecker;
use super::ty::Type;
use super::typed::{TypedArgument, TypedBlock, TypedBodyItem, TypedExpr, TypedExprKind, TypedLeaf};
use crate::core::{
    BindingId, ClosureKind, CoreModule, ExprKind, ItemKind, Leaf, PrimitiveType, RustTemplate,
    TypeExpr, TypeExprKind,
};
use crate::diagnostic::Diagnostic;
use crate::modules::ModuleSources;

impl TypeChecker {
    pub(super) fn record_string_view(&mut self, modules: &[CoreModule]) {
        let view = Self::string_view_type();
        self.string_view = Self::prelude_templates(modules)
            .find(|(_, template)| self.lowers_to(&template.type_expr, &view))
            .map(|(binding, _)| binding);
    }

    pub(super) fn view_closure_result(
        &mut self,
        mut body: TypedBlock,
        result: &Type,
        found: &Type,
    ) -> Result<TypedBlock, Diagnostic> {
        if !self.sits_in_function_body() || !self.needs_string_view(result, found) {
            return Ok(body);
        }
        let Some(view) = self.string_view else {
            return Ok(body);
        };
        if let Some(TypedBodyItem::Expr(last)) = body.items.last_mut() {
            *last = self.call_string_view(view, last.clone())?;
        }
        Ok(body)
    }

    pub(super) fn views_string(view: &Type, owned: &Type) -> bool {
        matches!(
            (view, owned),
            (
                Type::Primitive(PrimitiveType::Str),
                Type::Primitive(PrimitiveType::String)
            )
        )
    }

    fn prelude_templates(
        modules: &[CoreModule],
    ) -> impl Iterator<Item = (BindingId, &RustTemplate)> {
        modules
            .iter()
            .filter(|module| ModuleSources::is_prelude(&module.path))
            .flat_map(|module| &module.items)
            .filter_map(|item| match &item.kind {
                ItemKind::Def(binding, value) => match value.kind() {
                    ExprKind::Leaf(Leaf::Rust(template)) => Some((*binding, template.as_ref())),
                    _ => None,
                },
                _ => None,
            })
    }

    fn lowers_to(&mut self, written: &TypeExpr, expected: &Type) -> bool {
        !matches!(written.kind(), TypeExprKind::Where(..))
            && self
                .lower_type(written, &HashMap::new())
                .is_ok_and(|lowered| &lowered == expected)
    }

    fn string_view_type() -> Type {
        Type::Function(
            ClosureKind::Fn,
            vec![Self::viewed_string()],
            Box::new(Type::str_ref()),
        )
    }

    fn viewed_string() -> Type {
        Type::shared(Type::Primitive(PrimitiveType::String))
    }

    fn sits_in_function_body(&self) -> bool {
        self.checking_function_body
    }

    fn needs_string_view(&self, result: &Type, found: &Type) -> bool {
        match (self.unifier.resolve(result), self.unifier.resolve(found)) {
            (Type::Ref(_, view), Type::Ref(_, owned)) => Self::views_string(&view, &owned),
            _ => false,
        }
    }

    fn call_string_view(
        &mut self,
        view: BindingId,
        value: TypedExpr,
    ) -> Result<TypedExpr, Diagnostic> {
        let span = value.span;
        let callee = TypedExpr {
            kind: TypedExprKind::Leaf(TypedLeaf::Global(view)),
            ty: Self::string_view_type(),
            span,
        };
        let borrow = self.pass_call_argument(Some(&callee), 0, &Self::viewed_string(), &value)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Call(Box::new(callee), vec![TypedArgument { value, borrow }]),
            ty: Type::str_ref(),
            span,
        })
    }
}
