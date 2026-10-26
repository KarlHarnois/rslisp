use std::collections::{HashMap, HashSet};

use super::functions::{FunctionTable, ParamMode};
use crate::core::{BindingId, PrimitiveType};
use crate::types::{
    GenericId, RustType, Type, TypedArgument, TypedBlock, TypedExpr, TypedExprKind, TypedLeaf,
    TypedProgram,
};

#[derive(Clone)]
enum Shrink {
    Slice(Type),
    Str,
}

pub struct ShrinkSelection<'a> {
    program: &'a TypedProgram,
    functions: &'a FunctionTable,
    modes: &'a HashMap<BindingId, ParamMode>,
    declared: HashMap<BindingId, Vec<Type>>,
    values: HashSet<BindingId>,
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
        Some(Self {
            program,
            functions,
            modes,
            declared,
            values,
            slice,
        })
    }

    pub fn run(&self) -> HashMap<BindingId, Type> {
        let candidates = self.candidates();
        let mut eligible: HashSet<BindingId> = candidates.keys().copied().collect();
        loop {
            let refused = self.refused(&candidates, &eligible);
            if refused.is_empty() {
                break;
            }
            eligible.retain(|binding| !refused.contains(binding));
        }
        eligible
            .into_iter()
            .map(|binding| (binding, self.shrunk(&candidates[&binding])))
            .collect()
    }

    fn shrunk(&self, shrink: &Shrink) -> Type {
        match shrink {
            Shrink::Slice(element) => Type::shared(Type::Named(self.slice, vec![element.clone()])),
            Shrink::Str => Type::shared(Type::Primitive(PrimitiveType::Str)),
        }
    }

    fn candidates(&self) -> HashMap<BindingId, Shrink> {
        let mut found = HashMap::new();
        for (function, info) in self.functions.iter() {
            if self.values.contains(function) {
                continue;
            }
            for slot in &info.params {
                if !slot.inferable || self.modes.get(&slot.binding) != Some(&ParamMode::Ref) {
                    continue;
                }
                if let Some(shrink) = self.candidate_shrink(&slot.ty) {
                    found.insert(slot.binding, shrink);
                }
            }
        }
        found
    }

    fn candidate_shrink(&self, ty: &Type) -> Option<Shrink> {
        match ty {
            Type::Named(vector, arguments) if self.program.rust_types.is_vector(*vector) => {
                match arguments.as_slice() {
                    [element] => Some(Shrink::Slice(element.clone())),
                    _ => None,
                }
            }
            Type::Primitive(PrimitiveType::String) => Some(Shrink::Str),
            _ => None,
        }
    }

    fn refused(
        &self,
        candidates: &HashMap<BindingId, Shrink>,
        eligible: &HashSet<BindingId>,
    ) -> HashSet<BindingId> {
        let mut refused = HashSet::new();
        for (function, info) in self.functions.iter() {
            let local: HashMap<BindingId, Shrink> = info
                .params
                .iter()
                .filter_map(|slot| {
                    candidates
                        .get(&slot.binding)
                        .map(|shrink| (slot.binding, shrink.clone()))
                })
                .collect();
            if local.is_empty() {
                continue;
            }
            let mut uses = BodyUses::new(self, *function, &local, eligible);
            uses.block(&info.lambda.body);
            refused.extend(uses.refused);
        }
        refused
    }

    fn argument_accepts(
        &self,
        callee: &TypedExpr,
        position: usize,
        shrink: &Shrink,
        eligible: &HashSet<BindingId>,
    ) -> bool {
        match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Global(function)) => {
                self.global_argument_accepts(*function, position, shrink, eligible)
            }
            TypedExprKind::Leaf(TypedLeaf::Rust(_)) => {
                self.rust_argument_accepts(callee, position, shrink)
            }
            _ => false,
        }
    }

    fn global_argument_accepts(
        &self,
        function: BindingId,
        position: usize,
        shrink: &Shrink,
        eligible: &HashSet<BindingId>,
    ) -> bool {
        if let Some(info) = self.functions.get(function)
            && let Some(slot) = info.params.get(position)
            && eligible.contains(&slot.binding)
        {
            return true;
        }
        self.declared.get(&function).is_some_and(|parameters| {
            parameters
                .get(position)
                .is_some_and(|parameter| self.accepts(parameter, shrink))
        })
    }

    fn rust_argument_accepts(&self, callee: &TypedExpr, position: usize, shrink: &Shrink) -> bool {
        callee
            .ty
            .parameters()
            .and_then(|parameters| parameters.get(position))
            .is_some_and(|parameter| self.accepts(parameter, shrink))
    }

    fn recur_accepts(
        &self,
        function: BindingId,
        position: usize,
        eligible: &HashSet<BindingId>,
    ) -> bool {
        self.functions
            .get(function)
            .and_then(|info| info.params.get(position))
            .is_some_and(|slot| eligible.contains(&slot.binding))
    }

    fn accepts(&self, parameter: &Type, shrink: &Shrink) -> bool {
        let Type::Ref(_, inner) = parameter else {
            return false;
        };
        match shrink {
            Shrink::Slice(element) => self.accepts_slice(inner, element),
            Shrink::Str => self.accepts_str(inner),
        }
    }

    fn accepts_slice(&self, inner: &Type, element: &Type) -> bool {
        match inner {
            Type::Named(binding, _) if self.program.rust_types.is_slice(*binding) => true,
            Type::Generic(generic) => {
                self.generic_accepts(*generic, &Type::Named(self.slice, vec![element.clone()]))
            }
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

struct BodyUses<'a, 'b> {
    selection: &'a ShrinkSelection<'b>,
    function: BindingId,
    candidates: &'a HashMap<BindingId, Shrink>,
    eligible: &'a HashSet<BindingId>,
    refused: HashSet<BindingId>,
}

impl<'a, 'b> BodyUses<'a, 'b> {
    fn new(
        selection: &'a ShrinkSelection<'b>,
        function: BindingId,
        candidates: &'a HashMap<BindingId, Shrink>,
        eligible: &'a HashSet<BindingId>,
    ) -> Self {
        Self {
            selection,
            function,
            candidates,
            eligible,
            refused: HashSet::new(),
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
                if self.candidates.contains_key(binding) =>
            {
                self.refuse(*binding);
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

    fn refuse(&mut self, binding: BindingId) {
        if self.eligible.contains(&binding) {
            self.refused.insert(binding);
        }
    }

    fn argument(&mut self, callee: &TypedExpr, position: usize, value: &TypedExpr) {
        if let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &value.kind
            && let Some(shrink) = self.candidates.get(binding)
        {
            if !self
                .selection
                .argument_accepts(callee, position, shrink, self.eligible)
            {
                self.refuse(*binding);
            }
            return;
        }
        self.expr(value);
    }

    fn recur_argument(&mut self, position: usize, value: &TypedExpr) {
        if let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &value.kind
            && self.candidates.contains_key(binding)
        {
            if !self
                .selection
                .recur_accepts(self.function, position, self.eligible)
            {
                self.refuse(*binding);
            }
            return;
        }
        self.expr(value);
    }
}
