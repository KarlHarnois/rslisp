use std::rc::Rc;

use super::Expander;
use super::problems::ExpandProblems;
use crate::core::{
    Binder, BindingInfo, BindingKind, Block, BodyItem, Expr, ExprKind, FieldLabel, FieldSpelling,
    Keyword, Lambda, Leaf, StructExpr,
};
use crate::diagnostic::Diagnostic;
use crate::reader::Form;
use crate::source::Span;

impl Expander<'_> {
    const FIELD_READER_PARAMETER: &'static str = "value";

    pub(super) fn lower_field_keyword_call(
        &mut self,
        form: &Form,
        field_name: &str,
    ) -> Result<Expr, Diagnostic> {
        let items = form.as_list().unwrap_or_default();
        let [keyword, value] = items else {
            return Err(Self::field_keyword_arity(form, items));
        };
        let value = self.lower_recovered(value)?;
        let label = Self::keyword_label(field_name, keyword.span());
        Ok(Self::field_read(value, label, form.span()))
    }

    pub(super) fn lower_field_reader(&mut self, field_name: &str, span: Span) -> ExprKind {
        let parameter = self.tables.bindings.add(BindingInfo::new(
            Self::FIELD_READER_PARAMETER.into(),
            BindingKind::Local,
            Some(span),
        ));
        let value = Expr::new(ExprKind::Leaf(Leaf::Var(parameter)), span);
        let read = Self::field_read(value, Self::keyword_label(field_name, span), span);
        ExprKind::Fn(Rc::new(Lambda {
            params: vec![Binder {
                binding: parameter,
                annotation: None,
                span,
            }],
            body: Block {
                items: vec![BodyItem::Expr(read)],
            },
        }))
    }

    pub(super) fn reject_keyword_binder(name: &str, span: Span) -> Result<(), Diagnostic> {
        match Keyword::field_name(name) {
            Some(_) => Err(ExpandProblems::keyword_binder(span)),
            None => Ok(()),
        }
    }

    fn field_keyword_arity(form: &Form, items: &[Form]) -> Diagnostic {
        let keyword = items[0].to_string();
        match items.get(2) {
            Some(extra) => ExpandProblems::extra_field_keyword_value(&keyword, extra.span()),
            None => ExpandProblems::missing_field_keyword_value(&keyword, form.span()),
        }
    }

    pub(super) fn keyword_label(field_name: &str, span: Span) -> FieldLabel {
        FieldLabel {
            name: field_name.into(),
            span,
            spelling: FieldSpelling::Keyword,
        }
    }

    fn field_read(value: Expr, label: FieldLabel, span: Span) -> Expr {
        Expr::new(
            ExprKind::Struct(StructExpr::Field(Box::new(value), label)),
            span,
        )
    }
}
