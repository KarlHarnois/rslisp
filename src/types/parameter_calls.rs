use std::rc::Rc;

use super::checker::TypeChecker;
use super::problems::TypeProblems;
use crate::core::{BindingArena, BindingId, CoreModule, Expr, ExprKind, ItemKind, Lambda, Leaf};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::source::Span;

pub struct ParameterCall {
    pub span: Span,
    pub arguments: usize,
    pub parameter: Rc<str>,
    pub shadows_function: bool,
}

pub(super) struct ParameterCallRequest {
    pub callee: BindingId,
    pub position: usize,
    pub argument: Span,
}

impl ParameterCallRequest {
    fn explains(&self, diagnostic: &Diagnostic) -> bool {
        *diagnostic.code() == DiagnosticCode::MISMATCHED_TYPES
            && diagnostic.primary().span() == self.argument
    }
}

struct CallSite {
    span: Span,
    arguments: usize,
}

struct ParameterCalls<'m> {
    modules: &'m [CoreModule],
    bindings: &'m BindingArena,
}

impl TypeChecker {
    pub(super) fn label_parameter_calls(&mut self, modules: &[CoreModule]) {
        if self.parameter_call_requests.is_empty() {
            return;
        }
        let requests = std::mem::take(&mut self.parameter_call_requests);
        let calls = ParameterCalls {
            modules,
            bindings: &self.bindings,
        };
        let diagnostics = std::mem::take(&mut self.diagnostics);
        self.diagnostics = diagnostics.map(|diagnostic| calls.label(&requests, diagnostic));
    }
}

impl<'m> ParameterCalls<'m> {
    fn label(&self, requests: &[ParameterCallRequest], diagnostic: Diagnostic) -> Diagnostic {
        let Some(request) = requests
            .iter()
            .find(|request| request.explains(&diagnostic))
        else {
            return diagnostic;
        };
        let Some(call) = self.find(request.callee, request.position) else {
            return diagnostic;
        };
        TypeProblems::parameter_call(diagnostic, self.bindings.get(request.callee).name(), &call)
    }

    fn find(&self, function: BindingId, position: usize) -> Option<ParameterCall> {
        let (module, lambda) = self.definition(function)?;
        let param = lambda.params.get(position)?;
        let site = Self::first_call(lambda.body.expressions(), param.binding)?;
        let parameter: Rc<str> = self.bindings.get(param.binding).name().into();
        Some(ParameterCall {
            span: site.span,
            arguments: site.arguments,
            shadows_function: self.defines_function(module, &parameter),
            parameter,
        })
    }

    fn definition(&self, function: BindingId) -> Option<(&'m CoreModule, &'m Lambda)> {
        self.modules.iter().find_map(|module| {
            Self::functions(module)
                .find(|(binding, _)| *binding == function)
                .map(|(_, lambda)| (module, lambda))
        })
    }

    fn defines_function(&self, module: &CoreModule, name: &str) -> bool {
        Self::functions(module).any(|(function, _)| self.bindings.get(function).name() == name)
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
