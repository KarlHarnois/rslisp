mod arguments;
mod builtins;
mod code_list;
mod problems;
mod value;

pub use builtins::BuiltinCall;
pub use code_list::CodeList;
pub use problems::InterpretProblems;
pub use value::{Closure, Env, Value};

use std::rc::Rc;

use crate::core::{BindingId, Block, BodyItem, Control, Expr, ExprKind, Leaf, Literal};
use crate::diagnostic::Diagnostic;
use crate::reader::{Form, FormKind, Symbol};
use crate::source::Span;
use crate::stack::DeepRecursion;

pub trait CompileTimeHost {
    fn global_value(&mut self, binding: BindingId, span: Span) -> Result<Value, Diagnostic>;
    fn same_binding(&self, left: &Symbol, right: &Symbol) -> bool;
}

enum Step {
    Value(Value),
    Recur(Vec<Value>),
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
        match self.step(expr, env)? {
            Step::Value(value) => Ok(value),
            Step::Recur(..) => unreachable!("expansion allows `recur` only inside a function"),
        }
    }

    fn step(&mut self, expr: &Expr, env: &Env) -> Result<Step, Diagnostic> {
        DeepRecursion::guard(|| self.step_kind(expr, env))
    }

    fn step_kind(&mut self, expr: &Expr, env: &Env) -> Result<Step, Diagnostic> {
        let span = expr.span();
        match expr.kind() {
            ExprKind::Leaf(leaf) => self.eval_leaf(leaf, env, span).map(Step::Value),
            ExprKind::Fn(lambda) => Ok(Step::Value(Value::Closure(Rc::new(Closure {
                lambda: Rc::clone(lambda),
                env: env.clone(),
            })))),
            ExprKind::Call(callee, arguments) => self
                .eval_call(callee, arguments, env, span)
                .map(Step::Value),
            ExprKind::Recur(arguments) => self.eval_recur(arguments, env),
            ExprKind::Borrow(_, value) | ExprKind::Deref(value) => {
                self.eval(value, env).map(Step::Value)
            }
            _ => self.value(expr, env, span),
        }
    }

    fn value(&mut self, expr: &Expr, env: &Env, span: Span) -> Result<Step, Diagnostic> {
        match expr.kind() {
            ExprKind::Control(control) => self.step_control(control, env, span),
            ExprKind::Struct(_) => Err(InterpretProblems::runtime_only("structs", span)),
            ExprKind::Try(_) => Err(InterpretProblems::runtime_only("`try` forms", span)),
            ExprKind::Variant(..) => Err(InterpretProblems::runtime_only("enum values", span)),
            _ => unreachable!("the caller matches a value form"),
        }
    }

    fn eval_leaf(&mut self, leaf: &Leaf, env: &Env, span: Span) -> Result<Value, Diagnostic> {
        match leaf {
            Leaf::Literal(literal) => Ok(Self::literal(literal, span)),
            Leaf::Var(binding) => self.variable(*binding, env, span),
            Leaf::Quote(form) => Ok(Value::Form(form.clone())),
            Leaf::Rust(_) => Err(InterpretProblems::program_only(span)),
        }
    }

    fn step_control(
        &mut self,
        control: &Control,
        env: &Env,
        span: Span,
    ) -> Result<Step, Diagnostic> {
        match control {
            Control::Block(block) => self.step_block(block, env),
            Control::If(test, then, otherwise) => self.step_if(test, then, otherwise, env),
            Control::Match(..) => Err(InterpretProblems::match_only_at_runtime(span)),
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
        let values = self.eval_arguments(arguments, env)?;
        self.call(&function, values, span)
    }

    fn eval_recur(&mut self, arguments: &[Expr], env: &Env) -> Result<Step, Diagnostic> {
        Ok(Step::Recur(self.eval_arguments(arguments, env)?))
    }

    fn eval_arguments(&mut self, arguments: &[Expr], env: &Env) -> Result<Vec<Value>, Diagnostic> {
        let mut values = Vec::with_capacity(arguments.len());
        for argument in arguments {
            values.push(self.eval(argument, env)?);
        }
        Ok(values)
    }

    fn call_closure(
        &mut self,
        closure: &Closure,
        arguments: Vec<Value>,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        let params = &closure.lambda.params;
        if params.len() != arguments.len() {
            return Err(InterpretProblems::closure_argument_count(
                params.len(),
                arguments.len(),
                span,
            ));
        }
        if self.depth >= Self::MAX_DEPTH {
            return Err(InterpretProblems::too_deep(span));
        }
        let env = Self::bind_arguments(closure, arguments);
        self.depth += 1;
        let result = self.run_body(closure, env);
        self.depth -= 1;
        result
    }

    fn run_body(&mut self, closure: &Closure, mut env: Env) -> Result<Value, Diagnostic> {
        loop {
            match self.step_block(&closure.lambda.body, &env) {
                Ok(Step::Value(value)) => return Ok(value),
                Ok(Step::Recur(arguments)) => {
                    env = Self::bind_arguments(closure, arguments);
                }
                Err(diagnostic) => return Err(diagnostic),
            }
        }
    }

    fn bind_arguments(closure: &Closure, arguments: Vec<Value>) -> Env {
        closure
            .lambda
            .params
            .iter()
            .zip(arguments)
            .fold(closure.env.clone(), |env, (param, value)| {
                env.bind(param.binding, value)
            })
    }

    fn step_block(&mut self, block: &Block, env: &Env) -> Result<Step, Diagnostic> {
        let mut env = env.clone();
        let mut last = Step::Value(Value::Unit);
        for item in &block.items {
            last = match item {
                BodyItem::Let(binder, value) => {
                    let value = self.eval(value, &env)?;
                    env = env.bind(binder.binding, value);
                    Step::Value(Value::Unit)
                }
                BodyItem::Expr(expr) => self.step(expr, &env)?,
            };
        }
        Ok(last)
    }

    fn step_if(
        &mut self,
        test: &Expr,
        then: &Expr,
        otherwise: &Expr,
        env: &Env,
    ) -> Result<Step, Diagnostic> {
        match self.eval(test, env)? {
            Value::Bool(true) => self.step(then, env),
            Value::Bool(false) => self.step(otherwise, env),
            other => Err(InterpretProblems::not_a_boolean(&other, test.span())),
        }
    }

    fn variable(&mut self, binding: BindingId, env: &Env, span: Span) -> Result<Value, Diagnostic> {
        match env.lookup(binding) {
            Some(value) => Ok(value.clone()),
            None => self.host.global_value(binding, span),
        }
    }

    fn literal(literal: &Literal, span: Span) -> Value {
        match literal {
            Literal::Number(number) => {
                Value::Form(Form::new(FormKind::Number(number.clone()), span))
            }
            Literal::String(text) => {
                Value::Form(Form::new(FormKind::String(Rc::clone(text)), span))
            }
            Literal::Bool(value) => Value::Bool(*value),
        }
    }
}
