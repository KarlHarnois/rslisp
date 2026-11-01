use std::collections::BTreeSet;

use rustc_hash::{FxHashMap, FxHashSet};

use super::mangle::NameMangler;
use super::problems::EmitProblems;
use crate::core::{BindingId, ModuleId};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::source::Span;
use crate::types::{
    GenericParam, Type, TypedDefinition, TypedExpr, TypedExprKind, TypedLeaf, TypedProgram,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Function,
    Const,
    Static,
    Struct,
    Enum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Crate,
    Public,
}

impl Visibility {
    pub fn keyword(self) -> &'static str {
        match self {
            Visibility::Private => "",
            Visibility::Crate => "pub(crate) ",
            Visibility::Public => "pub ",
        }
    }
}

pub struct ItemInfo {
    pub module: ModuleId,
    pub name: String,
    pub kind: ItemKind,
    pub visibility: Visibility,
}

pub struct EmitPlan<'p> {
    program: &'p TypedProgram,
    main: BindingId,
    main_body: Option<String>,
    definitions: FxHashMap<BindingId, (&'p TypedDefinition, ModuleId)>,
    type_modules: FxHashMap<BindingId, ModuleId>,
    exported: FxHashSet<BindingId>,
    reached: Vec<BindingId>,
    crate_visible: FxHashSet<BindingId>,
    items: FxHashMap<BindingId, ItemInfo>,
    used_rust_types: BTreeSet<BindingId>,
    trait_uses: FxHashMap<ModuleId, BTreeSet<BindingId>>,
}

impl<'p> EmitPlan<'p> {
    const MAIN: &'static str = "main";
    const MODULE_SEPARATOR: char = '/';
    const MACRO_CALLS: [&'static str; 3] = ["!(", "![", "!{"];
    const FIRST_RENAME_SUFFIX: usize = 2;

    pub fn build(program: &'p TypedProgram, entry_span: Span) -> Result<Self, Diagnostics> {
        let main = Self::find_main(program)
            .ok_or_else(|| Diagnostics::from(EmitProblems::missing_main(entry_span)))?;
        let mut plan = Self {
            program,
            main,
            main_body: None,
            definitions: Self::index_definitions(program),
            type_modules: program
                .modules
                .iter()
                .flat_map(|module| {
                    module
                        .structs
                        .iter()
                        .chain(module.enums.iter())
                        .map(move |binding| (*binding, module.id))
                })
                .collect(),
            exported: program
                .modules
                .iter()
                .flat_map(|module| module.exports.iter().copied())
                .collect(),
            reached: Vec::new(),
            crate_visible: FxHashSet::default(),
            items: FxHashMap::default(),
            used_rust_types: BTreeSet::new(),
            trait_uses: FxHashMap::default(),
        };
        plan.visit_definition(main);
        plan.reach_implementations();
        plan.reach_trait_defaults();
        plan.main_body = plan.choose_main_body_name();
        plan.assign_items()?;
        plan.collect_trait_uses();
        Ok(plan)
    }

    pub fn item(&self, binding: BindingId) -> Option<&ItemInfo> {
        self.items.get(&binding)
    }

    pub fn is_result_main(&self, binding: BindingId) -> bool {
        binding == self.main && self.main_body.is_some()
    }

    pub fn main_body_name(&self) -> Option<&str> {
        self.main_body.as_deref()
    }

    fn choose_main_body_name(&self) -> Option<String> {
        self.definition(self.main)?
            .result_error(&self.program.enums)?;
        let taken: BTreeSet<String> = self
            .reached
            .iter()
            .filter(|binding| **binding != self.main)
            .filter(|binding| !self.program.synthetic_bindings.contains(*binding))
            .map(|binding| self.item_info(*binding))
            .filter(|info| info.module == self.program.entry)
            .map(|info| info.name)
            .collect();
        let mut name = "main_body".to_string();
        while taken.contains(&name) {
            name.push('_');
        }
        Some(name)
    }

    pub fn definition(&self, binding: BindingId) -> Option<&'p TypedDefinition> {
        self.definitions
            .get(&binding)
            .map(|(definition, _)| *definition)
    }

