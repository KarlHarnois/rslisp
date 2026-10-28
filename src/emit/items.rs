use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use super::expressions::ExpressionEmitter;
use super::fragment::{Code, Fragment};
use super::fresh_lifetimes::FreshLifetimes;
use super::literal_generics::LiteralGenerics;
use super::locals::LocalNames;
use super::mangle::NameMangler;
use super::mutable_locals::MutableLocals;
use super::opaque_borrows::OpaqueBorrows;
use super::plan::{EmitPlan, ItemInfo, ItemKind};
use super::runtime::RuntimeModule;
use super::signature_borrows::SignatureBorrows;
use super::types::{LifetimeScope, RustTypes, TypePosition};
use super::unsized_parameters::UnsizedParameters;
use crate::core::{BindingId, ModuleId};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    Borrow, Bound, GenericId, GenericParam, Lifetime, ReceiverKind, Type, TypedArgument,
    TypedAssociatedType, TypedBinder, TypedBlock, TypedBodyItem, TypedControl, TypedConversion,
    TypedDefinition, TypedExpr, TypedExprKind, TypedLambda, TypedLeaf, TypedProgram,
    TypedTraitMethod, VariantDefinition,
};

struct TraitHeader {
    binding: BindingId,
    rust_name: std::rc::Rc<str>,
    supertraits: Vec<Bound>,
    associated: Vec<TypedAssociatedType>,
    methods: Vec<TypedTraitMethod>,
}

#[derive(Default)]
struct ModuleTree {
    items: Vec<Vec<Fragment>>,
    children: BTreeMap<String, ModuleTree>,
}

impl ModuleTree {
    fn insert(&mut self, segments: &[String], items: Vec<Vec<Fragment>>) {
        match segments.split_first() {
            Some((first, rest)) => self
                .children
                .entry(first.clone())
                .or_default()
                .insert(rest, items),
            None => self.items.extend(items),
        }
    }

    fn fragments(&self, name: &str, keyword: &str) -> Vec<Fragment> {
        let mut inner = ItemEmitter::separated(self.items.clone());
        for (child, tree) in &self.children {
            inner.push(Fragment::Newline);
            inner.push(Fragment::Newline);
            inner.extend(tree.fragments(child, "pub(crate) mod"));
        }
        vec![
            Fragment::Text(format!("{keyword} {name} {{")),
            Fragment::Indented(Self::leading_newline(inner)),
            Fragment::Newline,
            Fragment::Text("}".to_string()),
        ]
    }

    fn leading_newline(fragments: Vec<Fragment>) -> Vec<Fragment> {
        let mut with_newline = vec![Fragment::Newline];
        with_newline.extend(fragments);
        with_newline
    }
}

struct ResultBorrows<'a> {
    program: &'a TypedProgram,
    parameters: HashSet<BindingId>,
    aliases: HashMap<BindingId, HashSet<BindingId>>,
}

impl<'a> ResultBorrows<'a> {
    fn of(program: &'a TypedProgram, lambda: &TypedLambda) -> HashSet<BindingId> {
        let parameters = lambda.params.iter().map(|param| param.binding).collect();
        let mut borrows = Self {
            program,
            parameters,
            aliases: HashMap::new(),
        };
        let mut flows = borrows.block(&lambda.body);
        let local_borrow_callees = LocalBorrowCalls::of(program, lambda);
        flows.retain(|binding| !local_borrow_callees.contains(binding));
        flows
    }

    fn block(&mut self, block: &TypedBlock) -> HashSet<BindingId> {
        for item in &block.items {
            if let TypedBodyItem::Let(binder, value) = item {
                let flows = self.value(value);
                self.aliases.insert(binder.binding, flows);
            }
        }
        match block.items.last() {
            Some(TypedBodyItem::Expr(expr)) => self.value(expr),
            _ => HashSet::new(),
        }
    }

    fn value(&mut self, expr: &TypedExpr) -> HashSet<BindingId> {
        if !self.carries_borrow(&expr.ty) {
            return HashSet::new();
        }
        match &expr.kind {
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => self.local(*binding),
            TypedExprKind::Control(control) => self.control(control),
            TypedExprKind::Call(callee, _) => self.call(callee, expr),
            _ => self.children(expr),
        }
    }

    fn carries_borrow(&self, ty: &Type) -> bool {
        self.program.type_holds_borrow(ty) || ty.is_unsized()
    }

    fn call(&mut self, callee: &TypedExpr, call: &TypedExpr) -> HashSet<BindingId> {
        let mut flows = self.children(call);
        if let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = callee.kind {
            flows.extend(self.local(binding));
        }
        flows
    }

    fn control(&mut self, control: &TypedControl) -> HashSet<BindingId> {
        match control {
            TypedControl::Block(block) => self.block(block),
            TypedControl::If(_, then, otherwise) => self.branches(then, otherwise),
            TypedControl::Match(matched) => self.matched(matched),
        }
    }

    fn branches(&mut self, then: &TypedExpr, otherwise: &TypedExpr) -> HashSet<BindingId> {
        let mut flows = self.value(then);
        flows.extend(self.value(otherwise));
        flows
    }

    fn matched(&mut self, matched: &crate::types::TypedMatch) -> HashSet<BindingId> {
        let mut flows = self.value(&matched.value);
        for arm in &matched.arms {
            flows.extend(self.block(&arm.body));
        }
        flows
    }

    fn children(&mut self, expr: &TypedExpr) -> HashSet<BindingId> {
        expr.children()
            .into_iter()
            .flat_map(|child| self.value(child))
            .collect()
    }

    fn local(&self, binding: BindingId) -> HashSet<BindingId> {
        if self.parameters.contains(&binding) {
            HashSet::from([binding])
        } else {
            self.aliases.get(&binding).cloned().unwrap_or_default()
        }
    }
}

