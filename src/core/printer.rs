use std::collections::HashMap;

use super::{
    Binder, BindingArena, BindingId, Block, BodyItem, CoreModule, Expr, ExprKind, Item, ItemKind,
    Literal, RustTemplate, TypeBinder, TypeExpr, TypeExprKind,
};

pub struct CorePrinter<'a> {
    bindings: &'a BindingArena,
    assigned: HashMap<BindingId, String>,
    uses_of_name: HashMap<String, usize>,
}

impl<'a> CorePrinter<'a> {
    pub fn new(bindings: &'a BindingArena) -> Self {
        Self {
            bindings,
            assigned: HashMap::new(),
            uses_of_name: HashMap::new(),
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
            ItemKind::Struct(binding, fields) => {
                let mut parts = vec![self.name(*binding)];
                parts.extend(fields.iter().map(|(label, field_type)| {
                    format!("({} {})", label.name, self.type_expr(field_type))
                }));
                Self::list("struct", parts)
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
            ExprKind::Literal(Literal::Number(number)) => number.text().to_string(),
            ExprKind::Literal(Literal::String(text)) => format!("{text:?}"),
            ExprKind::Var(binding) => self.name(*binding),
            ExprKind::Fn(lambda) => {
                let params: Vec<String> = lambda
                    .params
                    .iter()
                    .map(|param| self.binder(param))
                    .collect();
                let mut parts = vec![format!("({})", params.join(" "))];
                parts.extend(self.block_items(&lambda.body));
                Self::list("fn", parts)
            }
            ExprKind::Call(callee, arguments) => {
                let mut parts = vec![self.expr(callee)];
                parts.extend(arguments.iter().map(|argument| self.expr(argument)));
                format!("({})", parts.join(" "))
            }
            ExprKind::Block(block) => Self::list("do", self.block_items(block)),
            ExprKind::If(test, then, otherwise) => Self::list(
                "if",
                vec![self.expr(test), self.expr(then), self.expr(otherwise)],
            ),
            ExprKind::New(binding, fields) => {
                let mut parts = vec![self.name(*binding)];
                parts.extend(
                    fields
                        .iter()
                        .map(|(label, value)| format!("({} {})", label.name, self.expr(value))),
                );
                Self::list("new", parts)
            }
            ExprKind::Field(value, label) => {
                Self::list("field", vec![self.expr(value), label.name.to_string()])
            }
            ExprKind::Quote(form) => Self::list("quote", vec![form.to_string()]),
            ExprKind::Rust(template) => self.rust(template),
        }
    }

    fn rust(&mut self, template: &RustTemplate) -> String {
        let mut parts = vec![
            format!("{:?}", template.template),
            self.type_expr(&template.type_expr),
        ];
        parts.extend(template.twin.iter().map(|twin| self.expr(twin)));
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
            TypeExprKind::Named(binding) => self.name(*binding),
            TypeExprKind::Apply(binding, arguments) => {
                let mut parts = vec![self.name(*binding)];
                parts.extend(arguments.iter().map(|argument| self.type_expr(argument)));
                format!("({})", parts.join(" "))
            }
            TypeExprKind::Function(parameters, result) => {
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
            TypeExprKind::Ref(inner) => Self::list("ref", vec![self.type_expr(inner)]),
            TypeExprKind::Mut(inner) => Self::list("mut", vec![self.type_expr(inner)]),
            TypeExprKind::Hole => "_".to_string(),
            TypeExprKind::Where(binders, inner) => {
                let binders: Vec<String> = binders
                    .iter()
                    .map(|binder| self.type_binder(binder))
                    .collect();
                Self::list(
                    "where",
                    vec![format!("({})", binders.join(" ")), self.type_expr(inner)],
                )
            }
        }
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
