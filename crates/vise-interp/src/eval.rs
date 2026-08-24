//! A tree-walking evaluator.
//!
//! Deliberately simple and slow. Its job is to make "green" mean *passes its
//! tests* rather than merely *compiles*, so the benchmark of M3 can run before
//! a real backend exists. It will be thrown away.
//!
//! Two spec rules are enforced here rather than left to a later stage, because
//! they are runtime behaviour by definition:
//!
//! - **§4**: integer arithmetic traps on overflow. It never wraps.
//! - **§10**: `requires` and `ensures` are checked on every call.

use std::collections::BTreeMap;
use std::sync::Arc;

use vise_ast::{
    BinOp, Binding, Block, Expr, ExprKind, FnDecl, ItemKind, Literal, Module, Pattern, PatternKind,
    Stmt, StmtKind, StrPart, UnOp,
};

use crate::value::{Trap, Value};

/// What a program produced.
///
/// Output is carried alongside the result rather than inside it, because a
/// program that traps has usually printed something first, and that output is
/// the most useful thing about the failure.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// Lines written by `print`, in order, including those before a trap.
    pub stdout: Vec<String>,
    /// The value `main` returned, or the trap that stopped it.
    pub result: Result<Value, Trap>,
}

impl Run {
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.result.is_ok()
    }

    /// The returned value, panicking on a trap. For tests.
    ///
    /// # Panics
    /// Panics if the program trapped.
    #[must_use]
    pub fn value(&self) -> &Value {
        match &self.result {
            Ok(v) => v,
            Err(t) => panic!("program trapped: {t}"),
        }
    }
}

/// Why evaluation stopped early.
///
/// Modelling `return`, `break`, and `continue` as errors rather than as a
/// success variant means Rust's own `?` propagates them, so no call site can
/// forget to. An earlier shape returned them alongside a value, and a helper
/// that treated `Return` as ordinary silently swallowed `let v = f()?`.
enum Abort {
    Trap(Trap),
    Return(Value),
    Break,
    Continue,
}

impl From<Trap> for Abort {
    fn from(t: Trap) -> Self {
        Self::Trap(t)
    }
}

type Eval<T> = Result<T, Abort>;

/// Collapse an abort that reached the top of a function.
fn finish(outcome: Eval<Value>) -> Result<Value, Trap> {
    match outcome {
        Ok(v) | Err(Abort::Return(v)) => Ok(v),
        Err(Abort::Trap(t)) => Err(t),
        // The checker rejects these outside a loop, so reaching here means it
        // was bypassed.
        Err(Abort::Break | Abort::Continue) => Err(Trap::Unsupported(
            "`break` or `continue` outside a loop".to_owned(),
        )),
    }
}

/// Run `main` with no command-line arguments.
#[must_use]
pub fn run(module: &Module) -> Run {
    run_with_args(module, Vec::new())
}

/// Run `main`, giving it the arguments `args()` should report.
#[must_use]
pub fn run_with_args(module: &Module, args: Vec<String>) -> Run {
    call_with(module, "main", Vec::new(), args)
}

/// Call one function by name.
#[must_use]
pub fn call(module: &Module, name: &str, args: Vec<Value>) -> Run {
    call_with(module, name, args, Vec::new())
}

/// Call one function by name, with command-line arguments for `args()`.
#[must_use]
pub fn call_with(module: &Module, name: &str, args: Vec<Value>, argv: Vec<String>) -> Run {
    // A tree-walking evaluator uses many native frames per Vise call, so deep
    // recursion would abort the process before `MAX_DEPTH` was reached. A
    // benchmark harness cannot tell an aborted process apart from a bug in
    // itself, so the interpreter gets its own generous stack and always
    // returns a trap.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn_scoped(scope, || call_on_this_thread(module, name, args, argv))
            .expect("spawning the interpreter thread")
            .join()
            .unwrap_or_else(|_| Run {
                stdout: Vec::new(),
                result: Err(Trap::Unsupported("the interpreter panicked".to_owned())),
            })
    })
}

