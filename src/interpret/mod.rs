mod arguments;
mod builtins;
mod problems;
mod value;

pub use builtins::BuiltinCall;
pub use problems::InterpretProblems;
pub use value::{Closure, Env, Value};

use std::rc::Rc;

use crate::core::{BindingId, Block, BodyItem, Expr, ExprKind, Literal, RustTemplate};
use crate::diagnostic::{Diagnostic, DiagnosticCode, Label};
use crate::reader::{Form, FormKind, Symbol};
use crate::source::Span;

pub trait CompileTimeHost {
    fn global_value(&mut self, binding: BindingId, span: Span) -> Result<Value, Diagnostic>;
    fn same_binding(&self, left: &Symbol, right: &Symbol) -> bool;
}

pub struct Interpreter<'h> {
    host: &'h mut dyn CompileTimeHost,
    call_site: Span,
    depth: usize,
}

impl<'h> Interpreter<'h> {
    const MAX_DEPTH: usize = 1000;

    pub fn new(host: &'h mut dyn CompileTimeHost, call_site: Span) -> Self {
        Self {
            host,
            call_site,
            depth: 0,
        }
    }

    pub fn call(
        &mut self,
        callee: &Value,
        arguments: Vec<Value>,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        match callee {
            Value::Closure(closure) => self.call_closure(closure, arguments, span),
            Value::Builtin(builtin) => {
                BuiltinCall::new(*builtin, arguments, span, self.call_site).run(self.host)
            }
            other => Err(InterpretProblems::not_callable(other, span)),
        }
    }

    pub fn eval(&mut self, expr: &Expr, env: &Env) -> Result<Value, Diagnostic> {
        let span = expr.span();
        match expr.kind() {
            ExprKind::Literal(literal) => Ok(Self::literal(literal, span)),
            ExprKind::Var(binding) => self.variable(*binding, env, span),
            ExprKind::Fn(lambda) => Ok(Value::Closure(Rc::new(Closure {
                lambda: Rc::clone(lambda),
                env: env.clone(),
            }))),
            ExprKind::Call(callee, arguments) => self.eval_call(callee, arguments, env, span),
            ExprKind::Block(block) => self.eval_block(block, env),
            ExprKind::If(test, then, otherwise) => self.eval_if(test, then, otherwise, env),
            ExprKind::Quote(form) => Ok(Value::Form(form.clone())),
            ExprKind::Rust(template) => self.eval_rust(template, env, span),
            ExprKind::New(..) | ExprKind::Field(..) => {
                Err(InterpretProblems::runtime_only("structs", span))
            }
        }
    }

    fn eval_call(
        &mut self,
        callee: &Expr,
        arguments: &[Expr],
        env: &Env,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        let function = self.eval(callee, env)?;
        let values = arguments
            .iter()
            .map(|argument| self.eval(argument, env))
            .collect::<Result<Vec<Value>, Diagnostic>>()?;
        self.call(&function, values, span)
    }

    fn call_closure(
        &mut self,
        closure: &Closure,
        arguments: Vec<Value>,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        let params = &closure.lambda.params;
        if params.len() != arguments.len() {
            return Err(InterpretProblems::wrong_argument_count(
                "this function",
                params.len(),
                arguments.len(),
                span,
            ));
        }
        if self.depth >= Self::MAX_DEPTH {
            return Err(InterpretProblems::too_deep(span));
        }
        let env = params
            .iter()
            .zip(arguments)
            .fold(closure.env.clone(), |env, (param, value)| {
                env.bind(param.binding, value)
            });
        self.depth += 1;
        let result = self.eval_block(&closure.lambda.body, &env);
        self.depth -= 1;
        result
    }

    fn eval_block(&mut self, block: &Block, env: &Env) -> Result<Value, Diagnostic> {
        let mut env = env.clone();
        let mut last = Value::Unit;
        for item in &block.items {
            last = match item {
                BodyItem::Let(binder, value) => {
                    let value = self.eval(value, &env)?;
                    env = env.bind(binder.binding, value);
                    Value::Unit
                }
                BodyItem::Expr(expr) => self.eval(expr, &env)?,
            };
        }
        Ok(last)
    }

    fn eval_if(
        &mut self,
        test: &Expr,
        then: &Expr,
        otherwise: &Expr,
        env: &Env,
    ) -> Result<Value, Diagnostic> {
        match self.eval(test, env)? {
            Value::Bool(true) => self.eval(then, env),
            Value::Bool(false) => self.eval(otherwise, env),
            other => Err(InterpretProblems::not_a_boolean(&other, test.span())),
        }
    }

    fn variable(&mut self, binding: BindingId, env: &Env, span: Span) -> Result<Value, Diagnostic> {
        match env.lookup(binding) {
            Some(value) => Ok(value.clone()),
            None => self.host.global_value(binding, span),
        }
    }

    fn eval_rust(
        &mut self,
        template: &RustTemplate,
        env: &Env,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        match &template.twin {
            Some(twin) => self.eval(twin, env),
            None => Err(Diagnostic::error(
                DiagnosticCode::NO_COMPILE_TIME_TWIN,
                "this Rust code has no compile-time version",
                Label::new(span, "used at compile time here"),
            )
            .with_help(
                "give the `rust` form a twin from the `code` module, or avoid it in macro code",
            )),
        }
    }

    fn literal(literal: &Literal, span: Span) -> Value {
        let kind = match literal {
            Literal::Number(number) => FormKind::Number(number.clone()),
            Literal::String(text) => FormKind::String(Rc::clone(text)),
        };
        Value::Form(Form::new(kind, span))
    }
}
