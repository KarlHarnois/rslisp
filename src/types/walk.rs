use std::rc::Rc;

use super::typed::{
    TypedBinder, TypedBlock, TypedBodyItem, TypedControl, TypedExpr, TypedExprKind, TypedLambda,
    TypedStructExpr,
};

impl TypedExpr {
    pub fn children(&self) -> Vec<&TypedExpr> {
        match &self.kind {
            TypedExprKind::Leaf(_) => Vec::new(),
            TypedExprKind::Fn(lambda) => lambda.body.expressions().collect(),
            TypedExprKind::Call(callee, arguments) => std::iter::once(callee.as_ref())
                .chain(arguments.iter().map(|argument| &argument.value))
                .collect(),
            TypedExprKind::Recur(arguments) => {
                arguments.iter().map(|argument| &argument.value).collect()
            }
            TypedExprKind::Borrow(_, value) | TypedExprKind::Deref(value) => vec![value],
            TypedExprKind::Control(control) => control.children(),
            TypedExprKind::Struct(structure) => structure.children(),
        }
    }

    pub fn children_mut(&mut self) -> Vec<&mut TypedExpr> {
        match &mut self.kind {
            TypedExprKind::Leaf(_) => Vec::new(),
            TypedExprKind::Fn(lambda) => Rc::make_mut(lambda).body.expressions_mut().collect(),
            TypedExprKind::Call(callee, arguments) => std::iter::once(callee.as_mut())
                .chain(arguments.iter_mut().map(|argument| &mut argument.value))
                .collect(),
            TypedExprKind::Recur(arguments) => arguments
                .iter_mut()
                .map(|argument| &mut argument.value)
                .collect(),
            TypedExprKind::Borrow(_, value) | TypedExprKind::Deref(value) => vec![value],
            TypedExprKind::Control(control) => control.children_mut(),
            TypedExprKind::Struct(structure) => structure.children_mut(),
        }
    }

    pub fn binders(&self) -> Vec<&TypedBinder> {
        match &self.kind {
            TypedExprKind::Fn(lambda) => lambda.binders().collect(),
            TypedExprKind::Control(TypedControl::Block(block)) => block.binders().collect(),
            _ => Vec::new(),
        }
    }

    pub fn binders_mut(&mut self) -> Vec<&mut TypedBinder> {
        match &mut self.kind {
            TypedExprKind::Fn(lambda) => Rc::make_mut(lambda).binders_mut(),
            TypedExprKind::Control(TypedControl::Block(block)) => block.binders_mut().collect(),
            _ => Vec::new(),
        }
    }
}

impl TypedControl {
    fn children(&self) -> Vec<&TypedExpr> {
        match self {
            TypedControl::Block(block) => block.expressions().collect(),
            TypedControl::If(test, then, otherwise) => vec![test, then, otherwise],
        }
    }

    fn children_mut(&mut self) -> Vec<&mut TypedExpr> {
        match self {
            TypedControl::Block(block) => block.expressions_mut().collect(),
            TypedControl::If(test, then, otherwise) => {
                vec![test.as_mut(), then.as_mut(), otherwise.as_mut()]
            }
        }
    }
}

impl TypedStructExpr {
    fn children(&self) -> Vec<&TypedExpr> {
        match self {
            TypedStructExpr::New(_, fields) => fields.iter().map(|(_, value)| value).collect(),
            TypedStructExpr::Field(value, _) => vec![value],
        }
    }

    fn children_mut(&mut self) -> Vec<&mut TypedExpr> {
        match self {
            TypedStructExpr::New(_, fields) => fields.iter_mut().map(|(_, value)| value).collect(),
            TypedStructExpr::Field(value, _) => vec![value.as_mut()],
        }
    }
}

impl TypedLambda {
    pub fn binders(&self) -> impl Iterator<Item = &TypedBinder> {
        self.params.iter().chain(self.body.binders())
    }

    fn binders_mut(&mut self) -> Vec<&mut TypedBinder> {
        self.params
            .iter_mut()
            .chain(self.body.binders_mut())
            .collect()
    }
}

impl TypedBlock {
    pub fn expressions(&self) -> impl Iterator<Item = &TypedExpr> {
        self.items.iter().map(TypedBodyItem::expr)
    }

    fn expressions_mut(&mut self) -> impl Iterator<Item = &mut TypedExpr> {
        self.items.iter_mut().map(TypedBodyItem::expr_mut)
    }

    fn binders(&self) -> impl Iterator<Item = &TypedBinder> {
        self.items.iter().filter_map(TypedBodyItem::binder)
    }

    fn binders_mut(&mut self) -> impl Iterator<Item = &mut TypedBinder> {
        self.items.iter_mut().filter_map(TypedBodyItem::binder_mut)
    }
}

impl TypedBodyItem {
    pub fn expr(&self) -> &TypedExpr {
        match self {
            TypedBodyItem::Let(_, value) | TypedBodyItem::Expr(value) => value,
        }
    }

    fn expr_mut(&mut self) -> &mut TypedExpr {
        match self {
            TypedBodyItem::Let(_, value) | TypedBodyItem::Expr(value) => value,
        }
    }

    fn binder(&self) -> Option<&TypedBinder> {
        match self {
            TypedBodyItem::Let(binder, _) => Some(binder),
            TypedBodyItem::Expr(_) => None,
        }
    }

    fn binder_mut(&mut self) -> Option<&mut TypedBinder> {
        match self {
            TypedBodyItem::Let(binder, _) => Some(binder),
            TypedBodyItem::Expr(_) => None,
        }
    }
}
