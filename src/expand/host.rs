use super::Expander;
use super::problems::ExpandProblems;
use crate::core::{BindingId, BindingKind, ExprKind, Leaf, Namespace};
use crate::diagnostic::Diagnostic;
use crate::interpret::{BuiltinCall, CompileTimeHost, Value};
use crate::reader::Symbol;
use crate::scope::{Phase, Resolution};
use crate::source::Span;

impl CompileTimeHost for Expander<'_> {
    fn global_value(&mut self, binding: BindingId, span: Span) -> Result<Value, Diagnostic> {
        let name = self.bindings.get(binding).name().to_string();
        match self.kind(binding) {
            BindingKind::CodeBuiltin(builtin) => {
                Ok(BuiltinCall::constant(builtin).unwrap_or(Value::Builtin(builtin)))
            }
            BindingKind::Global(_) => self.compile_time_global(binding, &name, span),
            BindingKind::Macro => Err(ExpandProblems::macro_as_value(&name, span)),
            _ => Err(ExpandProblems::not_at_compile_time(&name, span)),
        }
    }

    fn same_binding(&self, left: &Symbol, right: &Symbol) -> bool {
        let left_resolution = self.table.resolve(left, Namespace::Value, Phase::Runtime);
        let right_resolution = self.table.resolve(right, Namespace::Value, Phase::Runtime);
        match (left_resolution, right_resolution) {
            (Resolution::Bound(left_binding), Resolution::Bound(right_binding)) => {
                left_binding == right_binding
            }
            (Resolution::Unbound, Resolution::Unbound) => left.name() == right.name(),
            _ => false,
        }
    }
}

impl Expander<'_> {
    fn compile_time_global(
        &mut self,
        binding: BindingId,
        name: &str,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        let lowered = self.lowered_global(binding, span)?;
        if let ExprKind::Leaf(Leaf::Rust(template)) = lowered.kind()
            && template.twin.is_none()
        {
            return Err(ExpandProblems::no_compile_time_version(
                name,
                span,
                lowered.span(),
            ));
        }
        self.evaluate_global(binding, span)
    }
}