    pub fn items_of(&self, module: ModuleId) -> Vec<BindingId> {
        let mut bindings: Vec<BindingId> = self
            .items
            .iter()
            .filter(|(_, info)| info.module == module)
            .map(|(binding, _)| *binding)
            .collect();
        bindings.sort();
        bindings
    }

    pub fn path_to(&self, binding: BindingId, from: ModuleId) -> String {
        let info = &self.items[&binding];
        if info.module == from {
            return info.name.clone();
        }
        let mut segments = vec!["crate".to_string()];
        segments.extend(self.module_segments(info.module));
        segments.push(info.name.clone());
        segments.join("::")
    }

    pub fn trait_path(&self, binding: BindingId, from: ModuleId) -> String {
        if let Some(path) = self.program.rust_trait_paths.get(&binding) {
            return path.to_string();
        }
        let definition = self
            .program
            .trait_definitions
            .iter()
            .find(|trait_definition| trait_definition.binding == binding)
            .expect("a trait method names a trait definition");
        if definition.module == from {
            return definition.rust_name.to_string();
        }
        let mut segments = vec!["crate".to_string()];
        segments.extend(self.module_segments(definition.module));
        segments.push(definition.rust_name.to_string());
        segments.join("::")
    }

    pub fn trait_module(&self, binding: BindingId) -> Option<ModuleId> {
        self.program
            .trait_definitions
            .iter()
            .find(|trait_definition| trait_definition.binding == binding)
            .map(|trait_definition| trait_definition.module)
    }

    pub fn trait_uses(&self, module: ModuleId) -> Option<&BTreeSet<BindingId>> {
        self.trait_uses.get(&module)
    }

    pub fn uses_trait(&self, module: ModuleId, binding: BindingId) -> bool {
        self.trait_uses
            .get(&module)
            .is_some_and(|uses| uses.contains(&binding))
    }

    fn collect_trait_uses(&mut self) {
        let modules: Vec<ModuleId> = self
            .program
            .modules
            .iter()
            .map(|module| module.id)
            .collect();
        for module in modules {
            let mut uses = BTreeSet::new();
            self.collect_module_trait_uses(module, &mut uses);
            self.collect_hidden_trait_uses(module, &mut uses);
            if !uses.is_empty() {
                self.trait_uses.insert(module, uses);
            }
        }
    }

    fn collect_module_trait_uses(&self, module: ModuleId, uses: &mut BTreeSet<BindingId>) {
        for binding in self.items_of(module) {
            if let Some(definition) = self.definition(binding) {
                Self::collect_expr_trait_uses(&definition.value, self.program, uses);
            }
        }
    }

    fn collect_hidden_trait_uses(&self, module: ModuleId, uses: &mut BTreeSet<BindingId>) {
        let impl_methods = self
            .program
            .trait_impls
            .iter()
            .filter(|implementation| implementation.module == module)
            .flat_map(|implementation| implementation.methods.iter())
            .map(|method| method.binding);
        let defaults = self
            .program
            .trait_definitions
            .iter()
            .filter(|trait_definition| trait_definition.module == module)
            .flat_map(|trait_definition| trait_definition.methods.iter())
            .filter_map(|method| method.default);
        for binding in impl_methods.chain(defaults) {
            if let Some(definition) = self.definition(binding) {
                Self::collect_expr_trait_uses(&definition.value, self.program, uses);
            }
        }
    }

    fn collect_expr_trait_uses(
        expr: &TypedExpr,
        program: &TypedProgram,
        uses: &mut BTreeSet<BindingId>,
    ) {
        if let TypedExprKind::Call(callee, _) = &expr.kind
            && let TypedExprKind::Leaf(TypedLeaf::Global(binding)) = &callee.kind
            && let Some(info) = program.trait_methods.get(binding)
            && info.receiver.is_some()
        {
            uses.insert(info.trait_binding);
        }
        for child in expr.children() {
            Self::collect_expr_trait_uses(child, program, uses);
        }
    }

