use super::Expander;
use super::problems::ExpandProblems;
use crate::core::{BindingId, BindingKind, Namespace};
use crate::diagnostic::Diagnostic;
use crate::interpret::{CompileTimeHost, Value};
use crate::reader::Symbol;
use crate::scope::{Phase, Resolution};
use crate::source::Span;

impl CompileTimeHost for Expander<'_> {
    fn global_value(&mut self, binding: BindingId, span: Span) -> Result<Value, Diagnostic> {
        let name = self.bindings.get(binding).name().to_string();
        match self.kind(binding) {
            BindingKind::CodeBuiltin(builtin) => Ok(Value::Builtin(builtin)),
            BindingKind::Global(_) => self.evaluate_global(binding, span),
            _ => Err(ExpandProblems::not_at_compile_time(&name, span)),
        }
    }

    fn same_binding(&self, left: &Symbol, right: &Symbol) -> bool {
        let pairs: Vec<(Resolution, Resolution)> = [Namespace::Value, Namespace::Type]
            .into_iter()
            .map(|namespace| {
                (
                    self.table.resolve(left, namespace, Phase::Runtime),
                    self.table.resolve(right, namespace, Phase::Runtime),
                )
            })
            .collect();
        let bound_together = pairs.iter().any(|pair| {
            matches!(pair, (Resolution::Bound(left_binding), Resolution::Bound(right_binding))
                if left_binding == right_binding)
        });
        let unbound_everywhere = pairs
            .iter()
            .all(|pair| matches!(pair, (Resolution::Unbound, Resolution::Unbound)));
        bound_together || (unbound_everywhere && left.name() == right.name())
    }
}
