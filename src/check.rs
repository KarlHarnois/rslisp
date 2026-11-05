use rustc_hash::{FxHashMap, FxHashSet};

use crate::core::{BindingId, StructDerives};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::expand::Program;
use crate::ownership::{InPlaceChanges, OwnershipChecker};
use crate::types::{
    DefinitionGraph, FormResult, FormResults, InferredCopies, InstantiationReport, LiteralDefaults,
    MainResult, Pinning, Type, TypeChecker, TypedProgram,
};

pub struct ProgramChecker;

impl ProgramChecker {
    #[inline(never)]
    pub fn check(program: &Program) -> Result<TypedProgram, Diagnostics> {
        CheckRuns::new(program).typed().and_then(MainResult::check)
    }

    pub fn form_results(
        program: &Program,
        printer: BindingId,
    ) -> Result<Vec<FormResult>, Diagnostics> {
        let typed = TypeChecker::check(
            program,
            &DefinitionGraph::of(program),
            &FxHashSet::default(),
            &InferredCopies::default(),
            &FxHashMap::default(),
            LiteralDefaults::default(),
            false,
        )
        .map_err(Self::errors_only)?;
        Ok(FormResults::of(&typed, printer))
    }

    fn errors_only(mut diagnostics: Diagnostics) -> Diagnostics {
        diagnostics.retain(Diagnostic::is_error);
        diagnostics
    }
}

struct CheckRuns<'a> {
    program: &'a Program,
    graph: DefinitionGraph<'a>,
    blocked: FxHashSet<BindingId>,
    inferred_copies: InferredCopies,
    pinning: Pinning,
    inferred_expressions: usize,
}

#[derive(Clone, Default)]
struct DefaultingRule {
    literal_defaults: LiteralDefaults,
    default_literal_parameters: bool,
}

struct CheckedProgram {
    program: TypedProgram,
    inferred_copies: InferredCopies,
    instantiations: InstantiationReport,
    defaulting: DefaultingRule,
}

impl<'a> CheckRuns<'a> {
    fn new(program: &'a Program) -> Self {
        Self {
            program,
            graph: DefinitionGraph::of(program),
            blocked: FxHashSet::default(),
            inferred_copies: InferredCopies::default(),
            pinning: Pinning::default(),
            inferred_expressions: 0,
        }
    }

    fn typed(mut self) -> Result<TypedProgram, Diagnostics> {
        let first = self.open_run()?;
        let last = self.finish(first)?;
        let mut typed = self.with_unpinned_type_texts(last);
        typed.checking.inferred_expressions = self.inferred_expressions;
        Ok(typed)
    }

    fn with_unpinned_type_texts(&mut self, last: CheckedProgram) -> TypedProgram {
        let mut typed = last.program;
        if !self.needs_unpinned_type_texts(&typed) {
            return typed;
        }
        if let Ok(unpinned) = self.check_types(self.pinning.literal_pins(), last.defaulting) {
            self.inferred_expressions += unpinned.checking.inferred_expressions;
            typed
                .type_texts
                .rewrite(&mut typed.modules, &unpinned.type_texts);
        }
        typed
    }

    fn needs_unpinned_type_texts(&self, typed: &TypedProgram) -> bool {
        !typed.type_texts.is_empty() && self.pinning.pins_type_parameters()
    }

    fn finish(&mut self, first: CheckedProgram) -> Result<CheckedProgram, Diagnostics> {
        self.learn_from(&first);
        if self.is_settled(&first) {
            return Ok(first);
        }
        let latest = match self.pin_open_literals()? {
            Some(open) if open.program.literal_generics.is_empty() => return Ok(open),
            Some(open) => open,
            None => first,
        };
        self.pin_until_stable(LiteralDefaults::of(&latest.instantiations, &latest.program))
    }

    fn learn_from(&mut self, first: &CheckedProgram) {
        self.blocked = Self::inferred_copy_changed(&first.program);
        self.inferred_copies = first.inferred_copies.clone();
        self.pinning.extend(Pinning::of(&first.instantiations));
    }

    fn is_settled(&self, first: &CheckedProgram) -> bool {
        self.blocked.is_empty()
            && self.inferred_copies.is_empty()
            && self.pinning.is_empty()
            && first.program.literal_generics.is_empty()
    }

    fn pin_open_literals(&mut self) -> Result<Option<CheckedProgram>, Diagnostics> {
        if self.pinning.is_empty() {
            return Ok(None);
        }
        loop {
            let open = self.open_run()?;
            let next = self.new_pins(&open.instantiations);
            if next.is_empty() {
                return Ok(Some(open));
            }
            self.pinning.extend(next);
        }
    }

    fn pin_until_stable(
        &mut self,
        literal_defaults: LiteralDefaults,
    ) -> Result<CheckedProgram, Diagnostics> {
        loop {
            let checked = self.defaulting_run(literal_defaults.clone())?;
            let next = self.new_pins(&checked.instantiations);
            if next.is_empty() {
                return Ok(checked);
            }
            self.pinning.extend(next);
        }
    }

    fn new_pins(&self, report: &InstantiationReport) -> Pinning {
        Pinning::of(report).unknown_to(&self.pinning)
    }

    fn open_run(&mut self) -> Result<CheckedProgram, Diagnostics> {
        self.attempt(DefaultingRule::default())
    }

