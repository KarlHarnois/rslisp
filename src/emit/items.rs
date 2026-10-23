use std::collections::BTreeMap;

use super::expressions::ExpressionEmitter;
use super::fragment::{Code, Fragment};
use super::literal_generics::LiteralGenerics;
use super::locals::LocalNames;
use super::mangle::NameMangler;
use super::mutable_locals::MutableLocals;
use super::plan::{EmitPlan, ItemInfo, ItemKind};
use super::runtime::RuntimeModule;
use super::types::{RustTypes, TypePosition};
use super::unsized_parameters::UnsizedParameters;
use crate::core::{BindingId, ModuleId};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::types::{
    GenericParam, Type, TypedDefinition, TypedExpr, TypedExprKind, TypedProgram, VariantDefinition,
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
        self.plan
            .items_of(module)
            .into_iter()
            .map(|binding| self.item(binding))
            .collect()
    }

    fn item(&mut self, binding: BindingId) -> Vec<Fragment> {
        let info = self.plan.item(binding).expect("planned item");
        self.names.push((
            info.name.clone(),
            self.program.bindings.get(binding).name().to_string(),
        ));
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

    fn function(&mut self, definition: &TypedDefinition, info: &ItemInfo) -> Vec<Fragment> {
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            return Vec::new();
        };
        let mut emitter = self.expressions(info.module, &definition.value);
        emitter.prepare_tail_loop(definition.binding, lambda);
        let types = emitter.types();
        let params: Vec<String> = lambda
            .params
            .iter()
            .map(|param| emitter.bind_parameter(param, TypePosition::Signature))
            .collect();
        let result = match &definition.value.ty {
            Type::Function(parameters, result) if !RustTypes::is_unit(result) => {
                let position = if parameters.iter().any(Type::holds_reference) {
                    TypePosition::Signature
                } else {
                    TypePosition::StaticResult
                };
                format!(" -> {}", types.show(result, position))
            }
            _ => String::new(),
        };
        let literal_generics = LiteralGenerics::of(&definition.value);
        let generics = types.generics(
            &definition.scheme.params,
            &definition.scheme.body,
            &literal_generics,
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
        let mut fields = Vec::new();
        for ((name, field_type), span) in definition.fields.iter().zip(&definition.field_spans) {
            fields.push(Fragment::Newline);
            fields.push(Fragment::Text(format!(
                "{visibility}{}: ",
                NameMangler::value(name)
            )));
            fields.push(Fragment::Spanned(
                *span,
                vec![Fragment::Text(types.show(field_type, TypePosition::Value))],
            ));
            fields.push(Fragment::Text(",".to_string()));
        }
        let params = Self::struct_params(&definition.params);
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
        let params = Self::struct_params(&definition.params);
        let mut variants = Vec::new();
        for variant in &definition.variants {
            variants.push(Fragment::Newline);
            variants.extend(Self::variant(variant, &types));
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

    fn variant(variant: &VariantDefinition, types: &RustTypes) -> Vec<Fragment> {
        let mut fragments = vec![Fragment::Text(NameMangler::type_name(&variant.name))];
        fragments.extend(Self::variant_payload(variant, types));
        fragments.push(Fragment::Text(",".to_string()));
        fragments
    }

    fn variant_payload(variant: &VariantDefinition, types: &RustTypes) -> Vec<Fragment> {
        match &variant.payload {
            crate::types::VariantPayload::None => Vec::new(),
            crate::types::VariantPayload::Positional(payload_types) => {
                Self::positional_payload(payload_types, &variant.payload_spans, types)
            }
            crate::types::VariantPayload::Named(fields) => {
                Self::named_payload(fields, &variant.payload_spans, types)
            }
        }
    }

    fn positional_payload(
        payload_types: &[Type],
        spans: &[Span],
        types: &RustTypes,
    ) -> Vec<Fragment> {
        let mut fragments = vec![Fragment::Text("(".to_string())];
        for (position, (payload, span)) in payload_types.iter().zip(spans).enumerate() {
            if position > 0 {
                fragments.push(Fragment::Text(", ".to_string()));
            }
            fragments.push(Fragment::Spanned(
                *span,
                vec![Fragment::Text(types.show(payload, TypePosition::Value))],
            ));
        }
        fragments.push(Fragment::Text(")".to_string()));
        fragments
    }

    fn named_payload(
        fields: &[(std::rc::Rc<str>, Type)],
        spans: &[Span],
        types: &RustTypes,
    ) -> Vec<Fragment> {
        let mut fragments = vec![Fragment::Text(" { ".to_string())];
        for (position, ((label, field_type), span)) in fields.iter().zip(spans).enumerate() {
            if position > 0 {
                fragments.push(Fragment::Text(", ".to_string()));
            }
            fragments.push(Fragment::Text(format!("{}: ", NameMangler::value(label))));
            fragments.push(Fragment::Spanned(
                *span,
                vec![Fragment::Text(types.show(field_type, TypePosition::Value))],
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

    fn struct_params(params: &[GenericParam]) -> String {
        if params.is_empty() {
            return String::new();
        }
        let names: Vec<String> = params.iter().map(RustTypes::generic_name).collect();
        format!("<{}>", names.join(", "))
    }

    fn expressions(&self, module: ModuleId, value: &TypedExpr) -> ExpressionEmitter<'a> {
        let reserved = self
            .plan
            .items_of(module)
            .into_iter()
            .filter_map(|binding| self.plan.item(binding))
            .map(|item| item.name.clone());
        ExpressionEmitter::new(
            self.program,
            self.plan,
            self.unsized_parameters,
            module,
            LocalNames::new(reserved.collect::<Vec<String>>()),
        )
        .with_mutable_locals(MutableLocals::of(value))
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
