use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::core::Namespace;
use crate::source::Span;

#[derive(Default)]
pub(super) struct EvaluatedForms {
    evaluated: FxHashSet<Span>,
    definitions: FxHashSet<Span>,
    left_out: FxHashSet<Span>,
    spliced: FxHashMap<Span, Vec<Span>>,
    bound: Vec<BoundName>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundName {
    pub name: Rc<str>,
    pub namespace: Namespace,
    pub span: Span,
}

impl BoundName {
    pub fn names_the_same(&self, other: &BoundName) -> bool {
        self.name == other.name && self.namespace == other.namespace
    }
}

impl EvaluatedForms {
    pub(super) fn new(spans: impl IntoIterator<Item = Span>) -> Self {
        Self {
            evaluated: spans.into_iter().collect(),
            definitions: FxHashSet::default(),
            left_out: FxHashSet::default(),
            spliced: FxHashMap::default(),
            bound: Vec::new(),
        }
    }

    pub(super) fn surrounds(&self, span: Span) -> bool {
        self.evaluated
            .iter()
            .any(|evaluated| evaluated.contains(span))
    }

    pub(super) fn note_bound(&mut self, span: Span, name: impl FnOnce() -> Option<BoundName>) {
        if self.surrounds(span) {
            self.bound.extend(name());
        }
    }

    pub(super) fn bound_by(&self, span: Span) -> Vec<BoundName> {
        self.bound
            .iter()
            .filter(|name| span.contains(name.span))
            .cloned()
            .collect()
    }

    pub(super) fn holds(&self, span: Span) -> bool {
        self.evaluated.contains(&span)
    }

    pub(super) fn note_definition(&mut self, span: Span) {
        if self.holds(span) {
            self.definitions.insert(span);
        }
    }

    pub(super) fn leave_out(&mut self, span: Span) {
        self.left_out.insert(span);
    }

    pub(super) fn splice(&mut self, span: Span, items: impl Iterator<Item = Span>) {
        if !self.holds(span) {
            return;
        }
        let items: Vec<Span> = items.filter(|item| *item != span).collect();
        self.evaluated.extend(items.iter().copied());
        self.spliced.insert(span, items);
    }

    pub(super) fn is_definition(&self, span: Span) -> bool {
        self.definitions.contains(&span) || self.splices_definition(span)
    }

    pub(super) fn misplaced_expressions(&self, span: Span) -> Vec<Span> {
        if !self.is_definition(span) {
            return Vec::new();
        }
        let mut misplaced = Vec::new();
        self.collect_left_out(span, &mut misplaced);
        misplaced
    }

    fn collect_left_out(&self, span: Span, misplaced: &mut Vec<Span>) {
        for item in self.spliced.get(&span).into_iter().flatten() {
            if self.left_out.contains(item) {
                misplaced.push(*item);
            }
            self.collect_left_out(*item, misplaced);
        }
    }

    fn splices_definition(&self, span: Span) -> bool {
        self.spliced
            .get(&span)
            .is_some_and(|items| items.iter().any(|item| self.is_definition(*item)))
    }
}

#[cfg(test)]
mod tests {
    use super::{BoundName, EvaluatedForms};
    use crate::core::Namespace;
    use crate::source::{SourceId, Span};

    struct SpanFixture;

    impl SpanFixture {
        const DO_FORM: usize = 0;
        const FIRST_ITEM: usize = 4;
        const SECOND_ITEM: usize = 8;
        const UNEVALUATED: usize = 20;

        fn span(start: usize) -> Span {
            Span::new(SourceId::new(0), start..start + 1)
        }

        fn spliced_do() -> EvaluatedForms {
            let mut forms = EvaluatedForms::new([Self::span(Self::DO_FORM)]);
            let items = [Self::FIRST_ITEM, Self::SECOND_ITEM].map(Self::span);
            forms.splice(Self::span(Self::DO_FORM), items.into_iter());
            forms
        }
    }

    #[test]
    fn an_evaluated_form_that_defines_is_a_definition() {
        let mut forms = EvaluatedForms::new([SpanFixture::span(SpanFixture::DO_FORM)]);
        forms.note_definition(SpanFixture::span(SpanFixture::DO_FORM));
        assert!(forms.is_definition(SpanFixture::span(SpanFixture::DO_FORM)));
    }