fn call_on_this_thread(module: &Module, name: &str, args: Vec<Value>, argv: Vec<String>) -> Run {
    let mut fns = BTreeMap::new();
    let mut ctors = BTreeMap::new();
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => {
                fns.insert(f.name.name.clone(), (**f).clone());
            }
            ItemKind::Enum(d) => {
                for v in &d.variants {
                    ctors.insert(v.name.name.clone(), v.fields.len());
                }
            }
            _ => {}
        }
    }
    for (name, arity) in [("Ok", 1), ("Err", 1), ("Some", 1), ("None", 0)] {
        ctors.insert(name.to_owned(), arity);
    }

    let mut interp = Interp {
        fns,
        ctors,
        argv,
        stdout: Vec::new(),
        scopes: Vec::new(),
        depth: 0,
    };
    let result = interp.call(name, args);
    Run {
        stdout: interp.stdout,
        result,
    }
}

/// Recursion limit, so a runaway program traps instead of aborting the process
/// on a native stack overflow.
const MAX_DEPTH: usize = 512;

struct Interp {
    fns: BTreeMap<String, FnDecl>,
    ctors: BTreeMap<String, usize>,
    argv: Vec<String>,
    stdout: Vec<String>,
    scopes: Vec<BTreeMap<String, Value>>,
    depth: usize,
}

impl Interp {
    // --- scopes ----------------------------------------------------------

