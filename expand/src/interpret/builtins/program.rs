use super::Forms;
use crate::diagnostic::Diagnostic;
use crate::interpret::arguments::Arguments;
use crate::interpret::{CompileTimeHost, Value};
use crate::reader::{Form, FormKind, Symbol};
use crate::source::Span;

pub struct ProgramOperations<'a> {
    arguments: &'a Arguments,
    call_site: Span,
}

impl<'a> ProgramOperations<'a> {
    pub fn new(arguments: &'a Arguments, call_site: Span) -> Self {
        Self {
            arguments,
            call_site,
        }
    }

    pub fn form_location(&self, host: &mut dyn CompileTimeHost) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let location = host.form_location(&self.arguments.form(0)?);
        Ok(Value::Form(Form::new(
            FormKind::String(location.into()),
            self.call_site,
        )))
    }

    pub fn program_modules(&self, host: &mut dyn CompileTimeHost) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(0)?;
        Ok(self.symbols(host.program_modules()))
    }

    pub fn module_exports(&self, host: &mut dyn CompileTimeHost) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let path = self.arguments.form(0)?;
        let exports = host.module_exports(self.arguments.symbol(0)?, path.span())?;
        Ok(self.symbols(exports))
    }

    pub fn module_symbol(&self, host: &mut dyn CompileTimeHost) -> Result<Value, Diagnostic> {
        self.arguments.expect_count(1)?;
        let path = self.arguments.form(0)?;
        let module = host.module_symbol(self.arguments.symbol(0)?, path.span())?;
        Ok(Value::Form(Form::new(
            FormKind::Symbol(module),
            self.call_site,
        )))
    }

    fn symbols(&self, symbols: Vec<Symbol>) -> Value {
        let forms = symbols
            .into_iter()
            .map(|symbol| Form::new(FormKind::Symbol(symbol), self.call_site))
            .collect();
        Forms::list(forms, self.call_site)
    }
}
