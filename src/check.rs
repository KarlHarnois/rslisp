use std::collections::{HashMap, HashSet};

use crate::core::{BindingId, StructDerives};
use crate::diagnostic::Diagnostics;
use crate::expand::Program;
use crate::ownership::{InPlaceChanges, OwnershipChecker};
use crate::types::{
    DefinitionGraph, InferredCopies, InstantiationReport, LiteralDefaults, MainResult, Pinning,
    Type, TypeChecker, TypedProgram,
};

pub struct ProgramChecker;

impl ProgramChecker {
    pub fn check(program: &Program) -> Result<TypedProgram, Diagnostics> {
        CheckRuns::new(program).typed().and_then(MainResult::check)
    }
}

struct CheckRuns<'a> {
    program: &'a Program,
    graph: DefinitionGraph<'a>,
    blocked: HashSet<BindingId>,
    inferred_copies: InferredCopies,
    pins: HashMap<BindingId, Type>,
}

struct CheckedProgram {
    program: TypedProgram,
    inferred_copies: InferredCopies,
    instantiations: InstantiationReport,
}

impl<'a> CheckRuns<'a> {
    fn new(program: &'a Program) -> Self {
        Self {
            program,
            graph: DefinitionGraph::of(program),
            blocked: HashSet::new(),
            inferred_copies: InferredCopies::default(),
            pins: HashMap::new(),
        }
    }

    fn typed(mut self) -> Result<TypedProgram, Diagnostics> {
        let first = self.open_run()?;
        self.finish(first)
    }

    fn finish(&mut self, first: CheckedProgram) -> Result<TypedProgram, Diagnostics> {
        self.blocked = Self::inferred_copy_changed(&first.program);
        self.inferred_copies = first.inferred_copies.clone();
        self.pins.extend(Self::pins(&first.instantiations));
        if self.is_settled(&first) {
            return Ok(first.program);
        }
        let latest = match self.pin_open_literals()? {
            Some(open) if open.program.literal_generics.is_empty() => return Ok(open.program),
            Some(open) => open,
            None => first,
        };
        self.pin_until_stable(LiteralDefaults::of(&latest.instantiations, &latest.program))
    }

    fn is_settled(&self, first: &CheckedProgram) -> bool {
        self.blocked.is_empty()
            && self.inferred_copies.is_empty()
            && self.pins.is_empty()
            && first.program.literal_generics.is_empty()
    }

    fn pin_open_literals(&mut self) -> Result<Option<CheckedProgram>, Diagnostics> {
        if self.pins.is_empty() {
            return Ok(None);
        }
        loop {
            let open = self.open_run()?;
            let next = self.new_pins(&open.instantiations);
            if next.is_empty() {
                return Ok(Some(open));
            }
            self.pins.extend(next);
        }
    }

    fn pin_until_stable(
        &mut self,
        literal_defaults: LiteralDefaults,
    ) -> Result<TypedProgram, Diagnostics> {
        loop {
            let checked = self.defaulting_run(literal_defaults.clone())?;
            let next = self.new_pins(&checked.instantiations);
            if next.is_empty() {
                return Ok(checked.program);
            }
            self.pins.extend(next);
        }
    }

    fn pins(report: &InstantiationReport) -> HashMap<BindingId, Type> {
        Pinning::of(report).pins().iter().cloned().collect()
    }

    fn new_pins(&self, report: &InstantiationReport) -> HashMap<BindingId, Type> {
        Self::pins(report)
            .into_iter()
            .filter(|(binding, _)| !self.pins.contains_key(binding))
            .collect()
    }

    fn open_run(&self) -> Result<CheckedProgram, Diagnostics> {
        self.attempt(LiteralDefaults::default(), false)
    }

    fn defaulting_run(
        &self,
        literal_defaults: LiteralDefaults,
    ) -> Result<CheckedProgram, Diagnostics> {
        self.attempt(literal_defaults, true)
    }

    fn attempt(
        &self,
        literal_defaults: LiteralDefaults,
        default_literal_parameters: bool,
    ) -> Result<CheckedProgram, Diagnostics> {
        match self.typed_and_owned(literal_defaults, default_literal_parameters) {
            Ok(mut checked) => {
                checked
                    .program
                    .warnings
                    .extend(self.program.warnings.clone());
                Ok(checked)
            }
            Err(mut diagnostics) => {
                diagnostics.extend(self.program.warnings.clone());
                Err(diagnostics)
            }
        }
    }

    fn typed_and_owned(
        &self,
        literal_defaults: LiteralDefaults,
        default_literal_parameters: bool,
    ) -> Result<CheckedProgram, Diagnostics> {
        let mut typed = TypeChecker::check(
            self.program,
            &self.graph,
            &self.blocked,
            &self.inferred_copies,
            &self.pins,
            literal_defaults,
            default_literal_parameters,
        )?;
        let instantiations = InstantiationReport::of(self.program, &self.graph, &typed);
        let inferred_copies = Self::check_ownership(&mut typed)?;
        Ok(CheckedProgram {
            program: typed,
            inferred_copies,
            instantiations,
        })
    }

    fn check_ownership(typed: &mut TypedProgram) -> Result<InferredCopies, Diagnostics> {
        let mut diagnostics = std::mem::take(&mut typed.unnameable_types);
        let checked = OwnershipChecker::check(typed);
        if diagnostics.is_empty() {
            return checked;
        }
        if let Err(ownership_errors) = checked {
            diagnostics.extend(ownership_errors);
        }
        diagnostics.extend(std::mem::take(&mut typed.warnings));
        Err(diagnostics)
    }

    fn inferred_copy_changed(typed: &TypedProgram) -> HashSet<BindingId> {
        InPlaceChanges::of(typed)
            .into_iter()
            .filter(|binding| {
                typed.structs.get(*binding).is_some_and(|definition| {
                    definition.written_derive.is_none()
                        && definition.derives.contains(&StructDerives::COPY)
                }) || typed.enums.get(*binding).is_some_and(|definition| {
                    definition.written_derive.is_none()
                        && definition.derives.contains(&StructDerives::COPY)
                })
            })
            .collect()
    }
}
