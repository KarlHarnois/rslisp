use std::collections::HashMap;

use super::{
    Binder, BindingArena, BindingId, Block, BodyItem, Control, CoreModule, Expr, ExprKind,
    FieldLabel, Item, ItemKind, KernelConstants, KernelForm, Lambda, Leaf, Literal, ParsedTemplate,
    RustTemplate, StructExpr, TypeBinder, TypeExpr, TypeExprKind,
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
        lines.extend(module.items.iter().map(|item| self.item(item)));
        lines.iter().map(|line| format!("{line}\n")).collect()
    }

    fn item(&mut self, item: &Item) -> String {
        match &item.kind {
            ItemKind::Def(binding, value) => {
                format!("(def {} {})", self.name(*binding), self.expr(value))
            }
            ItemKind::Signature(binding, type_expr) => format!(
                "(type {} {})",
                self.name(*binding),
                self.type_expr(type_expr)
            ),
            ItemKind::Struct(binding, params, fields) => self.struct_item(*binding, params, fields),
            ItemKind::Derive(binding, traits) => {
                let mut parts = vec![self.name(*binding)];
                parts.extend(
                    traits
                        .iter()
                        .map(|(trait_binding, _)| self.name(*trait_binding)),
                );
                Self::list("derive", parts)
            }
            ItemKind::RustType(binding, params, template) => {
                self.rust_type_item(*binding, params, template)
            }
            ItemKind::RustTrait(binding, path, implementors) => {
                let mut parts = vec![self.name(*binding), format!("{path:?}")];
                parts.extend(
                    implementors
                        .iter()
                        .map(|implementor| self.type_expr(implementor)),
                );
                Self::list("rust-trait", parts)
            }
        }
    }

    fn expr(&mut self, expr: &Expr) -> String {
        match expr.kind() {
            ExprKind::Leaf(leaf) => self.leaf(leaf),
            ExprKind::Fn(lambda) => self.lambda(lambda),
            ExprKind::Call(callee, arguments) => self.call(callee, arguments),
            ExprKind::Control(control) => self.control(control),
            ExprKind::Struct(structure) => self.structure(structure),
        }
    }

    fn leaf(&mut self, leaf: &Leaf) -> String {
        match leaf {
            Leaf::Literal(Literal::Number(number)) => number.text().to_string(),
            Leaf::Literal(Literal::String(text)) => format!("{text:?}"),
            Leaf::Literal(Literal::Bool(value)) => value.to_string(),
            Leaf::Var(binding) => self.name(*binding),
            Leaf::Quote(form) => Self::list("quote", vec![form.to_string()]),
            Leaf::Rust(template) => self.rust(template),
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
        }
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
        let mut parts = vec![self.name(binding)];
        let params = self.type_binders(params);
        let fields: Vec<String> = fields
            .iter()
            .map(|(label, field_type)| format!("({} {})", label.name, self.type_expr(field_type)))
            .collect();
        if params.is_empty() {
            parts.extend(fields);
        } else {
            parts.push(Self::where_list(params, fields));
        }
        Self::list("struct", parts)
    }

    fn rust_type_item(
        &mut self,
        binding: BindingId,
        params: &[TypeBinder],
        template: &ParsedTemplate,
    ) -> String {
        let mut parts = vec![self.name(binding)];
        let params = self.type_binders(params);
        let template = format!("{:?}", template.text());
        if params.is_empty() {
            parts.push(template);
        } else {
            parts.push(Self::where_list(params, vec![template]));
        }
        Self::list("rust-type", parts)
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
            .map(|bound| self.name(*bound))
            .collect();
        format!("({variable} {})", bounds.join(" "))
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
