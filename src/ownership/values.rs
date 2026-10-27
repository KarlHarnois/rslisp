use std::collections::HashMap;
use std::rc::Rc;

use super::instantiation::UseInstantiation;
use super::problems::OwnershipProblems;
use crate::core::{BindingArena, BindingId, FieldLabel};
use crate::diagnostic::Diagnostic;
use crate::types::{
    Borrow, CopyRules, FunctionValueWrapper, GenericId, GenericParam, StructTable, Type,
    TypeDisplay, TypedArgument, TypedBlock, TypedBodyItem, TypedControl, TypedDefinition,
    TypedExpr, TypedExprKind, TypedLeaf, TypedProgram, TypedStructExpr,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Adjustment {
    None,
    Shared,
    Mutable,
    Deref,
}

struct WrappedType<'a> {
    parameters: &'a [Type],
    expected_result: &'a Type,
    actual_result: &'a Type,
    actual: &'a Type,
}

impl Adjustment {
    fn borrow(self) -> Borrow {
        match self {
            Self::None => Borrow::None,
            Self::Shared => Borrow::Shared,
            Self::Mutable => Borrow::Mutable,
            Self::Deref => Borrow::Deref(1),
        }
    }
}

pub struct FunctionValueModes<'a> {
    bindings: &'a mut BindingArena,
    generics: &'a HashMap<GenericId, GenericParam>,
    structs: &'a StructTable,
    function_params: &'a HashMap<BindingId, Vec<Type>>,
    copy: CopyRules<'a>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> FunctionValueModes<'a> {
    pub fn apply(
        program: &'a mut TypedProgram,
        function_params: &'a HashMap<BindingId, Vec<Type>>,
    ) -> Vec<Diagnostic> {
        let copy = CopyRules::new(
            &program.traits,
            &program.generics,
            &program.structs,
            &program.enums,
        );
        let mut values = Self {
            bindings: &mut program.bindings,
            generics: &program.generics,
            structs: &program.structs,
            function_params,
            copy,
            diagnostics: Vec::new(),
        };
        for definition in program
            .modules
            .iter_mut()
            .flat_map(|module| module.definitions.iter_mut())
        {
            values.definition(definition);
        }
        values.diagnostics
    }

    fn definition(&mut self, definition: &mut TypedDefinition) {
        let current = self.function_params.get(&definition.binding).cloned();
        self.expr(&mut definition.value, current.as_deref());
    }

    fn expr(&mut self, expr: &mut TypedExpr, current: Option<&[Type]>) {
        let ty = expr.ty.clone();
        match &mut expr.kind {
            TypedExprKind::Call(callee, arguments) => {
                let expected = self.expected_params(callee);
                self.arguments(arguments, expected.as_deref(), current);
                self.expr(callee, current);
            }
            TypedExprKind::Control(control) => self.control(control, current),
            other => self.nested_expr(other, &ty, current),
        }
    }

    fn nested_expr(&mut self, kind: &mut TypedExprKind, ty: &Type, current: Option<&[Type]>) {
        match kind {
            TypedExprKind::Recur(arguments) => self.arguments(arguments, current, current),
            TypedExprKind::Struct(structure) => self.structure(structure, ty, current),
            TypedExprKind::Variant(_, arguments) => self.expressions(arguments, current),
            TypedExprKind::Assoc(value, keys, item) => self.assoc(value, keys, item, current),
            TypedExprKind::TraitObject(value, _) => self.expr(value, current),
            _ => self.value_kind(kind, current),
        }
    }

    fn value_kind(&mut self, kind: &mut TypedExprKind, current: Option<&[Type]>) {
        match kind {
            TypedExprKind::Try { value, .. }
            | TypedExprKind::Borrow(_, value)
            | TypedExprKind::Deref(value) => self.expr(value, current),
            TypedExprKind::Fn(lambda) => {
                let lambda = Rc::make_mut(lambda);
                let params: Vec<Type> =
                    lambda.params.iter().map(|param| param.ty.clone()).collect();
                self.block(&mut lambda.body, Some(&params));
            }
            _ => {}
        }
    }

    fn expressions(&mut self, arguments: &mut [TypedExpr], current: Option<&[Type]>) {
        for argument in arguments {
            self.expr(argument, current);
        }
    }

    fn assoc(
        &mut self,
        value: &mut TypedExpr,
        keys: &mut [TypedExpr],
        item: &mut TypedExpr,
        current: Option<&[Type]>,
    ) {
        self.expr(value, current);
        self.expressions(keys, current);
        self.expr(item, current);
    }

    fn control(&mut self, control: &mut TypedControl, current: Option<&[Type]>) {
        match control {
            TypedControl::Block(block) => self.block(block, current),
            TypedControl::If(test, then, otherwise) => {
                self.expr(test, current);
                self.expr(then, current);
                self.expr(otherwise, current);
            }
            TypedControl::Match(matched) => {
                self.expr(&mut matched.value, current);
                for arm in &mut matched.arms {
                    self.block(&mut arm.body, current);
                }
            }
        }
    }

    fn structure(&mut self, structure: &mut TypedStructExpr, ty: &Type, current: Option<&[Type]>) {
        match structure {
            TypedStructExpr::Field(value, _) => self.expr(value, current),
            TypedStructExpr::New(binding, fields) => {
                self.fields(*binding, fields, ty, current);
            }
            TypedStructExpr::Update(binding, base, fields) => {
                self.expr(base, current);
                self.fields(*binding, fields, ty, current);
            }
        }
    }

    fn fields(
        &mut self,
        binding: BindingId,
        fields: &mut [(FieldLabel, TypedExpr)],
        ty: &Type,
        current: Option<&[Type]>,
    ) {
        let field_types = self.field_types(binding, ty);
        for (label, value) in fields {
            self.field(&field_types, label, value, current);
        }
    }

    fn field(
        &mut self,
        field_types: &[(std::rc::Rc<str>, Type)],
        label: &FieldLabel,
        value: &mut TypedExpr,
        current: Option<&[Type]>,
    ) {
        let (_, expected) = field_types
            .iter()
            .find(|(name, _)| name.as_ref() == label.name.as_ref())
            .expect("the type checker checked every field");
        let expected = expected.clone();
        if !self.compare(value, &expected) {
            self.expr(value, current);
        }
    }

    fn block(&mut self, block: &mut TypedBlock, current: Option<&[Type]>) {
        for item in &mut block.items {
            match item {
                TypedBodyItem::Let(binder, value) => {
                    let expected = binder.ty.clone();
                    if !self.compare(value, &expected) {
                        self.expr(value, current);
                    }
                }
                TypedBodyItem::Expr(expr) => self.expr(expr, current),
            }
        }
    }

    fn arguments(
        &mut self,
        arguments: &mut [TypedArgument],
        expected: Option<&[Type]>,
        current: Option<&[Type]>,
    ) {
        for (position, argument) in arguments.iter_mut().enumerate() {
            let handled = expected
                .and_then(|parameters| parameters.get(position))
                .is_some_and(|expected| self.compare(&mut argument.value, expected));
            if !handled {
                self.expr(&mut argument.value, current);
            }
        }
    }

    fn expected_params(&self, callee: &TypedExpr) -> Option<Vec<Type>> {
        if let TypedExprKind::Leaf(TypedLeaf::Global(binding)) = &callee.kind
            && let Some(parameters) = self.used_params(*binding, &callee.ty)
        {
            return Some(parameters);
        }
        match &callee.ty {
            Type::Function(_, parameters, _) => Some(parameters.clone()),
            _ => None,
        }
    }

    fn used_params(&self, binding: BindingId, used: &Type) -> Option<Vec<Type>> {
        let declared = self.function_params.get(&binding)?;
        let Type::Function(_, used_parameters, _) = used else {
            return Some(declared.clone());
        };
        Some(UseInstantiation::of(declared, used_parameters).apply(declared))
    }

    fn field_types(&self, binding: BindingId, ty: &Type) -> Vec<(std::rc::Rc<str>, Type)> {
        let definition = self
            .structs
            .get(binding)
            .expect("the type checker checked every struct");
        let empty: &[Type] = &[];
        let arguments = match ty {
            Type::Named(_, arguments) => arguments.as_slice(),
            _ => empty,
        };
        definition.instantiated_fields(arguments)
    }

    fn compare(&mut self, value: &mut TypedExpr, expected: &Type) -> bool {
        let Type::Function(_, expected_parameters, expected_result) = expected else {
            return false;
        };
        let Some((binding, actual)) = self.function_value(value) else {
            return false;
        };
        let Type::Function(_, actual_parameters, actual_result) = &actual else {
            return false;
        };
        let adjustments = match self.adjustments(expected_parameters, actual_parameters) {
            Ok(adjustments) => adjustments,
            Err(parameter) => return self.mismatch(value, binding, &actual, expected, parameter),
        };
        if expected == &actual {
            if matches!(value.kind, TypedExprKind::Fn(_)) {
                self.unwrap(value, binding, &actual);
            }
            return false;
        }
        self.wrap_value(
            value,
            binding,
            WrappedType {
                parameters: expected_parameters,
                expected_result,
                actual_result,
                actual: &actual,
            },
            adjustments,
        );
        true
    }

    fn adjustments(&self, expected: &[Type], actual: &[Type]) -> Result<Vec<Adjustment>, usize> {
        if expected.len() != actual.len() {
            return Err(1);
        }
        expected.iter().zip(actual).enumerate().try_fold(
            Vec::new(),
            |mut found, (position, (expected, actual))| match self.adjustment(expected, actual) {
                Some(adjustment) => {
                    found.push(adjustment);
                    Ok(found)
                }
                None => Err(position + 1),
            },
        )
    }

    fn wrap_value(
        &mut self,
        value: &mut TypedExpr,
        binding: BindingId,
        wrapped: WrappedType<'_>,
        adjustments: Vec<Adjustment>,
    ) {
        let original = self.global(binding, wrapped.actual, value.span);
        let mut prepared = FunctionValueWrapper::new(self.bindings).prepare(
            wrapped.parameters,
            original,
            Some(binding),
        );
        for (argument, adjustment) in prepared.arguments.iter_mut().zip(adjustments) {
            argument.borrow = adjustment.borrow();
        }
        *value = prepared.finish(wrapped.expected_result, wrapped.actual_result);
    }

    fn mismatch(
        &mut self,
        value: &mut TypedExpr,
        binding: BindingId,
        actual: &Type,
        expected: &Type,
        parameter: usize,
    ) -> bool {
        let (expected, found) = {
            let display = TypeDisplay::new(self.bindings, self.generics);
            (display.show(expected), display.show(actual))
        };
        self.diagnostics
            .push(OwnershipProblems::function_value_mismatch(
                &expected, &found, value.span, parameter,
            ));
        self.unwrap(value, binding, actual);
        true
    }

    fn unwrap(&self, value: &mut TypedExpr, binding: BindingId, actual: &Type) {
        *value = self.global(binding, actual, value.span);
    }

    fn global(&self, binding: BindingId, actual: &Type, span: crate::source::Span) -> TypedExpr {
        TypedExpr {
            kind: TypedExprKind::Leaf(TypedLeaf::Global(binding)),
            ty: actual.clone(),
            span,
        }
    }

    fn function_value(&self, value: &TypedExpr) -> Option<(BindingId, Type)> {
        let binding = match &value.kind {
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => *binding,
            TypedExprKind::Fn(lambda) => lambda.wraps?,
            _ => return None,
        };
        let parameters = self.used_params(binding, &value.ty)?;
        let Type::Function(_, _, result) = &value.ty else {
            return None;
        };
        Some((
            binding,
            Type::Function(crate::types::ClosureKind::Fn, parameters, result.clone()),
        ))
    }

    fn adjustment(&self, expected: &Type, actual: &Type) -> Option<Adjustment> {
        if expected == actual {
            return Some(Adjustment::None);
        }
        self.added_borrow(expected, actual)
            .or_else(|| self.copied_value(expected, actual))
            .or_else(|| self.reborrowed(expected, actual))
            .or_else(|| self.removed_borrows(expected, actual))
    }

    fn added_borrow(&self, expected: &Type, actual: &Type) -> Option<Adjustment> {
        if expected.is_reference() {
            return None;
        }
        match actual {
            Type::Ref(_, _) => Some(Adjustment::Shared),
            Type::Mut(_, _) => Some(Adjustment::Mutable),
            _ => None,
        }
    }

    fn copied_value(&self, expected: &Type, actual: &Type) -> Option<Adjustment> {
        if !expected.is_reference() || actual.is_reference() {
            return None;
        }
        self.copy.is_small_copy(actual).then_some(Adjustment::Deref)
    }

    fn reborrowed(&self, expected: &Type, actual: &Type) -> Option<Adjustment> {
        matches!((expected, actual), (Type::Mut(_, _), Type::Ref(_, _))).then_some(Adjustment::None)
    }

    fn removed_borrows(&self, expected: &Type, actual: &Type) -> Option<Adjustment> {
        if !actual.is_reference() {
            return None;
        }
        let expected_depth = Self::reference_depth(expected);
        let actual_depth = Self::reference_depth(actual);
        (expected_depth > actual_depth
            && Self::without_references(expected, expected_depth - actual_depth) == *actual)
            .then_some(Adjustment::None)
    }

    fn reference_depth(value: &Type) -> usize {
        match value {
            Type::Ref(_, inner) => 1 + Self::reference_depth(inner),
            _ => 0,
        }
    }

    fn without_references(value: &Type, levels: usize) -> Type {
        match value {
            Type::Ref(_, inner) if levels > 0 => Self::without_references(inner, levels - 1),
            _ => value.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BindingInfo, BindingKind, ModuleId, PrimitiveType};
    use crate::types::{Implementor, RustTrait, Scheme, TraitTable};

    struct Adjustments {
        bindings: BindingArena,
        generics: HashMap<GenericId, GenericParam>,
        structs: StructTable,
        enums: crate::types::EnumTable,
        function_params: HashMap<BindingId, Vec<Type>>,
        traits: TraitTable,
    }

    impl Adjustments {
        fn new() -> Self {
            let mut bindings = BindingArena::default();
            let mut traits = TraitTable::default();
            let copy = bindings.add(BindingInfo::new(
                "copy".into(),
                BindingKind::Trait(ModuleId::new(0)),
                None,
            ));
            traits.add(
                copy,
                RustTrait {
                    path: std::rc::Rc::from("Copy"),
                    implementors: vec![Implementor {
                        scheme: Scheme::monomorphic(Type::Primitive(PrimitiveType::I64)),
                        argument: None,
                    }],
                    help: None,
                    supertraits: Vec::new(),
                    associated: None,
                },
            );
            Self {
                bindings,
                generics: HashMap::new(),
                structs: StructTable::default(),
                enums: crate::types::EnumTable::default(),
                function_params: HashMap::new(),
                traits,
            }
        }

        fn number(&self) -> Type {
            Type::Primitive(PrimitiveType::I64)
        }

        fn text(&self) -> Type {
            Type::Primitive(PrimitiveType::String)
        }

        fn borrowed(&self, value: Type) -> Type {
            Type::shared(value)
        }

        fn changed(&self, value: Type) -> Type {
            Type::mutable(value)
        }

        fn decide(&mut self, expected: &Type, actual: &Type) -> Option<Adjustment> {
            self.values().adjustment(expected, actual)
        }

        fn decide_many(
            &mut self,
            expected: &[Type],
            actual: &[Type],
        ) -> Result<Vec<Adjustment>, usize> {
            self.values().adjustments(expected, actual)
        }

        fn values(&mut self) -> FunctionValueModes<'_> {
            let copy = CopyRules::new(&self.traits, &self.generics, &self.structs, &self.enums);
            FunctionValueModes {
                bindings: &mut self.bindings,
                generics: &self.generics,
                structs: &self.structs,
                function_params: &self.function_params,
                copy,
                diagnostics: Vec::new(),
            }
        }
    }

    #[test]
    fn every_adjustment_names_its_borrow() {
        assert_eq!(Adjustment::None.borrow(), Borrow::None);
        assert_eq!(Adjustment::Shared.borrow(), Borrow::Shared);
        assert_eq!(Adjustment::Mutable.borrow(), Borrow::Mutable);
        assert_eq!(Adjustment::Deref.borrow(), Borrow::Deref(1));
    }

    #[test]
    fn different_owned_types_are_unfixable() {
        let mut fixture = Adjustments::new();
        let number = fixture.number();
        let unit = Type::Primitive(crate::core::PrimitiveType::Unit);
        assert_eq!(fixture.decide(&number, &unit), None);
    }

    #[test]
    fn matching_parameters_need_no_adjustment() {
        let mut fixture = Adjustments::new();
        let number = fixture.number();
        let borrowed = fixture.borrowed(number.clone());
        let changed = fixture.changed(number.clone());
        assert_eq!(fixture.decide(&number, &number), Some(Adjustment::None));
        assert_eq!(fixture.decide(&borrowed, &borrowed), Some(Adjustment::None));
        assert_eq!(fixture.decide(&changed, &changed), Some(Adjustment::None));
    }

    #[test]
    fn an_owned_parameter_takes_a_borrow() {
        let mut fixture = Adjustments::new();
        let number = fixture.number();
        assert_eq!(
            fixture.decide(&number, &fixture.borrowed(number.clone())),
            Some(Adjustment::Shared)
        );
        assert_eq!(
            fixture.decide(&number, &fixture.changed(number.clone())),
            Some(Adjustment::Mutable)
        );
    }

    #[test]
    fn a_borrowed_parameter_copies_a_small_value() {
        let mut fixture = Adjustments::new();
        let number = fixture.number();
        assert_eq!(
            fixture.decide(&fixture.borrowed(number.clone()), &number),
            Some(Adjustment::Deref)
        );
        assert_eq!(
            fixture.decide(&fixture.changed(number.clone()), &number),
            Some(Adjustment::Deref)
        );
    }

    #[test]
    fn a_borrowed_parameter_cannot_take_a_large_value() {
        let mut fixture = Adjustments::new();
        let text = fixture.text();
        assert_eq!(fixture.decide(&fixture.borrowed(text.clone()), &text), None);
        assert_eq!(fixture.decide(&fixture.changed(text.clone()), &text), None);
    }

    #[test]
    fn a_mutable_parameter_reborrows_a_shared_one() {
        let mut fixture = Adjustments::new();
        let number = fixture.number();
        assert_eq!(
            fixture.decide(&fixture.changed(number.clone()), &fixture.borrowed(number)),
            Some(Adjustment::None)
        );
    }

    #[test]
    fn a_shared_parameter_cannot_take_a_mutable_borrow() {
        let mut fixture = Adjustments::new();
        let number = fixture.number();
        assert_eq!(
            fixture.decide(&fixture.borrowed(number.clone()), &fixture.changed(number)),
            None
        );
    }

    #[test]
    fn extra_borrows_are_removed() {
        let mut fixture = Adjustments::new();
        let number = fixture.number();
        let borrowed = fixture.borrowed(number.clone());
        assert_eq!(
            fixture.decide(&fixture.borrowed(borrowed.clone()), &borrowed),
            Some(Adjustment::None)
        );
    }

    #[test]
    fn a_different_number_of_parameters_is_unfixable() {
        let mut fixture = Adjustments::new();
        let number = fixture.number();
        assert_eq!(fixture.decide_many(&[number], &[]), Err(1));
    }

    #[test]
    fn missing_borrows_are_not_added_to_a_reference() {
        let mut fixture = Adjustments::new();
        let number = fixture.number();
        let borrowed = fixture.borrowed(number);
        assert_eq!(
            fixture.decide(&borrowed, &fixture.borrowed(borrowed.clone())),
            None
        );
    }
}
