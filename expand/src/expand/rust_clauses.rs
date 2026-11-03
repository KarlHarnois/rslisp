use super::Expander;
use super::problems::ExpandProblems;
use super::shape::Shape;
use crate::core::{ParsedTemplate, RustClause, TypeBinder, TypeExpr, TypeExprKind};
use crate::diagnostic::Diagnostic;
use crate::reader::Form;
use crate::source::Span;

pub(super) struct ClausedTemplate<'t> {
    pub parsed: &'t ParsedTemplate,
    pub span: Span,
    pub type_expr: &'t TypeExpr,
    pub type_span: Span,
}

impl Expander<'_> {
    pub(super) const RUST_PARTS: usize = 3;

    pub(super) fn lower_rust_clauses(
        &mut self,
        shape: &Shape,
        template: &ClausedTemplate,
        clauses: &[Form],
    ) -> Result<Vec<RustClause>, Diagnostic> {
        let pairs = clauses
            .iter()
            .map(|clause| Self::clause_pair(shape, clause))
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        let parameters = Self::type_parameters(template.type_expr).count();
        if !pairs.is_empty() && parameters != 1 {
            return Err(ExpandProblems::rust_clauses_need_one_parameter(
                parameters,
                template.type_span,
            ));
        }
        pairs
            .into_iter()
            .map(|pair| self.lower_rust_clause(shape, template, pair))
            .collect()
    }

    fn clause_pair<'f>(shape: &Shape, clause: &'f Form) -> Result<[&'f Form; 2], Diagnostic> {
        match clause.as_list() {
            Some([target, text]) => Ok([target, text]),
            _ => Err(shape.malformed(clause)),
        }
    }

    fn lower_rust_clause(
        &mut self,
        shape: &Shape,
        template: &ClausedTemplate,
        [target, text]: [&Form; 2],
    ) -> Result<RustClause, Diagnostic> {
        let parsed = Self::parse_rust_template(shape.string(text)?, text.span())?;
        self.check_template_type_placeholders(&parsed, template.type_expr, text.span())?;
        if let Some(difference) = template.parsed.passing_difference(&parsed) {
            return Err(ExpandProblems::clause_passes_arguments_unlike_template(
                difference,
                text.span(),
                template.span,
            ));
        }
        Ok(RustClause {
            target: self.lower_type(target)?,
            template: parsed.text().into(),
        })
    }

    pub(super) fn parse_rust_template(
        text: &str,
        span: Span,
    ) -> Result<ParsedTemplate, Diagnostic> {
        ParsedTemplate::parse(text).map_err(|_| ExpandProblems::malformed_template(span))
    }

    pub(super) fn where_binders(type_expr: &TypeExpr) -> &[TypeBinder] {
        match type_expr.kind() {
            TypeExprKind::Where(binders, _) => binders.as_slice(),
            _ => &[],
        }
    }

    fn type_parameters(type_expr: &TypeExpr) -> impl Iterator<Item = &TypeBinder> {
        Self::where_binders(type_expr)
            .iter()
            .filter(|binder| !binder.lifetime)
    }
}