struct LocalBorrowCalls<'a> {
    program: &'a TypedProgram,
    outliving: HashSet<BindingId>,
    callees: HashSet<BindingId>,
}

impl<'a> LocalBorrowCalls<'a> {
    fn of(program: &'a TypedProgram, lambda: &TypedLambda) -> HashSet<BindingId> {
        let outliving = lambda
            .params
            .iter()
            .filter(|param| program.type_holds_borrow(&param.ty))
            .map(|param| param.binding)
            .collect();
        let mut calls = Self {
            program,
            outliving,
            callees: HashSet::new(),
        };
        calls.block(&lambda.body);
        calls.callees
    }

    fn block(&mut self, block: &TypedBlock) {
        for item in &block.items {
            match item {
                TypedBodyItem::Let(binder, value) => self.binding(binder, value),
                TypedBodyItem::Expr(expr) => self.expr(expr),
            }
        }
    }

    fn binding(&mut self, binder: &TypedBinder, value: &TypedExpr) {
        self.expr(value);
        if self.program.type_holds_borrow(&binder.ty) && self.outlives_body(value) {
            self.outliving.insert(binder.binding);
        }
    }

    fn expr(&mut self, expr: &TypedExpr) {
        if let TypedExprKind::Call(callee, arguments) = &expr.kind {
            self.call(callee, arguments);
        }
        match &expr.kind {
            TypedExprKind::Fn(lambda) => self.block(&lambda.body),
            TypedExprKind::Control(TypedControl::Block(block)) => self.block(block),
            TypedExprKind::Control(TypedControl::Match(matched)) => self.matched(matched),
            _ => expr
                .children()
                .into_iter()
                .for_each(|child| self.expr(child)),
        }
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &[TypedArgument]) {
        if let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = callee.kind
            && !self.arguments_outlive_body(arguments)
        {
            self.callees.insert(binding);
        }
    }

    fn matched(&mut self, matched: &crate::types::TypedMatch) {
        self.expr(&matched.value);
        for arm in &matched.arms {
            self.block(&arm.body);
        }
    }

    fn arguments_outlive_body(&self, arguments: &[TypedArgument]) -> bool {
        arguments
            .iter()
            .all(|argument| self.argument_outlives_body(argument))
    }

    fn argument_outlives_body(&self, argument: &TypedArgument) -> bool {
        if self.program.type_holds_borrow(&argument.value.ty) {
            self.outlives_body(&argument.value)
        } else {
            argument.borrow == Borrow::None
        }
    }

    fn outlives_body(&self, expr: &TypedExpr) -> bool {
        match &expr.kind {
            TypedExprKind::Leaf(TypedLeaf::Literal(_) | TypedLeaf::Global(_)) => true,
            TypedExprKind::Leaf(TypedLeaf::Local(binding)) => self.outliving.contains(binding),
            TypedExprKind::Call(_, arguments) => self.arguments_outlive_body(arguments),
            _ => false,
        }
    }
}

pub struct ItemEmitter<'a> {
    program: &'a TypedProgram,
    plan: &'a EmitPlan<'a>,
    unsized_parameters: &'a UnsizedParameters,
    pub problems: Vec<Diagnostic>,
    pub names: Vec<(String, String)>,
}

impl<'a> ItemEmitter<'a> {
    const RUNTIME_MODULE: &'static str = "rt";

    pub fn new(
        program: &'a TypedProgram,
        plan: &'a EmitPlan<'a>,
        unsized_parameters: &'a UnsizedParameters,
    ) -> Self {
        Self {
            program,
            plan,
            unsized_parameters,
            problems: Vec::new(),
            names: Vec::new(),
        }
    }

    pub fn crate_fragments(&mut self) -> Vec<Fragment> {
        let mut fragments = Self::separated(self.module_items(self.program.entry));
        let mut tree = ModuleTree::default();
        for module in &self.program.modules {
            if module.id != self.program.entry {
                let items = self.module_items(module.id);
                if !items.is_empty() {
                    tree.insert(&self.plan.module_segments(module.id), items);
                }
            }
        }
        for (name, child) in &tree.children {
            fragments.extend([Fragment::Newline, Fragment::Newline]);
            fragments.extend(child.fragments(name, "mod"));
        }
        if let Some(runtime) = RuntimeModule::of(&Code::text_of(&fragments)) {
            fragments.extend([Fragment::Newline, Fragment::Newline]);
            fragments.extend(Self::runtime(&runtime.source()));
        }
        fragments.push(Fragment::Newline);
        fragments
    }

    pub fn separated(items: Vec<Vec<Fragment>>) -> Vec<Fragment> {
        let mut fragments = Vec::new();
        for (position, item) in items.into_iter().enumerate() {
            if position > 0 {
                fragments.extend([Fragment::Newline, Fragment::Newline]);
            }
            fragments.extend(item);
        }
        fragments
    }

    fn module_items(&mut self, module: ModuleId) -> Vec<Vec<Fragment>> {
        let mut items: Vec<Vec<Fragment>> = self
            .plan
            .items_of(module)
            .into_iter()
            .map(|binding| self.item(binding))
            .collect();
        items.extend(self.traits(module));
        items.extend(self.trait_impls(module));
        items.extend(self.conversions(module));
        let uses = self.use_lines(module);
        if !uses.is_empty() {
            items.insert(0, uses);
        }
        items
    }

    fn conversions(&self, module: ModuleId) -> Vec<Vec<Fragment>> {
        self.program
            .conversions
            .iter()
            .filter(|conversion| {
                self.plan
                    .item(conversion.enum_binding)
                    .is_some_and(|info| info.module == module)
            })
            .map(|conversion| self.conversion(conversion))
            .collect()
    }

