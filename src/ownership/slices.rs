use std::collections::{HashMap, HashSet};

use super::functions::{FunctionInfo, FunctionTable, ParamMode, ParamSlot};
use super::names::ChangingFunctionNames;
use super::refusals::Refusals;
use crate::core::{BindingId, PrimitiveType};
use crate::types::{
    GenericId, Lifetime, RustType, Type, TypedArgument, TypedBlock, TypedExpr, TypedExprKind,
    TypedLeaf, TypedProgram,
};

#[derive(Clone)]
enum Shrink {
    Slice(Reference, Type),
    Str,
}

#[derive(Clone, Copy)]
enum Reference {
    Shared(Lifetime),
    Mutable(Lifetime),
}

enum ArgumentUse {
    Accepted,
    Refused,
    Follows(BindingId),
}

struct Candidates {
    by_binding: Vec<(BindingId, Shrink)>,
}

pub struct ShrinkSelection<'a> {
    program: &'a TypedProgram,
    functions: &'a FunctionTable,
    modes: &'a HashMap<BindingId, ParamMode>,
    declared: HashMap<BindingId, Vec<Type>>,
    values: HashSet<BindingId>,
    trait_methods: HashSet<BindingId>,
    slice: BindingId,
}

impl<'a> ShrinkSelection<'a> {
    pub fn of(
        program: &'a TypedProgram,
        functions: &'a FunctionTable,
        modes: &'a HashMap<BindingId, ParamMode>,
    ) -> Option<Self> {
        let slice = program
            .rust_types
            .binding_with_template(RustType::SLICE_TEMPLATE)?;
        let mut values = HashSet::new();
        let declared = program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .filter_map(|definition| match &definition.scheme.body {
                Type::Function(_, parameters, _) => Some((definition.binding, parameters.clone())),
                _ => None,
            })
            .collect();
        for definition in program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
        {
            Self::scan(&definition.value, false, &mut values);
        }
        let mut trait_methods = ChangingFunctionNames::impl_methods(program);
        trait_methods.extend(ChangingFunctionNames::default_methods(program));
        Some(Self {
            program,
            functions,
            modes,
            declared,
            values,
            trait_methods,
            slice,
        })
    }

    pub fn run(&self) -> HashMap<BindingId, Type> {
        let candidates = self.candidates();
        let refused = self.refusals(&candidates).spread();
        candidates
            .iter()
            .filter(|(binding, _)| !refused.contains(binding))
            .map(|(binding, shrink)| (*binding, self.shrunk(shrink)))
            .collect()
    }

    fn shrunk(&self, shrink: &Shrink) -> Type {
        match shrink {
            Shrink::Slice(Reference::Shared(lifetime), element) => {
                Type::Ref(*lifetime, Box::new(self.slice_of(element)))
            }
            Shrink::Slice(Reference::Mutable(lifetime), element) => {
                Type::Mut(*lifetime, Box::new(self.slice_of(element)))
            }
            Shrink::Str => Type::shared(Type::Primitive(PrimitiveType::Str)),
        }
    }

    fn slice_of(&self, element: &Type) -> Type {
        Type::Named(self.slice, vec![element.clone()])
    }

    fn candidates(&self) -> Candidates {
        let mut found = Vec::new();
        for function in self.functions.definition_bindings() {
            if self.values.contains(&function) || self.trait_methods.contains(&function) {
                continue;
            }
            let Some(info) = self.functions.definition(function) else {
                continue;
            };
            for slot in &info.params {
                if let Some(shrink) = self.slot_shrink(slot) {
                    found.push((slot.binding, shrink));
                }
            }
        }
        Candidates::sorted(found)
    }

    fn slot_shrink(&self, slot: &ParamSlot) -> Option<Shrink> {
        match &slot.ty {
            Type::Ref(lifetime, vector) => self.slice_shrink(Reference::Shared(*lifetime), vector),
            Type::Mut(lifetime, vector) => self.slice_shrink(Reference::Mutable(*lifetime), vector),
            owned if slot.inferable => self.inferred_shrink(owned, self.modes.get(&slot.binding)?),
            _ => None,
        }
    }

