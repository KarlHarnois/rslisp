use super::display::TypeDisplay;
use super::problems::TypeProblems;
use super::ty::Type;
use super::typed::{
    TypedBodyItem, TypedControl, TypedDefinition, TypedExpr, TypedExprKind, TypedLambda,
    TypedProgram,
};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::source::Span;

pub struct MainResult<'a> {
    program: &'a TypedProgram,
}

impl<'a> MainResult<'a> {
    pub fn check(program: TypedProgram) -> Result<TypedProgram, Diagnostics> {
        let problem = MainResult { program: &program }.problem();
        match problem {
            Some(problem) => Err(Self::with_warnings(problem, program.warnings)),
            None => Ok(program),
        }
    }

    fn with_warnings(problem: Diagnostic, warnings: Diagnostics) -> Diagnostics {
        let mut diagnostics = Diagnostics::from(problem);
        diagnostics.extend(warnings);
        diagnostics
    }

    fn problem(&self) -> Option<Diagnostic> {
        self.program
            .entry_module()
            .definitions
            .iter()
            .filter(|definition| self.is_main(definition))
            .find_map(|definition| self.definition_problem(definition))
    }

    fn is_main(&self, definition: &TypedDefinition) -> bool {
        self.program.entry_function == Some(definition.binding)
    }

    fn definition_problem(&self, definition: &TypedDefinition) -> Option<Diagnostic> {
        let TypedExprKind::Fn(lambda) = &definition.value.kind else {
            return None;
        };
        let Type::Function(_, _, result) = &definition.value.ty else {
            return None;
        };
        if self.is_admissible(definition, result) {
            return None;
        }
        Some(TypeProblems::main_returns_value(
            &self.display().show(result),
            Self::returned_span(definition, lambda),
        ))
    }

    fn is_admissible(&self, definition: &TypedDefinition, result: &Type) -> bool {
        *result == Type::unit() || definition.result_error(&self.program.enums).is_some()
    }

    fn display(&self) -> TypeDisplay<'a> {
        TypeDisplay::new(&self.program.bindings, &self.program.generics)
            .with_projections(self.program.traits.projections())
    }

    fn returned_span(definition: &TypedDefinition, lambda: &TypedLambda) -> Span {
        match lambda.body.items.last() {
            Some(TypedBodyItem::Expr(expr)) => Self::returned_expression(expr).span,
            _ => definition.span,
        }
    }

    fn returned_expression(expr: &TypedExpr) -> &TypedExpr {
        match &expr.kind {
            TypedExprKind::Control(TypedControl::Block(block)) => match block.items.last() {
                Some(TypedBodyItem::Expr(last)) => Self::returned_expression(last),
                _ => expr,
            },
            _ => expr,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MainResult;
    use crate::diagnostic::DiagnosticCode;
    use crate::types::TypedSource;

    struct EntryFixture;

    impl EntryFixture {
        const TWO_VALUED_FUNCTIONS: &str = "
            (defn main () 1)
            (defn start () 2)";

        fn reports_a_returned_value(entry: &str) -> bool {
            let mut source = TypedSource::expand(Self::TWO_VALUED_FUNCTIONS);
            source.enter_at(entry);
            MainResult::check(source.typed()).is_err_and(|diagnostics| {
                diagnostics
                    .in_source_order()
                    .iter()
                    .any(|diagnostic| *diagnostic.code() == DiagnosticCode::MAIN_RETURNS_VALUE)
            })
        }
    }

    #[test]
    fn the_entry_binding_must_return_unit_whatever_its_name() {
        assert!(EntryFixture::reports_a_returned_value("start"));
    }

    #[test]
    fn a_main_that_is_not_the_entry_may_return_a_value() {
        assert!(!EntryFixture::reports_a_returned_value("absent"));
    }
}