    fn conversion(&self, conversion: &TypedConversion) -> Vec<Fragment> {
        let info = self
            .plan
            .item(conversion.enum_binding)
            .expect("a used conversion names a planned enum");
        let definition = self
            .program
            .enums
            .get(conversion.enum_binding)
            .expect("a conversion names an enum");
        let variant = &definition.variants[conversion.variant];
        let types = RustTypes::new(
            self.program,
            self.plan,
            self.unsized_parameters,
            info.module,
        );
        let error = types.show(&conversion.error, TypePosition::Signature);
        let variant_name = NameMangler::variant_name(&variant.name);
        vec![
            Fragment::Text(format!("impl From<{error}> for {} {{", info.name)),
            Fragment::Indented(vec![
                Fragment::Newline,
                Fragment::Text(format!("fn from(error: {error}) -> Self {{")),
                Fragment::Indented(vec![
                    Fragment::Newline,
                    Fragment::Text(format!("{}::{variant_name}(error)", info.name)),
                ]),
                Fragment::Newline,
                Fragment::Text("}".to_string()),
            ]),
            Fragment::Newline,
            Fragment::Text("}".to_string()),
        ]
    }

    fn item(&mut self, binding: BindingId) -> Vec<Fragment> {
        let info = self.plan.item(binding).expect("planned item");
        self.names.push((
            info.name.clone(),
            self.program.bindings.get(binding).name().to_string(),
        ));
        if self.plan.is_result_main(binding) {
            let definition = self
                .plan
                .definition(binding)
                .expect("a planned main has a definition");
            return self.result_main(definition, info);
        }
        match (info.kind, self.plan.definition(binding)) {
            (ItemKind::Struct, _) => self.structure(binding, info),
            (ItemKind::Enum, _) => self.enumeration(binding, info),
            (ItemKind::Function, definition) => self.function(
                definition.expect("a planned function has a definition"),
                info,
            ),
            (ItemKind::Const, definition) => self.constant(
                definition.expect("a planned constant has a definition"),
                info,
                false,
            ),
            (ItemKind::Static, definition) => self.constant(
                definition.expect("a planned static has a definition"),
                info,
                true,
            ),
        }
    }

    fn traits(&mut self, module: ModuleId) -> Vec<Vec<Fragment>> {
        let definitions: Vec<TraitHeader> = self
            .program
            .trait_definitions
            .iter()
            .filter(|trait_definition| trait_definition.module == module)
            .map(|trait_definition| TraitHeader {
                binding: trait_definition.binding,
                rust_name: trait_definition.rust_name.clone(),
                supertraits: trait_definition.supertraits.clone(),
                associated: trait_definition.associated.clone(),
                methods: trait_definition.methods.clone(),
            })
            .collect();
        definitions
            .into_iter()
            .map(|trait_definition| {
                let mut methods = Vec::new();
                for associated in &trait_definition.associated {
                    methods.push(Fragment::Newline);
                    methods.push(Fragment::Text(format!(
                        "type {};",
                        NameMangler::type_name(&associated.name)
                    )));
                }
                for method in &trait_definition.methods {
                    methods.push(Fragment::Newline);
                    methods.extend(self.trait_method(module, method));
                }
                let visibility = self.trait_visibility(module, trait_definition.binding);
                let supertraits = self.supertrait_paths(&trait_definition.supertraits, module);
                let header = if supertraits.is_empty() {
                    format!("{visibility}trait {} {{", trait_definition.rust_name)
                } else {
                    format!(
                        "{visibility}trait {}: {} {{",
                        trait_definition.rust_name,
                        supertraits.join(" + ")
                    )
                };
                vec![
                    Fragment::Text(header),
                    Fragment::Indented(methods),
                    Fragment::Newline,
                    Fragment::Text("}".to_string()),
                ]
            })
            .collect()
    }

    fn supertrait_paths(&self, supertraits: &[Bound], module: ModuleId) -> Vec<String> {
        supertraits
            .iter()
            .map(|supertrait| self.plan.trait_path(supertrait.binding, module))
            .collect()
    }

