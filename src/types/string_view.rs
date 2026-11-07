use super::checker::TypeChecker;
use super::rust_types::RustTypeTable;
use super::ty::Type;
use super::typed::{TypedArgument, TypedBlock, TypedBodyItem, TypedExpr, TypedExprKind, TypedLeaf};
use crate::core::{BindingId, PrimitiveType, ReferenceKind};
use crate::diagnostic::Diagnostic;

impl TypeChecker {
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
        let ty = self.global_type(view, span);
        let Type::Function(_, parameters, result) = &ty else {
            unreachable!("the string view is a function")
        };
        let (parameter, result) = (parameters[0].clone(), result.as_ref().clone());
        let callee = TypedExpr {
            kind: TypedExprKind::Leaf(TypedLeaf::Global(view)),
            ty,
            span,
        };
        let borrow = self.pass_call_argument(Some(&callee), 0, &parameter, &value)?;
        Ok(TypedExpr {
            kind: TypedExprKind::Call(Box::new(callee), vec![TypedArgument { value, borrow }]),
            ty: result,
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
            items,
            Some(vec![(
                Type::Primitive(PrimitiveType::U8),
                Type::Primitive(PrimitiveType::I64)
            )])
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
