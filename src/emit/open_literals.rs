use std::collections::HashSet;

use super::generic_positions::GenericPositions;
use super::let_annotation::LetAnnotation;
use super::plan::EmitPlan;
use super::rust_fallback::RustFallback;
use super::type_anchor::TypeAnchor;
use crate::core::{BindingId, BindingKind, FieldLabel, Literal, VariantInfo};
use crate::reader::Number;
use crate::source::Span;
use crate::types::{
    TypedArgument, TypedBinder, TypedBlock, TypedBodyItem, TypedControl, TypedExpr, TypedExprKind,
    TypedLeaf, TypedProgram, TypedStructExpr,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TypeContext {
    Known,
    Unknown,
}

#[derive(Default)]
pub(super) struct OpenLiterals {
    spans: HashSet<Span>,
}

impl OpenLiterals {
    pub(super) fn of(program: &TypedProgram, plan: &EmitPlan, value: &TypedExpr) -> Self {
        let mut search = OpenLiteralSearch {
            program,
            plan,
            spans: HashSet::new(),
        };
        search.expr(value, TypeContext::Known);
        Self {
            spans: search.spans,
        }
    }

    pub(super) fn contains(&self, literal: &TypedExpr) -> bool {
        self.spans.contains(&literal.span)
    }
}

struct OpenLiteralSearch<'a> {
    program: &'a TypedProgram,
    plan: &'a EmitPlan<'a>,
    spans: HashSet<Span>,
}

impl OpenLiteralSearch<'_> {
    fn expr(&mut self, expr: &TypedExpr, context: TypeContext) {
        match &expr.kind {
            TypedExprKind::Leaf(TypedLeaf::Literal(Literal::Number(number))) => {
                self.literal(expr, number, context)
            }
            TypedExprKind::Fn(lambda) => self.block(&lambda.body, context),
            TypedExprKind::Call(callee, arguments) => self.call(callee, arguments, context),
            TypedExprKind::Borrow(_, value) => self.expr(value, context),
            TypedExprKind::Control(control) => self.control(control, context),
            TypedExprKind::Struct(structure) => self.structure(structure, context),
            TypedExprKind::Variant(binding, payloads) => self.variant(*binding, payloads, context),
            _ => self.children(expr),
        }
    }

    fn literal(&mut self, literal: &TypedExpr, number: &Number, context: TypeContext) {
        if context == TypeContext::Unknown && !RustFallback::gives(number, &literal.ty) {
            self.spans.insert(literal.span);
        }
    }

    fn children(&mut self, expr: &TypedExpr) {
        for child in expr.children() {
            self.expr(child, TypeContext::Known);
        }
    }

    fn call(&mut self, callee: &TypedExpr, arguments: &[TypedArgument], context: TypeContext) {
        self.expr(callee, context);
        let values: Vec<&TypedExpr> = arguments.iter().map(|argument| &argument.value).collect();
        let contexts = match self.declared_parameters(callee) {
            Some(positions) => positions.contexts(&values, context),
            None => vec![TypeContext::Known; values.len()],
        };
        self.positions(&values, contexts);
    }

    fn declared_parameters(&self, callee: &TypedExpr) -> Option<GenericPositions> {
        match &callee.kind {
            TypedExprKind::Leaf(TypedLeaf::Global(binding)) => {
                let scheme = &self.plan.definition(*binding)?.scheme;
                GenericPositions::of_function(&scheme.params, &scheme.body)
            }
            TypedExprKind::Leaf(TypedLeaf::Rust(template)) => {
                GenericPositions::of_template(&template.type_expr)
            }
            _ => None,
        }
    }

    fn positions(&mut self, values: &[&TypedExpr], contexts: Vec<TypeContext>) {
        for (value, context) in values.iter().zip(contexts) {
            self.expr(value, context);
        }
    }

    fn control(&mut self, control: &TypedControl, context: TypeContext) {
        match control {
            TypedControl::Block(block) => self.block(block, context),
            TypedControl::If(test, then, otherwise) => {
                self.expr(test, TypeContext::Known);
                let branches = [then.as_ref(), otherwise.as_ref()];
                let contexts = Self::branch_contexts(&branches.map(Some), context);
                self.positions(&branches, contexts);
            }
            TypedControl::Match(matched) => {
                self.expr(&matched.value, TypeContext::Unknown);
                let tails: Vec<Option<&TypedExpr>> =
                    matched.arms.iter().map(|arm| arm.body.tail()).collect();
                let contexts = Self::branch_contexts(&tails, context);
                for (arm, context) in matched.arms.iter().zip(contexts) {
                    self.block(&arm.body, context);
                }
            }
        }
    }

    fn branch_contexts(tails: &[Option<&TypedExpr>], outer: TypeContext) -> Vec<TypeContext> {
        let anchor = TypeAnchor::among(
            tails
                .iter()
                .enumerate()
                .filter_map(|(branch, tail)| tail.map(|tail| (branch, tail))),
        );
        (0..tails.len())
            .map(|branch| {
                if anchor == Some(branch) {
                    outer
                } else {
                    TypeContext::Known
                }
            })
            .collect()
    }

    fn block(&mut self, block: &TypedBlock, context: TypeContext) {
        let tail = block.items.len().saturating_sub(1);
        for (position, item) in block.items.iter().enumerate() {
            match item {
                TypedBodyItem::Let(binder, value) => self.expr(value, self.let_context(binder)),
                TypedBodyItem::Expr(value) if position == tail => self.expr(value, context),
                TypedBodyItem::Expr(value) => self.expr(value, TypeContext::Unknown),
            }
        }
    }

    fn let_context(&self, binder: &TypedBinder) -> TypeContext {
        if LetAnnotation::new(self.program).is_emitted(binder) {
            TypeContext::Known
        } else {
            TypeContext::Unknown
        }
    }

    fn structure(&mut self, structure: &TypedStructExpr, context: TypeContext) {
        match structure {
            TypedStructExpr::New(binding, fields) => self.construction(*binding, fields, context),
            TypedStructExpr::Update(_, base, fields) => {
                self.expr(base, context);
                for (_, value) in fields {
                    self.expr(value, TypeContext::Known);
                }
            }
            TypedStructExpr::Field(owner, _) => self.expr(owner, TypeContext::Unknown),
        }
    }

    fn construction(
        &mut self,
        binding: BindingId,
        fields: &[(FieldLabel, TypedExpr)],
        context: TypeContext,
    ) {
        let definition = self
            .program
            .structs
            .get(binding)
            .expect("a constructed struct has a definition");
        let declared = fields.iter().map(|(label, _)| {
            definition
                .field(&label.name)
                .expect("the type stage checked each field label")
        });
        let values: Vec<&TypedExpr> = fields.iter().map(|(_, value)| value).collect();
        let contexts =
            GenericPositions::of_fields(&definition.params, declared).contexts(&values, context);
        self.positions(&values, contexts);
    }

    fn variant(&mut self, binding: BindingId, payloads: &[TypedExpr], context: TypeContext) {
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
        let declared = definition.variants[index].payload_types().into_iter();
        let values: Vec<&TypedExpr> = payloads.iter().collect();
        let contexts =
            GenericPositions::of_fields(&definition.params, declared).contexts(&values, context);
        self.positions(&values, contexts);
    }
}
