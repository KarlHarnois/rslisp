use std::collections::HashMap;

use super::{
    Arm, Binder, BindingArena, BindingId, Block, BodyItem, Control, CoreModule, Expr, ExprKind,
    FieldLabel, Item, ItemKind, KernelConstants, KernelForm, Lambda, Leaf, Literal, ParsedTemplate,
    Pattern, PatternKind, RustTemplate, StructExpr, TypeBinder, TypeBound, TypeExpr, TypeExprKind,
    VariantDeclaration, VariantPayload,
};

pub struct CorePrinter<'a> {
    bindings: &'a BindingArena,
    assigned: HashMap<BindingId, String>,
    uses_of_name: HashMap<String, usize>,
}

impl<'a> CorePrinter<'a> {
    pub fn new(bindings: &'a BindingArena) -> Self {
        let kernel_names = KernelForm::all()
            .map(KernelForm::name)
            .chain(KernelConstants::BOOLEANS.map(|(name, _)| name))
            .map(|name| (name.to_string(), 1));
        Self {
            bindings,
            assigned: HashMap::new(),
            uses_of_name: kernel_names.collect(),
        }
    }

    pub fn module(&mut self, module: &CoreModule) -> String {
        let mut lines: Vec<String> = Vec::new();
        if !module.exports.is_empty() {
            let exports: Vec<String> = module
                .exports
                .iter()
                .map(|binding| self.name(*binding))
                .collect();
            lines.push(Self::list("export", exports));
        }
        lines.extend(
            module
                .items
                .iter()
                .filter(|item| !item.hidden)
                .map(|item| self.item(item)),
        );
        lines.iter().map(|line| format!("{line}\n")).collect()
    }

    fn item(&mut self, item: &Item) -> String {
        if let ItemKind::Def(binding, value) = &item.kind {
            return format!("(def {} {})", self.name(*binding), self.expr(value));
        }
        if let ItemKind::Signature(binding, type_expr) = &item.kind {
            return format!(
                "(type {} {})",
                self.name(*binding),
                self.type_expr(type_expr)
            );
        }
        self.type_item(item)
    }

    fn type_item(&mut self, item: &Item) -> String {
        if let ItemKind::Struct(binding, params, fields) = &item.kind {
            return self.struct_item(*binding, params, fields);
        }
        if let ItemKind::Enum(binding, params, variants, rust) = &item.kind {
            return self.enum_item(*binding, params, variants, rust);
        }
        if let ItemKind::Derive(binding, _, traits) = &item.kind {
            let mut parts = vec![self.name(*binding)];
            parts.extend(
                traits
                    .iter()
                    .map(|(trait_binding, _)| self.name(*trait_binding)),
            );
            return Self::list("derive", parts);
        }
        self.imported_type_item(item)
    }

    fn imported_type_item(&mut self, item: &Item) -> String {
        if let ItemKind::RustType(binding, params, template, clauses) = &item.kind {
            return self.rust_type_item(*binding, params, template, clauses);
        }
        if let ItemKind::RustTrait(binding, path, implementors) = &item.kind {
            let mut parts = vec![self.name(*binding), format!("{path:?}")];
            parts.extend(
                implementors
                    .iter()
                    .map(|implementor| self.type_expr(implementor)),
            );
            return Self::list("rust-trait", parts);
        }
        if let ItemKind::Trait(binding, _, _, methods) = &item.kind {
            let mut parts = vec![self.name(*binding)];
            parts.extend(methods.iter().map(|method| {
                format!(
                    "({} {})",
                    self.name(method.binding),
                    self.type_expr(&method.signature)
                )
            }));
            return Self::list("trait", parts);
        }
        if let ItemKind::Impl(trait_binding, implementor, methods) = &item.kind {
            let mut parts = vec![self.name(*trait_binding), self.type_expr(implementor)];
            parts.extend(methods.iter().map(|method| self.impl_method(method)));
            return Self::list("impl", parts);
        }
        String::new()
    }

