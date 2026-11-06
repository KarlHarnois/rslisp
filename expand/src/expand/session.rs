use super::generated_entry::GeneratedEntry;
use super::result_printer::GeneratedFunction;
use super::{Expander, Program};
use crate::core::{
    BindingId, BindingInfo, BindingKind, Block, BodyItem, Control, Expr, ExprKind, ModuleId,
    ModulePath, Namespace,
};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::modules::{ModuleRecord, ModuleSources};
use crate::reader::{Form, Reader, Symbol};
use crate::source::{SourceId, Span, TextPosition};

pub struct ExpansionSession<'s> {
    expander: Expander<'s>,
    entry: ModuleId,
}

impl<'s> Expander<'s> {
    pub fn open(
        mut self,
        path: ModulePath,
        sources: &[SourceId],
    ) -> Result<ExpansionSession<'s>, Diagnostics> {
        let entry = self.load_source_module(path, sources);
        self.take_diagnostics().into_result(ExpansionSession {
            expander: self,
            entry,
        })
    }
}

impl ExpansionSession<'_> {
    pub fn add_source(&mut self, name: &str, text: &str) -> SourceId {
        self.expander.sources.add(name, text)
    }

    pub fn add_source_inside(
        &mut self,
        enclosing: SourceId,
        position: TextPosition,
        text: &str,
    ) -> Option<SourceId> {
        self.expander.sources.add_inside(enclosing, position, text)
    }

    pub fn read(&self, source: SourceId) -> Result<Vec<Form>, Diagnostics> {
        Reader::new(self.expander.sources.file(source)).read()
    }

    pub fn expression(&mut self, form: Form) -> Result<Expr, Diagnostics> {
        let scoped = self.in_entry_scope(form);
        let lowered = self.lower_in_entry(|expander| expander.lower_expr(&scoped));
        self.collect_diagnostics(lowered)
    }

    pub fn generated_entry(
        &mut self,
        forms: &[Form],
        span: Span,
    ) -> Result<GeneratedEntry, Diagnostics> {
        let scoped: Vec<Form> = forms
            .iter()
            .map(|form| self.in_entry_scope(form.clone()))
            .collect();
        self.entry_lowered_in(self.entry, &scoped, span)
    }

    pub fn test_runner_entry(&mut self, source: SourceId) -> Result<GeneratedEntry, Diagnostics> {
        let span = Span::new(source, 0..self.expander.sources.file(source).text().len());
        let runner = self
            .expander
            .load_module(&ModulePath::new(ModuleSources::TEST), span)
            .map_err(Diagnostics::from)?;
        let forms = self.read(source)?;
        let scoped: Vec<Form> = forms
            .iter()
            .map(|form| {
                let mut scoped = form.clone();
                Self::add_module_scopes(self.expander.modules.get(runner), &mut scoped);
                scoped
            })
            .collect();
        self.entry_lowered_in(runner, &scoped, span)
    }

    pub fn loads(&self, path: &ModulePath) -> bool {
        self.expander.modules.find(path).is_some()
    }

    fn entry_lowered_in(
        &mut self,
        module: ModuleId,
        scoped: &[Form],
        span: Span,
    ) -> Result<GeneratedEntry, Diagnostics> {
        let lowered = self.lower_in(module, |expander| expander.lower_body(scoped));
        let mut body = self.collect_diagnostics(lowered)?;
        body.items.push(Self::unit(span));
        Ok(GeneratedEntry::new(self.entry_binding(span), body, span))
    }

    pub fn prelude_function(
        &mut self,
        name: &str,
        source: SourceId,
    ) -> Result<GeneratedFunction, Diagnostics> {
        let prelude = self.prelude().expect("the session loads the prelude");
        let mut form = self.read_one(source)?;
        Self::add_module_scopes(self.expander.modules.get(prelude), &mut form);
        let lowered = self.lower_in(prelude, |expander| expander.lower_expr(&form));
        let value = self.collect_diagnostics(lowered)?;
        let binding = self.introduced_global(name, value.span());
        Ok(GeneratedFunction::new(binding, value))
    }

    pub fn defines(&self, span: Span) -> bool {
        self.expander.evaluated_forms.is_definition(span)
    }

    pub fn prelude_value(&self, name: &str) -> Option<BindingId> {
        let prelude = self.prelude()?;
        self.expander
            .modules
            .get(prelude)
            .exported(name, Namespace::Value)
    }

    pub fn binding(&self, term: &Form) -> Option<BindingId> {
        let scoped = self.in_entry_scope(term.clone());
        self.expander.term_binding(scoped.as_symbol()?)
    }

    pub fn hide_type(&mut self, term: &Form) -> Option<Form> {
        let mut hidden = self.in_entry_scope(term.clone());
        if !self.names_a_value_and_a_type(hidden.as_symbol()?) {
            return None;
        }
        hidden.add_scope(self.expander.fresh_local_scope());
        self.expander
            .define_local(&hidden, Namespace::Type, BindingKind::TypeVariable, None)
            .expect("a fresh scope holds no binding");
        Some(hidden)
    }

    pub fn finish(self) -> Result<Program, Diagnostics> {
        self.expander.into_program(self.entry)
    }

    fn names_a_value_and_a_type(&self, term: &Symbol) -> bool {
        [Namespace::Value, Namespace::Type]
            .into_iter()
            .all(|namespace| self.expander.namespace_binding(term, namespace).is_some())
    }

    fn prelude(&self) -> Option<ModuleId> {
        self.expander
            .modules
            .find(&ModulePath::new(ModuleSources::PRELUDE))
    }

    fn read_one(&self, source: SourceId) -> Result<Form, Diagnostics> {
        let forms = self.read(source)?;
        let [form] = <[Form; 1]>::try_from(forms).expect("a generated function is one form");
        Ok(form)
    }

    fn in_entry_scope(&self, mut form: Form) -> Form {
        Self::add_module_scopes(self.expander.modules.get(self.entry), &mut form);
        form
    }

    fn add_module_scopes(module: &ModuleRecord, form: &mut Form) {
        form.add_scope(module.base_scope());
        form.add_scope(module.body_scope());
    }

    fn lower_in_entry<T>(
        &mut self,
        lower: impl FnOnce(&mut Expander) -> Result<T, Diagnostic>,
    ) -> Result<T, Diagnostic> {
        self.lower_in(self.entry, lower)
    }

    fn lower_in<T>(
        &mut self,
        module: ModuleId,
        lower: impl FnOnce(&mut Expander) -> Result<T, Diagnostic>,
    ) -> Result<T, Diagnostic> {
        let outer_module = self.expander.current_module.replace(module);
        let lowered = lower(&mut self.expander);
        self.expander.current_module = outer_module;
        lowered
    }

    fn entry_binding(&mut self, span: Span) -> BindingId {
        self.introduced_global(Program::WRITTEN_ENTRY_FUNCTION, span)
    }

    fn introduced_global(&mut self, name: &str, span: Span) -> BindingId {
        let info = BindingInfo::new(name.into(), BindingKind::Global(self.entry), Some(span));
        let bindings = &mut self.expander.tables.bindings;
        let binding = bindings.add(info);
        bindings.mark_introduced(binding);
        binding
    }

    fn unit(span: Span) -> BodyItem {
        let empty = Control::Block(Block::default());
        BodyItem::Expr(Expr::new(ExprKind::Control(empty), span))
    }

    fn collect_diagnostics<T>(&mut self, lowered: Result<T, Diagnostic>) -> Result<T, Diagnostics> {
        let mut diagnostics = self.expander.take_diagnostics();
        match lowered {
            Ok(expression) => diagnostics.into_result(expression),
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                Err(diagnostics)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::core::{ItemKind, Leaf};
    use crate::expand::ResultPrinter;
    use crate::modules::ModuleSources;
    use crate::reader::{FormKind, Symbol};
    use crate::source::{SourceMap, Span};

    struct SessionFixture {
        sources: SourceMap,
        entry: SourceId,
    }

    impl SessionFixture {
        const ENTRY: &str = "entry";
        const DOUBLING: &str = "(defn twice (x) (* 2 x))";
        const POINTS: &str = "(defstruct point :x i64 :y i64)\n(defn point () (->point 1 2))";
        const GRID: &str = "(defstruct cell :row i64)";
        const DEFINED_NAME: &str = "twice";
        const UNBOUND_NAME: &str = "thrice";
        const SHARED_NAME: &str = "point";
        const TYPE_NAME: &str = "cell";
        const PRINTER_NAME: &str = "print-value";
        const PRINTER: &str = "(fn (value) (println \"marker\" (pr-str value)))";
        const VALUE_TEXT: &str = "pr-str";

        fn new(program: &str) -> Self {
            let mut sources = SourceMap::default();
            let entry = sources.add(Self::ENTRY, program);
            Self { sources, entry }
        }

        fn session(&mut self) -> ExpansionSession<'_> {
            let entry = self.entry;
            self.expander()
                .open(ModulePath::new(Self::ENTRY), &[entry])
                .unwrap_or_else(|_| panic!("the entry expands"))
        }

        fn expanded(&mut self) -> Program {
            let entry = self.entry;
            self.expander()
                .expand(ModulePath::new(Self::ENTRY), entry)
                .unwrap_or_else(|_| panic!("the entry expands"))
        }

        fn expander(&mut self) -> Expander<'_> {
            Expander::new(
                &mut self.sources,
                ModuleSources::new(PathBuf::new(), PathBuf::new()),
            )
        }

        fn symbol(&self, name: &str) -> Form {
            Form::new(FormKind::Symbol(Symbol::new(name)), self.span())
        }

        fn list(&self, name: &str) -> Form {
            Form::new(FormKind::List(vec![self.symbol(name)]), self.span())
        }

        fn span(&self) -> Span {
            Span::new(self.entry, 0..0)
        }

        fn finished(session: ExpansionSession<'_>) -> Program {
            session
                .finish()
                .unwrap_or_else(|_| panic!("the session finishes"))
        }

        fn defined_binding(program: &Program) -> Option<BindingId> {
            program
                .entry_module()
                .items
                .iter()
                .find_map(|item| match item.kind {
                    ItemKind::Def(binding, _) => Some(binding),
                    _ => None,
                })
        }

        fn printing(&mut self) -> (Program, GeneratedEntry) {
            let call = self.list(Self::DEFINED_NAME);
            let span = self.span();
            let mut session = self.session();
            let entry = session
                .generated_entry(&[call], span)
                .unwrap_or_else(|_| panic!("the call lowers"));
            let source = session.add_source(Self::PRINTER_NAME, Self::PRINTER);
            let printer = session
                .prelude_function(Self::PRINTER_NAME, source)
                .unwrap_or_else(|_| panic!("the printer lowers"));
            let text = session
                .prelude_value(Self::VALUE_TEXT)
                .expect("the prelude defines the value's text");
            let printing = entry.printing(&ResultPrinter::through(printer, text), &[0]);
            (Self::finished(session), printing)
        }

        fn first_entry_form(program: &Program) -> &Expr {
            let entry = program.entry_function.expect("the program has an entry");
            let (_, value) = program
                .entry_module()
                .definitions()
                .find(|(binding, _)| *binding == entry)
                .expect("the entry module defines the entry");
            let ExprKind::Fn(lambda) = value.kind() else {
                panic!("the entry is a function");
            };
            lambda.body.items[0].expr()
        }

        fn called(expr: &Expr) -> Option<(BindingId, &[Expr])> {
            let ExprKind::Call(callee, arguments) = expr.kind() else {
                return None;
            };
            let ExprKind::Leaf(Leaf::Var(binding)) = callee.kind() else {
                return None;
            };
            Some((*binding, arguments))
        }

        fn definitions_named(program: &Program, name: &str) -> usize {
            program
                .entry_module()
                .definitions()
                .filter(|(binding, _)| program.bindings.get(*binding).name() == name)
                .count()
        }

        fn entry_module_text(program: &Program) -> String {
            format!("{:?}", program.entry_module())
        }

        fn type_binding(session: &ExpansionSession<'_>, term: &Form) -> Option<BindingId> {
            let scoped = session.in_entry_scope(term.clone());
            session
                .expander
                .namespace_binding(scoped.as_symbol()?, Namespace::Type)
        }
    }

    #[test]
    fn a_session_resolves_a_name_to_the_definition_it_finishes_with() {
        let mut fixture = SessionFixture::new(SessionFixture::DOUBLING);
        let name = fixture.symbol(SessionFixture::DEFINED_NAME);
        let session = fixture.session();
        let binding = session.binding(&name);
        let program = SessionFixture::finished(session);
        assert!(binding.is_some());
        assert_eq!(binding, SessionFixture::defined_binding(&program));
    }

    #[test]
    fn entering_a_generated_entry_again_replaces_its_definition() {
        let mut fixture = SessionFixture::new(SessionFixture::DOUBLING);
        let call = fixture.list(SessionFixture::DEFINED_NAME);
        let span = fixture.span();
        let mut session = fixture.session();
        let entry = session
            .generated_entry(&[call], span)
            .unwrap_or_else(|_| panic!("the call lowers"));
        let mut program = SessionFixture::finished(session);
        program.enter(&entry);
        program.enter(&entry);
        let entries = program
            .entry_module()
            .definitions()
            .filter(|(binding, _)| Some(*binding) == program.entry_function)
            .count();
        assert_eq!(entries, 1);
    }

    #[test]
    fn entering_a_printing_entry_again_replaces_its_functions() {
        let mut fixture = SessionFixture::new(SessionFixture::DOUBLING);
        let (mut program, printing) = fixture.printing();
        program.enter(&printing);
        program.enter(&printing);
        assert_eq!(
            SessionFixture::definitions_named(&program, SessionFixture::PRINTER_NAME),
            1
        );
    }

    #[test]
    fn a_printed_form_is_the_whole_argument_of_the_generated_printer() {
        let mut fixture = SessionFixture::new(SessionFixture::DOUBLING);
        let (mut program, printing) = fixture.printing();
        program.enter(&printing);
        let printed = SessionFixture::first_entry_form(&program);
        let (printer, arguments) = SessionFixture::called(printed).expect("the form is a call");
        let form = arguments.first().expect("the printer takes the form");
        assert_eq!(
            program.bindings.get(printer).name(),
            SessionFixture::PRINTER_NAME
        );
        assert_eq!(arguments.len(), 1);
        let (callee, _) = SessionFixture::called(form).expect("the form is a call");
        assert_eq!(
            program.bindings.get(callee).name(),
            SessionFixture::DEFINED_NAME
        );
    }

    #[test]
    fn a_session_resolves_no_binding_for_an_unbound_name() {
        let mut fixture = SessionFixture::new(SessionFixture::DOUBLING);
        let name = fixture.symbol(SessionFixture::UNBOUND_NAME);
        assert_eq!(fixture.session().binding(&name), None);
    }

    #[test]
    fn a_session_resolves_no_binding_for_a_list() {
        let mut fixture = SessionFixture::new(SessionFixture::DOUBLING);
        let list = fixture.list(SessionFixture::DEFINED_NAME);
        assert_eq!(fixture.session().binding(&list), None);
    }

    #[test]
    fn a_finished_session_holds_the_program_that_expansion_returns() {
        let finished =
            SessionFixture::finished(SessionFixture::new(SessionFixture::DOUBLING).session());
        let expanded = SessionFixture::new(SessionFixture::DOUBLING).expanded();
        assert_eq!(
            SessionFixture::entry_module_text(&finished),
            SessionFixture::entry_module_text(&expanded)
        );
    }

    #[test]
    fn a_session_hides_the_type_of_a_name_that_also_names_a_value() {
        let mut fixture = SessionFixture::new(SessionFixture::POINTS);
        let name = fixture.symbol(SessionFixture::SHARED_NAME);
        let mut session = fixture.session();
        let value = session.binding(&name);
        let named_type = SessionFixture::type_binding(&session, &name);
        let hidden = session
            .hide_type(&name)
            .unwrap_or_else(|| panic!("a name of a value and a type hides its type"));
        let hiding_type = SessionFixture::type_binding(&session, &hidden);
        assert!(named_type.is_some());
        assert!(hiding_type.is_some());
        assert_ne!(hiding_type, named_type);
        assert_eq!(session.binding(&hidden), value);
        assert_eq!(SessionFixture::type_binding(&session, &name), named_type);
    }

    #[test]
    fn a_session_hides_nothing_of_a_name_that_names_only_a_value() {
        let mut fixture = SessionFixture::new(SessionFixture::DOUBLING);
        let name = fixture.symbol(SessionFixture::DEFINED_NAME);
        assert!(fixture.session().hide_type(&name).is_none());
    }

    #[test]
    fn a_session_hides_nothing_of_a_name_that_names_only_a_type() {
        let mut fixture = SessionFixture::new(SessionFixture::GRID);
        let name = fixture.symbol(SessionFixture::TYPE_NAME);
        assert!(fixture.session().hide_type(&name).is_none());
    }

    #[test]
    fn a_session_hides_nothing_of_a_list() {
        let mut fixture = SessionFixture::new(SessionFixture::POINTS);
        let list = fixture.list(SessionFixture::SHARED_NAME);
        assert!(fixture.session().hide_type(&list).is_none());
    }
}
