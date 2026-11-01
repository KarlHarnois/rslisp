use rustc_hash::FxHashMap;

use crate::core::{BindingId, TraitMethod, TypeExpr, TypeExprKind};
use crate::source::Span;

#[derive(Clone, Copy)]
pub(super) struct SelflessMethod {
    pub trait_binding: BindingId,
    pub signature: Span,
}

#[derive(Default)]
pub(super) struct SelflessMethods {
    methods: FxHashMap<BindingId, SelflessMethod>,
}

impl SelflessMethods {
    pub fn record(
        &mut self,
        method: &TraitMethod,
        trait_binding: BindingId,
        self_variable: BindingId,
    ) {
        if !Self::mentions(&method.signature, self_variable) {
            let signature = method.signature.span();
            let selfless = SelflessMethod {
                trait_binding,
                signature,
            };
            self.methods.insert(method.binding, selfless);
        }
    }

    pub fn get(&self, method: BindingId) -> Option<SelflessMethod> {
        self.methods.get(&method).copied()
    }

    fn mentions(signature: &TypeExpr, variable: BindingId) -> bool {
        match signature.kind() {
            TypeExprKind::Named(binding, _) if *binding == variable => true,
            TypeExprKind::Where(_, inner) => Self::mentions(inner, variable),
            TypeExprKind::With(outer, inner) => {
                Self::mentions(outer, variable) || Self::mentions(inner, variable)
            }
            _ => signature
                .parts()
                .into_iter()
                .any(|part| Self::mentions(part, variable)),
        }
    }
}