    fn push(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn pop(&mut self) {
        self.scopes.pop();
    }

    fn define(&mut self, name: &str, value: Value) {
        if let Some(s) = self.scopes.last_mut() {
            s.insert(name.to_owned(), value);
        }
    }

    fn assign(&mut self, name: &str, value: Value) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(slot) = scope.get_mut(name) {
                *slot = value;
                return;
            }
        }
        self.define(name, value);
    }

    fn lookup(&self, name: &str) -> Option<Value> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    // --- calls -----------------------------------------------------------

    fn call(&mut self, name: &str, args: Vec<Value>) -> Result<Value, Trap> {
        if vise_check::builtin(name).is_some() {
            return self.builtin(name, args);
        }
        if let Some(arity) = self.ctors.get(name).copied() {
            if args.len() == arity {
                return Ok(Value::variant(name, args));
            }
            return Err(Trap::Unsupported(format!(
                "`{name}` takes {arity} field(s)"
            )));
        }

        let Some(f) = self.fns.get(name).cloned() else {
            return Err(Trap::Unsupported(format!("no function `{name}`")));
        };

        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return Err(Trap::Unsupported(format!(
                "recursion deeper than {MAX_DEPTH} calls in `{name}`"
            )));
        }

        let saved = std::mem::take(&mut self.scopes);
        self.push();
        for (param, value) in f.params.iter().zip(args) {
            self.define(&param.name.name, value);
        }

        let outcome = self.body(&f);

        self.scopes = saved;
        self.depth -= 1;
        outcome
    }

    /// Every `core` function.
    ///
    /// The set comes from `vise_check::builtins`, and a test asserts this
    /// implements all of it, so the table and the runtime cannot drift.
    #[allow(clippy::too_many_lines)]
    fn builtin(&mut self, name: &str, args: Vec<Value>) -> Result<Value, Trap> {
        let str_at = |i: usize| -> String {
            match args.get(i) {
                Some(Value::Str(s)) => s.to_string(),
                Some(other) => other.to_string(),
                None => String::new(),
            }
        };
        let int_at = |i: usize| -> i64 {
            match args.get(i) {
                Some(Value::Int(n)) => *n,
                _ => 0,
            }
        };
        let ok = |v: Value| Value::variant("Ok", vec![v]);
        let err = |m: String| Value::variant("Err", vec![Value::str(m)]);
        let some = |v: Value| Value::variant("Some", vec![v]);
        let none = || Value::variant("None", Vec::new());

        Ok(match name {
            "print" => {
                self.stdout.push(str_at(0));
                Value::Unit
            }

            "length" => match args.first() {
                Some(Value::List(items)) => {
                    Value::Int(i64::try_from(items.len()).unwrap_or(i64::MAX))
                }
                _ => Value::Int(0),
            },
            "append" => match (args.first(), args.get(1)) {
                (Some(Value::List(items)), Some(v)) => {
                    let mut next = items.as_ref().clone();
                    next.push(v.clone());
                    Value::List(Arc::new(next))
                }
                _ => Value::List(Arc::new(Vec::new())),
            },
            "at" => match (args.first(), args.get(1)) {
                (Some(Value::List(items)), Some(Value::Int(i))) => usize::try_from(*i)
                    .ok()
                    .and_then(|u| items.get(u).cloned())
                    .map_or_else(none, some),
                _ => none(),
            },

            "str_length" => {
                Value::Int(i64::try_from(str_at(0).chars().count()).unwrap_or(i64::MAX))
            }
            "lines" => {
                let text = str_at(0);
                // A trailing newline ends the last line; it does not begin an
                // empty one. `lines` on "a\n" is one line, not two.
                let items: Vec<Value> = text.lines().map(Value::str).collect();
                Value::List(Arc::new(items))
            }
            "split" => {
                let text = str_at(0);
                let sep = str_at(1);
                let items: Vec<Value> = if sep.is_empty() {
                    text.chars().map(|c| Value::str(c.to_string())).collect()
                } else {
                    text.split(sep.as_str()).map(Value::str).collect()
                };
                Value::List(Arc::new(items))
            }
            "join" => match args.first() {
                Some(Value::List(items)) => {
                    let parts: Vec<String> = items.iter().map(ToString::to_string).collect();
                    Value::str(parts.join(&str_at(1)))
                }
                _ => Value::str(""),
            },
            "starts_with" => Value::Bool(str_at(0).starts_with(&str_at(1))),
            "contains" => Value::Bool(str_at(0).contains(&str_at(1))),
            "parse_int" => str_at(0)
                .trim()
                .parse::<i64>()
                .map_or_else(|_| none(), |n| some(Value::Int(n))),

            "read_file" => match std::fs::read_to_string(str_at(0)) {
                Ok(text) => ok(Value::str(text)),
                Err(e) => err(e.to_string()),
            },
            "write_file" => match std::fs::write(str_at(0), str_at(1)) {
                Ok(()) => ok(Value::Unit),
                Err(e) => err(e.to_string()),
            },
            "list_dir" => match std::fs::read_dir(str_at(0)) {
                Ok(entries) => {
                    let mut names: Vec<String> = entries
                        .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned()))
                        .collect();
                    // Sorted, because §11 says a program's output must not
                    // depend on the order a filesystem happens to report.
                    names.sort();
                    ok(Value::List(Arc::new(
                        names.into_iter().map(Value::str).collect(),
                    )))
                }
                Err(e) => err(e.to_string()),
            },
            // Not `Path::is_dir`, which follows the link. See vise_is_dir.
            "is_dir" => Value::Bool(std::fs::symlink_metadata(str_at(0)).is_ok_and(|m| m.is_dir())),
            "file_size" => match std::fs::metadata(str_at(0)) {
                Ok(m) => ok(Value::Int(i64::try_from(m.len()).unwrap_or(i64::MAX))),
                Err(e) => err(e.to_string()),
            },

            "args" => Value::List(Arc::new(
                self.argv.iter().map(Value::str).collect::<Vec<_>>(),
            )),
            "now" => Value::Int(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
            ),
            "exit" => return Err(Trap::Exit(int_at(0))),

            other => {
                return Err(Trap::Unsupported(format!(
                    "`{other}` is in core but the interpreter does not implement it"
                )));
            }
        })
    }

    /// Run a function body with its contracts.
    fn body(&mut self, f: &FnDecl) -> Result<Value, Trap> {
        for clause in &f.requires {
            if !self.clause_holds(clause)? {
                return Err(Trap::Requires {
                    function: f.name.name.clone(),
                });
            }
        }

        let value = finish(self.block(&f.body))?;

        if !f.ensures.is_empty() {
            self.push();
            self.define("result", value.clone());
            for clause in &f.ensures {
                match self.clause_holds(clause) {
                    Ok(true) => {}
                    Ok(false) => {
                        self.pop();
                        return Err(Trap::Ensures {
                            function: f.name.name.clone(),
                        });
                    }
                    Err(t) => {
                        self.pop();
                        return Err(t);
                    }
                }
            }
            self.pop();
        }

        Ok(value)
    }

    /// Evaluate a contract clause. A clause is an expression, so it could in
    /// principle abort; anything other than a trap means it did not hold.
    fn clause_holds(&mut self, e: &Expr) -> Result<bool, Trap> {
        match self.truthy(e) {
            Ok(holds) => Ok(holds),
            Err(Abort::Trap(t)) => Err(t),
            Err(_) => Ok(false),
        }
    }

    fn truthy(&mut self, e: &Expr) -> Eval<bool> {
        Ok(matches!(self.eval(e)?, Value::Bool(true)))
    }

    // --- statements ------------------------------------------------------

    fn block(&mut self, b: &Block) -> Eval<Value> {
        self.push();
        let out = self.block_inner(b);
        self.pop();
        out
    }

    fn block_inner(&mut self, b: &Block) -> Eval<Value> {
        for s in &b.stmts {
            self.stmt(s)?;
        }
        match &b.tail {
            Some(t) => self.eval(t),
            None => Ok(Value::Unit),
        }
    }

    fn stmt(&mut self, s: &Stmt) -> Eval<()> {
        match &s.kind {
            StmtKind::Let { name, value, .. } => {
                let v = self.eval(value)?;
                if let Binding::Name(ident) = name {
                    self.define(&ident.name, v);
                }
                Ok(())
            }
            StmtKind::Assign { target, value } => {
                let v = self.eval(value)?;
                match &target.kind {
                    ExprKind::Path(p) => {
                        if let Some(name) = p.as_single() {
                            self.assign(&name.name, v);
                        }
                        Ok(())
                    }
                    _ => Err(
                        Trap::Unsupported("only a plain name can be assigned to".to_owned()).into(),
                    ),
                }
            }
            StmtKind::For {
                binding,
                iter,
                body,
            } => {
                let seq = self.eval(iter)?;
                let Value::List(items) = seq else {
                    return Err(Trap::Unsupported(format!(
                        "`for` cannot iterate {}",
                        seq.type_name()
                    ))
                    .into());
                };
                for item in items.iter() {
                    self.push();
                    if let Binding::Name(ident) = binding {
                        self.define(&ident.name, item.clone());
                    }
                    let outcome = self.block(body);
                    self.pop();
                    match outcome {
                        Err(Abort::Break) => break,
                        Err(Abort::Continue) | Ok(_) => {}
                        Err(other) => return Err(other),
                    }
                }
                Ok(())
            }
            StmtKind::While { cond, body } => {
                while self.truthy(cond)? {
                    match self.block(body) {
                        Err(Abort::Break) => break,
                        Err(Abort::Continue) | Ok(_) => {}
                        Err(other) => return Err(other),
                    }
                }
                Ok(())
            }
            StmtKind::Expr(e) => self.eval(e).map(|_| ()),
        }
    }

    // --- expressions -----------------------------------------------------

    #[allow(clippy::too_many_lines)]
    fn eval(&mut self, e: &Expr) -> Eval<Value> {
        let v = match &e.kind {
            ExprKind::Literal(lit) => self.literal(lit)?,
            ExprKind::Path(p) => {
                let Some(name) = p.as_single() else {
                    return Err(Trap::Unsupported("qualified path".to_owned()).into());
                };
                match self.lookup(&name.name) {
                    Some(v) => v,
                    None if name.name == "Unit" => Value::Unit,
                    None if self.ctors.get(&name.name) == Some(&0) => {
                        Value::variant(&name.name, Vec::new())
                    }
                    None => {
                        return Err(
                            Trap::Unsupported(format!("`{}` has no value", name.name)).into()
                        );
                    }
                }
            }
            ExprKind::Call { callee, args } => {
                let ExprKind::Path(p) = &callee.kind else {
                    return Err(Trap::Unsupported("call of a computed value".to_owned()).into());
                };
                let Some(name) = p.as_single() else {
                    return Err(Trap::Unsupported("qualified call".to_owned()).into());
                };
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(self.eval(a)?);
                }
                self.call(&name.name, values)?
            }
            ExprKind::Field { base, name } => {
                let b = self.eval(base)?;
                match b {
                    Value::Record { fields, .. } => fields
                        .get(&name.name)
                        .cloned()
                        .ok_or_else(|| Trap::Unsupported(format!("no field `{}`", name.name)))?,
                    other => {
                        return Err(Trap::Unsupported(format!(
                            "{} has no fields",
                            other.type_name()
                        ))
                        .into());
                    }
                }
            }
            ExprKind::Index { base, index } => {
                let b = self.eval(base)?;
                let i = self.eval(index)?;
                match (b, i) {
                    (Value::List(items), Value::Int(n)) => {
                        let len = items.len();
                        usize::try_from(n)
                            .ok()
                            .and_then(|u| items.get(u).cloned())
                            .ok_or(Trap::IndexOutOfBounds { index: n, len })?
                    }
                    (b, _) => {
                        return Err(
                            Trap::Unsupported(format!("cannot index {}", b.type_name())).into()
                        );
                    }
                }
            }
            ExprKind::Unary { op, operand } => {
                let v = self.eval(operand)?;
                match (op, v) {
                    (UnOp::Not, Value::Bool(b)) => Value::Bool(!b),
                    (UnOp::Neg, Value::Int(n)) => {
                        Value::Int(n.checked_neg().ok_or(Trap::Overflow("-"))?)
                    }
                    (UnOp::Neg, Value::Float(x)) => Value::Float(-x),
                    (op, v) => {
                        return Err(Trap::Unsupported(format!(
                            "`{}` on {}",
                            op.as_str(),
                            v.type_name()
                        ))
                        .into());
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs)?,
            // Borrows are transparent at runtime: nothing is mutated in place,
            // so a borrow and its referent behave identically.
            ExprKind::Borrow { operand, .. } => self.eval(operand)?,
            ExprKind::Try(inner) => {
                let v = self.eval(inner)?;
                match v {
                    Value::Variant { name, fields } if &*name == "Ok" => {
                        fields.first().cloned().unwrap_or(Value::Unit)
                    }
                    // `?` returns the whole `Err` from the enclosing function.
                    v if v.is_err() => return Err(Abort::Return(v)),
                    other => {
                        return Err(Trap::Unsupported(format!(
                            "`?` applied to {}",
                            other.type_name()
                        ))
                        .into());
                    }
                }
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                return if self.truthy(cond)? {
                    self.block(then)
                } else {
                    self.eval(otherwise)
                };
            }
            ExprKind::Match { scrutinee, arms } => {
                let subject = self.eval(scrutinee)?;
                for arm in arms {
                    let mut bindings = Vec::new();
                    if matches(&arm.pattern, &subject, &mut bindings) {
                        self.push();
                        for (name, value) in bindings {
                            self.define(&name, value);
                        }
                        let flow = self.eval(&arm.body);
                        self.pop();
                        return flow;
                    }
                }
                return Err(Trap::NoMatchingArm.into());
            }
            ExprKind::Block(b) => return self.block(b),
            ExprKind::RecordLit { name, fields } => {
                let mut values = BTreeMap::new();
                for f in fields {
                    values.insert(f.name.name.clone(), self.eval(&f.value)?);
                }
                Value::Record {
                    name: Arc::from(name.name.as_str()),
                    fields: Arc::new(values),
                }
            }
            ExprKind::ListLit(items) => {
                let mut values = Vec::with_capacity(items.len());
                for i in items {
                    values.push(self.eval(i)?);
                }
                Value::List(Arc::new(values))
            }
            ExprKind::Return(value) => {
                let v = match value {
                    Some(v) => self.eval(v)?,
                    None => Value::Unit,
                };
                return Err(Abort::Return(v));
            }
            ExprKind::Break => return Err(Abort::Break),
            ExprKind::Continue => return Err(Abort::Continue),
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                let value = self.eval(receiver)?;
                for a in args {
                    self.eval(a)?;
                }
                if method.name != "clone" || !args.is_empty() {
                    // The checker rejects this, so reaching here means it was
                    // bypassed.
                    return Err(
                        Trap::Unsupported(format!("`{}` is not a method", method.name)).into(),
                    );
                }
                // Values are immutable and shared, so a clone is the value
                // itself. What `.clone()` buys is ownership, which matters to
                // the borrow checker rather than to the evaluator.
                value
            }
        };
        Ok(v)
    }

    fn literal(&mut self, lit: &Literal) -> Eval<Value> {
        Ok(match lit {
            Literal::Int(v) => Value::Int(*v),
            Literal::Float(text) => Value::Float(text.parse().unwrap_or(0.0)),
            Literal::Bool(v) => Value::Bool(*v),
            Literal::Char(v) => Value::Char(*v),
            Literal::Str(parts) => {
                let mut out = String::new();
                for p in parts {
                    match p {
                        StrPart::Text(t) => out.push_str(t),
                        StrPart::Interpolation(e) => {
                            let v = self.eval(e)?;
                            out.push_str(&v.to_string());
                        }
                    }
                }
                Value::str(out)
            }
        })
    }

    fn binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Eval<Value> {
        // Short-circuit before evaluating the right side.
        if matches!(op, BinOp::And | BinOp::Or) {
            let l = self.truthy(lhs)?;
            return Ok(Value::Bool(match op {
                BinOp::And if !l => false,
                BinOp::Or if l => true,
                _ => self.truthy(rhs)?,
            }));
        }

        let l = self.eval(lhs)?;
        let r = self.eval(rhs)?;

        Ok(match op {
            BinOp::Eq => Value::Bool(l == r),
            BinOp::Ne => Value::Bool(l != r),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let ordering = compare(&l, &r)
                    .ok_or_else(|| Trap::Unsupported(format!("cannot order {}", l.type_name())))?;
                Value::Bool(match op {
                    BinOp::Lt => ordering.is_lt(),
                    BinOp::Le => ordering.is_le(),
                    BinOp::Gt => ordering.is_gt(),
                    _ => ordering.is_ge(),
                })
            }
            _ => arithmetic(op, &l, &r)?,
        })
    }
}

