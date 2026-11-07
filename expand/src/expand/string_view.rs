use crate::core::{
    BindingId, ClosureKind, CoreModule, ExprKind, Leaf, PrimitiveType, ReferenceKind, TypeExpr,
    TypeExprKind,
};

pub(super) struct StringViewSearch;

impl StringViewSearch {
    pub(super) fn in_modules<'m>(
        mut modules: impl Iterator<Item = &'m CoreModule>,
    ) -> Option<BindingId> {
        modules.find_map(Self::in_module)
    }

    fn in_module(module: &CoreModule) -> Option<BindingId> {
        module
            .definitions()
            .find(|(_, value)| match value.kind() {
                ExprKind::Leaf(Leaf::Rust(template)) => Self::views_a_string(&template.type_expr),
                _ => false,
            })
            .map(|(binding, _)| binding)
    }

    fn views_a_string(written: &TypeExpr) -> bool {
        let TypeExprKind::Function(ClosureKind::Fn, parameters, result) = written.kind() else {
            return false;
        };
        let [parameter] = parameters.as_slice() else {
            return false;
        };
        Self::is_shared_primitive(parameter, PrimitiveType::String)
            && Self::is_shared_primitive(result, PrimitiveType::Str)
    }

    fn is_shared_primitive(written: &TypeExpr, primitive: PrimitiveType) -> bool {
        let TypeExprKind::Reference(ReferenceKind::Shared, _, referent) = written.kind() else {
            return false;
        };
        matches!(referent.kind(), TypeExprKind::Primitive(found) if *found == primitive)
    }
}

#[cfg(test)]
mod tests {
    use super::StringViewSearch;
    use crate::core::{ClosureKind, PrimitiveType, ReferenceKind, TypeExpr, TypeExprKind};
    use crate::source::{SourceId, Span};

    struct SignatureFixture;

    impl SignatureFixture {
        fn written(kind: TypeExprKind) -> TypeExpr {
            TypeExpr::new(kind, Span::new(SourceId::new(0), 0..0))
        }

        fn reference(kind: ReferenceKind, primitive: PrimitiveType) -> TypeExpr {
            let referent = Self::written(TypeExprKind::Primitive(primitive));
            Self::written(TypeExprKind::Reference(kind, None, Box::new(referent)))
        }

        fn shared(primitive: PrimitiveType) -> TypeExpr {
            Self::reference(ReferenceKind::Shared, primitive)
        }

        fn function(kind: ClosureKind, parameters: Vec<TypeExpr>, result: TypeExpr) -> TypeExpr {
            Self::written(TypeExprKind::Function(kind, parameters, Box::new(result)))
        }

        fn string_view() -> TypeExpr {
            Self::function(
                ClosureKind::Fn,
                vec![Self::shared(PrimitiveType::String)],
                Self::shared(PrimitiveType::Str),
            )
        }
    }

    #[test]
    fn a_function_from_a_borrowed_string_to_a_borrowed_str_views_a_string() {
        assert!(StringViewSearch::views_a_string(
            &SignatureFixture::string_view()
        ));
    }

    #[test]
    fn a_value_does_not_view_a_string() {
        assert!(!StringViewSearch::views_a_string(
            &SignatureFixture::shared(PrimitiveType::Str)
        ));
    }

    #[test]
    fn a_closure_that_moves_does_not_view_a_string() {
        let closure = SignatureFixture::function(
            ClosureKind::FnOnce,
            vec![SignatureFixture::shared(PrimitiveType::String)],
            SignatureFixture::shared(PrimitiveType::Str),
        );
        assert!(!StringViewSearch::views_a_string(&closure));
    }

    #[test]
    fn a_second_parameter_does_not_view_a_string() {
        let parameters = vec![
            SignatureFixture::shared(PrimitiveType::String),
            SignatureFixture::shared(PrimitiveType::String),
        ];
        let function = SignatureFixture::function(
            ClosureKind::Fn,
            parameters,
            SignatureFixture::shared(PrimitiveType::Str),
        );
        assert!(!StringViewSearch::views_a_string(&function));
    }

    #[test]
    fn a_borrowed_str_parameter_does_not_view_a_string() {
        let function = SignatureFixture::function(
            ClosureKind::Fn,
            vec![SignatureFixture::shared(PrimitiveType::Str)],
            SignatureFixture::shared(PrimitiveType::Str),
        );
        assert!(!StringViewSearch::views_a_string(&function));
    }

    #[test]
    fn an_owned_result_does_not_view_a_string() {
        let owned = SignatureFixture::written(TypeExprKind::Primitive(PrimitiveType::Str));
        let function = SignatureFixture::function(
            ClosureKind::Fn,
            vec![SignatureFixture::shared(PrimitiveType::String)],
            owned,
        );
        assert!(!StringViewSearch::views_a_string(&function));
    }

    #[test]
    fn a_written_borrow_does_not_view_a_string() {
        let function = SignatureFixture::function(
            ClosureKind::Fn,
            vec![SignatureFixture::reference(
                ReferenceKind::Mutable,
                PrimitiveType::String,
            )],
            SignatureFixture::shared(PrimitiveType::Str),
        );
        assert!(!StringViewSearch::views_a_string(&function));
    }
}
