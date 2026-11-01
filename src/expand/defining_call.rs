use super::Expander;
use crate::core::BindingId;
use crate::reader::{Form, Reader};
use crate::source::Span;

impl Expander<'_> {
    pub(super) fn defining_call(&self, binding: BindingId) -> Option<Form> {
        let name = self.module_level_name(binding)?;
        let call_site = self.macro_origin_at(name)?.call_site;
        self.written_form(call_site)
    }

    fn module_level_name(&self, binding: BindingId) -> Option<Span> {
        self.definition_module(binding)
            .and_then(|_| self.bindings.get(binding).span())
    }

    fn written_form(&self, span: Span) -> Option<Form> {
        let forms = Reader::new(self.sources.file(span.source())).read().ok()?;
        SpanSearch::new(span).find(&forms)
    }
}

struct SpanSearch {
    target: Span,
}

impl SpanSearch {
    fn new(target: Span) -> Self {
        Self { target }
    }

    fn find(&self, forms: &[Form]) -> Option<Form> {
        forms.iter().find_map(|form| self.search(form))
    }

    fn search(&self, form: &Form) -> Option<Form> {
        if form.span() == self.target {
            Some(form.clone())
        } else if form.span().contains(self.target) {
            self.find(form.as_list()?)
        } else {
            None
        }
    }
}
