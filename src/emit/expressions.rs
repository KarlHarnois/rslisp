use std::rc::Rc;

use super::fragment::{Code, Fragment, Precedence};
use super::locals::LocalNames;
use super::mangle::NameMangler;
use super::plan::{EmitPlan, ItemKind};
use super::problems::EmitProblems;
use super::template::{Template, TemplateError};
use super::types::{RustTypes, TypePosition};
use crate::core::{BindingId, FieldLabel, Literal, ModuleId, PrimitiveType, RustTemplate};
use crate::diagnostic::Diagnostic;
use crate::reader::{Number, NumberValue};
use crate::source::Span;
use crate::types::{
    Borrow, Type, TypedArgument, TypedBinder, TypedBlock, TypedBodyItem, TypedExpr, TypedExprKind,
    TypedLambda, TypedProgram,
};

pub struct ExpressionEmitter<'a> {
    program: &'a TypedProgram,
    plan: &'a EmitPlan<'a>,
    module: ModuleId,
    pub locals: LocalNames,
    pub problems: Vec<Diagnostic>,
}

impl<'a> ExpressionEmitter<'a> {
    const ETA_PARAMETER: &'static str = "argument_";
    const I64_SUFFIX: &'static str = "_i64";
    const RECEIVER: &'static str = "{0}.";
    const INTEGER_CONVERSION: &'static str = "from_literal";
    const FLOAT_CONVERSION: &'static str = "from_float_literal";

    pub fn new(
        program: &'a TypedProgram,
        plan: &'a EmitPlan<'a>,
        module: ModuleId,
        locals: LocalNames,
    ) -> Self {
        Self {
            program,
            plan,
            module,
            locals,
            problems: Vec::new(),
        }
    }

