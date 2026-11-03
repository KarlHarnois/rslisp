use rustc_hash::FxHashSet;

use super::ExpressionEmitter;
use crate::core::BindingId;
use crate::emit::fragment::{Code, Precedence};
use crate::ownership::LocalFunctions;
use crate::types::{Type, TypedBinder, TypedExpr, TypedExprKind, TypedLambda};

struct LendingSignature<'t> {
    parameters: &'t [Type],
    result: &'t Type,
    shared: FxHashSet<BindingId>,
}

impl ExpressionEmitter<'_> {
    const LOCAL_FUNCTION_LIFETIME: &'static str = "'a";

    pub(super) fn local_function(
        &mut self,
        binder: &TypedBinder,
        value: &TypedExpr,
    ) -> Option<Code> {
        let (TypedExprKind::Fn(lambda), Type::Function(_, parameters, result)) =
            (&value.kind, &value.ty)
        else {
            return None;
        };
        if !LocalFunctions::new(self.program).emits_as_function(binder, value) {
            return None;
        }
        let signature = LendingSignature {
            parameters,
            result,
            shared: Self::shared_parameters(value),
        };
        Some(
            self.lending_local_function(binder, lambda, signature)
                .spanned(value.span),
        )
    }

    fn shared_parameters(closure: &TypedExpr) -> FxHashSet<BindingId> {
        LocalFunctions::lent_parameters(closure)
            .into_iter()
            .flatten()
            .map(|param| param.binding)
            .collect()
    }

    fn lending_local_function(
        &mut self,
        binder: &TypedBinder,
        lambda: &TypedLambda,
        signature: LendingSignature,
    ) -> Code {
        let lifetime = self
            .program
            .function_type_needs_shared_lifetime(signature.parameters, signature.result)
            .then_some(Self::LOCAL_FUNCTION_LIFETIME);
        let shared = lifetime.map_or_else(FxHashSet::default, |_| signature.shared);
        let (params, body) = self.in_local_function_scope(lifetime, shared, |emitter| {
            emitter.lambda_parts(
                lambda,
                |emitter| {
                    lambda
                        .params
                        .iter()
                        .map(|param| emitter.bind_value_closure_parameter(param))
                        .collect()
                },
                |emitter| emitter.braced_lambda_body(lambda),
            )
        });
        let generics = lifetime.map_or_else(String::new, |lifetime| format!("<{lifetime}>"));
        let result = self
            .types()
            .local_function_result(signature.result, lifetime);
        let name = self.locals.bind(
            binder.binding,
            self.program.bindings.get(binder.binding).name(),
        );
        let head = format!("fn {name}{generics}({}) -> {result} ", params.join(", "));
        Code::joined(vec![Code::atom(head), body], Precedence::Block)
    }

    fn in_local_function_scope<T>(
        &mut self,
        lifetime: Option<&str>,
        shared: FxHashSet<BindingId>,
        emit: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let enclosing =
            std::mem::replace(&mut self.enclosing_lifetime, lifetime.map(str::to_string));
        let outer_shared = std::mem::replace(&mut self.result_parameters, shared);
        let emitted = emit(self);
        self.enclosing_lifetime = enclosing;
        self.result_parameters = outer_shared;
        emitted
    }
}
