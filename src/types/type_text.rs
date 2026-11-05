use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::checker::TypeChecker;
use super::ty::{Bound, GenericParam, Scheme, Type, TypeVar};
use super::typed::{TypedExpr, TypedExprKind, TypedLeaf, TypedModule};
use crate::core::{BindingId, Literal};
use crate::source::Span;

#[derive(Clone, Debug, Default)]
pub struct TypeTexts {
    by_definition: FxHashMap<BindingId, Vec<Rc<str>>>,
}

impl TypeTexts {
    pub fn is_empty(&self) -> bool {
        self.by_definition.is_empty()
    }

    pub fn rewrite(&self, modules: &mut [TypedModule], replacements: &TypeTexts) {
        for definition in modules
            .iter_mut()
            .flat_map(|module| &mut module.definitions)
        {
            let written = self.of(definition.binding);
            let replacing = replacements.of(definition.binding);
            Self::rewrite_expr(&mut definition.value, written, replacing);
        }
    }

    fn of(&self, definition: BindingId) -> &[Rc<str>] {
        self.by_definition
            .get(&definition)
            .map_or(&[], Vec::as_slice)
    }

    fn rewrite_expr(expr: &mut TypedExpr, written: &[Rc<str>], replacing: &[Rc<str>]) {
        if let TypedExprKind::Leaf(TypedLeaf::Literal(Literal::String(text))) = &mut expr.kind
            && let Some(position) = written.iter().position(|known| Rc::ptr_eq(known, text))
            && let Some(replacement) = replacing.get(position)
        {
            *text = Rc::clone(replacement);
        }
        for child in expr.children_mut() {
            Self::rewrite_expr(child, written, replacing);
        }
    }

    fn record(&mut self, definition: BindingId, text: Rc<str>) {
        self.by_definition.entry(definition).or_default().push(text);
    }
}

pub struct TypeTextWriter {
    definition: BindingId,
    enclosing_names: FxHashSet<String>,
}

impl TypeTextWriter {
    pub fn new(definition: BindingId, enclosing: &Scheme) -> Self {
        let enclosing_names = enclosing
            .params
            .iter()
            .map(|param| param.name.to_string())
            .collect();
        Self {
            definition,
            enclosing_names,
        }
    }

    pub fn write(&self, checker: &mut TypeChecker, operand: &TypedExpr) -> Rc<str> {
        let scheme = self.generalize(checker, &operand.ty, operand.span);
        let text: Rc<str> = checker.display().scheme(&scheme).into();
        checker.type_texts.record(self.definition, Rc::clone(&text));
        text
    }

    fn generalize(&self, checker: &mut TypeChecker, operand: &Type, span: Span) -> Scheme {
        let bounded: Vec<(TypeVar, Vec<Bound>)> = Self::open_vars(checker, operand, span)
            .into_iter()
            .map(|var| (var, checker.self_plain_bounds(var)))
            .collect();
        let mut names = self.free_names();
        let params: Vec<GenericParam> = bounded
            .into_iter()
            .map(|(var, bounds)| Self::generalize_var(checker, var, bounds, &mut names))
            .collect();
        Scheme {
            lifetimes: Vec::new(),
            params: params
                .into_iter()
                .map(|param| Self::with_named_bounds(checker, param))
                .collect(),
            body: checker.unifier.resolve(operand),
        }
    }

    fn open_vars(checker: &mut TypeChecker, operand: &Type, span: Span) -> Vec<TypeVar> {
        let mut open_vars = Vec::new();
        for var in Self::vars_with_their_bounds(checker, operand) {
            match checker.unifier.literal_kind(var) {
                Some(literal) => checker.default_literal(var, literal, span),
                None => open_vars.push(var),
            }
        }
        open_vars
    }

    fn vars_with_their_bounds(checker: &TypeChecker, operand: &Type) -> Vec<TypeVar> {
        let mut vars = checker.free_vars(operand);
        let mut position = 0;
        while let Some(var) = vars.get(position).copied() {
            for named in Self::vars_in_bounds(checker, var) {
                if !vars.contains(&named) {
                    vars.push(named);
                }
            }
            position += 1;
        }
        vars
    }

    fn vars_in_bounds(checker: &TypeChecker, var: TypeVar) -> Vec<TypeVar> {
        checker
            .self_plain_bounds(var)
            .iter()
            .filter_map(|bound| bound.argument.as_ref())
            .flat_map(|argument| checker.free_vars(argument))
            .collect()
    }

    fn free_names(&self) -> impl Iterator<Item = String> + use<'_> {
        (0..)
            .map(TypeChecker::parameter_name)
            .filter(|name| !self.enclosing_names.contains(name))
    }

    fn generalize_var(
        checker: &mut TypeChecker,
        var: TypeVar,
        bounds: Vec<Bound>,
        names: &mut impl Iterator<Item = String>,
    ) -> GenericParam {
        let name = names.next().expect("parameter names never run out");
        let param = checker.new_generic(&name, bounds);
        checker.unifier.bind(var, Type::Generic(param.id));
        param
    }

    fn with_named_bounds(checker: &TypeChecker, mut param: GenericParam) -> GenericParam {
        for bound in &mut param.bounds {
            bound.argument = bound
                .argument
                .as_ref()
                .map(|argument| checker.unifier.resolve(argument));
        }
        param
    }
}