    pub fn module_segments(&self, module: ModuleId) -> Vec<String> {
        if module == self.program.entry {
            return Vec::new();
        }
        let path = &self
            .program
            .modules
            .iter()
            .find(|candidate| candidate.id == module)
            .expect("module exists")
            .path;
        path.as_str()
            .split(Self::MODULE_SEPARATOR)
            .map(NameMangler::value)
            .collect()
    }

    pub fn rust_names(&self) -> impl Iterator<Item = (&str, &str)> {
        self.items.iter().map(|(binding, info)| {
            (
                info.name.as_str(),
                self.program.bindings.get(*binding).name(),
            )
        })
    }

    fn find_main(program: &TypedProgram) -> Option<BindingId> {
        program
            .entry_module()
            .definitions
            .iter()
            .find(|definition| {
                program.bindings.get(definition.binding).name() == Self::MAIN
                    && matches!(definition.value.kind, TypedExprKind::Fn(_))
            })
            .map(|definition| definition.binding)
    }

    fn index_definitions(
        program: &TypedProgram,
    ) -> FxHashMap<BindingId, (&TypedDefinition, ModuleId)> {
        program
            .modules
            .iter()
            .flat_map(|module| {
                module
                    .definitions
                    .iter()
                    .map(move |definition| (definition.binding, (definition, module.id)))
            })
            .collect()
    }

    fn reach_trait_defaults(&mut self) {
        let defaults: Vec<BindingId> = self
            .program
            .trait_definitions
            .iter()
            .flat_map(|trait_definition| trait_definition.methods.iter())
            .filter_map(|method| method.default)
            .collect();
        for binding in defaults {
            self.visit_definition(binding);
        }
    }

    fn reach_implementations(&mut self) {
        let implementations: Vec<(Type, ModuleId, Vec<BindingId>)> = self
            .program
            .trait_impls
            .iter()
            .map(|implementation| {
                (
                    implementation.implementor.clone(),
                    implementation.module,
                    implementation
                        .methods
                        .iter()
                        .map(|method| method.binding)
                        .collect(),
                )
            })
            .collect();
        for (implementor, module, methods) in implementations {
            self.visit_type(&implementor, module);
            for method in methods {
                self.visit_definition(method);
            }
        }
    }

    fn visit_definition(&mut self, binding: BindingId) {
        let Some((definition, module)) = self.definitions.get(&binding).copied() else {
            return;
        };
        if matches!(
            definition.value.kind,
            TypedExprKind::Leaf(TypedLeaf::Rust(_))
        ) || self.reached.contains(&binding)
        {
            return;
        }
        self.reached.push(binding);
        self.visit_type(&definition.scheme.body, module);
        self.visit_params(&definition.scheme.params, module);
        self.visit_expr(&definition.value, module);
    }

    fn visit_params(&mut self, params: &[GenericParam], module: ModuleId) {
        for param in params {
            let mut arguments: Vec<Type> = param
                .bounds
                .iter()
                .filter_map(|bound| bound.argument.clone())
                .collect();
            arguments.extend(
                self.program
                    .implied_bounds
                    .get(&param.id)
                    .into_iter()
                    .flatten()
                    .filter_map(|bound| bound.argument.clone()),
            );
            for argument in &arguments {
                self.visit_type(argument, module);
            }
        }
    }

    fn visit_expr(&mut self, expr: &TypedExpr, module: ModuleId) {
        self.visit_type(&expr.ty, module);
        if let TypedExprKind::Leaf(TypedLeaf::Global(binding)) = &expr.kind {
            self.reference(*binding, module);
        }
        for binder in expr.binders() {
            self.visit_type(&binder.ty, module);
        }
        for child in expr.children() {
            self.visit_expr(child, module);
        }
    }

    fn visit_type(&mut self, visited: &Type, module: ModuleId) {
        match visited {
            Type::Named(binding, arguments) => {
                arguments
                    .iter()
                    .for_each(|argument| self.visit_type(argument, module));
                self.reach_named(*binding, module);
            }
            Type::Function(_, parameters, result) => {
                parameters
                    .iter()
                    .for_each(|parameter| self.visit_type(parameter, module));
                self.visit_type(result, module);
            }
            Type::Ref(_, inner) | Type::Mut(_, inner) => self.visit_type(inner, module),
            Type::Var(_) | Type::Generic(_) | Type::Primitive(_) | Type::TraitObject(_) => {}
        }
    }

