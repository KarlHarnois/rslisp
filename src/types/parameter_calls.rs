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
            let mut first_calls = Self::first_calls(lambda);
            for (position, param) in lambda.params.iter().enumerate() {
                let Some(site) = first_calls.remove(&param.binding) else {
                    continue;
                };
                let parameter: Rc<str> = bindings.get(param.binding).name().into();
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

    fn first_calls(lambda: &Lambda) -> HashMap<BindingId, CallSite> {
        let mut first_calls = HashMap::new();
        for expr in lambda.body.expressions() {
            Self::record_first_calls(expr, &mut first_calls);
        }
        first_calls
    }

    fn record_first_calls(expr: &Expr, first_calls: &mut HashMap<BindingId, CallSite>) {
        if let ExprKind::Call(callee, arguments) = expr.kind()
            && let ExprKind::Leaf(Leaf::Var(binding)) = callee.kind()
        {
            first_calls.entry(*binding).or_insert(CallSite {
                span: expr.span(),
                arguments: arguments.len(),
            });
        }
        for child in expr.children() {
            Self::record_first_calls(child, first_calls);
        }
    }
}
