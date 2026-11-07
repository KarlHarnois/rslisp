use rustc_hash::FxHashMap;

use super::{BindingArena, BindingId, BindingKind, Control, Expr, ExprKind, Leaf, Pattern};

#[derive(Default)]
pub struct References {
    globals: Vec<BindingId>,
    uses_compile_time_code: bool,
    arm_outers: FxHashMap<BindingId, BindingId>,
}

impl References {
    pub fn of(value: &Expr, bindings: &BindingArena) -> Self {
        let mut references = Self::default();
        references.collect(value, bindings);
        references
    }

    pub fn globals(&self) -> &[BindingId] {
        &self.globals
    }

    pub fn into_globals(self) -> Vec<BindingId> {
        self.globals
    }

    pub fn uses_compile_time_code(&self) -> bool {
        self.uses_compile_time_code
    }

    fn collect(&mut self, expr: &Expr, bindings: &BindingArena) {
        match expr.kind() {
            ExprKind::Leaf(Leaf::Var(binding)) => self.note_read(*binding, bindings),
            ExprKind::Leaf(Leaf::Quote(_)) => self.uses_compile_time_code = true,
            ExprKind::Control(Control::Match(_, arms)) => {
                for arm in arms {
                    self.collect_arm_outers(&arm.pattern);
                }
            }
            _ => {}
        }
        for child in expr.children() {
            self.collect(child, bindings);
        }
    }

    fn collect_arm_outers(&mut self, pattern: &Pattern) {
        self.arm_outers.extend(pattern.arm_outer());
        for nested in pattern.nested() {
            self.collect_arm_outers(nested);
        }
    }

    fn note_read(&mut self, binding: BindingId, bindings: &BindingArena) {
        self.note_reference(binding, bindings);
        if let Some(outer) = self.arm_outers.get(&binding).copied() {
            self.note_reference(outer, bindings);
        }
    }

    fn note_reference(&mut self, binding: BindingId, bindings: &BindingArena) {
        match bindings.get(binding).kind() {
            BindingKind::Global(_) => self.globals.push(binding),
            BindingKind::CodeBuiltin(_) => self.uses_compile_time_code = true,
            _ => {}
        }
    }
}