    fn reach_named(&mut self, binding: BindingId, from: ModuleId) {
        if self.program.rust_types.get(binding).is_some() {
            self.used_rust_types.insert(binding);
        } else if self.program.enums.get(binding).is_some() {
            self.reach_enum(binding, from);
        } else {
            self.reach_struct(binding, from);
        }
    }

    fn reach_enum(&mut self, binding: BindingId, from: ModuleId) {
        let Some(owner) = self.type_modules.get(&binding).copied() else {
            return;
        };
        self.note_visibility(binding, owner, from);
        if self.reached.contains(&binding) {
            return;
        }
        self.reached.push(binding);
        let payloads: Vec<Type> = self
            .program
            .enums
            .get(binding)
            .map(|definition| definition.payload_types().cloned().collect())
            .unwrap_or_default();
        payloads
            .iter()
            .for_each(|payload| self.visit_type(payload, owner));
    }

    fn reach_struct(&mut self, binding: BindingId, from: ModuleId) {
        let Some(owner) = self.type_modules.get(&binding).copied() else {
            return;
        };
        self.note_visibility(binding, owner, from);
        if self.reached.contains(&binding) {
            return;
        }
        self.reached.push(binding);
        let fields: Vec<Type> = self
            .program
            .structs
            .get(binding)
            .map(|definition| {
                definition
                    .fields
                    .iter()
                    .map(|(_, field)| field.clone())
                    .collect()
            })
            .unwrap_or_default();
        fields
            .iter()
            .for_each(|field| self.visit_type(field, owner));
    }

    fn reference(&mut self, binding: BindingId, from: ModuleId) {
        if let Some((_, owner)) = self.definitions.get(&binding).copied() {
            self.note_visibility(binding, owner, from);
        }
        self.visit_definition(binding);
    }

    fn note_visibility(&mut self, binding: BindingId, owner: ModuleId, from: ModuleId) {
        if owner != from && !self.exported.contains(&binding) {
            self.crate_visible.insert(binding);
        }
    }

    fn assign_items(&mut self) -> Result<(), Diagnostics> {
        let mut taken: FxHashMap<(ModuleId, String), BindingId> = FxHashMap::default();
        let mut diagnostics = Diagnostics::default();
        for binding in self.written_items_first() {
            let info = self.item_info(binding);
            let key = (info.module, info.name.clone());
            match taken
                .get(&key)
                .copied()
                .or_else(|| self.shadowed_rust_type(&info))
            {
                Some(_) if self.program.bindings.is_introduced(binding) => {
                    let renamed = Self::next_free_name(info, &taken);
                    self.claim(&mut taken, binding, renamed);
                }
                Some(other) => diagnostics.push(self.collision(&info.name, other, binding)),
                None => self.claim(&mut taken, binding, info),
            }
        }
        diagnostics.into_result(())
    }

    fn written_items_first(&self) -> Vec<BindingId> {
        let (introduced, written): (Vec<BindingId>, Vec<BindingId>) = self
            .reached
            .iter()
            .copied()
            .filter(|binding| !self.program.synthetic_bindings.contains(binding))
            .partition(|binding| self.program.bindings.is_introduced(*binding));
        written.into_iter().chain(introduced).collect()
    }

    fn claim(
        &mut self,
        taken: &mut FxHashMap<(ModuleId, String), BindingId>,
        binding: BindingId,
        info: ItemInfo,
    ) {
        taken.insert((info.module, info.name.clone()), binding);
        self.items.insert(binding, info);
    }

    fn next_free_name(
        info: ItemInfo,
        taken: &FxHashMap<(ModuleId, String), BindingId>,
    ) -> ItemInfo {
        let separator = Self::rename_separator(&info);
        let name = (Self::FIRST_RENAME_SUFFIX..)
            .map(|suffix| format!("{}{separator}{suffix}", info.name))
            .find(|candidate| !taken.contains_key(&(info.module, candidate.clone())))
            .expect("an unbounded range holds a free name");
        ItemInfo { name, ..info }
    }