    fn inferred_shrink(&self, ty: &Type, mode: &ParamMode) -> Option<Shrink> {
        match (mode, ty) {
            (ParamMode::Ref, Type::Primitive(PrimitiveType::String)) => Some(Shrink::Str),
            (ParamMode::Ref, vector) => {
                self.slice_shrink(Reference::Shared(Lifetime::Elided), vector)
            }
            (ParamMode::Mut, vector) => {
                self.slice_shrink(Reference::Mutable(Lifetime::Elided), vector)
            }
            _ => None,
        }
    }

    fn slice_shrink(&self, reference: Reference, ty: &Type) -> Option<Shrink> {
        match ty {
            Type::Named(vector, arguments) if self.program.rust_types.is_vector(*vector) => {
                match arguments.as_slice() {
                    [element] => Some(Shrink::Slice(reference, element.clone())),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn refusals(&self, candidates: &Candidates) -> Refusals {
        let mut refusals = Refusals::default();
        for (function, info) in self.holders(candidates) {
            BodyUses::new(self, *function, candidates, &mut refusals).block(&info.lambda.body);
        }
        refusals
    }

    fn holders<'s>(
        &'s self,
        candidates: &'s Candidates,
    ) -> impl Iterator<Item = (&'s BindingId, &'s FunctionInfo)> {
        self.functions
            .iter()
            .filter(|(_, info)| candidates.holds_any(&info.params))
    }

    fn argument_use(&self, callee: &TypedExpr, position: usize, shrink: &Shrink) -> ArgumentUse {
        match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Global(function)) => {
                self.global_argument_use(*function, position, shrink)
            }
            TypedExprKind::Leaf(TypedLeaf::Rust(_)) => {
                self.rust_argument_use(callee, position, shrink)
            }
            _ => ArgumentUse::Refused,
        }
    }

    fn global_argument_use(
        &self,
        function: BindingId,
        position: usize,
        shrink: &Shrink,
    ) -> ArgumentUse {
        if self.declared_argument_accepts(function, position, shrink) {
            return ArgumentUse::Accepted;
        }
        self.parameter_use(function, position)
    }

    fn declared_argument_accepts(
        &self,
        function: BindingId,
        position: usize,
        shrink: &Shrink,
    ) -> bool {
        self.declared.get(&function).is_some_and(|parameters| {
            parameters
                .get(position)
                .is_some_and(|parameter| self.accepts(parameter, shrink))
        })
    }

    fn rust_argument_use(
        &self,
        callee: &TypedExpr,
        position: usize,
        shrink: &Shrink,
    ) -> ArgumentUse {
        callee
            .ty
            .parameters()
            .and_then(|parameters| parameters.get(position))
            .filter(|parameter| self.accepts(parameter, shrink))
            .map_or(ArgumentUse::Refused, |_| ArgumentUse::Accepted)
    }

    fn parameter_use(&self, function: BindingId, position: usize) -> ArgumentUse {
        self.functions
            .get(function)
            .and_then(|info| info.params.get(position))
            .map_or(ArgumentUse::Refused, |slot| {
                ArgumentUse::Follows(slot.binding)
            })
    }

    fn accepts(&self, parameter: &Type, shrink: &Shrink) -> bool {
        match (shrink, parameter) {
            (Shrink::Slice(_, element), Type::Ref(_, inner))
            | (Shrink::Slice(Reference::Mutable(_), element), Type::Mut(_, inner)) => {
                self.accepts_slice(inner, element)
            }
            (Shrink::Str, Type::Ref(_, inner)) => self.accepts_str(inner),
            _ => false,
        }
    }

    fn accepts_slice(&self, inner: &Type, element: &Type) -> bool {
        match inner {
            Type::Named(binding, _) if self.program.rust_types.is_slice(*binding) => true,
            Type::Generic(generic) => self.generic_accepts(*generic, &self.slice_of(element)),
            _ => false,
        }
    }

    fn accepts_str(&self, inner: &Type) -> bool {
        match inner {
            Type::Primitive(PrimitiveType::Str) => true,
            Type::Generic(generic) => {
                self.generic_accepts(*generic, &Type::Primitive(PrimitiveType::Str))
            }
            _ => false,
        }
    }

    fn generic_accepts(&self, generic: GenericId, candidate: &Type) -> bool {
        self.program
            .generics
            .get(&generic)
            .into_iter()
            .flat_map(|parameter| &parameter.bounds)
            .chain(
                self.program
                    .implied_bounds
                    .get(&generic)
                    .into_iter()
                    .flatten(),
            )
            .all(|bound| {
                self.program
                    .traits
                    .satisfies(bound, candidate, &|_, _| false)
            })
    }

    fn scan(expr: &TypedExpr, callee: bool, values: &mut HashSet<BindingId>) {
        match (&expr.kind, callee) {
            (TypedExprKind::Leaf(TypedLeaf::Global(binding)), false) => {
                values.insert(*binding);
            }
            (TypedExprKind::Call(callee_expr, arguments), _) => {
                Self::scan(callee_expr, true, values);
                for argument in arguments {
                    Self::scan(&argument.value, false, values);
                }
            }
            _ => {
                for child in expr.children() {
                    Self::scan(child, false, values);
                }
            }
        }
    }
}

impl Candidates {
    fn sorted(mut found: Vec<(BindingId, Shrink)>) -> Self {
        found.sort_unstable_by_key(|(binding, _)| *binding);
        Self { by_binding: found }
    }