    fn trait_visibility(&self, module: ModuleId, binding: BindingId) -> &'static str {
        let exported = self
            .program
            .modules
            .iter()
            .find(|candidate| candidate.id == module)
            .is_some_and(|candidate| candidate.exports.contains(&binding));
        if exported { "pub " } else { "" }
    }

    fn trait_method(&mut self, module: ModuleId, method: &TypedTraitMethod) -> Vec<Fragment> {
        let declaration = method.default.unwrap_or(method.binding);
        let definition = self
            .plan
            .definition(declaration)
            .expect("a trait method has a definition");
        let Type::Function(_, parameters, result) = &definition.scheme.body else {
            unreachable!("a trait method has a function type")
        };
        let self_generic = definition.scheme.params.first().map(|param| param.id);
        let types = RustTypes::new(
            self.program,
            self.plan,
            self.unsized_parameters,
            self.program.entry,
        );
        let inferred = self.inferred_lifetime_for_method(
            &definition.scheme.lifetimes,
            &definition.value,
            method.receiver.is_some(),
        );
        let scope = self.lifetime_scope(&definition.scheme.lifetimes, inferred.as_deref());
        let mut rendered = Vec::new();
        if let Some(kind) = method.receiver {
            let lifetime = parameters.first().and_then(|parameter| {
                types.reference_lifetime_name(parameter, TypePosition::Signature, &scope)
            });
            rendered.push(Self::receiver_keyword(kind, lifetime.as_deref()));
        }
        let first = usize::from(method.receiver.is_some());
        rendered.extend(parameters.iter().skip(first).map(|parameter| {
            format!(
                "_: {}",
                Self::show_self_type(
                    parameter,
                    self_generic,
                    &types,
                    TypePosition::Signature,
                    &scope,
                )
            )
        }));
        let result = if RustTypes::is_unit(result) {
            String::new()
        } else {
            format!(
                " -> {}",
                Self::show_self_type(
                    result,
                    self_generic,
                    &types,
                    TypePosition::Signature,
                    &scope,
                )
            )
        };
        let name = NameMangler::value(self.program.bindings.get(method.binding).name());
        let generics = Self::with_lifetimes(
            &self.declared_lifetimes(inferred.as_deref(), &definition.scheme.lifetimes),
            String::new(),
        );
        let header = format!("fn {name}{generics}({}){result}", rendered.join(", "));
        if method.default.is_none() {
            return vec![Fragment::Text(format!("{header};"))];
        }
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            unreachable!("a default method has a function value")
        };
        let lambda = std::rc::Rc::clone(lambda);
        let mut emitter = self.expressions(module, &definition.value);
        emitter.prepare_tail_loop(declaration, &lambda);
        if method.receiver.is_some() {
            emitter.locals.bind_receiver(lambda.params[0].binding);
        }
        let body = emitter.function_body(&lambda.body);
        self.absorb(emitter);
        vec![
            Fragment::Text(format!("{header} {{")),
            Fragment::Indented(body),
            Fragment::Newline,
            Fragment::Text("}".to_string()),
        ]
    }

    fn receiver_keyword(kind: ReceiverKind, lifetime: Option<&str>) -> String {
        let lifetime = lifetime.map_or_else(String::new, |lifetime| format!("{lifetime} "));
        match kind {
            ReceiverKind::Owned => "self".to_string(),
            ReceiverKind::Shared => format!("&{lifetime}self"),
            ReceiverKind::Mutable => format!("&{lifetime}mut self"),
        }
    }

    fn show_self_type(
        ty: &Type,
        self_generic: Option<GenericId>,
        types: &RustTypes,
        position: TypePosition,
        scope: &LifetimeScope,
    ) -> String {
        if Self::is_self_type(ty, self_generic) {
            return "Self".to_string();
        }
        Self::show_reference(ty, self_generic, types, position, scope)
            .unwrap_or_else(|| types.show_in(ty, position, scope))
    }

    fn is_self_type(ty: &Type, self_generic: Option<GenericId>) -> bool {
        matches!(ty, Type::Generic(generic) if Some(*generic) == self_generic)
    }

    fn show_reference(
        ty: &Type,
        self_generic: Option<GenericId>,
        types: &RustTypes,
        position: TypePosition,
        scope: &LifetimeScope,
    ) -> Option<String> {
        let (lifetime, mutability, inner) = match ty {
            Type::Ref(lifetime, inner) => (*lifetime, "", inner),
            Type::Mut(lifetime, inner) => (*lifetime, "mut ", inner),
            _ => return None,
        };
        let lifetime = types
            .lifetime_name(lifetime, position, scope)
            .map_or_else(String::new, |name| format!("{name} "));
        Some(format!(
            "&{lifetime}{mutability}{}",
            Self::show_self_type(inner, self_generic, types, position, scope)
        ))
    }

    fn trait_impls(&mut self, module: ModuleId) -> Vec<Vec<Fragment>> {
        let mut items = Vec::new();
        for position in 0..self.program.trait_impls.len() {
            if self.program.trait_impls[position].module != module {
                continue;
            }
            let trait_binding = self.program.trait_impls[position].trait_binding;
            let implementor = self.program.trait_impls[position].implementor.clone();
            let associated = self.program.trait_impls[position].associated.clone();
            let methods = self.program.trait_impls[position].methods.clone();
            let trait_path = self.plan.trait_path(trait_binding, module);
            let lifetime = self.implementor_lifetime(&implementor);
            let params = self.program.trait_impls[position].params.clone();
            let (type_name, generics) = {
                let types =
                    RustTypes::new(self.program, self.plan, self.unsized_parameters, module);
                let scope = LifetimeScope::default().with_elided(lifetime.clone());
                let type_name = types.show_in(&implementor, TypePosition::Signature, &scope);
                let declared = types.generics(&params, &implementor, &HashSet::new());
                let lifetimes: Vec<String> = lifetime.iter().cloned().collect();
                (type_name, Self::with_lifetimes(&lifetimes, declared))
            };
            let associated_lines: Vec<String> = {
                let types =
                    RustTypes::new(self.program, self.plan, self.unsized_parameters, module);
                let scope = LifetimeScope::default().with_elided(lifetime.clone());
                associated
                    .iter()
                    .map(|(_, name, value)| {
                        format!(
                            "type {} = {};",
                            NameMangler::type_name(name),
                            types.show_in(value, TypePosition::Signature, &scope)
                        )
                    })
                    .collect()
            };
            let mut body = Vec::new();
            for line in associated_lines {
                body.push(Fragment::Newline);
                body.push(Fragment::Text(line));
            }
            for method in &methods {
                body.push(Fragment::Newline);
                body.extend(self.impl_method(
                    method.binding,
                    method.receiver,
                    module,
                    lifetime.as_deref(),
                ));
            }
            items.push(vec![
                Fragment::Text(format!("impl{generics} {trait_path} for {type_name} {{")),
                Fragment::Indented(body),
                Fragment::Newline,
                Fragment::Text("}".to_string()),
            ]);
        }
        items
    }

    fn use_lines(&self, module: ModuleId) -> Vec<Fragment> {
        let Some(uses) = self.plan.trait_uses(module) else {
            return Vec::new();
        };
        let mut paths: Vec<String> = uses
            .iter()
            .filter(|binding| self.plan.trait_module(**binding) != Some(module))
            .map(|binding| self.plan.trait_path(*binding, module))
            .collect();
        paths.sort();
        paths.dedup();
        let mut fragments = Vec::new();
        for (position, path) in paths.iter().enumerate() {
            if position > 0 {
                fragments.push(Fragment::Newline);
            }
            fragments.push(Fragment::Text(format!("use {path};")));
        }
        fragments
    }

    fn impl_method(
        &mut self,
        binding: BindingId,
        receiver: Option<ReceiverKind>,
        module: ModuleId,
        reserved_lifetime: Option<&str>,
    ) -> Vec<Fragment> {
        let definition = self
            .plan
            .definition(binding)
            .expect("an implementation method has a definition");
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            unreachable!("an implementation method is a function")
        };
        let mut emitter = self.expressions(module, &definition.value);
        emitter.prepare_tail_loop(definition.binding, lambda);
        let types = emitter.types();
        let inferred = self.inferred_lifetime_for_method_avoiding(
            &definition.scheme.lifetimes,
            &definition.value,
            receiver.is_some(),
            reserved_lifetime,
        );
        let scope = self.lifetime_scope(&definition.scheme.lifetimes, inferred.as_deref());
        let used = self.used_written_lifetimes(&definition.value);
        let flowing = ResultBorrows::of(self.program, lambda);
        let mut params = Vec::new();
        if let Some(kind) = receiver {
            let first = &lambda.params[0];
            emitter.locals.bind_receiver(first.binding);
            let receiver_lifetime = self.receiver_lifetime(first, &scope);
            params.push(Self::receiver_keyword(kind, receiver_lifetime.as_deref()));
            params.extend(lambda.params[1..].iter().map(|param| {
                self.bind_parameter_lifetime(
                    &mut emitter,
                    param,
                    &definition.scheme.lifetimes,
                    &used,
                    inferred.as_deref(),
                    &flowing,
                )
            }));
        } else {
            params.extend(lambda.params.iter().map(|param| {
                self.bind_parameter_lifetime(
                    &mut emitter,
                    param,
                    &definition.scheme.lifetimes,
                    &used,
                    inferred.as_deref(),
                    &flowing,
                )
            }));
        }
        let result = match &definition.value.ty {
            Type::Function(_, parameters, result) if !RustTypes::is_unit(result) => {
                let position = self.result_position(parameters, inferred.as_deref());
                format!(" -> {}", types.show_in(result, position, &scope))
            }
            _ => String::new(),
        };
        let body = emitter.function_body(&lambda.body);
        self.absorb(emitter);
        let name = NameMangler::value(self.program.bindings.get(binding).name());
        let generics = Self::with_lifetimes(
            &self.declared_lifetimes(inferred.as_deref(), &definition.scheme.lifetimes),
            String::new(),
        );
        vec![Fragment::Spanned(
            definition.span,
            vec![
                Fragment::Text(format!(
                    "fn {name}{generics}({}){result} {{",
                    params.join(", ")
                )),
                Fragment::Indented(body),
                Fragment::Newline,
                Fragment::Text("}".to_string()),
            ],
        )]
    }

    fn result_main(&mut self, definition: &TypedDefinition, info: &ItemInfo) -> Vec<Fragment> {
        let mut fragments = self.function(definition, info);
        fragments.extend([Fragment::Newline, Fragment::Newline]);
        fragments.extend(self.main_wrapper(definition.span));
        fragments
    }

    fn main_wrapper(&self, span: Span) -> Vec<Fragment> {
        let body = self
            .plan
            .main_body_name()
            .expect("a result main has a body name");
        vec![Fragment::Spanned(
            span,
            vec![
                Fragment::Text("fn main() {".to_string()),
                Fragment::Indented(vec![
                    Fragment::Newline,
                    Fragment::Text(format!("if let Err(error) = {body}() {{")),
                    Fragment::Indented(vec![
                        Fragment::Newline,
                        Fragment::Text("eprintln!(\"{}\", error);".to_string()),
                        Fragment::Newline,
                        Fragment::Text("std::process::exit(1);".to_string()),
                    ]),
                    Fragment::Newline,
                    Fragment::Text("}".to_string()),
                ]),
                Fragment::Newline,
                Fragment::Text("}".to_string()),
            ],
        )]
    }

    fn bind_parameter_lifetime(
        &self,
        emitter: &mut ExpressionEmitter<'_>,
        param: &TypedBinder,
        lifetimes: &[BindingId],
        used: &HashSet<BindingId>,
        inferred: Option<&str>,
        flowing: &HashSet<BindingId>,
    ) -> String {
        let scope = self.parameter_scope(param, lifetimes, used, inferred, flowing);
        emitter.bind_parameter_in(param, TypePosition::Signature, &scope)
    }

    fn parameter_scope(
        &self,
        param: &TypedBinder,
        lifetimes: &[BindingId],
        used: &HashSet<BindingId>,
        inferred: Option<&str>,
        flowing: &HashSet<BindingId>,
    ) -> LifetimeScope {
        let elided = if flowing.contains(&param.binding) {
            inferred
        } else {
            None
        };
        self.lifetime_scope_used(lifetimes, used, elided)
            .reserving(inferred)
    }

    fn function(&mut self, definition: &TypedDefinition, info: &ItemInfo) -> Vec<Fragment> {
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            return Vec::new();
        };
        let mut emitter = self.expressions(info.module, &definition.value);
        emitter.prepare_tail_loop(definition.binding, lambda);
        emitter.prepare_stored_functions(lambda);
        let types = emitter.types();
        let used = self.used_written_lifetimes(&definition.value);
        let inferred = self.inferred_lifetime(&definition.scheme.lifetimes, &definition.value);
        let scope =
            self.lifetime_scope_used(&definition.scheme.lifetimes, &used, inferred.as_deref());
        let written: Vec<BindingId> = definition
            .scheme
            .lifetimes
            .iter()
            .copied()
            .filter(|binding| used.contains(binding))
            .collect();
        let flowing = ResultBorrows::of(self.program, lambda);
        let opaque_lifetimes = Rc::new(FreshLifetimes::new(
            self.declared_lifetimes(inferred.as_deref(), &definition.scheme.lifetimes),
        ));
        let params: Vec<String> = lambda
            .params
            .iter()
            .map(|param| {
                let scope = self
                    .parameter_scope(
                        param,
                        &definition.scheme.lifetimes,
                        &used,
                        inferred.as_deref(),
                        &flowing,
                    )
                    .naming_opaque_lifetimes(&opaque_lifetimes);
                emitter.bind_parameter_in(param, TypePosition::Signature, &scope)
            })
            .collect();
        let result = match &definition.value.ty {
            Type::Function(_, parameters, result) if !RustTypes::is_unit(result) => {
                let position = self.result_position(parameters, inferred.as_deref());
                format!(" -> {}", types.show_in(result, position, &scope))
            }
            _ => String::new(),
        };
        let literal_generics = LiteralGenerics::of(&definition.value);
        let mut lifetimes = self.declared_lifetimes(inferred.as_deref(), &written);
        lifetimes.extend(opaque_lifetimes.issued());
        let generics = Self::with_lifetimes(
            &lifetimes,
            types.generics(
                &definition.scheme.params,
                &definition.scheme.body,
                &literal_generics,
            ),
        );
        let where_clause = types.where_clause(&definition.scheme.params);
        let header = format!(
            "{}fn {}{generics}({}){result}{where_clause} {{",
            info.visibility.keyword(),
            info.name,
            params.join(", ")
        );
        let body = emitter.function_body(&lambda.body);
        self.absorb(emitter);
        vec![Fragment::Spanned(
            definition.span,
            vec![
                Fragment::Text(header),
                Fragment::Indented(body),
                Fragment::Newline,
                Fragment::Text("}".to_string()),
            ],
        )]
    }

    fn constant(
        &mut self,
        definition: &TypedDefinition,
        info: &ItemInfo,
        is_lazy: bool,
    ) -> Vec<Fragment> {
        let mut emitter = self.expressions(info.module, &definition.value);
        let value_type = emitter
            .types()
            .show(&definition.scheme.body, TypePosition::Value);
        let value = emitter.expr(&definition.value);
        self.absorb(emitter);
        let visibility = info.visibility.keyword();
        let (head, tail) = if is_lazy {
            (
                format!(
                    "{visibility}static {}: std::sync::LazyLock<{value_type}> = std::sync::LazyLock::new(|| ",
                    info.name
                ),
                ");",
            )
        } else {
            (
                format!("{visibility}const {}: {value_type} = ", info.name),
                ";",
            )
        };
        let mut fragments = vec![Fragment::Text(head)];
        fragments.extend(value.fragments);
        fragments.push(Fragment::Text(tail.to_string()));
        vec![Fragment::Spanned(definition.span, fragments)]
    }

    fn structure(&mut self, binding: BindingId, info: &ItemInfo) -> Vec<Fragment> {
        let types = RustTypes::new(
            self.program,
            self.plan,
            self.unsized_parameters,
            info.module,
        );
        let visibility = info.visibility.keyword();
        let definition = self
            .program
            .structs
            .get(binding)
            .expect("a reached struct has a definition");
        let inferred = (definition.lifetimes.is_empty()
            && self.program.definition_holds_borrow(binding))
        .then(|| Self::unused_lifetime_name(self.program, &definition.lifetimes));
        let lifetimes = self.declared_lifetimes(inferred.as_deref(), &definition.lifetimes);
        let scope =
            self.lifetime_scope(&definition.lifetimes, lifetimes.first().map(String::as_str));
        let mut fields = Vec::new();
        for ((name, field_type), span) in definition.fields.iter().zip(&definition.field_spans) {
            fields.push(Fragment::Newline);
            fields.push(Fragment::Text(format!(
                "{visibility}{}: ",
                NameMangler::value(name)
            )));
            fields.push(Fragment::Spanned(
                *span,
                vec![Fragment::Text(types.show_in(
                    field_type,
                    TypePosition::Value,
                    &scope,
                ))],
            ));
            fields.push(Fragment::Text(",".to_string()));
        }
        let params = Self::struct_params(&definition.params, &lifetimes);
        let mut fragments = Vec::new();
        if !definition.derives.is_empty() {
            fragments.push(Fragment::Text(format!(
                "#[derive({})]",
                definition.derives.join(", ")
            )));
            fragments.push(Fragment::Newline);
        }
        fragments.extend([
            self.spanned_head(binding, format!("{visibility}struct {}{params}", info.name)),
            Fragment::Text(" {".to_string()),
            Fragment::Indented(fields),
            Fragment::Newline,
            Fragment::Text("}".to_string()),
        ]);
        fragments
    }

    fn enumeration(&mut self, binding: BindingId, info: &ItemInfo) -> Vec<Fragment> {
        let types = RustTypes::new(
            self.program,
            self.plan,
            self.unsized_parameters,
            info.module,
        );
        let visibility = info.visibility.keyword();
        let definition = self
            .program
            .enums
            .get(binding)
            .expect("a reached enum has a definition");
        let inferred = (definition.lifetimes.is_empty()
            && self.program.definition_holds_borrow(binding))
        .then(|| Self::unused_lifetime_name(self.program, &definition.lifetimes));
        let lifetimes = self.declared_lifetimes(inferred.as_deref(), &definition.lifetimes);
        let scope =
            self.lifetime_scope(&definition.lifetimes, lifetimes.first().map(String::as_str));
        let params = Self::struct_params(&definition.params, &lifetimes);
        let mut variants = Vec::new();
        for variant in &definition.variants {
            variants.push(Fragment::Newline);
            variants.extend(Self::variant(variant, &types, &scope));
        }
        let mut fragments = Vec::new();
        if !definition.derives.is_empty() {
            fragments.push(Fragment::Text(format!(
                "#[derive({})]",
                definition.derives.join(", ")
            )));
            fragments.push(Fragment::Newline);
        }
        fragments.extend([
            self.spanned_head(binding, format!("{visibility}enum {}{params}", info.name)),
            Fragment::Text(" {".to_string()),
            Fragment::Indented(variants),
            Fragment::Newline,
            Fragment::Text("}".to_string()),
        ]);
        fragments
    }

    fn variant(
        variant: &VariantDefinition,
        types: &RustTypes,
        scope: &LifetimeScope,
    ) -> Vec<Fragment> {
        let mut fragments = vec![Fragment::Text(NameMangler::variant_name(&variant.name))];
        fragments.extend(Self::variant_payload(variant, types, scope));
        fragments.push(Fragment::Text(",".to_string()));
        fragments
    }

    fn variant_payload(
        variant: &VariantDefinition,
        types: &RustTypes,
        scope: &LifetimeScope,
    ) -> Vec<Fragment> {
        match &variant.payload {
            crate::types::VariantPayload::None => Vec::new(),
            crate::types::VariantPayload::Positional(payload_types) => {
                Self::positional_payload(payload_types, &variant.payload_spans, types, scope)
            }
            crate::types::VariantPayload::Named(fields) => {
                Self::named_payload(fields, &variant.payload_spans, types, scope)
            }
        }
    }

    fn positional_payload(
        payload_types: &[Type],
        spans: &[Span],
        types: &RustTypes,
        scope: &LifetimeScope,
    ) -> Vec<Fragment> {
        let mut fragments = vec![Fragment::Text("(".to_string())];
        for (position, (payload, span)) in payload_types.iter().zip(spans).enumerate() {
            if position > 0 {
                fragments.push(Fragment::Text(", ".to_string()));
            }
            fragments.push(Fragment::Spanned(
                *span,
                vec![Fragment::Text(types.show_in(
                    payload,
                    TypePosition::Value,
                    scope,
                ))],
            ));
        }
        fragments.push(Fragment::Text(")".to_string()));
        fragments
    }

    fn named_payload(
        fields: &[(std::rc::Rc<str>, Type)],
        spans: &[Span],
        types: &RustTypes,
        scope: &LifetimeScope,
    ) -> Vec<Fragment> {
        let mut fragments = vec![Fragment::Text(" { ".to_string())];
        for (position, ((label, field_type), span)) in fields.iter().zip(spans).enumerate() {
            if position > 0 {
                fragments.push(Fragment::Text(", ".to_string()));
            }
            fragments.push(Fragment::Text(format!("{}: ", NameMangler::value(label))));
            fragments.push(Fragment::Spanned(
                *span,
                vec![Fragment::Text(types.show_in(
                    field_type,
                    TypePosition::Value,
                    scope,
                ))],
            ));
        }
        fragments.push(Fragment::Text(" }".to_string()));
        fragments
    }

    fn spanned_head(&self, binding: BindingId, head: String) -> Fragment {
        match self.program.bindings.get(binding).span() {
            Some(span) => Fragment::Spanned(span, vec![Fragment::Text(head)]),
            None => Fragment::Text(head),
        }
    }

    fn struct_params(params: &[GenericParam], lifetimes: &[String]) -> String {
        if params.is_empty() && lifetimes.is_empty() {
            return String::new();
        }
        let mut names = lifetimes.to_vec();
        names.extend(params.iter().map(RustTypes::generic_name));
        format!("<{}>", names.join(", "))
    }

    fn result_position(&self, parameters: &[Type], inferred: Option<&str>) -> TypePosition {
        let holds_borrow = parameters
            .iter()
            .any(|parameter| self.program.type_holds_borrow(parameter));
        let shares_lifetime = inferred.is_some();
        if holds_borrow || shares_lifetime {
            TypePosition::Signature
        } else {
            TypePosition::StaticResult
        }
    }

    fn needs_shared_lifetime(&self, value: &TypedExpr) -> bool {
        let Type::Function(_, parameters, result) = &value.ty else {
            return false;
        };
        self.program
            .function_type_needs_shared_lifetime(parameters, result)
            || (self.program.borrow_lifetimes(result) > 0 && self.result_flows_past_elision(value))
    }

    fn result_flows_past_elision(&self, value: &TypedExpr) -> bool {
        let TypedExprKind::Fn(lambda) = &value.kind else {
            return false;
        };
        let flowing = ResultBorrows::of(self.program, lambda);
        lambda.params.iter().any(|param| {
            flowing.contains(&param.binding) && self.hides_borrow_from_elision(&param.ty)
        })
    }

    fn hides_borrow_from_elision(&self, ty: &Type) -> bool {
        SignatureBorrows::new(self.program).is_borrowing_function(ty)
            || OpaqueBorrows::new(self.program).holds_borrow(ty)
    }

    fn inferred_lifetime(&self, lifetimes: &[BindingId], value: &TypedExpr) -> Option<String> {
        self.inferred_lifetime_avoiding(lifetimes, value, None)
    }

    fn inferred_lifetime_for_method(
        &self,
        lifetimes: &[BindingId],
        value: &TypedExpr,
        has_receiver: bool,
    ) -> Option<String> {
        self.inferred_lifetime_for_method_avoiding(lifetimes, value, has_receiver, None)
    }

    fn inferred_lifetime_avoiding(
        &self,
        lifetimes: &[BindingId],
        value: &TypedExpr,
        reserved: Option<&str>,
    ) -> Option<String> {
        self.inferred_lifetime_for_method_avoiding(lifetimes, value, false, reserved)
    }

    fn inferred_lifetime_for_method_avoiding(
        &self,
        lifetimes: &[BindingId],
        value: &TypedExpr,
        has_receiver: bool,
        reserved: Option<&str>,
    ) -> Option<String> {
        if has_receiver {
            return None;
        }
        self.needs_shared_lifetime(value).then(|| {
            let mut used: Vec<String> = lifetimes
                .iter()
                .map(|binding| RustTypes::binding_lifetime(self.program, *binding))
                .collect();
            used.extend(reserved.map(str::to_string));
            FreshLifetimes::unused_name(&used)
        })
    }

    fn unused_lifetime_name(program: &TypedProgram, lifetimes: &[BindingId]) -> String {
        let used: Vec<String> = lifetimes
            .iter()
            .map(|binding| RustTypes::binding_lifetime(program, *binding))
            .collect();
        FreshLifetimes::unused_name(&used)
    }

    fn used_written_lifetimes(&self, value: &TypedExpr) -> HashSet<BindingId> {
        let mut used = Self::linking_written_lifetimes(&value.ty);
        used.extend(OpaqueBorrows::new(self.program).written_lifetimes(&value.ty));
        used
    }

    fn linking_written_lifetimes(ty: &Type) -> HashSet<BindingId> {
        let mut counts: HashMap<BindingId, usize> = HashMap::new();
        Self::count_written_lifetimes(ty, &mut counts);
        counts
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(binding, _)| binding)
            .collect()
    }

    fn count_written_lifetimes(ty: &Type, counts: &mut HashMap<BindingId, usize>) {
        match ty {
            Type::Ref(..) | Type::Mut(..) => Self::count_reference_lifetime(ty, counts),
            Type::Named(_, arguments) => {
                for argument in arguments {
                    Self::count_written_lifetimes(argument, counts);
                }
            }
            Type::Function(_, parameters, result) => {
                for parameter in parameters {
                    Self::count_written_lifetimes(parameter, counts);
                }
                Self::count_written_lifetimes(result, counts);
            }
            _ => {}
        }
    }

    fn count_reference_lifetime(ty: &Type, counts: &mut HashMap<BindingId, usize>) {
        let (lifetime, inner) = match ty {
            Type::Ref(lifetime, inner) | Type::Mut(lifetime, inner) => (lifetime, inner),
            _ => unreachable!("the caller matches a reference"),
        };
        if let Lifetime::Written(binding) = lifetime {
            *counts.entry(*binding).or_default() += 1;
        }
        Self::count_written_lifetimes(inner, counts);
    }

    fn lifetime_scope_used(
        &self,
        lifetimes: &[BindingId],
        used: &HashSet<BindingId>,
        elided: Option<&str>,
    ) -> LifetimeScope {
        lifetimes
            .iter()
            .fold(self.lifetime_scope(lifetimes, elided), |scope, binding| {
                if used.contains(binding) {
                    scope
                } else {
                    scope.eliding(*binding)
                }
            })
    }

    fn lifetime_scope(&self, lifetimes: &[BindingId], elided: Option<&str>) -> LifetimeScope {
        LifetimeScope::new(
            lifetimes
                .iter()
                .map(|binding| {
                    (
                        *binding,
                        RustTypes::binding_lifetime(self.program, *binding),
                    )
                })
                .collect(),
        )
        .with_elided(elided.map(str::to_string))
    }

    fn declared_lifetimes(&self, inferred: Option<&str>, written: &[BindingId]) -> Vec<String> {
        let mut lifetimes: Vec<String> = inferred.map(str::to_string).into_iter().collect();
        lifetimes.extend(
            written
                .iter()
                .map(|binding| RustTypes::binding_lifetime(self.program, *binding)),
        );
        lifetimes
    }

    fn with_lifetimes(lifetimes: &[String], generics: String) -> String {
        if lifetimes.is_empty() {
            return generics;
        }
        let mut parts = lifetimes.to_vec();
        if !generics.is_empty() {
            parts.push(generics[1..generics.len() - 1].to_string());
        }
        format!("<{}>", parts.join(", "))
    }

    fn implementor_lifetime(&self, implementor: &Type) -> Option<String> {
        let Type::Named(binding, _) = implementor else {
            return None;
        };
        if !self.program.has_lifetime_parameter(*binding) {
            return None;
        }
        let written = if let Some(definition) = self.program.structs.get(*binding) {
            definition.lifetimes.first().copied()
        } else {
            self.program
                .enums
                .get(*binding)
                .and_then(|definition| definition.lifetimes.first().copied())
        };
        Some(written.map_or_else(
            || Self::unused_lifetime_name(self.program, &[]),
            |binding| RustTypes::binding_lifetime(self.program, binding),
        ))
    }

    fn receiver_lifetime(&self, first: &TypedBinder, scope: &LifetimeScope) -> Option<String> {
        match first.ty.reference_lifetime() {
            Some(Lifetime::Elided) => scope.elided().map(str::to_string),
            _ => None,
        }
    }

    fn expressions(&self, module: ModuleId, value: &TypedExpr) -> ExpressionEmitter<'a> {
        let reserved = self
            .plan
            .items_of(module)
            .into_iter()
            .filter_map(|binding| self.plan.item(binding))
            .map(|item| item.name.clone());
        let mut mutable = MutableLocals::of(value);
        mutable.extend(self.program.mutable_function_bindings.iter().copied());
        ExpressionEmitter::new(
            self.program,
            self.plan,
            self.unsized_parameters,
            module,
            LocalNames::new(reserved.collect::<Vec<String>>()),
        )
        .with_mutable_locals(mutable.into_set())
    }

    fn absorb(&mut self, emitter: ExpressionEmitter) {
        for (binding, name) in emitter.locals.pairs() {
            self.names.push((
                name.to_string(),
                self.program.bindings.get(binding).name().to_string(),
            ));
        }
        self.problems.extend(emitter.problems);
    }

    fn runtime(source: &str) -> Vec<Fragment> {
        let mut lines = vec![Fragment::Text(format!("mod {} {{", Self::RUNTIME_MODULE))];
        let mut inner = Vec::new();
        for line in source.lines() {
            inner.push(Fragment::Newline);
            if !line.is_empty() {
                inner.push(Fragment::Text(line.to_string()));
            }
        }
        lines.push(Fragment::Indented(inner));
        lines.push(Fragment::Newline);
        lines.push(Fragment::Text("}".to_string()));
        lines
    }
}
