use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::core::{BindingArena, BindingId, CoreModule, Expr, ExprKind, ItemKind, Lambda, Leaf};
use crate::source::Span;

pub struct ParameterCall {
    pub span: Span,
    pub arguments: usize,
    pub parameter: Rc<str>,
    pub shadows_function: bool,
}

struct CallSite {
    span: Span,
    arguments: usize,
}

#[derive(Default)]
pub struct ParameterCalls {
    calls: HashMap<(BindingId, usize), ParameterCall>,
}

impl ParameterCalls {
    pub fn of(modules: &[CoreModule], bindings: &BindingArena) -> Self {
        let mut parameter_calls = Self::default();
        for module in modules {
            parameter_calls.record_module(module, bindings);
        }
        parameter_calls
    }

    pub fn get(&self, function: BindingId, position: usize) -> Option<&ParameterCall> {
        self.calls.get(&(function, position))
    }

    fn record_module(&mut self, module: &CoreModule, bindings: &BindingArena) {
        let functions = Self::function_names(module, bindings);
        for (function, lambda) in Self::functions(module) {
            for (position, param) in lambda.params.iter().enumerate() {
                let parameter: Rc<str> = bindings.get(param.binding).name().into();
                let Some(site) = Self::first_call(lambda.body.expressions(), param.binding) else {
                    continue;
                };
                let call = ParameterCall {
                    span: site.span,
                    arguments: site.arguments,
                    shadows_function: functions.contains(&parameter),
                    parameter,
                };
                self.calls.insert((function, position), call);
            }
        }
    }

    fn function_names(module: &CoreModule, bindings: &BindingArena) -> HashSet<Rc<str>> {
        Self::functions(module)
            .map(|(function, _)| bindings.get(function).name().into())
            .collect()
    }

    fn functions(module: &CoreModule) -> impl Iterator<Item = (BindingId, &Lambda)> {
        module.items.iter().filter_map(|item| match &item.kind {
            ItemKind::Def(binding, value) => match value.kind() {
                ExprKind::Fn(lambda) => Some((*binding, lambda.as_ref())),
                _ => None,
            },
            _ => None,
        })
    }

    fn first_call<'a>(
        mut expressions: impl Iterator<Item = &'a Expr>,
        parameter: BindingId,
    ) -> Option<CallSite> {
        expressions.find_map(|expr| Self::call_within(expr, parameter))
    }

    fn call_within(expr: &Expr, parameter: BindingId) -> Option<CallSite> {
        match Self::arguments_of_call(expr, parameter) {
            Some(arguments) => Some(CallSite {
                span: expr.span(),
                arguments,
            }),
            None => Self::first_call(expr.children().into_iter(), parameter),
        }
    }

    fn arguments_of_call(expr: &Expr, parameter: BindingId) -> Option<usize> {
        match expr.kind() {
            ExprKind::Call(callee, arguments) if matches!(callee.kind(), ExprKind::Leaf(Leaf::Var(binding)) if *binding == parameter) => {
                Some(arguments.len())
            }
            _ => None,
        }
    }
}