    fn get(&self, binding: BindingId) -> Option<&Shrink> {
        self.by_binding
            .binary_search_by_key(&binding, |(candidate, _)| *candidate)
            .ok()
            .map(|index| &self.by_binding[index].1)
    }

    fn contains(&self, binding: BindingId) -> bool {
        self.get(binding).is_some()
    }

    fn holds_any(&self, params: &[ParamSlot]) -> bool {
        params.iter().any(|slot| self.contains(slot.binding))
    }

    fn iter(&self) -> impl Iterator<Item = &(BindingId, Shrink)> {
        self.by_binding.iter()
    }
}

struct BodyUses<'a, 'b> {
    selection: &'a ShrinkSelection<'b>,
    function: BindingId,
    candidates: &'a Candidates,
    refusals: &'a mut Refusals,
}

impl<'a, 'b> BodyUses<'a, 'b> {
    fn new(
        selection: &'a ShrinkSelection<'b>,
        function: BindingId,
        candidates: &'a Candidates,
        refusals: &'a mut Refusals,
    ) -> Self {
        Self {
            selection,
            function,
            candidates,
            refusals,
        }
    }

    fn block(&mut self, block: &TypedBlock) {
        for expr in block.expressions() {
            self.expr(expr);
        }
    }

    fn expr(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding))
                if self.candidates.contains(*binding) =>
            {
                self.refusals.refuse(*binding);
            }
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments),
            TypedExprKind::Recur(arguments) => self.recur(arguments),
            _ => self.children(expr),
        }
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &[TypedArgument]) {
        for (position, argument) in arguments.iter().enumerate() {
            self.argument(callee, position, &argument.value);
        }
        if !matches!(callee.kind, TypedExprKind::Leaf(TypedLeaf::Global(_))) {
            self.expr(callee);
        }
    }

    fn recur(&mut self, arguments: &[TypedArgument]) {
        for (position, argument) in arguments.iter().enumerate() {
            self.recur_argument(position, &argument.value);
        }
    }

    fn children(&mut self, expr: &TypedExpr) {
        for child in expr.children() {
            self.expr(child);
        }
    }

    fn argument(&mut self, callee: &TypedExpr, position: usize, value: &TypedExpr) {
        if let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &value.kind
            && let Some(shrink) = self.candidates.get(*binding)
        {
            let used = self.selection.argument_use(callee, position, shrink);
            self.record(*binding, used);
            return;
        }
        self.expr(value);
    }

    fn recur_argument(&mut self, position: usize, value: &TypedExpr) {
        if let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &value.kind
            && self.candidates.contains(*binding)
        {
            let used = self.selection.parameter_use(self.function, position);
            self.record(*binding, used);
            return;
        }
        self.expr(value);
    }

    fn record(&mut self, binding: BindingId, used: ArgumentUse) {
        match used {
            ArgumentUse::Accepted => {}
            ArgumentUse::Follows(parameter) if self.candidates.contains(parameter) => {
                self.refusals.follow(binding, parameter);
            }
            ArgumentUse::Follows(_) | ArgumentUse::Refused => self.refusals.refuse(binding),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    use super::ShrinkSelection;
    use crate::core::{BindingId, ModulePath};
    use crate::expand::Expander;
    use crate::modules::ModuleSources;
    use crate::ownership::functions::{FunctionTable, ParamMode};
    use crate::ownership::modes::ModeInference;
    use crate::source::SourceMap;
    use crate::types::{
        CopyRules, DefinitionGraph, InferredCopies, LiteralDefaults, TypeChecker, TypedProgram,
    };

    struct SelectionFixture {
        program: TypedProgram,
        functions: FunctionTable,
        modes: HashMap<BindingId, ParamMode>,
    }

    impl SelectionFixture {
        const CALLER_CHAIN: &str = "
            (defn record-year! (log year) (push! log year))
            (defn record-twice! (history year)
              (record-year! history year)
              (record-year! history (+ year 1)))
            (defn oldest-year (years) (at years 0))
            (defn next-year (year) (+ year 1))
            (defn main ()
              (let years (vec 2011))
              (record-twice! (mut years) 2019)
              (println (oldest-year years) (next-year 2024)))";

        fn new() -> Self {
            let program = Self::typed(Self::CALLER_CHAIN);
            let functions = FunctionTable::collect(&program);
            let copy = CopyRules::new(
                &program.traits,
                &program.generics,
                &program.structs,
                &program.enums,
            );
            let modes = ModeInference::new(&functions, copy).run();
            Self {
                program,
                functions,
                modes,
            }
        }

        fn typed(source: &str) -> TypedProgram {
            let mut sources = SourceMap::default();
            let entry = sources.add("chain.rslisp", source);
            let module_sources = ModuleSources::new(PathBuf::from("."), PathBuf::from("."));
            let program = Expander::new(&mut sources, module_sources)
                .expand(ModulePath::new("chain"), entry)
                .expect("the chain expands");
            TypeChecker::check(
                &program,
                &DefinitionGraph::of(&program),
                &HashSet::new(),
                &InferredCopies::default(),
                &HashMap::new(),
                LiteralDefaults::default(),
                false,
            )
            .expect("the chain checks")
        }

        fn selection(&self) -> ShrinkSelection<'_> {
            ShrinkSelection::of(&self.program, &self.functions, &self.modes)
                .expect("the prelude defines slices")
        }

        fn walked_bodies(&self) -> HashSet<&str> {
            let selection = self.selection();
            let candidates = selection.candidates();
            selection
                .holders(&candidates)
                .map(|(function, _)| self.name(*function))
                .collect()
        }

        fn shrunk_parameters(&self) -> HashSet<&str> {
            self.selection()
                .run()
                .keys()
                .map(|binding| self.name(*binding))
                .collect()
        }

        fn name(&self, binding: BindingId) -> &str {
            self.program.bindings.get(binding).name()
        }
    }

    #[test]
    fn only_the_bodies_that_hold_a_candidate_are_walked() {
        let fixture = SelectionFixture::new();
        let holders = HashSet::from(["record-year!", "record-twice!", "oldest-year"]);
        assert_eq!(fixture.walked_bodies(), holders);
    }

    #[test]
    fn a_vector_only_callee_keeps_the_vector_of_its_caller() {
        let fixture = SelectionFixture::new();
        assert_eq!(fixture.shrunk_parameters(), HashSet::from(["years"]));
    }
}
