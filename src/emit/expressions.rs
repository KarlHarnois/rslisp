use std::collections::HashSet;
use std::rc::Rc;

use super::fragment::{Code, Fragment, Precedence};
use super::locals::LocalNames;
use super::mangle::NameMangler;
use super::operator::{RustOperator, Side};
use super::plan::{EmitPlan, ItemKind};
use super::problems::EmitProblems;
use super::template::{Template, TemplateError};
use super::types::{RustTypes, TypePosition};
use super::unsized_parameters::UnsizedParameters;
use crate::core::{
    BindingId, BindingKind, FieldLabel, Literal, ModuleId, PrimitiveType, ReferenceKind,
    RustTemplate, VariantInfo,
};
use crate::diagnostic::Diagnostic;
use crate::reader::{Number, NumberValue};
use crate::source::Span;
use crate::types::{
    Borrow, Type, TypedArgument, TypedArm, TypedBinder, TypedBlock, TypedBodyItem, TypedControl,
    TypedExpr, TypedExprKind, TypedLambda, TypedLeaf, TypedPattern, TypedPatternKind, TypedProgram,
    TypedStructExpr, VariantPayload,
};

mod tail_loop;
use tail_loop::TailLoop;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TypeContext {
    Known,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArgumentRole {
    Receiver,
    Borrowed,
    MutPlace,
    Ordinary,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoopExit {
    Return,
    Break,
}

impl LoopExit {
    fn keyword(self) -> &'static str {
        match self {
            LoopExit::Return => "return",
            LoopExit::Break => "break",
        }
    }
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
    unsized_parameters: &'a UnsizedParameters,
    module: ModuleId,
    pub locals: LocalNames,
    pub problems: Vec<Diagnostic>,
    mutable_locals: HashSet<BindingId>,
    tail_loop: Option<TailLoop>,
    loop_exit: LoopExit,
}

impl<'a> ExpressionEmitter<'a> {
    const ETA_PARAMETER: &'static str = "argument_";
    const RECEIVER: &'static str = "{0}.";
    const INTEGER_CONVERSION: &'static str = "from_literal";
    const FLOAT_CONVERSION: &'static str = "from_float_literal";

    pub fn new(
        program: &'a TypedProgram,
        plan: &'a EmitPlan<'a>,
        unsized_parameters: &'a UnsizedParameters,
        module: ModuleId,
        locals: LocalNames,
    ) -> Self {
        Self {
            program,
            plan,
            unsized_parameters,
            module,
            locals,
            problems: Vec::new(),
            mutable_locals: HashSet::new(),
            tail_loop: None,
            loop_exit: LoopExit::Return,
        }
    }

    pub fn prepare_tail_loop(&mut self, function: BindingId, lambda: &TypedLambda) {
        self.tail_loop = TailLoop::of(function, lambda);
    }

    pub fn with_mutable_locals(mut self, mutable_locals: HashSet<BindingId>) -> Self {
        self.mutable_locals = mutable_locals;
        self
    }

    pub fn types(&self) -> RustTypes<'a> {
        RustTypes::new(
            self.program,
            self.plan,
            self.unsized_parameters,
            self.module,
        )
    }

    pub fn bind_parameter(&mut self, param: &TypedBinder, position: TypePosition) -> String {
        let name = self.locals.bind(
            param.binding,
            self.program.bindings.get(param.binding).name(),
        );
        let reassigned = self
            .tail_loop
            .as_ref()
            .is_some_and(|tail_loop| tail_loop.reassigns(param.binding));
        let mutable = if reassigned
            || (self.mutable_locals.contains(&param.binding) && !param.ty.is_reference())
        {
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
        if self.tail_loop.is_some() {
            let mut fragments = vec![Fragment::Newline];
            fragments.extend(self.loop_expression(block, LoopExit::Return));
            return fragments;
        }
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
                vec![Code::atom("move "), self.closure(lambda, &tail.ty)],
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
            let is_tail = Some(position) == last;
            if is_tail
                && let TypedBodyItem::Expr(expr) = item
                && let Some(contents) = self.tail_block_contents(expr)
            {
                fragments.extend(contents);
                continue;
            }
            fragments.push(Fragment::Newline);
            fragments.extend(self.statement(item, is_tail).fragments);
        }
        fragments
    }

    fn tail_block_contents(&mut self, expr: &TypedExpr) -> Option<Vec<Fragment>> {
        match &expr.kind {
            TypedExprKind::Control(TypedControl::Block(block)) => Some(self.block_contents(block)),
            TypedExprKind::Call(callee, arguments) => match &callee.kind {
                TypedExprKind::Fn(lambda) if !Self::contains_try(&lambda.body) => {
                    Some(self.inline_call_contents(lambda, arguments))
                }
                _ => None,
            },
            _ => None,
        }
    }

    pub fn expr(&mut self, expr: &TypedExpr) -> Code {
        self.expr_in(expr, TypeContext::Known).spanned(expr.span)
    }

    fn expr_in(&mut self, expr: &TypedExpr, context: TypeContext) -> Code {
        match &expr.kind {
            TypedExprKind::Leaf(leaf) => self.leaf(leaf, expr, context),
            TypedExprKind::Fn(lambda) => self.closure(lambda, &expr.ty),
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments, expr.span),
            TypedExprKind::Recur(_) => {
                unreachable!("expansion allows `recur` only in tail position")
            }
            TypedExprKind::Try(_) | TypedExprKind::Borrow(..) | TypedExprKind::Deref(..) => {
                self.written(expr)
            }
            _ => self.value(expr),
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

    fn propagate(&mut self, value: &TypedExpr) -> Code {
        let value = self.expr_in(value, TypeContext::Known);
        let value = if value.is_atomic() {
            value
        } else {
            value.parenthesized()
        };
        Code::joined(vec![value, Code::atom("?")], Precedence::Atom)
    }

    fn written(&mut self, expr: &TypedExpr) -> Code {
        match &expr.kind {
            TypedExprKind::Try(value) => self.propagate(value),
            TypedExprKind::Borrow(kind, value) => self.borrow(*kind, value),
            TypedExprKind::Deref(value) => self.deref(value),
            _ => unreachable!("the caller matches a written form"),
        }
    }

    fn value(&mut self, expr: &TypedExpr) -> Code {
        match &expr.kind {
            TypedExprKind::Control(control) => self.control(control),
            TypedExprKind::Struct(structure) => self.structure(structure, &expr.ty),
            TypedExprKind::Variant(binding, arguments) => self.variant(*binding, arguments),
            _ => unreachable!("the caller matches a value form"),
        }
    }

    fn borrow(&mut self, kind: ReferenceKind, value: &TypedExpr) -> Code {
        if Self::passes_through_reference(self.program, kind, value) {
            return self.expr_in(value, TypeContext::Known);
        }
        let place = self.place(value, TypeContext::Unknown);
        let prefix = match kind {
            ReferenceKind::Shared => "&",
            ReferenceKind::Mutable => "&mut ",
        };
        Self::prefixed(prefix, place)
    }

    fn passes_through_reference(
        program: &TypedProgram,
        kind: ReferenceKind,
        value: &TypedExpr,
    ) -> bool {
        let TypedExprKind::Leaf(TypedLeaf::Local(binding)) = &value.kind else {
            return false;
        };
        program.borrowed_parameters.contains(binding)
            && matches!(
                (kind, &value.ty),
                (ReferenceKind::Shared, Type::Ref(_)) | (ReferenceKind::Mutable, Type::Mut(_))
            )
    }

    fn deref(&mut self, value: &TypedExpr) -> Code {
        Self::prefixed("*", self.expr_in(value, TypeContext::Known))
    }

    fn place(&mut self, value: &TypedExpr, context: TypeContext) -> Code {
        match &value.kind {
            TypedExprKind::Struct(TypedStructExpr::Field(owner, label)) => {
                self.field_place(owner, label)
            }
            _ => self.expr_in(value, context),
        }
    }

    fn control(&mut self, control: &TypedControl) -> Code {
        match control {
            TypedControl::Block(block) => self.block(block),
            TypedControl::If(test, then, otherwise) => self.conditional(test, then, otherwise),
            TypedControl::Match(value, arms) => self.matching(value, arms),
        }
    }

    fn matching(&mut self, value: &TypedExpr, arms: &[TypedArm]) -> Code {
        let uses_strings = Self::uses_string_literals(arms);
        let string_value = Self::is_string_value(&value.ty);
        let value = self.expr(value);
        let value = if uses_strings && string_value {
            self.string_view(value)
        } else {
            value
        };
        let mut fragments = vec![Fragment::Text("match ".to_string())];
        fragments.extend(value.fragments);
        fragments.push(Fragment::Text(" {".to_string()));
        let mut inner = Vec::new();
        for arm in arms {
            inner.push(Fragment::Newline);
            self.locals.enter();
            inner.extend(self.pattern(&arm.pattern).fragments);
            inner.push(Fragment::Text(" => ".to_string()));
            inner.extend(self.arm_body(&arm.body));
            inner.push(Fragment::Text(",".to_string()));
            self.locals.leave();
        }
        fragments.push(Fragment::Indented(inner));
        fragments.push(Fragment::Newline);
        fragments.push(Fragment::Text("}".to_string()));
        Code {
            fragments,
            precedence: Precedence::Block,
        }
    }

    fn uses_string_literals(arms: &[TypedArm]) -> bool {
        arms.iter().any(|arm| {
            matches!(
                arm.pattern.kind,
                TypedPatternKind::Literal(Literal::String(_), _)
            )
        })
    }

    fn is_string_value(ty: &Type) -> bool {
        match ty {
            Type::Primitive(PrimitiveType::String) => true,
            Type::Ref(inner) => matches!(inner.as_ref(), Type::Primitive(PrimitiveType::String)),
            _ => false,
        }
    }

    fn string_view(&self, value: Code) -> Code {
        let value = if value.is_atomic() {
            value
        } else {
            value.parenthesized()
        };
        Code::joined(vec![value, Code::atom(".as_str()")], Precedence::Atom)
    }

    fn arm_body(&mut self, body: &TypedBlock) -> Vec<Fragment> {
        match body.items.as_slice() {
            [] => vec![Fragment::Text("{}".to_string())],
            [TypedBodyItem::Expr(only)] if Self::is_empty_block(only) => {
                vec![Fragment::Text("{}".to_string())]
            }
            [TypedBodyItem::Expr(only)] => self.expr(only).fragments,
            _ => self.block(body).fragments,
        }
    }

    fn pattern(&mut self, pattern: &TypedPattern) -> Code {
        match &pattern.kind {
            TypedPatternKind::Wildcard => Code::atom("_"),
            TypedPatternKind::Bind(binder) => Code::atom(self.locals.bind(
                binder.binding,
                self.program.bindings.get(binder.binding).name(),
            )),
            TypedPatternKind::Variant(binding, patterns) => {
                self.variant_pattern(*binding, patterns)
            }
            TypedPatternKind::Literal(literal, ty) => self.literal_pattern(literal, ty),
        }
    }

    fn literal_pattern(&self, literal: &Literal, ty: &Type) -> Code {
        match literal {
            Literal::Number(number) => Code::atom(Self::number_digits(number, ty)),
            Literal::String(text) => Code::atom(format!("{text:?}")),
            Literal::Bool(value) => Code::atom(value.to_string()),
        }
    }

    fn variant_pattern(&mut self, binding: BindingId, patterns: &[TypedPattern]) -> Code {
        let BindingKind::Variant(_, VariantInfo { owner, index, .. }) =
            self.program.bindings.get(binding).kind()
        else {
            unreachable!("a typed variant pattern names a variant binding");
        };
        let definition = self
            .program
            .enums
            .get(owner)
            .expect("a reached enum has a definition");
        let variant = &definition.variants[index];
        let name = variant.name.clone();
        let payload = variant.payload.clone();
        let rust_path = definition.rust.as_ref().map(|rust| rust.path.clone());
        let path = Self::variant_path(self.types().struct_path(owner), rust_path, &name);
        match &payload {
            VariantPayload::None => Code::atom(path),
            VariantPayload::Positional(_) => self.positional_pattern(path, patterns),
            VariantPayload::Named(fields) => self.named_pattern(path, fields, patterns),
        }
    }

    fn variant_path(owner_path: String, rust_path: Option<std::rc::Rc<str>>, name: &str) -> String {
        let owner = rust_path.map_or(owner_path, |path| path.to_string());
        format!("{owner}::{}", NameMangler::type_name(name))
    }

    fn positional_pattern(&mut self, path: String, patterns: &[TypedPattern]) -> Code {
        let mut parts = vec![Code::atom(format!("{path}("))];
        for (position, pattern) in patterns.iter().enumerate() {
            if position > 0 {
                parts.push(Code::atom(", "));
            }
            parts.push(self.pattern(pattern));
        }
        parts.push(Code::atom(")"));
        Code::joined(parts, Precedence::Atom)
    }

    fn named_pattern(
        &mut self,
        path: String,
        fields: &[(std::rc::Rc<str>, Type)],
        patterns: &[TypedPattern],
    ) -> Code {
        let mut parts = vec![Code::atom(format!("{path} {{ "))];
        let mut written = 0;
        let mut ignores = false;
        for ((label, _), pattern) in fields.iter().zip(patterns) {
            if matches!(pattern.kind, TypedPatternKind::Wildcard) {
                ignores = true;
                continue;
            }
            if written > 0 {
                parts.push(Code::atom(", "));
            }
            written += 1;
            let field = NameMangler::value(label);
            let value = self.pattern(pattern);
            if value.text() == field {
                parts.push(value);
            } else {
                parts.push(Code::atom(format!("{field}: ")));
                parts.push(value);
            }
        }
        if ignores {
            if written > 0 {
                parts.push(Code::atom(", "));
            }
            parts.push(Code::atom(".."));
        }
        parts.push(Code::atom(" }"));
        Code::joined(parts, Precedence::Atom)
    }

    fn variant(&mut self, binding: BindingId, arguments: &[TypedExpr]) -> Code {
        let BindingKind::Variant(_, VariantInfo { owner, index, .. }) =
            self.program.bindings.get(binding).kind()
        else {
            unreachable!("a typed variant expression names a variant binding");
        };
        let definition = self
            .program
            .enums
            .get(owner)
            .expect("a reached enum has a definition");
        let variant = &definition.variants[index];
        let name = variant.name.clone();
        let payload = variant.payload.clone();
        let rust_path = definition.rust.as_ref().map(|rust| rust.path.clone());
        let path = Self::variant_path(self.types().struct_path(owner), rust_path, &name);
        match &payload {
            VariantPayload::None => Code::atom(path),
            VariantPayload::Positional(_) => self.positional_variant(path, arguments),
            VariantPayload::Named(fields) => self.named_variant(path, fields, arguments),
        }
    }

    fn positional_variant(&mut self, path: String, arguments: &[TypedExpr]) -> Code {
        let mut parts = vec![Code::atom(format!("{path}("))];
        for (position, argument) in arguments.iter().enumerate() {
            if position > 0 {
                parts.push(Code::atom(", "));
            }
            parts.push(self.expr(argument));
        }
        parts.push(Code::atom(")"));
        Code::joined(parts, Precedence::Atom)
    }

    fn named_variant(
        &mut self,
        path: String,
        fields: &[(std::rc::Rc<str>, Type)],
        arguments: &[TypedExpr],
    ) -> Code {
        if fields.is_empty() {
            return Code::atom(path);
        }
        let mut parts = vec![Code::atom(format!("{path} {{ "))];
        for (position, ((label, _), argument)) in fields.iter().zip(arguments).enumerate() {
            if position > 0 {
                parts.push(Code::atom(", "));
            }
            let field = NameMangler::value(label);
            parts.push(Code::atom(format!("{field}: ")));
            parts.push(self.expr(argument));
        }
        parts.push(Code::atom(" }"));
        Code::joined(parts, Precedence::Atom)
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
        let parsed = Template::new(&template.template);
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

    fn inline_with_places(
        &mut self,
        template: &Template,
        arguments: Vec<Code>,
        places: &[bool],
        span: Span,
    ) -> Code {
        match template.inline_with_places(arguments, places) {
            Ok(code) => code,
            Err(error) => self.template_problem(error, span),
        }
    }

    fn template_problem(&mut self, error: TemplateError, span: Span) -> Code {
        let message = match error {
            TemplateError::MissingArgument(index) => {
                format!("`{{{index}}}` has no matching argument")
            }
        };
        self.problems
            .push(EmitProblems::bad_template(&message, span));
        Code::atom("unreachable!()")
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &[TypedArgument], span: Span) -> Code {
        if let TypedExprKind::Fn(lambda) = &callee.kind
            && !Self::contains_try(&lambda.body)
        {
            return self.inline_call(lambda, arguments);
        }
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

    fn contains_try(block: &TypedBlock) -> bool {
        block.expressions().any(Self::expression_contains_try)
    }

    fn expression_contains_try(expr: &TypedExpr) -> bool {
        match &expr.kind {
            TypedExprKind::Try(_) => true,
            TypedExprKind::Fn(_) => false,
            _ => expr
                .children()
                .into_iter()
                .any(Self::expression_contains_try),
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
        let parsed = Template::new(&template.template);
        let context = Self::argument_context(arguments);
        let roles: Vec<ArgumentRole> = arguments
            .iter()
            .enumerate()
            .map(|(position, argument)| Self::argument_role(template, &parsed, position, argument))
            .collect();
        let places: Vec<bool> = roles
            .iter()
            .map(|role| *role == ArgumentRole::MutPlace)
            .collect();
        let arguments: Vec<Code> = arguments
            .iter()
            .zip(&roles)
            .map(|(argument, role)| self.argument(argument, context, *role))
            .collect();
        self.inline_with_places(&parsed, arguments, &places, span)
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

    fn argument_role(
        template: &RustTemplate,
        parsed: &Template,
        position: usize,
        argument: &TypedArgument,
    ) -> ArgumentRole {
        if Self::is_receiver(template, parsed, position) {
            ArgumentRole::Receiver
        } else if Self::is_mut_place(parsed, position, argument) {
            ArgumentRole::MutPlace
        } else if parsed.borrows(position) {
            ArgumentRole::Borrowed
        } else {
            ArgumentRole::Ordinary
        }
    }

    fn is_receiver(template: &RustTemplate, parsed: &Template, position: usize) -> bool {
        (position == 0 && template.template.starts_with(Self::RECEIVER)) || parsed.indexes(position)
    }

    fn is_mut_place(parsed: &Template, position: usize, argument: &TypedArgument) -> bool {
        parsed.dereferences(position)
            && (Self::written_mut_place(&argument.value)
                || (argument.borrow == Borrow::Mutable
                    && !argument.value.ty.is_reference()
                    && argument.value.place_root().is_some()))
    }

    fn written_mut_place(value: &TypedExpr) -> bool {
        matches!(
            &value.kind,
            TypedExprKind::Borrow(ReferenceKind::Mutable, place)
                if place.is_place() && !place.ty.is_reference()
        )
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
        let prefix = match role {
            ArgumentRole::MutPlace => None,
            _ => Self::borrow_prefix(argument, role),
        };
        match prefix {
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
            (TypedExprKind::Borrow(ReferenceKind::Mutable, place), ArgumentRole::MutPlace) => {
                self.place(place, TypeContext::Known)
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

    fn closure(&mut self, lambda: &TypedLambda, ty: &Type) -> Code {
        let annotated = Self::contains_try(&lambda.body);
        let previous = std::mem::replace(&mut self.tail_loop, TailLoop::of_closure(lambda));
        self.locals.enter();
        let params: Vec<String> = lambda
            .params
            .iter()
            .map(|param| self.bind_parameter(param, TypePosition::Value))
            .collect();
        let body = self.closure_body(lambda, annotated);
        self.locals.leave();
        self.tail_loop = previous;
        let head = match (annotated, ty) {
            (true, Type::Function(_, result)) => {
                let result = self.types().show(result, TypePosition::Value);
                Code::atom(format!("|{}| -> {result} ", params.join(", ")))
            }
            _ => Code::atom(format!("|{}| ", params.join(", "))),
        };
        Code::joined(vec![head, body], Precedence::Operator)
    }

    fn closure_body(&mut self, lambda: &TypedLambda, annotated: bool) -> Code {
        if self.tail_loop.is_some() {
            let fragments = self.loop_expression(&lambda.body, LoopExit::Return);
            return if annotated {
                Self::braced(fragments)
            } else {
                Code {
                    fragments,
                    precedence: Precedence::Block,
                }
            };
        }
        if annotated {
            return self.block(&lambda.body);
        }
        match lambda.body.items.as_slice() {
            [TypedBodyItem::Expr(only)] => self.expr(only),
            _ => self.block(&lambda.body),
        }
    }

    fn braced(fragments: Vec<Fragment>) -> Code {
        let mut inner = vec![Fragment::Newline];
        inner.extend(fragments);
        Code {
            fragments: vec![
                Fragment::Text("{".to_string()),
                Fragment::Indented(inner),
                Fragment::Newline,
                Fragment::Text("}".to_string()),
            ],
            precedence: Precedence::Block,
        }
    }

    fn inline_call(&mut self, lambda: &TypedLambda, arguments: &[TypedArgument]) -> Code {
        let fragments = self.inline_call_contents(lambda, arguments);
        Code {
            fragments: vec![
                Fragment::Text("{".to_string()),
                Fragment::Indented(fragments),
                Fragment::Newline,
                Fragment::Text("}".to_string()),
            ],
            precedence: Precedence::Block,
        }
    }

    fn inline_call_contents(
        &mut self,
        lambda: &TypedLambda,
        arguments: &[TypedArgument],
    ) -> Vec<Fragment> {
        let previous = std::mem::replace(&mut self.tail_loop, TailLoop::of_closure(lambda));
        self.locals.enter();
        let mut fragments = Vec::new();
        for (param, argument) in lambda.params.iter().zip(arguments) {
            let binding = self.bind_parameter(param, TypePosition::Value);
            let value = self.argument(argument, TypeContext::Known, ArgumentRole::Ordinary);
            fragments.push(Fragment::Newline);
            fragments.push(Fragment::Text(format!("let {binding} = ")));
            fragments.extend(value.fragments);
            fragments.push(Fragment::Text(";".to_string()));
        }
        if self.tail_loop.is_some() {
            fragments.push(Fragment::Newline);
            fragments.extend(self.loop_expression(&lambda.body, LoopExit::Break));
        } else {
            fragments.extend(self.statements(&lambda.body));
        }
        self.locals.leave();
        self.tail_loop = previous;
        fragments
    }

    fn block(&mut self, block: &TypedBlock) -> Code {
        let statements = self.block_contents(block);
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

    fn block_contents(&mut self, block: &TypedBlock) -> Vec<Fragment> {
        self.locals.enter();
        let statements = self.statements(block);
        self.locals.leave();
        statements
    }

    fn conditional(&mut self, test: &TypedExpr, then: &TypedExpr, otherwise: &TypedExpr) -> Code {
        if let Some(operator) = Self::short_circuit_operator(then, otherwise) {
            let value = if Self::boolean_literal(then) == Some(true) {
                otherwise
            } else {
                then
            };
            return self.short_circuit(operator, test, value);
        }
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

    fn short_circuit(
        &mut self,
        operator: RustOperator,
        test: &TypedExpr,
        value: &TypedExpr,
    ) -> Code {
        let test = self.expr(test);
        let value = self.expr(value);
        let test = self.binary_operand(test, operator, Side::Left);
        let value = self.binary_operand(value, operator, Side::Right);
        Code::joined(
            vec![test, Code::atom(format!(" {} ", operator.symbol())), value],
            Precedence::Operator,
        )
    }

    fn binary_operand(&self, code: Code, operator: RustOperator, side: Side) -> Code {
        let needs_parentheses = match code.precedence {
            Precedence::Atom | Precedence::Prefix => false,
            Precedence::Block => true,
            Precedence::Operator => RustOperator::outermost(&code.text())
                .is_none_or(|inner| inner.needs_parentheses(operator, side)),
        };
        if needs_parentheses {
            code.parenthesized()
        } else {
            code
        }
    }

    fn short_circuit_operator(then: &TypedExpr, otherwise: &TypedExpr) -> Option<RustOperator> {
        match (
            Self::boolean_literal(then),
            Self::boolean_literal(otherwise),
        ) {
            (Some(true), None) => Some(RustOperator::or()),
            (None, Some(false)) => Some(RustOperator::and()),
            _ => None,
        }
    }

    fn boolean_literal(expr: &TypedExpr) -> Option<bool> {
        match &expr.kind {
            TypedExprKind::Leaf(TypedLeaf::Literal(Literal::Bool(value))) => Some(*value),
            _ => None,
        }
    }

    fn branch(&mut self, branch: &TypedExpr) -> Vec<Fragment> {
        if let Some(contents) = self.tail_block_contents(branch) {
            return contents;
        }
        let mut fragments = vec![Fragment::Newline];
        fragments.extend(self.expr(branch).fragments);
        fragments
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
