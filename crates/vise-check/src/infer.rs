//! Type inference and checking.
//!
//! Signatures are given, bodies are inferred (§4). Because there are no traits
//! and no overloading, inference is unification against a nominal type
//! constructor — a call site has exactly one candidate, and a generic parameter
//! is rigid inside its own body and instantiated at each call.
//!
//! # Scope
//!
//! Method calls and calls to imported functions produce [`Ty::Error`], which
//! absorbs unification rather than inventing a constraint. Neither has a
//! signature to check against yet: methods need a trait or module system, and
//! imports need the module system of open spec issue 7. This keeps the checker
//! quiet where it cannot know, instead of confidently wrong.

use std::collections::BTreeMap;

use vise_ast::{
    BinOp, Binding, Block, EnumDecl, Expr, ExprKind, FnDecl, ItemKind, Literal, MatchArm, Module,
    Pattern, PatternKind, RecordDecl, Stmt, StmtKind, StrPart, Type, TypeKind, UnOp,
};
use vise_diag::{Code, Diagnostic, Span};

use crate::types::{Mismatch, Table, Ty};

/// Types that arithmetic accepts.
const NUMERIC: &[&str] = &["Int", "Float"];
/// Types that `<`, `<=`, `>`, `>=` accept.
const ORDERED: &[&str] = &["Int", "Float", "Str", "Char"];

/// A function's declared interface.
#[derive(Debug, Clone)]
struct Sig {
    generics: Vec<String>,
    params: Vec<Ty>,
    ret: Ty,
}

/// A record's interface: its type parameters and its fields in order.
#[derive(Debug, Clone)]
struct RecordSig {
    generics: Vec<String>,
    fields: Vec<(String, Ty)>,
}

impl RecordSig {
    fn field_names(&self) -> Vec<String> {
        self.fields.iter().map(|(n, _)| n.clone()).collect()
    }
}

/// A constructor's interface: which enum it builds and what it takes.
#[derive(Debug, Clone)]
struct CtorSig {
    enum_name: String,
    generics: Vec<String>,
    fields: Vec<Ty>,
}

/// The inferred type of every expression, keyed by its span.
///
/// Handed to the backend so it does not have to reconstruct what the checker
/// already worked out. Keyed by byte range rather than by `Span`, because a
/// module is one file and two expressions cannot share a range.
pub type TypeMap = BTreeMap<(u32, u32), Ty>;

/// Type-check every function in `module`.
#[must_use]
pub fn check(module: &Module) -> Vec<Diagnostic> {
    check_with_types(module).0
}

/// Type-check, and return what each expression turned out to be.
#[must_use]
pub fn check_with_types(module: &Module) -> (Vec<Diagnostic>, TypeMap) {
    let mut c = Checker::new(module);
    for item in &module.items {
        if let ItemKind::Fn(f) = &item.kind {
            c.function(f);
        }
    }
    // Resolve now: a type recorded mid-inference may still hold variables that
    // were only decided later.
    let resolved = c
        .recorded
        .iter()
        .map(|(span, ty)| (*span, c.table.resolve(ty)))
        .collect();
    (c.diagnostics, resolved)
}

struct Checker {
    table: Table,
    fns: BTreeMap<String, Sig>,
    records: BTreeMap<String, RecordSig>,
    ctors: BTreeMap<String, CtorSig>,
    /// Every type name that exists, so an unknown one is left to `V0201`.
    known_types: Vec<String>,
    /// `type UserId = Int` maps `UserId` to `Int`. A distinct type is not its
    /// base for unification, but arithmetic and literals see through to it.
    bases: BTreeMap<String, String>,
    scopes: Vec<BTreeMap<String, Ty>>,
    /// Return type of the function being checked.
    ret: Ty,
    /// What each expression came out as, for the backend.
    recorded: TypeMap,
    diagnostics: Vec<Diagnostic>,
}

