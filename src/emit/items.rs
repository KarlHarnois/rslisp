use std::collections::{BTreeMap, HashMap, HashSet};

use super::expressions::ExpressionEmitter;
use super::fragment::{Code, Fragment};
use super::literal_generics::LiteralGenerics;
use super::locals::LocalNames;
use super::mangle::NameMangler;
use super::mutable_locals::MutableLocals;
use super::plan::{EmitPlan, ItemInfo, ItemKind};
use super::runtime::RuntimeModule;
use super::types::{LifetimeScope, RustTypes, TypePosition};
use super::unsized_parameters::UnsizedParameters;
use crate::core::{BindingId, ModuleId};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    GenericId, GenericParam, Lifetime, ReceiverKind, Type, TypedBinder, TypedConversion,
    TypedDefinition, TypedExpr, TypedExprKind, TypedProgram, VariantDefinition,
};

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

    fn traits(&self, module: ModuleId) -> Vec<Vec<Fragment>> {
        self.program
            .trait_definitions
            .iter()
            .filter(|trait_definition| trait_definition.module == module)
            .map(|trait_definition| {
                let mut methods = Vec::new();
                for method in &trait_definition.methods {
                    methods.push(Fragment::Newline);
                    methods.extend(self.trait_method(method.binding, method.receiver));
                }
                let visibility = self.trait_visibility(module, trait_definition.binding);
                vec![
                    Fragment::Text(format!(
                        "{visibility}trait {} {{",
                        trait_definition.rust_name
                    )),
                    Fragment::Indented(methods),
                    Fragment::Newline,
                    Fragment::Text("}".to_string()),
                ]
            })
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

    fn trait_method(&self, binding: BindingId, receiver: Option<ReceiverKind>) -> Vec<Fragment> {
        let definition = self
            .plan
            .definition(binding)
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
            receiver.is_some(),
        );
        let scope = self.lifetime_scope(&definition.scheme.lifetimes, inferred.as_deref());
        let mut rendered = Vec::new();
        if let Some(kind) = receiver {
            let lifetime = parameters.first().and_then(|parameter| {
                types.reference_lifetime_name(parameter, TypePosition::Signature, &scope)
            });
            rendered.push(Self::receiver_keyword(kind, lifetime.as_deref()));
        }
        let first = usize::from(receiver.is_some());
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
        let name = NameMangler::value(self.program.bindings.get(binding).name());
        let generics = Self::with_lifetimes(
            &self.declared_lifetimes(inferred.as_deref(), &definition.scheme.lifetimes),
            String::new(),
        );
        vec![Fragment::Text(format!(
            "fn {name}{generics}({}){result};",
            rendered.join(", ")
        ))]
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
            let mut body = Vec::new();
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
        let mut params = Vec::new();
        if let Some(kind) = receiver {
            let first = &lambda.params[0];
            emitter.locals.bind_receiver(first.binding);
            let receiver_lifetime = self.receiver_lifetime(first, &scope);
            params.push(Self::receiver_keyword(kind, receiver_lifetime.as_deref()));
            params.extend(
                lambda.params[1..]
                    .iter()
                    .map(|param| emitter.bind_parameter_in(param, TypePosition::Signature, &scope)),
            );
        } else {
            params.extend(
                lambda
                    .params
                    .iter()
                    .map(|param| emitter.bind_parameter_in(param, TypePosition::Signature, &scope)),
            );
        }
        let result = match &definition.value.ty {
            Type::Function(_, parameters, result) if !RustTypes::is_unit(result) => {
                let position = if parameters
                    .iter()
                    .any(|parameter| self.program.type_holds_borrow(parameter))
                {
                    TypePosition::Signature
                } else {
                    TypePosition::StaticResult
                };
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

    fn function(&mut self, definition: &TypedDefinition, info: &ItemInfo) -> Vec<Fragment> {
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            return Vec::new();
        };
        let mut emitter = self.expressions(info.module, &definition.value);
        emitter.prepare_tail_loop(definition.binding, lambda);
        let types = emitter.types();
        let used = Self::used_written_lifetimes(&definition.value);
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
        let params: Vec<String> = lambda
            .params
            .iter()
            .map(|param| emitter.bind_parameter_in(param, TypePosition::Signature, &scope))
            .collect();
        let result = match &definition.value.ty {
            Type::Function(_, parameters, result) if !RustTypes::is_unit(result) => {
                let position = if parameters
                    .iter()
                    .any(|parameter| self.program.type_holds_borrow(parameter))
                {
                    TypePosition::Signature
                } else {
                    TypePosition::StaticResult
                };
                format!(" -> {}", types.show_in(result, position, &scope))
            }
            _ => String::new(),
        };
        let literal_generics = LiteralGenerics::of(&definition.value);
        let generics = Self::with_lifetimes(
            &self.declared_lifetimes(inferred.as_deref(), &written),
            types.generics(
                &definition.scheme.params,
                &definition.scheme.body,
                &literal_generics,
            ),
        );
        let header = format!(
            "{}fn {}{generics}({}){result} {{",
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

    fn needs_shared_lifetime(&self, value: &TypedExpr) -> bool {
        let Type::Function(_, parameters, result) = &value.ty else {
            return false;
        };
        let result_lifetimes = self.borrow_lifetimes(result);
        let parameter_lifetimes: usize = parameters
            .iter()
            .map(|parameter| self.borrow_lifetimes(parameter))
            .sum();
        result_lifetimes > 0 && parameter_lifetimes > 1
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
            Self::unused_lifetime_name_from(&used)
        })
    }

    fn unused_lifetime_name(program: &TypedProgram, lifetimes: &[BindingId]) -> String {
        let used: Vec<String> = lifetimes
            .iter()
            .map(|binding| RustTypes::binding_lifetime(program, *binding))
            .collect();
        Self::unused_lifetime_name_from(&used)
    }

    fn unused_lifetime_name_from(used: &[String]) -> String {
        let mut position = 0;
        loop {
            let name = if position < 26 {
                format!(
                    "'{}",
                    char::from(b'a' + u8::try_from(position).expect("fewer than 26"))
                )
            } else {
                format!("'a{position}")
            };
            if !used.contains(&name) {
                return name;
            }
            position += 1;
        }
    }

    fn used_written_lifetimes(value: &TypedExpr) -> HashSet<BindingId> {
        let mut counts: HashMap<BindingId, usize> = HashMap::new();
        Self::count_written_lifetimes(&value.ty, &mut counts);
        counts
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(binding, _)| binding)
            .collect()
    }

    fn count_written_lifetimes(ty: &Type, counts: &mut HashMap<BindingId, usize>) {
        match ty {
            Type::Ref(lifetime, inner) | Type::Mut(lifetime, inner) => {
                if let Lifetime::Written(binding) = lifetime {
                    *counts.entry(*binding).or_default() += 1;
                }
                Self::count_written_lifetimes(inner, counts);
            }
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
            Type::Var(_) | Type::Generic(_) | Type::Primitive(_) => {}
        }
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

    fn borrow_lifetimes(&self, ty: &Type) -> usize {
        match ty {
            Type::Ref(lifetime, inner) | Type::Mut(lifetime, inner) => {
                usize::from(*lifetime == Lifetime::Elided) + self.borrow_lifetimes(inner)
            }
            Type::Named(binding, arguments) => {
                usize::from(self.program.has_lifetime_parameter(*binding))
                    + arguments
                        .iter()
                        .map(|argument| self.borrow_lifetimes(argument))
                        .sum::<usize>()
            }
            Type::Function(..) | Type::Var(_) | Type::Generic(_) | Type::Primitive(_) => 0,
        }
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