    fn defaulting_run(
        &mut self,
        literal_defaults: LiteralDefaults,
    ) -> Result<CheckedProgram, Diagnostics> {
        self.attempt(DefaultingRule {
            literal_defaults,
            default_literal_parameters: true,
        })
    }

    fn attempt(&mut self, defaulting: DefaultingRule) -> Result<CheckedProgram, Diagnostics> {
        match self.typed_and_owned(defaulting) {
            Ok(mut checked) => {
                self.inferred_expressions += checked.program.checking.inferred_expressions;
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

    fn typed_and_owned(&self, defaulting: DefaultingRule) -> Result<CheckedProgram, Diagnostics> {
        let mut typed = self.check_types(self.pinning.pins(), defaulting.clone())?;
        let instantiations = InstantiationReport::of(self.program, &self.graph, &typed);
        let inferred_copies = Self::check_ownership(&mut typed)?;
        Ok(CheckedProgram {
            program: typed,
            inferred_copies,
            instantiations,
            defaulting,
        })
    }

    fn check_types(
        &self,
        pins: &FxHashMap<BindingId, Type>,
        defaulting: DefaultingRule,
    ) -> Result<TypedProgram, Diagnostics> {
        TypeChecker::check(
            self.program,
            &self.graph,
            &self.blocked,
            &self.inferred_copies,
            pins,
            defaulting.literal_defaults,
            defaulting.default_literal_parameters,
        )
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

    fn inferred_copy_changed(typed: &TypedProgram) -> FxHashSet<BindingId> {
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
    use crate::types::{LiteralDefaults, TypedSource};

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

        const TYPE_TEXT_OF_A_PINNED_GENERIC: &str = "
            (defn wrap (x) (println (infer x)) (some x))
            (defn main () (prn (wrap 1)))";

        const TYPE_TEXT_OF_A_PINNED_LITERAL: &str = "
            (defn bump (x) (println (infer x)) (+ x 1))
            (defn main () (prn (bump (the u16 2))))";

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

        fn open_run(&self, runs: &mut CheckRuns) -> CheckedProgram {
            runs.open_run().expect("the source checks")
        }

        fn learned_runs(&self) -> (CheckRuns<'_>, CheckedProgram) {
            let mut runs = self.runs();
            let first = self.open_run(&mut runs);
            runs.learn_from(&first);
            (runs, first)
        }

        fn settles_after_the_open_run(&self) -> bool {
            let (runs, first) = self.learned_runs();
            runs.is_settled(&first)
        }

        fn learned_pin_count(&self) -> usize {
            self.learned_runs().0.pinning.pins().len()
        }

        fn needs_unpinned_type_texts(&self) -> bool {
            let mut runs = self.runs();
            let first = self.open_run(&mut runs);
            let last = runs.finish(first).expect("the source checks");
            runs.needs_unpinned_type_texts(&last.program)
        }

        fn learned_in_place_change_count(&self) -> usize {
            self.learned_runs().0.blocked.len()
        }

        fn learns_inferred_copies(&self) -> bool {
            !self.learned_runs().0.inferred_copies.is_empty()
        }

        fn open_signatures(&self) -> String {
            self.open_run(&mut self.runs()).program.entry_signatures()
        }

        fn checked_signatures(&self) -> String {
            self.runs()
                .typed()
                .expect("the source checks")
                .entry_signatures()
        }

        fn checked_inferred_expressions(&self) -> usize {
            self.runs()
                .typed()
                .expect("the source checks")
                .checking
                .inferred_expressions
        }

        fn inferred_expressions_of_the_open_and_defaulting_runs(&self) -> [usize; 2] {
            let (mut runs, open) = self.learned_runs();
            let defaults = LiteralDefaults::of(&open.instantiations, &open.program);
            let defaulted = runs.defaulting_run(defaults).expect("the source checks");
            [open, defaulted].map(|run| run.program.checking.inferred_expressions)
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
    fn the_checked_program_counts_the_inferred_expressions_of_every_run() {
        let fixture = RunsFixture::new(RunsFixture::UNUSED_OPEN_LITERAL);
        let separate_runs: usize = fixture
            .inferred_expressions_of_the_open_and_defaulting_runs()
            .iter()
            .sum();
        assert_eq!(fixture.checked_inferred_expressions(), separate_runs);
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
    fn a_pinned_generic_without_a_type_text_needs_no_unpinned_run() {
        let fixture = RunsFixture::new(RunsFixture::GENERIC_USED_AT_ONE_TYPE);
        assert!(!fixture.needs_unpinned_type_texts());
    }

    #[test]
    fn a_type_text_in_a_pinned_generic_needs_an_unpinned_run() {
        let fixture = RunsFixture::new(RunsFixture::TYPE_TEXT_OF_A_PINNED_GENERIC);
        assert!(fixture.needs_unpinned_type_texts());
    }

    #[test]
    fn a_type_text_beside_only_literal_pins_needs_no_unpinned_run() {
        let fixture = RunsFixture::new(RunsFixture::TYPE_TEXT_OF_A_PINNED_LITERAL);
        assert!(!fixture.needs_unpinned_type_texts());
    }

    #[test]
    fn the_unpinned_run_keeps_the_pinned_signature() {
        let fixture = RunsFixture::new(RunsFixture::TYPE_TEXT_OF_A_PINNED_GENERIC);
        assert!(
            fixture
                .checked_signatures()
                .contains("(type wrap (fn (i64) (option i64)))")
        );
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
