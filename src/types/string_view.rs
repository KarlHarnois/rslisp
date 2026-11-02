use rustc_hash::FxHashMap;

use super::checker::TypeChecker;
use super::rust_types::RustTypeTable;
use super::ty::Type;
use super::typed::{TypedArgument, TypedBlock, TypedBodyItem, TypedExpr, TypedExprKind, TypedLeaf};
use crate::core::{
    BindingId, ClosureKind, CoreModule, ExprKind, ItemKind, Leaf, PrimitiveType, ReferenceKind,
    RustTemplate, TypeExpr, TypeExprKind,
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

    pub(super) fn view_borrowed_strings_beside_views(
        &mut self,
        positions: impl Iterator<Item = usize> + Clone,
        arguments: &mut [TypedArgument],
    ) -> Result<(), Diagnostic> {
        let Some(view) = self.string_view else {
            return Ok(());
        };
        if !positions
            .clone()
            .any(|position| self.is_string_view(&arguments[position].value.ty))
        {
            return Ok(());
        }
        for position in positions {
            let argument = &mut arguments[position];
            if self.borrows_string_as_written(&argument.value) {
                argument.value = self.call_string_view(view, argument.value.clone())?;
            }
        }
        Ok(())
    }

    fn is_string_view(&self, value: &Type) -> bool {
        match self.unifier.resolve(value) {
            Type::Ref(_, view) => Self::views_string(&view, &Self::owned_string()),
            _ => false,
        }
    }

    fn borrows_string_as_written(&self, value: &TypedExpr) -> bool {
        let TypedExprKind::Borrow(ReferenceKind::Shared, place) = &value.kind else {
            return false;
        };
        self.unifier.resolve(&self.place_type(place)) == Self::owned_string()
    }

    pub(crate) fn views_string(view: &Type, owned: &Type) -> bool {
        matches!(
            (view, owned),
            (
                Type::Primitive(PrimitiveType::Str),
                Type::Primitive(PrimitiveType::String)
            )
        )
    }

    pub(crate) fn viewed_items(
        rust_types: &RustTypeTable,
        view: &Type,
        owned: &Type,
    ) -> Option<Vec<(Type, Type)>> {
        match (view, owned) {
            (Type::Named(slice, items), Type::Named(vector, elements))
                if rust_types.is_slice(*slice) && rust_types.is_vector(*vector) =>
            {
                Some(
                    items
                        .iter()
                        .cloned()
                        .zip(elements.iter().cloned())
                        .collect(),
                )
            }
            _ => None,
        }
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
                .lower_type(written, &FxHashMap::default())
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
        Type::shared(Self::owned_string())
    }

    fn owned_string() -> Type {
        Type::Primitive(PrimitiveType::String)
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

#[cfg(test)]
mod tests {
    use crate::core::PrimitiveType;
    use crate::types::{Type, TypeChecker, TypedProgram, TypedSource};

    struct ViewFixture {
        program: TypedProgram,
    }

    impl ViewFixture {
        const SOURCE: &str = "
            (defn readings ((items (vec i64))) items)
            (defn window ((items (ref (slice u8)))) (len items))
            (defn reading ((value (option i64))) value)
            (defn main () (println \"Leaf\"))";

        fn new() -> Self {
            Self {
                program: TypedSource::expand(Self::SOURCE).typed(),
            }
        }

        fn viewed_items(&self, view: &Type, owned: &Type) -> Option<Vec<(Type, Type)>> {
            TypeChecker::viewed_items(&self.program.rust_types, view, owned)
        }

        fn parameter(&self, function: &str) -> Type {
            let definition = self
                .program
                .modules
                .iter()
                .flat_map(|module| &module.definitions)
                .find(|definition| self.program.bindings.get(definition.binding).name() == function)
                .expect("the source defines the function");
            let Type::Function(_, parameters, _) = &definition.scheme.body else {
                panic!("a defn has a function type");
            };
            parameters[0].referent().clone()
        }
    }

    #[test]
    fn a_slice_relates_its_items_to_the_elements_of_a_vector() {
        let fixture = ViewFixture::new();
        let items =
            fixture.viewed_items(&fixture.parameter("window"), &fixture.parameter("readings"));
        assert_eq!(
            format!("{items:?}"),
            format!(
                "{:?}",
                Some(vec![(
                    Type::Primitive(PrimitiveType::U8),
                    Type::Primitive(PrimitiveType::I64)
                )])
            )
        );
    }

    #[test]
    fn a_type_that_is_not_a_slice_views_no_vector() {
        let fixture = ViewFixture::new();
        assert!(
            fixture
                .viewed_items(
                    &fixture.parameter("reading"),
                    &fixture.parameter("readings")
                )
                .is_none()
        );
    }

    #[test]
    fn a_slice_views_no_type_that_is_not_a_vector() {
        let fixture = ViewFixture::new();
        assert!(
            fixture
                .viewed_items(&fixture.parameter("window"), &fixture.parameter("reading"))
                .is_none()
        );
    }
}