/// §4: integer arithmetic traps on overflow, it never wraps.
fn arithmetic(op: BinOp, l: &Value, r: &Value) -> Result<Value, Trap> {
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => {
            let (a, b) = (*a, *b);
            let v = match op {
                BinOp::Add => a.checked_add(b).ok_or(Trap::Overflow("+"))?,
                BinOp::Sub => a.checked_sub(b).ok_or(Trap::Overflow("-"))?,
                BinOp::Mul => a.checked_mul(b).ok_or(Trap::Overflow("*"))?,
                BinOp::Div => a.checked_div(b).ok_or(if b == 0 {
                    Trap::DivideByZero
                } else {
                    Trap::Overflow("/")
                })?,
                BinOp::Rem => a.checked_rem(b).ok_or(if b == 0 {
                    Trap::DivideByZero
                } else {
                    Trap::Overflow("%")
                })?,
                _ => return Err(Trap::Unsupported(format!("`{}` on Int", op.as_str()))),
            };
            Ok(Value::Int(v))
        }
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(match op {
            BinOp::Add => a + b,
            BinOp::Sub => a - b,
            BinOp::Mul => a * b,
            BinOp::Div => a / b,
            BinOp::Rem => a % b,
            _ => return Err(Trap::Unsupported(format!("`{}` on Float", op.as_str()))),
        })),
        _ => Err(Trap::Unsupported(format!(
            "`{}` on {} and {}",
            op.as_str(),
            l.type_name(),
            r.type_name()
        ))),
    }
}