    fn impl_method(&mut self, method: &super::ImplMethod) -> String {
        let params: Vec<String> = method
            .lambda
            .params
            .iter()
            .map(|param| self.binder(param))
            .collect();
        let mut parts = vec![method.name.to_string(), format!("({})", params.join(" "))];
        parts.extend(self.block_items(&method.lambda.body));
        format!("({})", parts.join(" "))
    }

    fn expr(&mut self, expr: &Expr) -> String {
        match expr.kind() {
            ExprKind::Leaf(leaf) => self.leaf(leaf),
            ExprKind::Fn(lambda) => self.lambda(lambda),
            ExprKind::Call(callee, arguments) => self.call(callee, arguments),
            ExprKind::Control(control) => self.control(control),
            other => self.nested_expr(other),
        }
    }

    fn nested_expr(&mut self, kind: &ExprKind) -> String {
        match kind {
            ExprKind::Recur(arguments) => Self::list(
                "recur",
                arguments
                    .iter()
                    .map(|argument| self.expr(argument))
                    .collect(),
            ),
            ExprKind::Try(value) => Self::list("try", vec![self.expr(value)]),
            ExprKind::Borrow(..) | ExprKind::Deref(..) => self.written(kind),
            ExprKind::Struct(structure) => self.structure(structure),
            ExprKind::Variant(binding, arguments) => self.variant(*binding, arguments),
            _ => unreachable!("the caller matches a nested expression"),
        }
    }

    fn written(&mut self, kind: &ExprKind) -> String {
        match kind {
            ExprKind::Borrow(kind, value) => Self::list(kind.keyword(), vec![self.expr(value)]),
            ExprKind::Deref(value) => Self::list("deref", vec![self.expr(value)]),
            _ => unreachable!("the caller matches a written form"),
        }
    }

    fn leaf(&mut self, leaf: &Leaf) -> String {
        match leaf {
            Leaf::Literal(literal) => self.literal(literal),
            Leaf::Var(binding) => self.name(*binding),
            Leaf::Quote(form) => Self::list("quote", vec![form.to_string()]),
            Leaf::Rust(template) => self.rust(template),
        }
    }

    fn literal(&self, literal: &Literal) -> String {
        match literal {
            Literal::Number(number) => number.text().to_string(),
            Literal::String(text) => format!("{text:?}"),
            Literal::Bool(value) => value.to_string(),
        }
    }

    fn lambda(&mut self, lambda: &Lambda) -> String {
        let params: Vec<String> = lambda
            .params
            .iter()
            .map(|param| self.binder(param))
            .collect();
        let mut parts = vec![format!("({})", params.join(" "))];
        parts.extend(self.block_items(&lambda.body));
        Self::list("fn", parts)
    }

    fn call(&mut self, callee: &Expr, arguments: &[Expr]) -> String {
        let mut parts = vec![self.expr(callee)];
        parts.extend(arguments.iter().map(|argument| self.expr(argument)));
        format!("({})", parts.join(" "))
    }

    fn control(&mut self, control: &Control) -> String {
        match control {
            Control::Block(block) => Self::list("do", self.block_items(block)),
            Control::If(test, then, otherwise) => Self::list(
                "if",
                vec![self.expr(test), self.expr(then), self.expr(otherwise)],
            ),
            Control::Match(value, arms) => {
                let mut parts = vec![self.expr(value)];
                parts.extend(arms.iter().map(|arm| self.arm(arm)));
                Self::list("match", parts)
            }
        }
    }

    fn arm(&mut self, arm: &Arm) -> String {
        let mut parts = vec![self.pattern(&arm.pattern)];
        parts.extend(self.block_items(&arm.body));
        format!("({})", parts.join(" "))
    }