impl Checker {
    fn new(module: &Module) -> Self {
        let mut c = Self {
            table: Table::new(),
            fns: BTreeMap::new(),
            records: BTreeMap::new(),
            ctors: BTreeMap::new(),
            known_types: [
                "Int", "Float", "Bool", "Char", "Str", "Unit", "List", "Map", "Set", "Option",
                "Result",
            ]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
            bases: BTreeMap::new(),
            scopes: Vec::new(),
            ret: Ty::unit(),
            recorded: TypeMap::new(),
            diagnostics: Vec::new(),
        };

        c.ctors.insert(
            "Ok".into(),
            CtorSig {
                enum_name: "Result".into(),
                generics: vec!["T".into(), "E".into()],
                fields: vec![Ty::con("T")],
            },
        );
        c.ctors.insert(
            "Err".into(),
            CtorSig {
                enum_name: "Result".into(),
                generics: vec!["T".into(), "E".into()],
                fields: vec![Ty::con("E")],
            },
        );
        c.ctors.insert(
            "Some".into(),
            CtorSig {
                enum_name: "Option".into(),
                generics: vec!["T".into()],
                fields: vec![Ty::con("T")],
            },
        );
        c.ctors.insert(
            "None".into(),
            CtorSig {
                enum_name: "Option".into(),
                generics: vec!["T".into()],
                fields: Vec::new(),
            },
        );
        c.fns.insert(
            "print".into(),
            Sig {
                generics: Vec::new(),
                params: vec![Ty::con("Str")],
                ret: Ty::unit(),
            },
        );

        for item in &module.items {
            match &item.kind {
                ItemKind::Type(d) => {
                    c.known_types.push(d.name.name.clone());
                    if let TypeKind::Named { name, args } = &d.underlying.kind
                        && args.is_empty()
                    {
                        c.bases.insert(d.name.name.clone(), name.name.clone());
                    }
                }
                ItemKind::Record(d) => c.record(d),
                ItemKind::Enum(d) => c.enum_decl(d),
                ItemKind::Fn(_) => {}
            }
        }
        for item in &module.items {
            if let ItemKind::Fn(f) = &item.kind {
                let sig = Sig {
                    generics: f.generics.iter().map(|g| g.name().name.clone()).collect(),
                    params: f.params.iter().map(|p| c.to_ty(&p.ty)).collect(),
                    ret: f.ret.as_ref().map_or_else(Ty::unit, |t| c.to_ty(t)),
                };
                c.fns.insert(f.name.name.clone(), sig);
            }
        }
        c
    }

    fn record(&mut self, d: &RecordDecl) {
        self.known_types.push(d.name.name.clone());
        let generics: Vec<String> = d.generics.iter().map(|g| g.name.clone()).collect();
        let fields = d
            .fields
            .iter()
            .map(|f| (f.name.name.clone(), self.to_ty(&f.ty)))
            .collect();
        self.records
            .insert(d.name.name.clone(), RecordSig { generics, fields });
    }

    fn enum_decl(&mut self, d: &EnumDecl) {
        self.known_types.push(d.name.name.clone());
        let generics: Vec<String> = d.generics.iter().map(|g| g.name.clone()).collect();
        for v in &d.variants {
            self.ctors.insert(
                v.name.name.clone(),
                CtorSig {
                    enum_name: d.name.name.clone(),
                    generics: generics.clone(),
                    fields: v.fields.iter().map(|f| self.to_ty(&f.ty)).collect(),
                },
            );
        }
    }

    /// Follow `type X = Y` declarations down to the underlying type.
    fn base(&self, ty: &Ty) -> Ty {
        let mut current = self.table.resolve(ty);
        // The chain is finite: each step names a previously declared type, and
        // a cycle would have to name itself, which cannot resolve further.
        for _ in 0..self.bases.len() + 1 {
            let Ty::Con(name, args) = &current else { break };
            if !args.is_empty() {
                break;
            }
            let Some(next) = self.bases.get(name) else {
                break;
            };
            current = Ty::con(next);
        }
        current
    }

    /// Convert a written type. Unknown names become `Error` — `V0201` already
    /// reported them, and inventing a constructor would cascade.
    fn to_ty(&self, ty: &Type) -> Ty {
        match &ty.kind {
            TypeKind::Unit => Ty::unit(),
            TypeKind::Ref { is_mut, inner, .. } => Ty::borrow(self.to_ty(inner), *is_mut),
            TypeKind::Named { name, args } => {
                let args: Vec<Ty> = args.iter().map(|a| self.to_ty(a)).collect();
                Ty::Con(name.name.clone(), args)
            }
        }
    }

