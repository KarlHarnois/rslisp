use std::collections::{BTreeSet, HashMap, HashSet};

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
    definitions: HashMap<BindingId, (&'p TypedDefinition, ModuleId)>,
    struct_modules: HashMap<BindingId, ModuleId>,
    exported: HashSet<BindingId>,
    reached: Vec<BindingId>,
    crate_visible: HashSet<BindingId>,
    items: HashMap<BindingId, ItemInfo>,
    used_rust_types: BTreeSet<BindingId>,
}

impl<'p> EmitPlan<'p> {
    const MAIN: &'static str = "main";
    const MODULE_SEPARATOR: char = '/';
    const MACRO_CALLS: [&'static str; 3] = ["!(", "![", "!{"];

    pub fn build(program: &'p TypedProgram, entry_span: Span) -> Result<Self, Diagnostics> {
        let main = Self::find_main(program)
            .ok_or_else(|| Diagnostics::from(EmitProblems::missing_main(entry_span)))?;
        let mut plan = Self {
            program,
            definitions: Self::index_definitions(program),
            struct_modules: program
                .modules
                .iter()
                .flat_map(|module| {
                    module
                        .structs
                        .iter()
                        .map(move |binding| (*binding, module.id))
                })
                .collect(),
            exported: program
                .modules
                .iter()
                .flat_map(|module| module.exports.iter().copied())
                .collect(),
            reached: Vec::new(),
            crate_visible: HashSet::new(),
            items: HashMap::new(),
            used_rust_types: BTreeSet::new(),
        };
        plan.visit_definition(main);
        plan.assign_items()?;
        Ok(plan)
    }

    pub fn item(&self, binding: BindingId) -> Option<&ItemInfo> {
        self.items.get(&binding)
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
    ) -> HashMap<BindingId, (&TypedDefinition, ModuleId)> {
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
            Type::Function(parameters, result) => {
                parameters
                    .iter()
                    .for_each(|parameter| self.visit_type(parameter, module));
                self.visit_type(result, module);
            }
            Type::Ref(inner) | Type::Mut(inner) => self.visit_type(inner, module),
            Type::Var(_) | Type::Generic(_) | Type::Primitive(_) => {}
        }
    }

    fn reach_named(&mut self, binding: BindingId, from: ModuleId) {
        if self.program.rust_types.get(binding).is_some() {
            self.used_rust_types.insert(binding);
        } else {
            self.reach_struct(binding, from);
        }
    }

    fn reach_struct(&mut self, binding: BindingId, from: ModuleId) {
        let Some(owner) = self.struct_modules.get(&binding).copied() else {
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
        let mut taken: HashMap<(ModuleId, String), BindingId> = HashMap::new();
        let mut diagnostics = Diagnostics::default();
        for binding in self.reached.clone() {
            let info = self.item_info(binding);
            let key = (info.module, info.name.clone());
            match taken
                .get(&key)
                .copied()
                .or_else(|| self.shadowed_rust_type(&info))
            {
                Some(other) => diagnostics.push(self.collision(&info.name, other, binding)),
                None => {
                    taken.insert(key, binding);
                    self.items.insert(binding, info);
                }
            }
        }
        diagnostics.into_result(())
    }

    fn shadowed_rust_type(&self, info: &ItemInfo) -> Option<BindingId> {
        let is_struct = info.kind == ItemKind::Struct;
        self.used_rust_types.iter().copied().find(|binding| {
            is_struct
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
            None => (self.struct_modules[&binding], ItemKind::Struct),
        };
        let rust_name = match kind {
            ItemKind::Function => NameMangler::value(name),
            ItemKind::Const | ItemKind::Static => NameMangler::constant(name),
            ItemKind::Struct => NameMangler::type_name(name),
        };
        ItemInfo {
            module,
            name: rust_name,
            kind,
            visibility: self.visibility(binding),
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