    fn pattern(&mut self, pattern: &Pattern) -> String {
        match &pattern.kind {
            PatternKind::Wildcard => "_".to_string(),
            PatternKind::Bind(binding) => self.name(*binding),
            PatternKind::BareName { name, .. } => name.to_string(),
            PatternKind::Variant(binding, arguments) | PatternKind::Struct(binding, arguments) => {
                let name = self.name(*binding);
                self.prefixed_pattern(name, arguments)
            }
            PatternKind::ShortVariant {
                name, arguments, ..
            } => self.prefixed_pattern(name.to_string(), arguments),
            PatternKind::Literal(literal) => self.literal(literal),
        }
    }

    fn prefixed_pattern(&mut self, name: String, arguments: &[Pattern]) -> String {
        let mut parts = vec![name];
        parts.extend(arguments.iter().map(|argument| self.pattern(argument)));
        format!("({})", parts.join(" "))
    }

    fn structure(&mut self, structure: &StructExpr) -> String {
        match structure {
            StructExpr::New(binding, fields) => {
                let mut parts = vec![self.name(*binding)];
                parts.extend(
                    fields
                        .iter()
                        .map(|(label, value)| format!("({} {})", label.name, self.expr(value))),
                );
                Self::list("new", parts)
            }
            StructExpr::Field(value, label) => {
                Self::list("field", vec![self.expr(value), label.name.to_string()])
            }
        }
    }

    fn rust(&mut self, template: &RustTemplate) -> String {
        let parts = vec![
            format!("{:?}", template.template),
            self.type_expr(&template.type_expr),
        ];
        Self::list("rust", parts)
    }

    fn block_items(&mut self, block: &Block) -> Vec<String> {
        block
            .items
            .iter()
            .map(|item| match item {
                BodyItem::Let(binder, value) => {
                    Self::list("let", vec![self.binder(binder), self.expr(value)])
                }
                BodyItem::Expr(expr) => self.expr(expr),
            })
            .collect()
    }

    fn binder(&mut self, binder: &Binder) -> String {
        let name = self.name(binder.binding);
        match &binder.annotation {
            Some(annotation) => format!("({name} {})", self.type_expr(annotation)),
            None => name,
        }
    }

    fn type_expr(&mut self, type_expr: &TypeExpr) -> String {
        match type_expr.kind() {
            TypeExprKind::Primitive(primitive) => primitive.name().to_string(),
            TypeExprKind::Named(binding, arguments) => self.named_type(*binding, arguments),
            TypeExprKind::Function(parameters, result) => self.function_type(parameters, result),
            TypeExprKind::Reference(kind, inner) => {
                Self::list(kind.keyword(), vec![self.type_expr(inner)])
            }
            TypeExprKind::Hole => "_".to_string(),
            TypeExprKind::Where(binders, inner) => self.where_type(binders, inner),
            TypeExprKind::With(implementor, argument) => Self::list(
                "with",
                vec![self.type_expr(implementor), self.type_expr(argument)],
            ),
        }
    }

    fn named_type(&mut self, binding: BindingId, arguments: &[TypeExpr]) -> String {
        let name = self.name(binding);
        if arguments.is_empty() {
            return name;
        }
        let mut parts = vec![name];
        parts.extend(arguments.iter().map(|argument| self.type_expr(argument)));
        format!("({})", parts.join(" "))
    }

    fn function_type(&mut self, parameters: &[TypeExpr], result: &TypeExpr) -> String {
        let parameters: Vec<String> = parameters
            .iter()
            .map(|parameter| self.type_expr(parameter))
            .collect();
        Self::list(
            "fn",
            vec![
                format!("({})", parameters.join(" ")),
                self.type_expr(result),
            ],
        )
    }

    fn struct_item(
        &mut self,
        binding: BindingId,
        params: &[TypeBinder],
        fields: &[(FieldLabel, TypeExpr)],
    ) -> String {
        let mut parts = vec![self.type_head(binding, params)];
        parts.extend(
            fields.iter().map(|(label, field_type)| {
                format!("({} {})", label.name, self.type_expr(field_type))
            }),
        );
        Self::list("struct", parts)
    }