fn compare(l: &Value, r: &Value) -> Option<std::cmp::Ordering> {
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => Some(a.cmp(b)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Str(a), Value::Str(b)) => Some(a.cmp(b)),
        (Value::Char(a), Value::Char(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

/// Try a pattern, collecting what it binds. Bindings are discarded by the
/// caller if the pattern does not apply.
fn matches(pattern: &Pattern, value: &Value, out: &mut Vec<(String, Value)>) -> bool {
    match &pattern.kind {
        PatternKind::Wildcard => true,
        PatternKind::Binding(ident) => {
            out.push((ident.name.clone(), value.clone()));
            true
        }
        PatternKind::Literal(lit) => match (lit, value) {
            (Literal::Int(a), Value::Int(b)) => a == b,
            (Literal::Bool(a), Value::Bool(b)) => a == b,
            (Literal::Char(a), Value::Char(b)) => a == b,
            (Literal::Float(a), Value::Float(b)) => a.parse::<f64>().is_ok_and(|x| x == *b),
            (Literal::Str(parts), Value::Str(s)) => match parts.as_slice() {
                [StrPart::Text(t)] => t == &**s,
                [] => s.is_empty(),
                _ => false,
            },
            _ => false,
        },
        PatternKind::Variant { path, fields } => {
            let Some(name) = path.as_single() else {
                return false;
            };
            let Value::Variant {
                name: actual,
                fields: values,
            } = value
            else {
                return false;
            };
            if **actual != name.name || fields.len() > values.len() {
                return false;
            }
            fields
                .iter()
                .zip(values.iter())
                .all(|(p, v)| matches(p, v, out))
        }
    }
}
