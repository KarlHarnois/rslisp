use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use super::expressions::ExpressionEmitter;
use super::fragment::{Code, Fragment};
use super::fresh_lifetimes::FreshLifetimes;
use super::literal_generics::LiteralGenerics;
use super::local_closures::LocalClosures;
use super::locals::LocalNames;
use super::mangle::NameMangler;
use super::method_elisions::{MethodElisions, MethodLifetimes, TraitMethodElisions};
use super::mutable_locals::MutableLocals;
use super::opaque_borrows::OpaqueBorrows;
use super::plan::{EmitPlan, ItemInfo, ItemKind};
use super::result_borrows::{LambdaFlows, ResultBorrows, StoredFlows};
use super::runtime::RuntimeModule;
use super::signature_borrows::SignatureBorrows;
use super::types::{LifetimeScope, RustTypes, TypePosition};
use super::unsized_parameters::UnsizedParameters;
use crate::core::{BindingId, ModuleId};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    Bound, GenericId, GenericParam, Lifetime, ReceiverKind, Scheme, Type, TypedAssociatedType,
    TypedBinder, TypedConversion, TypedDefinition, TypedExpr, TypedExprKind, TypedImplMethod,
    TypedLambda, TypedProgram, TypedTraitMethod, VariantDefinition,
};

struct ImplementedTrait<'l> {
    binding: BindingId,
    implementor_lifetime: Option<&'l str>,
}

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

pub struct ItemEmitter<'a> {
    program: &'a TypedProgram,
    plan: &'a EmitPlan<'a>,
    unsized_parameters: &'a UnsizedParameters,
    result_borrows: ResultBorrows<'a>,
    trait_method_elisions: TraitMethodElisions,
    pub problems: Vec<Diagnostic>,
    pub names: Vec<(String, String)>,
}

impl<'a> ItemEmitter<'a> {
    const RUNTIME_MODULE: &'static str = "rt";
    const MAIN_HEADER: &'static str = "fn main() {";
    const SIZED_SELF: &'static str = " where Self: Sized";

    pub fn new(
        program: &'a TypedProgram,
        plan: &'a EmitPlan<'a>,
        unsized_parameters: &'a UnsizedParameters,
    ) -> Self {
        Self {
            program,
            plan,
            unsized_parameters,
            result_borrows: ResultBorrows::new(program, plan),
            trait_method_elisions: TraitMethodElisions::new(program),
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
            if let Some(call) = runtime.start_up_call() {
                Self::start_main_with(&mut fragments, call);
            }
            fragments.extend([Fragment::Newline, Fragment::Newline]);
            fragments.extend(Self::runtime(&runtime.source()));
        }
        fragments.push(Fragment::Newline);
        fragments
    }