    fn enum_item(
        &mut self,
        binding: BindingId,
        params: &[TypeBinder],
        variants: &[VariantDeclaration],
        rust: &Option<crate::core::EnumRust>,
    ) -> String {
        let mut parts = vec![self.type_head(binding, params)];
        if let Some(rust) = rust {
            parts.push(format!("(rust {:?})", rust.path));
        }
        parts.extend(
            variants
                .iter()
                .map(|variant| self.variant_declaration(variant)),
        );
        Self::list("enum", parts)
    }

    fn variant_declaration(&mut self, variant: &VariantDeclaration) -> String {
        let name = variant.name.to_string();
        match &variant.payload {
            VariantPayload::None => name,
            VariantPayload::Positional(payload) => {
                let mut parts = vec![name];
                parts.extend(payload.iter().map(|payload| self.type_expr(payload)));
                format!("({})", parts.join(" "))
            }
            VariantPayload::Named(fields) => {
                let fields: Vec<String> = fields
                    .iter()
                    .map(|(label, field_type)| {
                        format!("({} {})", label.name, self.type_expr(field_type))
                    })
                    .collect();
                format!("({name} (struct {}))", fields.join(" "))
            }
        }
    }

    fn variant(&mut self, binding: BindingId, arguments: &[Expr]) -> String {
        let mut parts = vec![self.name(binding)];
        parts.extend(arguments.iter().map(|argument| self.expr(argument)));
        format!("({})", parts.join(" "))
    }

    fn rust_type_item(
        &mut self,
        binding: BindingId,
        params: &[TypeBinder],
        template: &ParsedTemplate,
        clauses: &[(TypeExpr, std::rc::Rc<ParsedTemplate>)],
    ) -> String {
        let head = self.type_head(binding, params);
        let mut parts = vec![head, format!("{:?}", template.text())];
        parts.extend(clauses.iter().map(|(target, template)| {
            format!("({} {:?})", self.type_expr(target), template.text())
        }));
        Self::list("rust-type", parts)
    }

    fn type_head(&mut self, binding: BindingId, params: &[TypeBinder]) -> String {
        let name = self.name(binding);
        if params.is_empty() {
            return name;
        }
        let binders = self.type_binders(params);
        format!("({name} {})", binders.join(" "))
    }

    fn where_type(&mut self, binders: &[TypeBinder], inner: &TypeExpr) -> String {
        let binders = self.type_binders(binders);
        let inner = self.type_expr(inner);
        Self::where_list(binders, vec![inner])
    }

    fn type_binders(&mut self, binders: &[TypeBinder]) -> Vec<String> {
        binders
            .iter()
            .map(|binder| self.type_binder(binder))
            .collect()
    }

    fn where_list(binders: Vec<String>, body: Vec<String>) -> String {
        let mut parts = vec![format!("({})", binders.join(" "))];
        parts.extend(body);
        Self::list("where", parts)
    }

    fn type_binder(&mut self, binder: &TypeBinder) -> String {
        let variable = self.name(binder.variable);
        if binder.bounds.is_empty() {
            return variable;
        }
        let bounds: Vec<String> = binder
            .bounds
            .iter()
            .map(|bound| self.bound(bound))
            .collect();
        format!("({variable} {})", bounds.join(" "))
    }

    fn bound(&mut self, bound: &TypeBound) -> String {
        let name = self.name(bound.binding);
        match &bound.argument {
            Some(argument) => format!("({name} {})", self.type_expr(argument)),
            None => name,
        }
    }

    fn name(&mut self, binding: BindingId) -> String {
        if let Some(name) = self.assigned.get(&binding) {
            return name.clone();
        }
        let base = self.bindings.get(binding).name().to_string();
        let uses = self.uses_of_name.entry(base.clone()).or_insert(0);
        *uses += 1;
        let name = if *uses == 1 {
            base
        } else {
            format!("{base}_{uses}")
        };
        self.assigned.insert(binding, name.clone());
        name
    }

    fn list(head: &str, parts: Vec<String>) -> String {
        if parts.is_empty() {
            format!("({head})")
        } else {
            format!("({head} {})", parts.join(" "))
        }
    }
}
