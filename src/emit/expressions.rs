use std::collections::HashSet;
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
    Borrow, Type, TypedArgument, TypedBinder, TypedBlock, TypedBodyItem, TypedControl, TypedExpr,
    TypedExprKind, TypedLambda, TypedLeaf, TypedProgram, TypedStructExpr,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum TypeContext {
    Known,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArgumentRole {
    Receiver,
    Borrowed,
    Ordinary,
}

impl ArgumentRole {
    fn borrows_by_itself(self, borrow: Borrow) -> bool {
        matches!(
            (self, borrow),
            (ArgumentRole::Receiver, Borrow::Shared | Borrow::Mutable)
                | (ArgumentRole::Borrowed, Borrow::Shared)
        )
    }
}

pub struct ExpressionEmitter<'a> {
    program: &'a TypedProgram,
    plan: &'a EmitPlan<'a>,
    module: ModuleId,
    pub locals: LocalNames,
    pub problems: Vec<Diagnostic>,
    mutable_locals: HashSet<BindingId>,
}

impl<'a> ExpressionEmitter<'a> {
    const ETA_PARAMETER: &'static str = "argument_";
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
            mutable_locals: HashSet::new(),
        }
    }

    pub fn with_mutable_locals(mut self, mutable_locals: HashSet<BindingId>) -> Self {
        self.mutable_locals = mutable_locals;
        self
    }

    pub fn types(&self) -> RustTypes<'a> {
        RustTypes::new(self.program, self.plan, self.module)
    }

    pub fn bind_parameter(&mut self, param: &TypedBinder, position: TypePosition) -> String {
        let name = self.locals.bind(
            param.binding,
            self.program.bindings.get(param.binding).name(),
        );
        let mutable = if self.mutable_locals.contains(&param.binding) && !param.ty.is_reference() {
            "mut "
        } else {
            ""
        };
        format!(
            "{mutable}{name}: {}",
            self.types().show(&param.ty, position)
        )
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
        self.expr_in(expr, TypeContext::Known).spanned(expr.span)
    }

    fn expr_in(&mut self, expr: &TypedExpr, context: TypeContext) -> Code {
        match &expr.kind {
            TypedExprKind::Leaf(leaf) => self.leaf(leaf, expr, context),
            TypedExprKind::Fn(lambda) => self.closure(lambda),
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments, expr.span),
            TypedExprKind::Control(control) => self.control(control),
            TypedExprKind::Struct(structure) => self.structure(structure, &expr.ty),
        }
    }

    fn leaf(&mut self, leaf: &TypedLeaf, expr: &TypedExpr, context: TypeContext) -> Code {
        match leaf {
            TypedLeaf::Literal(literal) => self.literal(literal, &expr.ty, context),
            TypedLeaf::Local(binding) => {
                Code::atom(self.locals.get(*binding).unwrap_or("_").to_string())
            }
            TypedLeaf::Global(binding) => self.global(*binding, &expr.ty, expr.span),
            TypedLeaf::Rust(template) => self.rust_value(template, &expr.ty, expr.span),
        }
    }

    fn control(&mut self, control: &TypedControl) -> Code {
        match control {
            TypedControl::Block(block) => self.block(block),
            TypedControl::If(test, then, otherwise) => self.conditional(test, then, otherwise),
        }
    }

    fn structure(&mut self, structure: &TypedStructExpr, ty: &Type) -> Code {
        match structure {
            TypedStructExpr::New(binding, fields) => self.construction(*binding, fields),
            TypedStructExpr::Field(value, label) => self.field_access(value, label, ty),
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
        let annotation = if binder.annotated || self.needs_annotation(&binder.ty) {
            format!(": {}", self.types().show(&binder.ty, TypePosition::Value))
        } else {
            String::new()
        };
        Code::joined(
            vec![
                Code::atom(format!(
                    "{} {name}{annotation} = ",
                    self.declaration(binder)
                )),
                value,
                Code::atom(";"),
            ],
            Precedence::Block,
        )
    }

    fn declaration(&self, binder: &TypedBinder) -> &'static str {
        if self.mutable_locals.contains(&binder.binding) {
            "let mut"
        } else {
            "let"
        }
    }

    fn needs_annotation(&self, bound: &Type) -> bool {
        match bound {
            Type::Primitive(primitive) => {
                primitive.is_integer() && *primitive != PrimitiveType::I32
            }
            Type::Named(binding, arguments) => {
                !self.types().is_opaque(*binding)
                    && arguments
                        .iter()
                        .any(|argument| self.needs_annotation(argument))
            }
            _ => false,
        }
    }

    fn literal(&self, literal: &Literal, ty: &Type, context: TypeContext) -> Code {
        match (literal, ty) {
            (Literal::String(text), _) => Code::atom(format!("{text:?}")),
            (Literal::Bool(value), _) => Code::atom(value.to_string()),
            (Literal::Number(number), Type::Generic(_)) => self.converted_literal(number, ty),
            (Literal::Number(number), _) => Self::number(number, ty, context),
        }
    }

    fn number(number: &Number, ty: &Type, context: TypeContext) -> Code {
        let text = Self::number_text(number, ty, context);
        let precedence = if text.starts_with('-') {
            Precedence::Prefix
        } else {
            Precedence::Atom
        };
        Code::with(text, precedence)
    }

    fn number_text(number: &Number, ty: &Type, context: TypeContext) -> String {
        let digits = Self::number_digits(number, ty);
        match (ty, context) {
            (Type::Primitive(primitive), TypeContext::Unknown) => {
                format!("{digits}_{}", primitive.name())
            }
            _ => digits,
        }
    }

    fn number_digits(number: &Number, ty: &Type) -> String {
        let digits = number.text().trim_start_matches('+').to_string();
        match (number.value(), ty) {
            (NumberValue::Integer(_), Type::Primitive(primitive)) if primitive.is_float() => {
                format!("{digits}.0")
            }
            _ => digits,
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
        if let Some(TypedExprKind::Leaf(TypedLeaf::Rust(template))) = self
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
            TemplateError::MalformedPlaceholder => {
                "a placeholder is `{` followed by an argument number and `}`, and `{{` is a literal brace"
                    .to_string()
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
        if let Some(template) = self.called_template(callee) {
            return self.template_call(&template, arguments, span);
        }
        if let Some(last) = Self::last_conflicting_argument(arguments) {
            return self.call_with_temporaries(callee, arguments, last);
        }
        let arguments = arguments
            .iter()
            .map(|argument| self.argument(argument, TypeContext::Known, ArgumentRole::Ordinary))
            .collect();
        self.call_code(callee, arguments)
    }

    fn call_code(&mut self, callee: &TypedExpr, arguments: Vec<Code>) -> Code {
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

    fn call_with_temporaries(
        &mut self,
        callee: &TypedExpr,
        arguments: &[TypedArgument],
        last: usize,
    ) -> Code {
        self.locals.enter();
        let mut statements = Vec::new();
        let mut codes = Vec::new();
        for (position, argument) in arguments.iter().enumerate() {
            let moves = argument.borrow != Borrow::Mutable && position <= last;
            if !moves {
                codes.push(self.argument(argument, TypeContext::Known, ArgumentRole::Ordinary));
                continue;
            }
            let temporary = self.locals.temporary(&format!("call_argument_{position}"));
            let value = self.argument(argument, TypeContext::Known, ArgumentRole::Ordinary);
            statements.push(Fragment::Newline);
            statements.push(Fragment::Text(format!("let {temporary} = ")));
            statements.extend(value.fragments);
            statements.push(Fragment::Text(";".to_string()));
            codes.push(Code::atom(temporary));
        }
        let call = self.call_code(callee, codes);
        self.locals.leave();
        statements.push(Fragment::Newline);
        statements.extend(call.fragments);
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

    fn last_conflicting_argument(arguments: &[TypedArgument]) -> Option<usize> {
        let borrowed: Vec<BindingId> = arguments
            .iter()
            .filter(|argument| argument.borrow == Borrow::Mutable)
            .filter_map(|argument| argument.value.place_root())
            .collect();
        arguments.iter().rposition(|argument| {
            argument.borrow != Borrow::Mutable
                && borrowed
                    .iter()
                    .any(|binding| argument.value.mentions_local(*binding))
        })
    }

    fn called_template(&self, callee: &TypedExpr) -> Option<Rc<RustTemplate>> {
        match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Rust(template)) => Some(template.clone()),
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => match self
                .plan
                .definition(*binding)
                .map(|definition| &definition.value.kind)
            {
                Some(TypedExprKind::Leaf(TypedLeaf::Rust(template))) => Some(template.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    fn template_call(
        &mut self,
        template: &RustTemplate,
        arguments: &[TypedArgument],
        span: Span,
    ) -> Code {
        let parsed = match Template::parse(&template.template) {
            Ok(parsed) => parsed,
            Err(error) => return self.template_problem(error, span),
        };
        let context = Self::argument_context(arguments);
        let arguments: Vec<Code> = arguments
            .iter()
            .enumerate()
            .map(|(position, argument)| {
                let role = Self::argument_role(template, &parsed, position);
                self.argument(argument, context, role)
            })
            .collect();
        self.inline(&parsed, arguments, span)
    }

    fn argument_context(arguments: &[TypedArgument]) -> TypeContext {
        let all_literals = arguments.iter().all(|argument| {
            matches!(
                argument.value.kind,
                TypedExprKind::Leaf(TypedLeaf::Literal(_))
            )
        });
        if all_literals {
            TypeContext::Unknown
        } else {
            TypeContext::Known
        }
    }

    fn argument_role(template: &RustTemplate, parsed: &Template, position: usize) -> ArgumentRole {
        if position == 0 && template.template.starts_with(Self::RECEIVER) {
            ArgumentRole::Receiver
        } else if parsed.borrows(position) {
            ArgumentRole::Borrowed
        } else {
            ArgumentRole::Ordinary
        }
    }

    fn argument(
        &mut self,
        argument: &TypedArgument,
        context: TypeContext,
        role: ArgumentRole,
    ) -> Code {
        let value = self
            .argument_value(&argument.value, context, role)
            .spanned(argument.value.span);
        match Self::borrow_prefix(argument, role) {
            Some(prefix) => Self::prefixed(&prefix, value).spanned(argument.value.span),
            None => value,
        }
    }

    fn argument_value(
        &mut self,
        value: &TypedExpr,
        context: TypeContext,
        role: ArgumentRole,
    ) -> Code {
        match (&value.kind, role) {
            (_, ArgumentRole::Ordinary) => self.expr_in(value, context),
            (TypedExprKind::Struct(TypedStructExpr::Field(owner, label)), _) => {
                self.field_place(owner, label)
            }
            (TypedExprKind::Leaf(TypedLeaf::Global(binding)), ArgumentRole::Receiver)
                if self.is_static(*binding) =>
            {
                Code::atom(self.plan.path_to(*binding, self.module))
            }
            _ => self.expr_in(value, context),
        }
    }

    fn is_static(&self, binding: BindingId) -> bool {
        self.plan.item(binding).map(|item| item.kind) == Some(ItemKind::Static)
    }

    fn borrow_prefix(argument: &TypedArgument, role: ArgumentRole) -> Option<String> {
        let borrowed_already = Self::borrowed_already(argument, role);
        match argument.borrow {
            Borrow::Shared if !borrowed_already => Some("&".to_string()),
            Borrow::Mutable if !borrowed_already => Some("&mut ".to_string()),
            Borrow::Deref(levels) => Some("*".repeat(levels)),
            Borrow::Reborrow => None,
            Borrow::Pending(_) => {
                unreachable!("the type checker decides every pending borrow")
            }
            _ => None,
        }
    }

    fn borrowed_already(argument: &TypedArgument, role: ArgumentRole) -> bool {
        argument.value.ty.is_reference() || role.borrows_by_itself(argument.borrow)
    }

    fn prefixed(prefix: &str, value: Code) -> Code {
        let value = if matches!(value.precedence, Precedence::Operator | Precedence::Block) {
            value.parenthesized()
        } else {
            value
        };
        Code::joined(vec![Code::atom(prefix), value], Precedence::Prefix)
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
        self.locals.enter();
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
        self.locals.leave();
        Code::joined(vec![head, body], Precedence::Operator)
    }

    fn block(&mut self, block: &TypedBlock) -> Code {
        self.locals.enter();
        let statements = self.statements(block);
        self.locals.leave();
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
        if let TypedExprKind::Control(TypedControl::If(next_test, next_then, next_otherwise)) =
            &otherwise.kind
        {
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
            TypedExprKind::Control(TypedControl::Block(block)) => self.statements(block),
            _ => {
                let mut fragments = vec![Fragment::Newline];
                fragments.extend(self.expr(branch).fragments);
                fragments
            }
        }
    }

    fn is_empty_block(expr: &TypedExpr) -> bool {
        matches!(&expr.kind, TypedExprKind::Control(TypedControl::Block(block)) if block.items.is_empty())
    }

    fn construction(&mut self, binding: BindingId, fields: &[(FieldLabel, TypedExpr)]) -> Code {
        let path = self.types().struct_path(binding);
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
        let access = self.field_place(value, label);
        if borrows_field {
            Code::joined(vec![Code::atom("&"), access], Precedence::Prefix)
        } else {
            access
        }
    }

    fn field_place(&mut self, value: &TypedExpr, label: &FieldLabel) -> Code {
        let value = self.expr(value);
        let value = if value.is_atomic() {
            value
        } else {
            value.parenthesized()
        };
        Code::joined(
            vec![
                value,
                Code::atom(format!(".{}", NameMangler::value(&label.name))),
            ],
            Precedence::Atom,
        )
    }

    fn declared_field_type(&self, value: &TypedExpr, label: &FieldLabel) -> Option<Type> {
        let mut owner = &value.ty;
        while let Type::Ref(inner) | Type::Mut(inner) = owner {
            owner = inner;
        }
        let Type::Named(binding, arguments) = owner else {
            return None;
        };
        self.program
            .structs
            .get(*binding)?
            .instantiated_field(&label.name, arguments)
    }
}
