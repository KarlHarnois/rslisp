use crate::core::KernelForm;
use crate::diagnostic::{Diagnostic, DiagnosticCode, Label};
use crate::reader::{Form, FormKind, Symbol};

pub struct Shape<'a> {
    kernel: KernelForm,
    form: &'a Form,
}

impl<'a> Shape<'a> {
    pub fn new(kernel: KernelForm, form: &'a Form) -> Self {
        Self { kernel, form }
    }

    pub fn items(&self) -> &'a [Form] {
        self.form.as_list().unwrap_or_default()
    }

    pub fn exactly(&self, count: usize) -> Result<&'a [Form], Diagnostic> {
        let items = self.items();
        if items.len() == count {
            Ok(items)
        } else {
            Err(self.malformed(self.form))
        }
    }

    pub fn at_least(&self, count: usize) -> Result<&'a [Form], Diagnostic> {
        let items = self.items();
        if items.len() >= count {
            Ok(items)
        } else {
            Err(self.malformed(self.form))
        }
    }

    pub fn symbol(&self, part: &'a Form) -> Result<&'a Symbol, Diagnostic> {
        part.as_symbol().ok_or_else(|| self.malformed(part))
    }

    pub fn list(&self, part: &'a Form) -> Result<&'a [Form], Diagnostic> {
        part.as_list().ok_or_else(|| self.malformed(part))
    }

    pub fn string(&self, part: &'a Form) -> Result<&'a str, Diagnostic> {
        match part.kind() {
            FormKind::String(text) => Ok(text),
            _ => Err(self.malformed(part)),
        }
    }

    pub fn malformed(&self, part: &Form) -> Diagnostic {
        self.malformed_with_help(part, format!("write it as `{}`", self.kernel.shape()))
    }

    pub fn malformed_form_with_help(&self, help: impl Into<String>) -> Diagnostic {
        self.malformed_with_help(self.form, help)
    }

    pub fn malformed_with_help(&self, part: &Form, help: impl Into<String>) -> Diagnostic {
        let name = self.kernel.name();
        Diagnostic::error(
            DiagnosticCode::MALFORMED_FORM,
            format!("malformed `{name}`"),
            Label::new(
                part.span(),
                format!("this does not fit the shape of `{name}`"),
            ),
        )
        .with_help(help)
    }
}