    pub fn types(&self) -> RustTypes<'a> {
        RustTypes::new(self.program, self.plan, self.module)
    }

    pub fn bind_parameter(&mut self, param: &TypedBinder, position: TypePosition) -> String {
        let name = self.locals.bind(
            param.binding,
            self.program.bindings.get(param.binding).name(),
        );
        format!("{name}: {}", self.types().show(&param.ty, position))
    }

    pub fn function_body(&mut self, block: &TypedBlock) -> Vec<Fragment> {
        let Some((TypedBodyItem::Expr(tail), leading)) = block.items.split_last() else {
            return self.statements(block);
        };
        let TypedExprKind::Fn(lambda) = &tail.kind else {
            return self.statements(block);
        };
        let mut fragments = self.statements(&TypedBlock {
            items: leading.to_vec(),
        });
        fragments.push(Fragment::Newline);
        fragments.extend(
            Code::joined(
                vec![Code::atom("move "), self.closure(lambda)],
                Precedence::Operator,
            )
            .spanned(tail.span)
            .fragments,
        );
        fragments
    }

    pub fn statements(&mut self, block: &TypedBlock) -> Vec<Fragment> {
        let last = block.items.len().checked_sub(1);
        let mut fragments = Vec::new();
        for (position, item) in block.items.iter().enumerate() {
            fragments.push(Fragment::Newline);
            let is_tail = Some(position) == last;
            fragments.extend(self.statement(item, is_tail).fragments);
        }
        fragments
    }

    pub fn expr(&mut self, expr: &TypedExpr) -> Code {
        self.expr_in(expr, false).spanned(expr.span)
    }

    fn expr_in(&mut self, expr: &TypedExpr, unpinned: bool) -> Code {
        match &expr.kind {
            TypedExprKind::Literal(literal) => self.literal(literal, &expr.ty, unpinned),
            TypedExprKind::Local(binding) => {
                Code::atom(self.locals.get(*binding).unwrap_or("_").to_string())
            }
            TypedExprKind::Global(binding) => self.global(*binding, &expr.ty, expr.span),
            TypedExprKind::Fn(lambda) => self.closure(lambda),
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments, expr.span),
            TypedExprKind::Block(block) => self.block(block),
            TypedExprKind::If(test, then, otherwise) => self.conditional(test, then, otherwise),
            TypedExprKind::New(binding, fields) => self.construction(*binding, fields),
            TypedExprKind::Field(value, label) => self.field_access(value, label, &expr.ty),
            TypedExprKind::Rust(template) => self.rust_value(template, &expr.ty, expr.span),
        }
    }

    fn statement(&mut self, item: &TypedBodyItem, is_tail: bool) -> Code {
        match item {
            TypedBodyItem::Let(binder, value) => self.let_statement(binder, value),
            TypedBodyItem::Expr(expr) if is_tail => self.expr(expr),
            TypedBodyItem::Expr(expr) => {
                let code = self.expr(expr);
                let ends_with_block =
                    code.precedence == Precedence::Block && Type::unit() == expr.ty;
                let terminator = if ends_with_block { "" } else { ";" };
                Code::joined(vec![code, Code::atom(terminator)], Precedence::Block)
            }
        }
    }

    fn let_statement(&mut self, binder: &TypedBinder, value: &TypedExpr) -> Code {
        let value = self.expr(value);
        let name = self.locals.bind(
            binder.binding,
            self.program.bindings.get(binder.binding).name(),
        );
        let annotation = if binder.annotated || Self::needs_annotation(&binder.ty) {
            format!(": {}", self.types().show(&binder.ty, TypePosition::Value))
        } else {
            String::new()
        };
        Code::joined(
            vec![
                Code::atom(format!("let {name}{annotation} = ")),
                value,
                Code::atom(";"),
            ],
            Precedence::Block,
        )
    }

    fn needs_annotation(bound: &Type) -> bool {
        matches!(bound, Type::Primitive(primitive) if primitive.is_integer() && *primitive != PrimitiveType::I32)
    }

    fn literal(&self, literal: &Literal, ty: &Type, unpinned: bool) -> Code {
        match (literal, ty) {
            (Literal::String(text), _) => Code::atom(format!("{text:?}")),
            (Literal::Number(number), Type::Generic(_)) => self.converted_literal(number, ty),
            (Literal::Number(number), _) => {
                let text = number.text().trim_start_matches('+').to_string();
                let text = match (number.value(), ty) {
                    (NumberValue::Integer(_), Type::Primitive(primitive))
                        if primitive.is_float() =>
                    {
                        format!("{text}.0")
                    }
                    (NumberValue::Integer(_), Type::Primitive(PrimitiveType::I64)) if unpinned => {
                        format!("{text}{}", Self::I64_SUFFIX)
                    }
                    _ => text,
                };
                let precedence = if text.starts_with('-') {
                    Precedence::Prefix
                } else {
                    Precedence::Atom
                };
                Code::with(text, precedence)
            }
        }
    }

    fn converted_literal(&self, number: &Number, ty: &Type) -> Code {
        let conversion = match number.value() {
            NumberValue::Integer(_) => Self::INTEGER_CONVERSION,
            NumberValue::Float(_) => Self::FLOAT_CONVERSION,
        };
        let parameter = self.types().show(ty, TypePosition::Value);
        let text = number.text().trim_start_matches('+');
        Code::atom(format!("{parameter}::{conversion}({text})"))
    }

    fn global(&mut self, binding: BindingId, ty: &Type, span: Span) -> Code {
        if let Some(TypedExprKind::Rust(template)) = self
            .plan
            .definition(binding)
            .map(|definition| &definition.value.kind)
        {
            return self.rust_value(template, ty, span);
        }
        let path = self.plan.path_to(binding, self.module);
        match self.plan.item(binding).map(|item| item.kind) {
            Some(ItemKind::Static) => Code::with(format!("*{path}"), Precedence::Prefix),
            _ => Code::atom(path),
        }
    }

    fn rust_value(&mut self, template: &Rc<RustTemplate>, ty: &Type, span: Span) -> Code {
        let parsed = match Template::parse(&template.template) {
            Ok(parsed) => parsed,
            Err(error) => return self.template_problem(error, span),
        };
        match ty {
            Type::Function(..) if parsed.is_path() => Code::atom(template.template.to_string()),
            Type::Function(parameters, _) => self.eta_expanded(&parsed, parameters.len(), span),
            _ => self.inline(&parsed, Vec::new(), span),
        }
    }

    fn eta_expanded(&mut self, template: &Template, count: usize, span: Span) -> Code {
        let names: Vec<String> = (0..count)
            .map(|index| format!("{}{index}", Self::ETA_PARAMETER))
            .collect();
        let arguments = names.iter().map(|name| Code::atom(name.clone())).collect();
        let body = self.inline(template, arguments, span);
        Code::joined(
            vec![Code::atom(format!("|{}| ", names.join(", "))), body],
            Precedence::Operator,
        )
    }

    fn inline(&mut self, template: &Template, arguments: Vec<Code>, span: Span) -> Code {
        match template.inline(arguments) {
            Ok(code) => code,
            Err(error) => self.template_problem(error, span),
        }
    }

    fn template_problem(&mut self, error: TemplateError, span: Span) -> Code {
        let message = match error {
            TemplateError::UnclosedPlaceholder => {
                "a `{` starts a placeholder that never closes".to_string()
            }
            TemplateError::MissingArgument(index) => {
                format!("`{{{index}}}` has no matching argument")
            }
        };
        self.problems
            .push(EmitProblems::bad_template(&message, span));
        Code::atom("unreachable!()")
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &[TypedArgument], span: Span) -> Code {
        let template = self.called_template(callee);
        let unpinned = template.is_some()
            && arguments
                .iter()
                .all(|argument| matches!(argument.value.kind, TypedExprKind::Literal(_)));
        let has_receiver = template
            .as_ref()
            .is_some_and(|template| template.template.starts_with(Self::RECEIVER));
        let arguments: Vec<Code> = arguments
            .iter()
            .enumerate()
            .map(|(position, argument)| {
                self.argument(argument, unpinned, has_receiver && position == 0)
            })
            .collect();
        if let Some(template) = template {
            return match Template::parse(&template.template) {
                Ok(parsed) => self.inline(&parsed, arguments, span),
                Err(error) => self.template_problem(error, span),
            };
        }
        let callee = self.expr(callee);
        let callee = if callee.is_atomic() {
            callee
        } else {
            callee.parenthesized()
        };
        let mut parts = vec![callee, Code::atom("(")];
        parts.extend(Self::separated(arguments));
        parts.push(Code::atom(")"));
        Code::joined(parts, Precedence::Atom)
    }

    fn called_template(&self, callee: &TypedExpr) -> Option<Rc<RustTemplate>> {
        match &callee.kind {
            TypedExprKind::Rust(template) => Some(template.clone()),
            TypedExprKind::Global(binding) => match self
                .plan
                .definition(*binding)
                .map(|definition| &definition.value.kind)
            {
                Some(TypedExprKind::Rust(template)) => Some(template.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    fn argument(&mut self, argument: &TypedArgument, unpinned: bool, is_receiver: bool) -> Code {
        let value = self
            .expr_in(&argument.value, unpinned)
            .spanned(argument.value.span);
        let already_borrowed = argument.value.ty.is_reference();
        let prefix = match argument.borrow {
            Borrow::None | Borrow::Passthrough => return value,
            Borrow::Shared | Borrow::Mutable if already_borrowed || is_receiver => return value,
            Borrow::Shared => "&",
            Borrow::Mutable => "&mut ",
            Borrow::Deref => "*",
        };
        let value = if matches!(value.precedence, Precedence::Operator | Precedence::Block) {
            value.parenthesized()
        } else {
            value
        };
        Code::joined(vec![Code::atom(prefix), value], Precedence::Prefix)
            .spanned(argument.value.span)
    }

    fn separated(arguments: Vec<Code>) -> Vec<Code> {
        let mut parts = Vec::new();
        for (position, argument) in arguments.into_iter().enumerate() {
            if position > 0 {
                parts.push(Code::atom(", "));
            }
            parts.push(argument);
        }
        parts
    }

    fn closure(&mut self, lambda: &TypedLambda) -> Code {
        let params: Vec<String> = lambda
            .params
            .iter()
            .map(|param| self.bind_parameter(param, TypePosition::Value))
            .collect();
        let head = Code::atom(format!("|{}| ", params.join(", ")));
        let body = match lambda.body.items.as_slice() {
            [TypedBodyItem::Expr(only)] => self.expr(only),
            _ => self.block(&lambda.body),
        };
        Code::joined(vec![head, body], Precedence::Operator)
    }

    fn block(&mut self, block: &TypedBlock) -> Code {
        let statements = self.statements(block);
        Code {
            fragments: vec![
                Fragment::Text("{".to_string()),
                Fragment::Indented(statements),
                Fragment::Newline,
                Fragment::Text("}".to_string()),
            ],
            precedence: Precedence::Block,
        }
    }

    fn conditional(&mut self, test: &TypedExpr, then: &TypedExpr, otherwise: &TypedExpr) -> Code {
        let test = self.expr(test);
        let mut fragments = vec![Fragment::Text("if ".to_string())];
        fragments.extend(test.fragments);
        fragments.push(Fragment::Text(" {".to_string()));
        fragments.push(Fragment::Indented(self.branch(then)));
        fragments.push(Fragment::Newline);
        fragments.push(Fragment::Text("}".to_string()));
        if let TypedExprKind::If(next_test, next_then, next_otherwise) = &otherwise.kind {
            fragments.push(Fragment::Text(" else ".to_string()));
            fragments.extend(
                self.conditional(next_test, next_then, next_otherwise)
                    .fragments,
            );
        } else if !Self::is_empty_block(otherwise) {
            fragments.push(Fragment::Text(" else {".to_string()));
            fragments.push(Fragment::Indented(self.branch(otherwise)));
            fragments.push(Fragment::Newline);
            fragments.push(Fragment::Text("}".to_string()));
        }
        Code {
            fragments,
            precedence: Precedence::Block,
        }
    }

    fn branch(&mut self, branch: &TypedExpr) -> Vec<Fragment> {
        match &branch.kind {
            TypedExprKind::Block(block) => self.statements(block),
            _ => {
                let mut fragments = vec![Fragment::Newline];
                fragments.extend(self.expr(branch).fragments);
                fragments
            }
        }
    }

    fn is_empty_block(expr: &TypedExpr) -> bool {
        matches!(&expr.kind, TypedExprKind::Block(block) if block.items.is_empty())
    }

    fn construction(&mut self, binding: BindingId, fields: &[(FieldLabel, TypedExpr)]) -> Code {
        let path = self
            .types()
            .show(&Type::Struct(binding), TypePosition::Value);
        let mut parts = vec![Code::atom(format!("{path} {{ "))];
        for (position, (label, value)) in fields.iter().enumerate() {
            if position > 0 {
                parts.push(Code::atom(", "));
            }
            let field = NameMangler::value(&label.name);
            let value = self.expr(value);
            if value.text() == field {
                parts.push(value);
            } else {
                parts.push(Code::atom(format!("{field}: ")));
                parts.push(value);
            }
        }
        parts.push(Code::atom(" }"));
        Code::joined(parts, Precedence::Atom)
    }

    fn field_access(&mut self, value: &TypedExpr, label: &FieldLabel, result: &Type) -> Code {
        let borrows_field = result.is_reference()
            && !self
                .declared_field_type(value, label)
                .is_some_and(|declared| declared.is_reference());
        let value = self.expr(value);
        let value = if value.is_atomic() {
            value
        } else {
            value.parenthesized()
        };
        let access = Code::joined(
            vec![
                value,
                Code::atom(format!(".{}", NameMangler::value(&label.name))),
            ],
            Precedence::Atom,
        );
        if borrows_field {
            Code::joined(vec![Code::atom("&"), access], Precedence::Prefix)
        } else {
            access
        }
    }

    fn declared_field_type(&self, value: &TypedExpr, label: &FieldLabel) -> Option<Type> {
        let mut owner = &value.ty;
        while let Type::Ref(inner) | Type::Mut(inner) = owner {
            owner = inner;
        }
        let Type::Struct(binding) = owner else {
            return None;
        };
        self.program
            .structs
            .get(*binding)?
            .field(&label.name)
            .cloned()
    }
}
