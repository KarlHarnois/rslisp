use std::collections::BTreeMap;

use super::expressions::ExpressionEmitter;
use super::fragment::{Code, Fragment};
use super::literal_generics::LiteralGenerics;
use super::locals::LocalNames;
use super::mangle::NameMangler;
use super::plan::{EmitPlan, ItemInfo, ItemKind};
use super::types::{RustTypes, TypePosition};
use crate::core::{BindingId, ModuleId, StructDerives};
use crate::diagnostic::Diagnostic;
use crate::types::{Type, TypedDefinition, TypedExprKind, TypedProgram};

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
    pub problems: Vec<Diagnostic>,
    pub names: Vec<(String, String)>,
}

impl<'a> ItemEmitter<'a> {
    const RUNTIME: &'static str = include_str!("../../prelude/runtime.rs");
    const RUNTIME_PATH: &'static str = "crate::rt::";
    const RUNTIME_MODULE: &'static str = "rt";

    pub fn new(program: &'a TypedProgram, plan: &'a EmitPlan<'a>) -> Self {
        Self {
            program,
            plan,
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
        if Code::text_of(&fragments).contains(Self::RUNTIME_PATH) {
            fragments.extend([Fragment::Newline, Fragment::Newline]);
            fragments.extend(Self::runtime());
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
            (ItemKind::Function, Some(definition)) => self.function(definition, info),
            (ItemKind::Const, Some(definition)) => self.constant(definition, info, false),
            (ItemKind::Static, Some(definition)) => self.constant(definition, info, true),
            (_, None) => Vec::new(),
        }
    }

    fn function(&mut self, definition: &TypedDefinition, info: &ItemInfo) -> Vec<Fragment> {
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            return Vec::new();
        };
        let mut emitter = self.expressions(info.module);
        let types = emitter.types();
        let params: Vec<String> = lambda
            .params
            .iter()
            .map(|param| emitter.bind_parameter(param, TypePosition::Signature))
            .collect();
        let result = match &definition.value.ty {
            Type::Function(parameters, result) if !RustTypes::is_unit(result) => {
                let position = if parameters.iter().any(Type::is_reference) {
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
        let mut emitter = self.expressions(info.module);
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
        let types = RustTypes::new(self.program, self.plan, info.module);
        let visibility = info.visibility.keyword();
        let mut fields = Vec::new();
        for (name, field_type) in self
            .program
            .structs
            .get(binding)
            .map(|definition| definition.fields.clone())
            .unwrap_or_default()
        {
            fields.push(Fragment::Newline);
            fields.push(Fragment::Text(format!(
                "{visibility}{}: {},",
                NameMangler::value(&name),
                types.show(&field_type, TypePosition::Value)
            )));
        }
        vec![
            Fragment::Text(StructDerives::attribute()),
            Fragment::Newline,
            Fragment::Text(format!("{visibility}struct {} {{", info.name)),
            Fragment::Indented(fields),
            Fragment::Newline,
            Fragment::Text("}".to_string()),
        ]
    }

    fn expressions(&self, module: ModuleId) -> ExpressionEmitter<'a> {
        let reserved = self
            .plan
            .items_of(module)
            .into_iter()
            .filter_map(|binding| self.plan.item(binding))
            .map(|item| item.name.clone());
        ExpressionEmitter::new(
            self.program,
            self.plan,
            module,
            LocalNames::new(reserved.collect::<Vec<String>>()),
        )
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

    fn runtime() -> Vec<Fragment> {
        let mut lines = vec![Fragment::Text(format!("mod {} {{", Self::RUNTIME_MODULE))];
        let mut inner = Vec::new();
        for line in Self::RUNTIME.lines() {
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
