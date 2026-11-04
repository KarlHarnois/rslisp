use std::rc::Rc;

use crate::core::Namespace;
use crate::source::{SourceId, Span};

#[derive(Default)]
pub(super) struct RecordedInputs {
    inputs: Vec<Span>,
    bound: Vec<BoundName>,
    loaded: Vec<LoadedModule>,
    registering: Option<Span>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundName {
    pub name: Rc<str>,
    pub namespace: Namespace,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadedModule {
    pub source: SourceId,
    pub input: Span,
}

impl BoundName {
    pub fn names_the_same(&self, other: &BoundName) -> bool {
        self.name == other.name && self.namespace == other.namespace
    }
}

impl RecordedInputs {
    pub(super) fn new(inputs: Vec<Span>) -> Self {
        Self {
            inputs,
            ..Self::default()
        }
    }

    pub(super) fn note_bound(&mut self, span: Span, name: impl FnOnce() -> Option<BoundName>) {
        if self.surrounds(span) {
            self.bound.extend(name());
        }
    }

    pub(super) fn enter(&mut self, written: Span) -> Option<Span> {
        let outer = self.registering;
        if self.inputs.contains(&written) {
            self.registering = Some(written);
        }
        outer
    }

    pub(super) fn leave(&mut self, outer: Option<Span>) {
        self.registering = outer;
    }

    pub(super) fn note_loaded(&mut self, source: SourceId) {
        if let Some(input) = self.registering {
            self.loaded.push(LoadedModule { source, input });
        }
    }

    pub(super) fn bound_by(&self, span: Span) -> Vec<BoundName> {
        self.bound
            .iter()
            .filter(|name| span.contains(name.span))
            .cloned()
            .collect()
    }

    pub(super) fn loaded(&self) -> &[LoadedModule] {
        &self.loaded
    }

    fn surrounds(&self, span: Span) -> bool {
        self.inputs.iter().any(|input| input.contains(span))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RecordFixture;

    impl RecordFixture {
        const INPUT: usize = 0;
        const OUTSIDE: usize = 20;

        fn span(start: usize) -> Span {
            Span::new(SourceId::new(0), start..start + 1)
        }

        fn name(span: Span) -> Option<BoundName> {
            Some(BoundName {
                name: "bound".into(),
                namespace: Namespace::Value,
                span,
            })
        }

        fn recording() -> RecordedInputs {
            RecordedInputs::new(vec![Self::span(Self::INPUT)])
        }
    }

    #[test]
    fn a_name_bound_outside_every_input_is_not_recorded() {
        let mut recorded = RecordFixture::recording();
        let outside = RecordFixture::span(RecordFixture::OUTSIDE);
        recorded.note_bound(outside, || RecordFixture::name(outside));
        let covering = Span::new(SourceId::new(0), 0..RecordFixture::OUTSIDE + 1);
        assert!(recorded.bound_by(covering).is_empty());
    }

    #[test]
    fn a_module_loaded_while_an_input_registers_belongs_to_the_input() {
        let mut recorded = RecordFixture::recording();
        let input = RecordFixture::span(RecordFixture::INPUT);
        let outer = recorded.enter(input);
        let nested = recorded.enter(RecordFixture::span(RecordFixture::OUTSIDE));
        recorded.note_loaded(SourceId::new(1));
        recorded.leave(nested);
        recorded.leave(outer);
        recorded.note_loaded(SourceId::new(2));
        assert_eq!(
            recorded.loaded(),
            [LoadedModule {
                source: SourceId::new(1),
                input
            }]
        );
    }
}
