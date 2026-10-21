use super::Expander;
use crate::core::{BindingKind, KernelType, Namespace, TypeBinder};
use crate::diagnostic::Diagnostic;
use crate::reader::Form;

pub(super) struct Parameterized {
    pub binders: Vec<TypeBinder>,
    pub body: Vec<Form>,
}

impl Expander<'_> {
    const PARAMETERIZED_PARTS: usize = 3;

    pub(super) fn parameterized_body<'f>(&self, body: &'f [Form]) -> Option<&'f [Form]> {
        let [form] = body else {
            return None;
        };
        let items = form.as_list()?;
        let opens_with_where = items.first().is_some_and(|head| self.is_where(head));
        (items.len() >= Self::PARAMETERIZED_PARTS && opens_with_where).then_some(items)
    }

    pub(super) fn lower_parameters(
        &mut self,
        where_items: &[Form],
        malformed: impl Fn() -> Diagnostic,
    ) -> Result<Parameterized, Diagnostic> {
        let scope = self.fresh_local_scope();
        let mut binders = where_items[1].as_list().ok_or_else(&malformed)?.to_vec();
        let mut body = where_items[2..].to_vec();
        binders
            .iter_mut()
            .chain(body.iter_mut())
            .for_each(|form| form.add_scope(scope));
        let binders = binders
            .iter()
            .map(|binder| self.lower_type_binder(binder))
            .collect::<Result<Vec<TypeBinder>, Diagnostic>>()?;
        Ok(Parameterized { binders, body })
    }

    fn is_where(&self, head: &Form) -> bool {
        head.as_symbol()
            .and_then(|symbol| self.try_resolve(symbol, Namespace::Type))
            .is_some_and(|binding| self.kind(binding) == BindingKind::KernelType(KernelType::Where))
    }
}
