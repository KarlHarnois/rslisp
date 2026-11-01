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
        self.learn_from(&first);
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

    fn learn_from(&mut self, first: &CheckedProgram) {
        self.blocked = Self::inferred_copy_changed(&first.program);
        self.inferred_copies = first.inferred_copies.clone();
        self.pins.extend(Self::pins(&first.instantiations));
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
        match OwnershipChecker::check(typed) {
            Ok(inferred_copies) if diagnostics.is_empty() => return Ok(inferred_copies),
            Ok(_) => {}
            Err(ownership_errors) => diagnostics.extend(ownership_errors),
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

#[cfg(test)]
mod tests {
    use super::{CheckRuns, CheckedProgram};
    use crate::types::TypedSource;

    struct RunsFixture {
        source: TypedSource,
    }

    impl RunsFixture {
        const WRITTEN_TYPES: &str = "
            (defn greet ((name str)) (println \"hello \" name))
            (defn main () (greet \"Ana\"))";

        const UNUSED_OPEN_LITERAL: &str = "
            (defn triple (n) (* n 3))
            (defn main () (println \"Leaf\"))";

        const GENERIC_USED_AT_ONE_TYPE: &str = "
            (defn larger (left right) (if (> left right) left right))
            (defn main () (println (larger (the u16 2024) (the u16 2019))))";

        const STRUCT_CHANGED_IN_PLACE: &str = "
            (def add-one! (rust \"std::ops::AddAssign::add_assign({0}, 1)\" (fn ((mut i64)) unit)))
            (defstruct tally :total i64)
            (defn bump-tally! (t) (add-one! (field t total)))
            (defn main ()
              (let t (->tally 5))
              (bump-tally! t)
              (println (:total t)))";

        const COPIED_GENERIC_BESIDE_A_PIN: &str = "
            (defn twice (x) (pair x x))
            (defn port () 8080)
            (defn listen ((p u16)) p)
            (defn main ()
              (let-pair (left right) (twice 7) (println left right))
              (let-pair (left right) (twice 2.5) (println left right))
              (println (listen (port))))";

        fn new(text: &str) -> Self {
            Self {
                source: TypedSource::expand(text),
            }
        }

        fn runs(&self) -> CheckRuns<'_> {
            CheckRuns::new(self.source.program())
        }

        fn open_run(&self, runs: &CheckRuns) -> CheckedProgram {
            runs.open_run().expect("the source checks")
        }

        fn learned_runs(&self) -> (CheckRuns<'_>, CheckedProgram) {
            let mut runs = self.runs();
            let first = self.open_run(&runs);
            runs.learn_from(&first);
            (runs, first)
        }

        fn settles_after_the_open_run(&self) -> bool {
            let (runs, first) = self.learned_runs();
            runs.is_settled(&first)
        }

        fn learned_pin_count(&self) -> usize {
            self.learned_runs().0.pins.len()
        }

        fn learned_in_place_change_count(&self) -> usize {
            self.learned_runs().0.blocked.len()
        }

        fn learns_inferred_copies(&self) -> bool {
            !self.learned_runs().0.inferred_copies.is_empty()
        }

        fn open_signatures(&self) -> String {
            self.open_run(&self.runs()).program.entry_signatures()
        }

        fn checked_signatures(&self) -> String {
            self.runs()
                .typed()
                .expect("the source checks")
                .entry_signatures()
        }
    }

    #[test]
    fn a_program_with_written_types_settles_after_the_open_run() {
        assert!(RunsFixture::new(RunsFixture::WRITTEN_TYPES).settles_after_the_open_run());
    }

    #[test]
    fn an_unused_open_literal_needs_a_defaulting_run() {
        assert!(!RunsFixture::new(RunsFixture::UNUSED_OPEN_LITERAL).settles_after_the_open_run());
    }

    #[test]
    fn the_defaulting_run_changes_the_open_literal_signature() {
        let fixture = RunsFixture::new(RunsFixture::UNUSED_OPEN_LITERAL);
        let defaulted = "(type triple (fn (i64) i64))";
        assert!(!fixture.open_signatures().contains(defaulted));
        assert!(fixture.checked_signatures().contains(defaulted));
    }

    #[test]
    fn a_generic_used_at_one_type_needs_a_pin_run() {
        let fixture = RunsFixture::new(RunsFixture::GENERIC_USED_AT_ONE_TYPE);
        assert_eq!(fixture.learned_pin_count(), 1);
    }

    #[test]
    fn the_pin_run_changes_the_generic_signature() {
        let fixture = RunsFixture::new(RunsFixture::GENERIC_USED_AT_ONE_TYPE);
        let pinned = "(type larger (fn (u16 u16) u16))";
        assert!(!fixture.open_signatures().contains(pinned));
        assert!(fixture.checked_signatures().contains(pinned));
    }

    #[test]
    fn the_open_run_finds_the_in_place_changes_that_the_next_run_reads() {
        let fixture = RunsFixture::new(RunsFixture::STRUCT_CHANGED_IN_PLACE);
        assert_eq!(fixture.learned_in_place_change_count(), 1);
    }

    #[test]
    fn the_open_run_infers_the_copies_that_the_next_run_reads() {
        let fixture = RunsFixture::new(RunsFixture::COPIED_GENERIC_BESIDE_A_PIN);
        assert!(fixture.learns_inferred_copies());
    }
}