    fn start_main_with(fragments: &mut [Fragment], call: &str) {
        let main_body = fragments.iter_mut().find_map(|fragment| match fragment {
            Fragment::Spanned(_, children) => match children.as_mut_slice() {
                [Fragment::Text(header), Fragment::Indented(body), ..]
                    if header == Self::MAIN_HEADER =>
                {
                    Some(body)
                }
                _ => None,
            },
            _ => None,
        });
        main_body
            .expect("an emitted crate holds a main function")
            .splice(0..0, [Fragment::Newline, Fragment::Text(call.to_string())]);
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
        let definition = self
            .plan
            .definition(method.declaration())
            .expect("a trait method has a definition");
        let Type::Function(_, parameters, result) = &definition.scheme.body else {
            unreachable!("a trait method has a function type")
        };
        let self_generic = Self::self_generic(&definition.scheme);
        let bound = Self::sized_self_bound(method, &definition.scheme.body, self_generic);
        let types = RustTypes::new(
            self.program,
            self.plan,
            self.unsized_parameters,
            self.program.entry,
        );
        let lifetimes = self.method_elisions().lifetimes(method.binding);
        let inferred = self
            .shared_method_lifetime(&definition.scheme.lifetimes, &lifetimes, None)
            .or_else(|| {
                self.inferred_lifetime_for_method(
                    &definition.scheme.lifetimes,
                    &definition.value,
                    method.receiver.is_some(),
                )
            });
        let scope = self.lifetime_scope(&definition.scheme.lifetimes, inferred.as_deref());
        let mut declared =
            self.declared_lifetimes(inferred.as_deref(), &definition.scheme.lifetimes);
        let opaque_lifetimes = Rc::new(FreshLifetimes::new(declared.clone()));
        let mut rendered = Vec::new();
        if let Some(kind) = method.receiver {
            let receiver_scope = Self::method_parameter_scope(&scope, &lifetimes, 0);
            let lifetime = parameters.first().and_then(|parameter| {
                types.reference_lifetime_name(parameter, TypePosition::Signature, &receiver_scope)
            });
            rendered.push(Self::receiver_keyword(kind, lifetime.as_deref()));
        }
        let first = usize::from(method.receiver.is_some());
        let parameter_types: Vec<String> = parameters
            .iter()
            .enumerate()
            .skip(first)
            .map(|(position, parameter)| {
                let parameter_scope = Self::naming_parameter_lifetimes(
                    Self::method_parameter_scope(&scope, &lifetimes, position),
                    &opaque_lifetimes,
                    lifetimes.elided(),
                    &position,
                );
                Self::show_self_type(
                    parameter,
                    self_generic,
                    &types,
                    TypePosition::Signature,
                    &parameter_scope,
                )
            })
            .collect();
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
        declared.extend(opaque_lifetimes.issued());
        let generics = Self::with_lifetimes(&declared, String::new());
        let header = |rendered: &[String]| {
            format!(
                "fn {name}{generics}({}){result}{bound}",
                rendered.join(", ")
            )
        };
        if method.default.is_none() {
            rendered.extend(parameter_types.iter().map(|ty| format!("_: {ty}")));
            return vec![Fragment::Text(format!("{};", header(&rendered)))];
        }
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            unreachable!("a default method has a function value")
        };
        let lambda = std::rc::Rc::clone(lambda);
        let mut emitter = self
            .expressions(module, &definition.value)
            .with_enclosing_lifetime(inferred.clone());
        emitter.prepare_tail_loop(method.declaration(), &lambda);
        if method.receiver.is_some() {
            emitter.locals.bind_receiver(lambda.params[0].binding);
        }
        rendered.extend(lambda.params.iter().skip(first).zip(&parameter_types).map(
            |(param, ty)| {
                let name = emitter.locals.bind(
                    param.binding,
                    self.program.bindings.get(param.binding).name(),
                );
                format!("{name}: {ty}")
            },
        ));
        let header = header(&rendered);
        let body = emitter.function_body(&lambda.body);
        self.absorb(emitter);
        vec![
            Fragment::Text(format!("{header} {{")),
            Fragment::Indented(body),
            Fragment::Newline,
            Fragment::Text("}".to_string()),
        ]
    }

    fn sized_self_bound(
        method: &TypedTraitMethod,
        signature: &Type,
        self_generic: Option<GenericId>,
    ) -> &'static str {
        let needs_sized = if method.default.is_some() {
            Self::body_needs_sized_self
        } else {
            Self::needs_sized_self
        };
        let holds_sized_self =
            |ty: &&Type| self_generic.is_some_and(|generic| needs_sized(ty, generic));
        if signature.parts().iter().any(holds_sized_self) {
            Self::SIZED_SELF
        } else {
            ""
        }
    }

    fn body_needs_sized_self(ty: &Type, generic: GenericId) -> bool {
        Self::is_self_type(ty, Some(generic)) || Self::needs_sized_self(ty, generic)
    }

    fn needs_sized_self(ty: &Type, generic: GenericId) -> bool {
        match ty {
            Type::Generic(_) => false,
            Type::Ref(_, inner) | Type::Mut(_, inner) => Self::needs_sized_self(inner, generic),
            other => other.mentions(generic),
        }
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
            let implemented = ImplementedTrait {
                binding: trait_binding,
                implementor_lifetime: lifetime.as_deref(),
            };
            for method in &methods {
                body.push(Fragment::Newline);
                body.extend(self.impl_method(*method, module, &implemented));
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
        method: TypedImplMethod,
        module: ModuleId,
        implemented: &ImplementedTrait,
    ) -> Vec<Fragment> {
        let TypedImplMethod { binding, receiver } = method;
        let reserved_lifetime = implemented.implementor_lifetime;
        let definition = self
            .plan
            .definition(binding)
            .expect("an implementation method has a definition");
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            unreachable!("an implementation method is a function")
        };
        let lifetimes = self.method_elisions().lifetimes(binding);
        let inferred = self
            .shared_method_lifetime(&definition.scheme.lifetimes, &lifetimes, reserved_lifetime)
            .or_else(|| {
                self.inferred_lifetime_for_method_avoiding(
                    &definition.scheme.lifetimes,
                    &definition.value,
                    receiver.is_some(),
                    reserved_lifetime,
                )
            });
        let signature = self.signature_naming_self(binding, &definition.value.ty, implemented);
        let Type::Function(_, shown_parameters, shown_result) = &signature else {
            unreachable!("an implementation method has a function type")
        };
        let mut emitter = self
            .expressions(module, &definition.value)
            .with_enclosing_lifetime(inferred.clone());
        emitter.prepare_tail_loop(definition.binding, lambda);
        let types = emitter.types();
        let scope = self.lifetime_scope(&definition.scheme.lifetimes, inferred.as_deref());
        let used = self.used_written_lifetimes(&definition.value);
        let flowing = self.flowing_parameters(lambda, &lifetimes);
        let mut declared =
            self.declared_lifetimes(inferred.as_deref(), &definition.scheme.lifetimes);
        let mut reserved = declared.clone();
        reserved.extend(reserved_lifetime.map(str::to_string));
        let opaque_lifetimes = Rc::new(FreshLifetimes::new(reserved));
        let mut params = Vec::new();
        if let Some(kind) = receiver {
            let first = &lambda.params[0];
            emitter.locals.bind_receiver(first.binding);
            let receiver_scope = self.parameter_scope(
                first,
                &definition.scheme.lifetimes,
                &used,
                inferred.as_deref(),
                &flowing,
            );
            let receiver_lifetime = self.receiver_lifetime(first, &receiver_scope);
            params.push(Self::receiver_keyword(kind, receiver_lifetime.as_deref()));
        }
        let first = usize::from(receiver.is_some());
        params.extend(
            lambda
                .params
                .iter()
                .zip(shown_parameters)
                .enumerate()
                .skip(first)
                .map(|(position, (param, shown))| {
                    let scope = Self::naming_parameter_lifetimes(
                        self.parameter_scope(
                            param,
                            &definition.scheme.lifetimes,
                            &used,
                            inferred.as_deref(),
                            &flowing,
                        ),
                        &opaque_lifetimes,
                        lifetimes.elided(),
                        &position,
                    );
                    emitter.bind_parameter_in(
                        &param.with_type(shown.clone()),
                        TypePosition::Signature,
                        &scope,
                    )
                }),
        );
        let result = match &definition.value.ty {
            Type::Function(_, parameters, result) if !RustTypes::is_unit(result) => {
                let position = self.result_position(parameters, inferred.as_deref());
                format!(" -> {}", types.show_in(shown_result, position, &scope))
            }
            _ => String::new(),
        };
        let body = emitter.function_body(&lambda.body);
        self.absorb(emitter);
        let name = NameMangler::value(self.program.bindings.get(binding).name());
        declared.extend(opaque_lifetimes.issued());
        let generics = Self::with_lifetimes(&declared, String::new());
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

    fn signature_naming_self(
        &self,
        method: BindingId,
        signature: &Type,
        implemented: &ImplementedTrait,
    ) -> Type {
        implemented
            .implementor_lifetime
            .and_then(|_| self.declared_signature(implemented.binding, method))
            .map_or_else(
                || signature.clone(),
                |(declared, self_generic)| signature.restore_generic(declared, self_generic),
            )
    }

    fn declared_signature(
        &self,
        trait_binding: BindingId,
        method: BindingId,
    ) -> Option<(&'a Type, GenericId)> {
        let name = self.program.bindings.get(method).name();
        let declared = self.program.declared_trait_method(trait_binding, name)?;
        let scheme = &self.plan.definition(declared.declaration())?.scheme;
        Some((&scheme.body, Self::self_generic(scheme)?))
    }

    fn self_generic(trait_method: &Scheme) -> Option<GenericId> {
        trait_method.params.first().map(|param| param.id)
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
                Fragment::Text(Self::MAIN_HEADER.to_string()),
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

    fn naming_parameter_lifetimes<Parameter: Eq + Hash>(
        scope: LifetimeScope,
        fresh: &Rc<FreshLifetimes>,
        higher_ranked: &HashSet<Parameter>,
        parameter: &Parameter,
    ) -> LifetimeScope {
        if higher_ranked.contains(parameter) {
            scope.naming_opaque_lifetimes(fresh)
        } else {
            scope.naming_signature_lifetimes(fresh)
        }
    }

    fn method_elisions(&self) -> MethodElisions<'_, 'a> {
        MethodElisions::new(
            self.program,
            self.plan,
            &self.result_borrows,
            &self.trait_method_elisions,
        )
    }

    fn method_parameter_scope(
        scope: &LifetimeScope,
        lifetimes: &MethodLifetimes,
        position: usize,
    ) -> LifetimeScope {
        if lifetimes.keeps_own_lifetime(position) {
            let shared = scope.elided().map(str::to_string);
            return scope.clone().with_elided(None).reserving(shared.as_deref());
        }
        scope.clone()
    }

    fn flowing_parameters(
        &self,
        lambda: &TypedLambda,
        lifetimes: &MethodLifetimes,
    ) -> HashSet<BindingId> {
        if !lifetimes.shares_lifetime() {
            return self.result_borrows.analyse(lambda).flows;
        }
        lambda
            .params
            .iter()
            .enumerate()
            .filter(|(position, _)| lifetimes.takes_shared_lifetime(*position))
            .map(|(_, param)| param.binding)
            .collect()
    }

    fn sharing_parameters(borrows: &LambdaFlows, stored: &StoredFlows) -> HashSet<BindingId> {
        let mut sharing = borrows.flows.clone();
        sharing.extend(&stored.values);
        sharing.extend(&stored.targets);
        sharing
    }

    fn store_targets(borrows: &LambdaFlows, stored: &StoredFlows) -> HashSet<BindingId> {
        stored.targets.difference(&borrows.flows).copied().collect()
    }

    fn store_target_scope(
        scope: LifetimeScope,
        store_targets: &HashSet<BindingId>,
        param: &TypedBinder,
    ) -> LifetimeScope {
        if store_targets.contains(&param.binding) {
            scope.eliding_outer_reference()
        } else {
            scope
        }
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
        let used = self.used_written_lifetimes(&definition.value);
        let stored = self.result_borrows.definition_stores(definition.binding);
        let inferred = self.function_lifetime(definition, &stored.flows);
        let mut emitter = self
            .expressions(info.module, &definition.value)
            .with_enclosing_lifetime(inferred.clone());
        emitter.prepare_tail_loop(definition.binding, lambda);
        let types = emitter.types();
        let scope =
            self.lifetime_scope_used(&definition.scheme.lifetimes, &used, inferred.as_deref());
        let written: Vec<BindingId> = definition
            .scheme
            .lifetimes
            .iter()
            .copied()
            .filter(|binding| used.contains(binding))
            .collect();
        let borrows = self.result_borrows.analyse(lambda);
        let sharing = Self::sharing_parameters(&borrows, &stored.flows);
        let store_targets = Self::store_targets(&borrows, &stored.flows);
        let higher_ranked = borrows.higher_ranked;
        let opaque_lifetimes = Rc::new(FreshLifetimes::new(
            self.declared_lifetimes(inferred.as_deref(), &definition.scheme.lifetimes),
        ));
        let params: Vec<String> = lambda
            .params
            .iter()
            .map(|param| {
                let scope = Self::naming_parameter_lifetimes(
                    Self::store_target_scope(
                        self.parameter_scope(
                            param,
                            &definition.scheme.lifetimes,
                            &used,
                            inferred.as_deref(),
                            &sharing,
                        ),
                        &store_targets,
                        param,
                    ),
                    &opaque_lifetimes,
                    &higher_ranked,
                    &param.binding,
                );
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

    fn function_lifetime(
        &self,
        definition: &TypedDefinition,
        stored: &StoredFlows,
    ) -> Option<String> {
        if stored.is_empty() {
            return self.inferred_lifetime(&definition.scheme.lifetimes, &definition.value);
        }
        Some(self.unused_lifetime_avoiding(&definition.scheme.lifetimes, None))
    }

    fn result_flows_past_elision(&self, value: &TypedExpr) -> bool {
        let TypedExprKind::Fn(lambda) = &value.kind else {
            return false;
        };
        let flowing = self.result_borrows.analyse(lambda).flows;
        lambda.params.iter().any(|param| {
            flowing.contains(&param.binding)
                && SignatureBorrows::new(self.program).hides_borrow_from_elision(&param.ty)
        })
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

    fn shared_method_lifetime(
        &self,
        lifetimes: &[BindingId],
        method: &MethodLifetimes,
        reserved: Option<&str>,
    ) -> Option<String> {
        method
            .shares_lifetime()
            .then(|| self.unused_lifetime_avoiding(lifetimes, reserved))
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
        self.needs_shared_lifetime(value)
            .then(|| self.unused_lifetime_avoiding(lifetimes, reserved))
    }

    fn unused_lifetime_avoiding(&self, lifetimes: &[BindingId], reserved: Option<&str>) -> String {
        let mut used: Vec<String> = lifetimes
            .iter()
            .map(|binding| RustTypes::binding_lifetime(self.program, *binding))
            .collect();
        used.extend(reserved.map(str::to_string));
        FreshLifetimes::unused_name(&used)
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
        let mut mutable = MutableLocals::of(value, &self.program.template_borrows);
        mutable.extend(self.program.mutable_function_bindings.iter().copied());
        let local_closures = LocalClosures::of(self.program, value);
        mutable.extend(local_closures.mutably_borrowed_arguments());
        ExpressionEmitter::new(
            self.program,
            self.plan,
            self.unsized_parameters,
            module,
            LocalNames::new(reserved.collect::<Vec<String>>()),
        )
        .with_mutable_locals(mutable.into_set())
        .with_local_closures(local_closures)
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