    // --- diagnostics -----------------------------------------------------

    fn expect(&mut self, expected: &Ty, found: &Ty, span: Span, context: &str) {
        match self.table.unify(expected, found) {
            Ok(()) => {}
            Err(Mismatch::Types { expected, found }) => {
                self.diagnostics.push(
                    Diagnostic::error(
                        Code::TypeMismatch,
                        span,
                        format!("expected `{expected}`, found `{found}`"),
                    )
                    .with_note(context.to_owned()),
                );
            }
            Err(Mismatch::Infinite) => {
                self.diagnostics.push(Diagnostic::error(
                    Code::TypeMismatch,
                    span,
                    "this value would have to contain itself",
                ));
            }
        }
    }

    // --- scopes ----------------------------------------------------------

    fn push(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn pop(&mut self) {
        self.scopes.pop();
    }

    fn define(&mut self, name: &str, ty: Ty) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_owned(), ty);
        }
    }

    fn lookup(&self, name: &str) -> Option<Ty> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    /// Replace a signature's generic names with fresh variables.
    fn instantiate(&mut self, generics: &[String], tys: &[Ty]) -> Vec<Ty> {
        let map: BTreeMap<String, Ty> = generics
            .iter()
            .map(|g| (g.clone(), self.table.fresh()))
            .collect();
        tys.iter().map(|t| substitute(t, &map)).collect()
    }

    // --- functions -------------------------------------------------------

    fn function(&mut self, f: &FnDecl) {
        self.push();
        let sig = self.fns[&f.name.name].clone();
        for (param, ty) in f.params.iter().zip(&sig.params) {
            self.define(&param.name.name, ty.clone());
        }
        self.ret = sig.ret.clone();

        for e in &f.requires {
            let t = self.expr(e);
            self.expect(
                &Ty::con("Bool"),
                &t,
                e.span,
                "a `requires` clause must be a `Bool`",
            );
        }
        if !f.ensures.is_empty() {
            self.push();
            self.define("result", sig.ret.clone());
            for e in &f.ensures {
                let t = self.expr(e);
                self.expect(
                    &Ty::con("Bool"),
                    &t,
                    e.span,
                    "an `ensures` clause must be a `Bool`",
                );
            }
            self.pop();
        }

        let body = self.block(&f.body);
        let span = f.body.tail.as_ref().map_or(f.body.span, |t| t.span);
        self.expect(
            &sig.ret,
            &body,
            span,
            "the block's value is the return value",
        );
        self.pop();
    }

    fn block(&mut self, b: &Block) -> Ty {
        self.push();
        for s in &b.stmts {
            self.stmt(s);
        }
        let ty = b.tail.as_ref().map_or_else(Ty::unit, |t| self.expr(t));
        self.pop();
        ty
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let {
                name, ty, value, ..
            } => {
                let found = self.expr(value);
                let bound = match ty {
                    Some(annotation) => {
                        let declared = self.to_ty(annotation);
                        self.expect(
                            &declared,
                            &found,
                            value.span,
                            "the annotation and the value disagree",
                        );
                        declared
                    }
                    None => found,
                };
                if let Binding::Name(ident) = name {
                    self.define(&ident.name, bound);
                }
            }
            StmtKind::Assign { target, value } => {
                let t = self.expr(target);
                let v = self.expr(value);
                self.expect(
                    &t,
                    &v,
                    value.span,
                    "the assigned value must match the target",
                );
            }
            StmtKind::For {
                binding,
                iter,
                body,
            } => {
                let element = self.table.fresh();
                let seq = self.expr(iter);
                // Accept List<T> or Set<T>; anything else is left alone rather
                // than guessed at.
                match self.table.shallow(&seq) {
                    Ty::Con(name, args) if matches!(name.as_str(), "List" | "Set") => {
                        if let Some(first) = args.first() {
                            let _ = self.table.unify(&element, first);
                        }
                    }
                    Ty::Con(_, _) => self.expect(
                        &Ty::app("List", vec![element.clone()]),
                        &seq,
                        iter.span,
                        "`for` iterates a `List` or a `Set`",
                    ),
                    _ => {}
                }
                self.push();
                if let Binding::Name(ident) = binding {
                    self.define(&ident.name, element);
                }
                self.block(body);
                self.pop();
            }
            StmtKind::While { cond, body } => {
                let c = self.expr(cond);
                self.expect(
                    &Ty::con("Bool"),
                    &c,
                    cond.span,
                    "a `while` condition must be a `Bool`",
                );
                self.block(body);
            }
            StmtKind::Expr(e) => {
                self.expr(e);
            }
        }
    }

    // --- expressions -----------------------------------------------------

    fn expr(&mut self, e: &Expr) -> Ty {
        let ty = self.expr_inner(e);
        self.recorded.insert((e.span.start, e.span.end), ty.clone());
        ty
    }

    fn expr_inner(&mut self, e: &Expr) -> Ty {
        match &e.kind {
            ExprKind::Literal(lit) => self.literal(lit),
            ExprKind::Path(p) => {
                let Some(name) = p.as_single() else {
                    return Ty::Error;
                };
                if let Some(ty) = self.lookup(&name.name) {
                    return ty;
                }
                // §6's match example writes `Unit` as a value, so the name is
                // both the type and its single inhabitant.
                if name.name == "Unit" {
                    return Ty::unit();
                }
                if let Some(ctor) = self.ctors.get(&name.name).cloned() {
                    // A nullary constructor used as a value, such as `None`.
                    if ctor.fields.is_empty() {
                        let args = self.instantiate(
                            &ctor.generics,
                            &ctor.generics.iter().map(|g| Ty::con(g)).collect::<Vec<_>>(),
                        );
                        return Ty::Con(ctor.enum_name, args);
                    }
                }
                Ty::Error
            }
            ExprKind::Call { callee, args } => self.call(e, callee, args),
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                let received = self.expr(receiver);
                for a in args {
                    self.expr(a);
                }
                // §9 defines exactly one method. Returning a poison type for
                // anything else would let `s.wibble()` type-check and then fail
                // at runtime, which is the failure this language exists to
                // prevent.
                if method.name == "clone" && args.is_empty() {
                    received
                } else {
                    self.diagnostics.push(
                        Diagnostic::error(
                            Code::UnknownName,
                            method.span,
                            format!("`{}` is not a method", method.name),
                        )
                        .with_note(
                            "Vise has no traits, so `.clone()` is the only method there is (§9)",
                        )
                        .with_scope(["clone"]),
                    );
                    Ty::Error
                }
            }
            ExprKind::Field { base, name } => {
                let base_ty = self.expr(base);
                self.field(&base_ty, &name.name, name.span)
            }
            ExprKind::Index { base, index } => {
                let b = self.expr(base);
                let i = self.expr(index);
                match self.table.shallow(&b) {
                    Ty::Con(n, args) if n == "List" => {
                        self.expect(
                            &Ty::con("Int"),
                            &i,
                            index.span,
                            "a `List` is indexed by `Int`",
                        );
                        args.first().cloned().unwrap_or(Ty::Error)
                    }
                    Ty::Con(n, args) if n == "Map" => {
                        if let Some(k) = args.first() {
                            let k = k.clone();
                            self.expect(&k, &i, index.span, "the key must match the map");
                        }
                        args.get(1).cloned().unwrap_or(Ty::Error)
                    }
                    _ => Ty::Error,
                }
            }
            ExprKind::Unary { op, operand } => {
                let t = self.expr(operand);
                match op {
                    UnOp::Not => {
                        self.expect(&Ty::con("Bool"), &t, e.span, "`!` applies to a `Bool`");
                        Ty::con("Bool")
                    }
                    UnOp::Neg => {
                        self.require_one_of(&t, NUMERIC, e.span, "`-` applies to a number");
                        t
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, e.span),
            ExprKind::Borrow { is_mut, operand } => Ty::borrow(self.expr(operand), *is_mut),
            ExprKind::Try(inner) => {
                let t = self.expr(inner);
                match self.table.shallow(&t) {
                    Ty::Con(n, args) if n == "Result" => {
                        // The error type must fit this function's own Result.
                        if let (Some(err), Ty::Con(rn, rargs)) =
                            (args.get(1), self.table.shallow(&self.ret.clone()))
                            && rn == "Result"
                            && let Some(ret_err) = rargs.get(1)
                        {
                            let (err, ret_err) = (err.clone(), ret_err.clone());
                            self.expect(
                                &ret_err,
                                &err,
                                inner.span,
                                "`?` propagates the error, so it must match this function's error type",
                            );
                        }
                        args.first().cloned().unwrap_or(Ty::Error)
                    }
                    Ty::Error | Ty::Var(_) => Ty::Error,
                    other => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                Code::TypeMismatch,
                                inner.span,
                                format!("`?` applies to a `Result`, found `{other}`"),
                            )
                            .with_note("Vise has no exceptions; `?` propagates an `Err`"),
                        );
                        Ty::Error
                    }
                }
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                let c = self.expr(cond);
                self.expect(
                    &Ty::con("Bool"),
                    &c,
                    cond.span,
                    "an `if` condition must be a `Bool`",
                );
                let t = self.block(then);
                let o = self.expr(otherwise);
                self.expect(
                    &t,
                    &o,
                    otherwise.span,
                    "both branches of an `if` must agree",
                );
                t
            }
            ExprKind::Match { scrutinee, arms } => self.match_expr(scrutinee, arms),
            ExprKind::Block(b) => self.block(b),
            ExprKind::RecordLit { name, fields } => self.record_lit(name, fields, e.span),
            ExprKind::ListLit(items) => {
                let element = self.table.fresh();
                for i in items {
                    let t = self.expr(i);
                    self.expect(&element, &t, i.span, "every element of a list has one type");
                }
                Ty::app("List", vec![element])
            }
            ExprKind::Return(value) => {
                let t = value.as_ref().map_or_else(Ty::unit, |v| self.expr(v));
                let ret = self.ret.clone();
                let span = value.as_ref().map_or(e.span, |v| v.span);
                self.expect(
                    &ret,
                    &t,
                    span,
                    "the returned value must match the signature",
                );
                // `return` never produces a value where it stands.
                Ty::Error
            }
            ExprKind::Break | ExprKind::Continue => Ty::Error,
        }
    }

    fn literal(&mut self, lit: &Literal) -> Ty {
        match lit {
            Literal::Int(_) => Ty::con("Int"),
            Literal::Float(_) => Ty::con("Float"),
            Literal::Bool(_) => Ty::con("Bool"),
            Literal::Char(_) => Ty::con("Char"),
            Literal::Str(parts) => {
                for p in parts {
                    if let StrPart::Interpolation(inner) = p {
                        self.expr(inner);
                    }
                }
                Ty::con("Str")
            }
        }
    }

    fn call(&mut self, whole: &Expr, callee: &Expr, args: &[Expr]) -> Ty {
        let ExprKind::Path(path) = &callee.kind else {
            for a in args {
                self.expr(a);
            }
            return Ty::Error;
        };
        let Some(name) = path.as_single() else {
            return Ty::Error;
        };

        if let Some(ctor) = self.ctors.get(&name.name).cloned() {
            let mut proto: Vec<Ty> = ctor.fields.clone();
            proto.extend(ctor.generics.iter().map(|g| Ty::con(g)));
            let fresh = self.instantiate(&ctor.generics, &proto);
            let (fields, params) = fresh.split_at(ctor.fields.len());
            self.check_args(args, fields, name.span, &name.name);
            return Ty::Con(ctor.enum_name, params.to_vec());
        }

        if let Some(sig) = self.fns.get(&name.name).cloned() {
            let mut proto = sig.params.clone();
            proto.push(sig.ret.clone());
            let fresh = self.instantiate(&sig.generics, &proto);
            let (params, ret) = fresh.split_at(sig.params.len());
            self.check_args(args, params, whole.span, &name.name);
            return ret.first().cloned().unwrap_or(Ty::Error);
        }

        // Imported, or a value used as a callee. No signature to check against.
        for a in args {
            self.expr(a);
        }
        Ty::Error
    }

    fn check_args(&mut self, args: &[Expr], params: &[Ty], span: Span, name: &str) {
        if args.len() != params.len() {
            self.diagnostics.push(
                Diagnostic::error(
                    Code::TypeMismatch,
                    span,
                    format!(
                        "`{name}` takes {} argument{}, but {} {} given",
                        params.len(),
                        if params.len() == 1 { "" } else { "s" },
                        args.len(),
                        if args.len() == 1 { "was" } else { "were" }
                    ),
                )
                .with_note("Vise has no default arguments and no varargs"),
            );
        }
        for (arg, param) in args.iter().zip(params) {
            let t = self.expr(arg);
            let param = param.clone();
            self.expect(&param, &t, arg.span, &format!("argument to `{name}`"));
        }
        for extra in args.iter().skip(params.len()) {
            self.expr(extra);
        }
    }

    /// Reconcile two numeric operands.
    ///
    /// A numeric literal adopts the type of the other operand, so `amount / 50`
    /// works when `amount` is declared over `Int`. This is the narrowest rule
    /// that makes the spec's own `fee` example type-check: it never lets two
    /// *named* types mix, so `Cents + UserId` is still an error, and passing a
    /// bare `1` where a `UserId` is expected is still an error. Only a literal,
    /// which has no domain of its own, is allowed to take one on.
    fn numeric_operands(&mut self, l: &Ty, lhs: &Expr, r: &Ty, rhs: &Expr) -> Ty {
        let lr = self.table.resolve(l);
        let rr = self.table.resolve(r);
        if is_numeric_literal(rhs) && self.base(&lr) == rr {
            return lr;
        }
        if is_numeric_literal(lhs) && self.base(&rr) == lr {
            return rr;
        }
        self.expect(l, r, rhs.span, "both sides must have one type");
        lr
    }

    fn binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr, span: Span) -> Ty {
        let l = self.expr(lhs);
        let r = self.expr(rhs);
        match op {
            BinOp::And | BinOp::Or => {
                self.expect(
                    &Ty::con("Bool"),
                    &l,
                    lhs.span,
                    "a logical operator takes `Bool`",
                );
                self.expect(
                    &Ty::con("Bool"),
                    &r,
                    rhs.span,
                    "a logical operator takes `Bool`",
                );
                Ty::con("Bool")
            }
            BinOp::Eq | BinOp::Ne => {
                self.expect(
                    &l,
                    &r,
                    rhs.span,
                    "both sides of a comparison must have one type",
                );
                Ty::con("Bool")
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let t = self.numeric_operands(&l, lhs, &r, rhs);
                self.require_one_of(&t, ORDERED, span, "this type has no ordering");
                Ty::con("Bool")
            }
            _ => {
                let t = self.numeric_operands(&l, lhs, &r, rhs);
                self.require_one_of(&t, NUMERIC, span, "arithmetic applies to numbers");
                t
            }
        }
    }

    /// Report when a resolved type is not one of `allowed`. Unresolved types
    /// are left alone: an inference variable is not yet wrong.
    fn require_one_of(&mut self, ty: &Ty, allowed: &[&str], span: Span, note: &str) {
        let resolved = self.table.resolve(ty);
        // Arithmetic on a distinct type over `Int` is arithmetic on `Int`.
        // Distinctness exists to stop different domains mixing, not to stop a
        // domain doing its own arithmetic.
        let underlying = self.base(&resolved);
        let Ty::Con(name, args) = &underlying else {
            return;
        };
        if !args.is_empty() || !allowed.contains(&name.as_str()) {
            self.diagnostics.push(
                Diagnostic::error(
                    Code::TypeMismatch,
                    span,
                    format!("`{resolved}` is not one of {}", allowed.join(", ")),
                )
                .with_note(note.to_owned()),
            );
        }
    }

    fn field(&mut self, base: &Ty, field: &str, span: Span) -> Ty {
        // Borrows are transparent for field access.
        let mut resolved = self.table.shallow(base);
        while let Ty::Ref { inner, .. } = resolved {
            resolved = self.table.shallow(&inner);
        }
        let Ty::Con(name, args) = &resolved else {
            return Ty::Error;
        };
        let Some(record) = self.records.get(name).cloned() else {
            return Ty::Error;
        };
        let map: BTreeMap<String, Ty> = record
            .generics
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();

        match record.fields.iter().find(|(n, _)| n == field) {
            Some((_, ty)) => substitute(ty, &map),
            None => {
                let names = record.field_names();
                self.diagnostics.push(
                    Diagnostic::error(
                        Code::UnknownField,
                        span,
                        format!("`{name}` has no field `{field}`"),
                    )
                    .with_scope(names),
                );
                Ty::Error
            }
        }
    }

    fn record_lit(
        &mut self,
        name: &vise_ast::Ident,
        inits: &[vise_ast::FieldInit],
        span: Span,
    ) -> Ty {
        let Some(record) = self.records.get(&name.name).cloned() else {
            for i in inits {
                self.expr(&i.value);
            }
            return Ty::Error;
        };

        let args = self.instantiate(
            &record.generics,
            &record
                .generics
                .iter()
                .map(|g| Ty::con(g))
                .collect::<Vec<_>>(),
        );
        let map: BTreeMap<String, Ty> = record
            .generics
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();

        for init in inits {
            let found = self.expr(&init.value);
            match record.fields.iter().find(|(n, _)| *n == init.name.name) {
                Some((_, declared)) => {
                    let declared = substitute(declared, &map);
                    self.expect(
                        &declared,
                        &found,
                        init.value.span,
                        &format!("field `{}` of `{}`", init.name.name, name.name),
                    );
                }
                None => {
                    let names = record.field_names();
                    self.diagnostics.push(
                        Diagnostic::error(
                            Code::UnknownField,
                            init.name.span,
                            format!("`{}` has no field `{}`", name.name, init.name.name),
                        )
                        .with_scope(names),
                    );
                }
            }
        }

        let missing: Vec<String> = record
            .field_names()
            .into_iter()
            .filter(|n| !inits.iter().any(|i| i.name.name == *n))
            .collect();
        if !missing.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(
                    Code::TypeMismatch,
                    span,
                    format!(
                        "`{}` is missing field `{}`",
                        name.name,
                        missing.join("`, `")
                    ),
                )
                .with_note("every field must be given; Vise has no partial construction"),
            );
        }

        Ty::Con(name.name.clone(), args)
    }

    fn match_expr(&mut self, scrutinee: &Expr, arms: &[MatchArm]) -> Ty {
        let subject = self.expr(scrutinee);
        let result = self.table.fresh();
        for arm in arms {
            self.push();
            self.pattern(&arm.pattern, &subject);
            let body = self.expr(&arm.body);
            self.expect(
                &result,
                &body,
                arm.body.span,
                "every match arm must have one type",
            );
            self.pop();
        }
        result
    }

    fn pattern(&mut self, pattern: &Pattern, subject: &Ty) {
        match &pattern.kind {
            PatternKind::Wildcard => {}
            PatternKind::Binding(ident) => self.define(&ident.name, subject.clone()),
            PatternKind::Literal(lit) => {
                let t = self.literal(lit);
                self.expect(
                    subject,
                    &t,
                    pattern.span,
                    "the pattern must match the value's type",
                );
            }
            PatternKind::Variant { path, fields } => {
                let Some(name) = path.as_single() else { return };
                let Some(ctor) = self.ctors.get(&name.name).cloned() else {
                    return; // already V0201
                };
                let mut proto = ctor.fields.clone();
                proto.extend(ctor.generics.iter().map(|g| Ty::con(g)));
                let fresh = self.instantiate(&ctor.generics, &proto);
                let (field_tys, params) = fresh.split_at(ctor.fields.len());

                let built = Ty::Con(ctor.enum_name.clone(), params.to_vec());
                self.expect(
                    subject,
                    &built,
                    pattern.span,
                    "the pattern must match the value's type",
                );
                for (sub, ty) in fields.iter().zip(field_tys) {
                    self.pattern(sub, ty);
                }
            }
        }
    }
}

fn is_numeric_literal(e: &Expr) -> bool {
    matches!(
        &e.kind,
        ExprKind::Literal(Literal::Int(_) | Literal::Float(_))
    )
}

/// Replace named type constructors, used to instantiate generics.
fn substitute(ty: &Ty, map: &BTreeMap<String, Ty>) -> Ty {
    match ty {
        Ty::Con(name, args) if args.is_empty() => {
            map.get(name).cloned().unwrap_or_else(|| ty.clone())
        }
        Ty::Con(name, args) => Ty::Con(
            name.clone(),
            args.iter().map(|a| substitute(a, map)).collect(),
        ),
        Ty::Ref { is_mut, inner } => Ty::Ref {
            is_mut: *is_mut,
            inner: Box::new(substitute(inner, map)),
        },
        other => other.clone(),
    }
}