    #[test]
    fn a_definition_that_is_not_evaluated_is_not_recorded() {
        let mut forms = EvaluatedForms::new([SpanFixture::span(SpanFixture::DO_FORM)]);
        let unevaluated = SpanFixture::span(SpanFixture::UNEVALUATED);
        forms.note_definition(unevaluated);
        assert!(!forms.is_definition(unevaluated));
    }

    #[test]
    fn a_spliced_do_of_expressions_is_not_a_definition() {
        let forms = SpanFixture::spliced_do();
        assert!(!forms.is_definition(SpanFixture::span(SpanFixture::DO_FORM)));
        assert!(forms.holds(SpanFixture::span(SpanFixture::SECOND_ITEM)));
    }

    #[test]
    fn a_spliced_do_that_holds_a_definition_is_a_definition() {
        let mut forms = SpanFixture::spliced_do();
        forms.note_definition(SpanFixture::span(SpanFixture::SECOND_ITEM));
        assert!(forms.is_definition(SpanFixture::span(SpanFixture::DO_FORM)));
    }

    #[test]
    fn a_spliced_do_of_a_definition_and_an_expression_misplaces_the_expression() {
        let mut forms = SpanFixture::spliced_do();
        forms.note_definition(SpanFixture::span(SpanFixture::FIRST_ITEM));
        forms.leave_out(SpanFixture::span(SpanFixture::SECOND_ITEM));
        let misplaced = forms.misplaced_expressions(SpanFixture::span(SpanFixture::DO_FORM));
        assert_eq!(misplaced, [SpanFixture::span(SpanFixture::SECOND_ITEM)]);
    }

    #[test]
    fn a_spliced_do_of_expressions_misplaces_nothing() {
        let mut forms = SpanFixture::spliced_do();
        forms.leave_out(SpanFixture::span(SpanFixture::FIRST_ITEM));
        forms.leave_out(SpanFixture::span(SpanFixture::SECOND_ITEM));
        let misplaced = forms.misplaced_expressions(SpanFixture::span(SpanFixture::DO_FORM));
        assert!(misplaced.is_empty());
    }

    #[test]
    fn a_nested_do_inside_a_definition_do_misplaces_its_expressions() {
        let mut forms = SpanFixture::spliced_do();
        let nested = [SpanFixture::UNEVALUATED].map(SpanFixture::span);
        forms.splice(
            SpanFixture::span(SpanFixture::SECOND_ITEM),
            nested.into_iter(),
        );
        forms.note_definition(SpanFixture::span(SpanFixture::FIRST_ITEM));
        forms.leave_out(nested[0]);
        let misplaced = forms.misplaced_expressions(SpanFixture::span(SpanFixture::DO_FORM));
        assert_eq!(misplaced, nested);
    }

    #[test]
    fn a_name_bound_outside_every_evaluated_form_is_not_recorded() {
        let form = SpanFixture::span(SpanFixture::DO_FORM);
        let mut forms = EvaluatedForms::new([form]);
        let outside = SpanFixture::span(SpanFixture::UNEVALUATED);
        forms.note_bound(outside, || {
            Some(BoundName {
                name: "outside".into(),
                namespace: Namespace::Value,
                span: outside,
            })
        });
        let covering = Span::new(form.source(), form.range().start..outside.range().end);
        assert!(forms.bound_by(covering).is_empty());
    }

    #[test]
    fn a_do_whose_items_carry_its_own_span_misplaces_nothing() {
        let do_form = SpanFixture::span(SpanFixture::DO_FORM);
        let mut forms = EvaluatedForms::new([do_form]);
        forms.splice(do_form, [do_form, do_form].into_iter());
        forms.note_definition(do_form);
        forms.leave_out(do_form);
        assert!(forms.misplaced_expressions(do_form).is_empty());
    }

    #[test]
    fn a_do_that_is_not_evaluated_splices_nothing_into_the_evaluated_forms() {
        let mut forms = EvaluatedForms::new([]);
        let item = SpanFixture::span(SpanFixture::FIRST_ITEM);
        forms.splice(SpanFixture::span(SpanFixture::DO_FORM), [item].into_iter());
        assert!(!forms.holds(item));
    }
}