    fn rename_separator(info: &ItemInfo) -> &'static str {
        if matches!(info.kind, ItemKind::Struct | ItemKind::Enum) {
            ""
        } else {
            "_"
        }
    }

    fn shadowed_rust_type(&self, info: &ItemInfo) -> Option<BindingId> {
        let is_type = matches!(info.kind, ItemKind::Struct | ItemKind::Enum);
        self.used_rust_types.iter().copied().find(|binding| {
            is_type
                && self
                    .program
                    .rust_types
                    .get(*binding)
                    .is_some_and(|rust_type| rust_type.leading_name() == info.name)
        })
    }

    fn item_info(&self, binding: BindingId) -> ItemInfo {
        let name = self.program.bindings.get(binding).name();
        let (module, kind) = match self.definitions.get(&binding) {
            Some((definition, module)) => (*module, self.definition_kind(definition)),
            None => (self.type_modules[&binding], self.named_type_kind(binding)),
        };
        let rust_name = match (kind, binding == self.main, &self.main_body) {
            (ItemKind::Function, true, Some(body)) => body.clone(),
            (ItemKind::Function, ..) => NameMangler::value(name),
            (ItemKind::Const | ItemKind::Static, ..) => NameMangler::constant(name),
            (ItemKind::Struct | ItemKind::Enum, ..) => NameMangler::type_name(name),
        };
        ItemInfo {
            module,
            name: rust_name,
            kind,
            visibility: self.visibility(binding),
        }
    }

    fn named_type_kind(&self, binding: BindingId) -> ItemKind {
        if self.program.enums.get(binding).is_some() {
            ItemKind::Enum
        } else {
            ItemKind::Struct
        }
    }

    fn definition_kind(&self, definition: &TypedDefinition) -> ItemKind {
        match &definition.value.kind {
            TypedExprKind::Fn(_) => ItemKind::Function,
            _ if self.is_const_evaluable(&definition.value) => ItemKind::Const,
            _ => ItemKind::Static,
        }
    }

    fn is_const_evaluable(&self, expr: &TypedExpr) -> bool {
        match &expr.kind {
            TypedExprKind::Leaf(leaf) => self.is_const_leaf(leaf),
            TypedExprKind::Call(callee, arguments) => {
                self.is_const_evaluable(callee)
                    && arguments
                        .iter()
                        .all(|argument| self.is_const_evaluable(&argument.value))
            }
            _ => false,
        }
    }

    fn is_const_leaf(&self, leaf: &TypedLeaf) -> bool {
        match leaf {
            TypedLeaf::Literal(_) => true,
            TypedLeaf::Global(binding) => self
                .definition(*binding)
                .is_some_and(|definition| self.is_const_evaluable(&definition.value)),
            TypedLeaf::Rust(template) => Self::is_operator_template(&template.template),
            TypedLeaf::Local(_) => false,
        }
    }

    fn is_operator_template(text: &str) -> bool {
        let is_plain_value = text.chars().all(char::is_alphanumeric);
        let calls_something =
            Self::MACRO_CALLS.iter().any(|call| text.contains(call)) || text.contains(['.', '(']);
        is_plain_value || !calls_something
    }

    fn visibility(&self, binding: BindingId) -> Visibility {
        if self.exported.contains(&binding) {
            Visibility::Public
        } else if self.crate_visible.contains(&binding) {
            Visibility::Crate
        } else {
            Visibility::Private
        }
    }

    fn collision(&self, rust_name: &str, first: BindingId, second: BindingId) -> Diagnostic {
        let first_info = self.program.bindings.get(first);
        let second_info = self.program.bindings.get(second);
        let span = second_info
            .span()
            .or(first_info.span())
            .expect("user definitions have spans");
        EmitProblems::name_collision(
            rust_name,
            first_info.name(),
            second_info.name(),
            span,
            first_info.span(),
        )
    }
}
