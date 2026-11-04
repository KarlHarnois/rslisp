use super::display::TypeDisplay;
use super::problems::TypeProblems;
use super::ty::{Bound, Type};
use super::typed::{TypedBodyItem, TypedDefinition, TypedExprKind, TypedProgram};
use crate::core::BindingId;
use crate::diagnostic::Diagnostic;
use crate::source::Span;

#[derive(Clone, Debug)]
pub enum FormResult {
    Unit,
    Printable,
    Unprintable(Diagnostic),
}

pub struct FormResults<'a> {
    program: &'a TypedProgram,
    printer_bounds: Vec<Bound>,
}

impl<'a> FormResults<'a> {
    pub fn of(program: &'a TypedProgram, printer: BindingId) -> Vec<FormResult> {
        let results = Self {
            program,
            printer_bounds: Self::argument_bounds(Self::definition(program, printer)),
        };
        results.entry_results()
    }

    fn entry_results(&self) -> Vec<FormResult> {
        let Some(TypedExprKind::Fn(lambda)) = self
            .program
            .entry_function
            .and_then(|entry| Self::definition(self.program, entry))
            .map(|definition| &definition.value.kind)
        else {
            return Vec::new();
        };
        let forms = lambda
            .body
            .items
            .split_last()
            .map_or(&[][..], |(_generated_unit, forms)| forms);
        forms.iter().map(|item| self.result_of(item)).collect()
    }

    fn result_of(&self, item: &TypedBodyItem) -> FormResult {
        match item {
            TypedBodyItem::Expr(expr) if expr.ty != Type::unit() => {
                self.printing(&expr.ty, expr.span)
            }
            _ => FormResult::Unit,
        }
    }

    fn printing(&self, ty: &Type, span: Span) -> FormResult {
        let argument = Self::borrowed(ty);
        let missing = self.printer_bounds.iter().find(|bound| {
            !self
                .program
                .traits
                .satisfies(bound, argument, &|_, _| false)
        });
        match missing {
            Some(bound) => FormResult::Unprintable(TypeProblems::unprintable_value(
                &self.display().show(ty),
                self.program.bindings.get(bound.binding).name(),
                span,
            )),
            None => FormResult::Printable,
        }
    }

    fn borrowed(ty: &Type) -> &Type {
        match ty {
            Type::Ref(_, inner) | Type::Mut(_, inner) => inner,
            other => other,
        }
    }

    fn display(&self) -> TypeDisplay<'a> {
        TypeDisplay::new(&self.program.bindings, &self.program.generics)
            .with_projections(self.program.traits.projections())
    }

    fn definition(program: &TypedProgram, binding: BindingId) -> Option<&TypedDefinition> {
        program
            .modules
            .iter()
            .flat_map(|module| &module.definitions)
            .find(|definition| definition.binding == binding)
    }

    fn argument_bounds(printer: Option<&TypedDefinition>) -> Vec<Bound> {
        let Some(printer) = printer else {
            return Vec::new();
        };
        let Type::Function(_, parameters, _) = &printer.scheme.body else {
            return Vec::new();
        };
        let Some(Type::Generic(argument)) = parameters.first().map(Self::borrowed) else {
            return Vec::new();
        };
        printer
            .scheme
            .params
            .iter()
            .filter(|param| param.id == *argument)
            .flat_map(|param| param.bounds.iter().cloned())
            .collect()
    }
}
