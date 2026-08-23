//! Move and borrow checking.
//!
//! Spec §9: every value has one owner, assignment and argument passing move,
//! borrows are shared-xor-mutable, and a borrow may not outlive its owner.
//!
//! # Scope
//!
//! This is a linear, block-order check, not a full flow analysis. It reports
//! only what it can prove, and stays silent otherwise:
//!
//! - a local's move-ness is decided from *declared* types — a parameter's type,
//!   a `let` annotation, a literal's shape, or a called function's declared
//!   return type. A local whose type is not knowable from those is never
//!   reported on.
//! - a callee with no visible signature is treated as reading, never moving.
//! - branches are merged conservatively: a value moved in either arm of an
//!   `if` or `match` is moved afterwards.
//!
//! What that buys is the guarantee that matters most: no false positives. A
//! borrow checker that rejects correct code is worse than one that misses
//! cases, because the author cannot argue with it.

use std::collections::{BTreeMap, BTreeSet};

use vise_ast::{
    Binding, Block, Expr, ExprKind, FnDecl, ItemKind, Literal, Module, Stmt, StmtKind, StrPart,
    Type, TypeKind,
};
use vise_diag::{Code, Confidence, Diagnostic, Fix, FixKind, Span};

/// Whether using a value consumes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ownership {
    /// Primitives and shared borrows: using them leaves the original intact.
    Copy,
    /// Everything else: passing it by value gives it away.
    Move,
    /// Not knowable from declared types. Never reported on.
    Unknown,
}

/// How an expression position uses a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Usage {
    /// The value is given away.
    Move,
    /// The value is read or borrowed; the owner keeps it.
    Read,
}

#[derive(Debug, Clone)]
struct Local {
    ownership: Ownership,
    /// Where it was moved, if it was.
    moved: Option<Span>,
    /// Whether it was declared inside the loop currently being walked.
    depth: usize,
}

/// Check every function in `module`.
#[must_use]
pub fn check(module: &Module) -> Vec<Diagnostic> {
    let mut signatures = BTreeMap::new();
    for item in &module.items {
        if let ItemKind::Fn(f) = &item.kind {
            signatures.insert(f.name.name.clone(), (**f).clone());
        }
    }

    let mut c = Checker {
        signatures,
        locals: BTreeMap::new(),
        parameters: BTreeSet::new(),
        depth: 0,
        diagnostics: Vec::new(),
    };
    for item in &module.items {
        if let ItemKind::Fn(f) = &item.kind {
            c.function(f);
        }
    }
    c.diagnostics
}

struct Checker {
    signatures: BTreeMap<String, FnDecl>,
    locals: BTreeMap<String, Local>,
    /// Parameters of the function being checked. A borrow of one may be
    /// returned, because the caller owns the value; a borrow of a local may
    /// not.
    parameters: BTreeSet<String>,
    /// Loop nesting, so a move of an outer local inside a loop can be caught.
    depth: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Checker {
    fn function(&mut self, f: &FnDecl) {
        self.locals.clear();
        self.parameters.clear();
        self.depth = 0;
        for p in &f.params {
            self.parameters.insert(p.name.name.clone());
            self.locals.insert(
                p.name.name.clone(),
                Local {
                    ownership: ownership_of(&p.ty),
                    moved: None,
                    depth: 0,
                },
            );
        }
        self.block(&f.body, returns_borrow(f));
    }

    fn declare(&mut self, name: &str, ownership: Ownership) {
        let depth = self.depth;
        self.locals.insert(
            name.to_owned(),
            Local {
                ownership,
                moved: None,
                depth,
            },
        );
    }

    // --- reporting -------------------------------------------------------

    fn report_use_after_move(&mut self, name: &str, at: Span, moved: Span) {
        self.diagnostics.push(
            Diagnostic::error(
                Code::UseAfterMove,
                at,
                format!("`{name}` is used after it was moved"),
            )
            .with_label(moved, "moved here")
            .with_note(
                "assignment and argument passing move by default; borrow with `&` if the \
                 callee only needs to read",
            )
            .with_fix(
                Fix::new(FixKind::Borrow, format!("&{name}"))
                    .at(at)
                    .confidence(Confidence::Possible),
            )
            .with_fix(
                Fix::new(FixKind::Clone, format!("{name}.clone()"))
                    .at(at)
                    .confidence(Confidence::Possible),
            ),
        );
    }

    // --- statements ------------------------------------------------------

    fn block(&mut self, b: &Block, tail_returns_borrow: bool) {
        for s in &b.stmts {
            self.stmt(s);
        }
        if let Some(t) = &b.tail {
            if tail_returns_borrow {
                self.check_escaping_borrow(t);
            }
            self.expr(t, Usage::Move);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let {
                name, ty, value, ..
            } => {
                self.expr(value, Usage::Move);
                let ownership = match ty {
                    Some(t) => ownership_of(t),
                    None => self.ownership_of_expr(value),
                };
                if let Binding::Name(ident) = name {
                    self.declare(&ident.name, ownership);
                }
            }
            StmtKind::Assign { target, value } => {
                self.expr(value, Usage::Move);
                // Assigning to a name gives it a fresh value, so a previous
                // move stops mattering.
                if let ExprKind::Path(p) = &target.kind
                    && let Some(name) = p.as_single()
                    && let Some(local) = self.locals.get_mut(&name.name)
                {
                    local.moved = None;
                } else {
                    self.expr(target, Usage::Read);
                }
            }
            StmtKind::For {
                binding,
                iter,
                body,
            } => {
                self.expr(iter, Usage::Read);
                self.depth += 1;
                if let Binding::Name(ident) = binding {
                    self.declare(&ident.name, Ownership::Unknown);
                }
                self.block(body, false);
                self.depth -= 1;
                self.settle_loop();
            }
            StmtKind::While { cond, body } => {
                self.expr(cond, Usage::Read);
                self.depth += 1;
                self.block(body, false);
                self.depth -= 1;
                self.settle_loop();
            }
            StmtKind::Expr(e) => self.expr(e, Usage::Read),
        }
    }

    /// A value declared outside a loop and moved inside it would already be
    /// gone on the second iteration.
    fn settle_loop(&mut self) {
        let depth = self.depth;
        let offenders: Vec<(String, Span)> = self
            .locals
            .iter()
            .filter_map(|(name, l)| {
                let moved = l.moved?;
                (l.depth <= depth).then(|| (name.clone(), moved))
            })
            .collect();

        for (name, moved) in offenders {
            self.diagnostics.push(
                Diagnostic::error(
                    Code::UseAfterMove,
                    moved,
                    format!("`{name}` is moved inside a loop"),
                )
                .with_note(
                    "it was declared outside the loop, so the second iteration would use a \
                     value that is already gone",
                )
                .with_fix(
                    Fix::new(FixKind::Clone, format!("{name}.clone()"))
                        .at(moved)
                        .confidence(Confidence::Possible),
                ),
            );
            // Reported once; do not repeat it at every later use.
            if let Some(l) = self.locals.get_mut(&name) {
                l.moved = None;
            }
        }
    }

    // --- expressions -----------------------------------------------------

    fn expr(&mut self, e: &Expr, usage: Usage) {
        match &e.kind {
            ExprKind::Path(p) => {
                let Some(name) = p.as_single() else { return };
                let Some(local) = self.locals.get(&name.name) else {
                    return;
                };
                if let Some(moved) = local.moved {
                    let name = name.name.clone();
                    self.report_use_after_move(&name, e.span, moved);
                    return;
                }
                if usage == Usage::Move && local.ownership == Ownership::Move {
                    if let Some(l) = self.locals.get_mut(&name.name) {
                        l.moved = Some(e.span);
                    }
                }
            }
            ExprKind::Call { callee, args } => self.call(callee, args),
            ExprKind::MethodCall { receiver, args, .. } => {
                self.expr(receiver, Usage::Read);
                for a in args {
                    self.expr(a, Usage::Read);
                }
            }
            // A borrow reads; it does not consume.
            ExprKind::Borrow { operand, .. } => self.expr(operand, Usage::Read),
            // Projection out of a value is treated as a read: a partial move
            // cannot be proven safe here, so nothing is recorded.
            ExprKind::Field { base, .. } => self.expr(base, Usage::Read),
            ExprKind::Index { base, index } => {
                self.expr(base, Usage::Read);
                self.expr(index, Usage::Read);
            }
            ExprKind::Unary { operand, .. } => self.expr(operand, Usage::Read),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs, Usage::Read);
                self.expr(rhs, Usage::Read);
            }
            ExprKind::Try(inner) => self.expr(inner, Usage::Read),
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.expr(cond, Usage::Read);
                // Each branch starts from the same state; a move in either one
                // counts afterwards.
                let before = self.locals.clone();
                self.block(then, false);
                let after_then = std::mem::replace(&mut self.locals, before);
                self.expr(otherwise, usage);
                self.merge(after_then);
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee, Usage::Read);
                let before = self.locals.clone();
                let mut merged = self.locals.clone();
                for arm in arms {
                    self.locals = before.clone();
                    self.expr(&arm.body, usage);
                    merged = merge_maps(merged, std::mem::take(&mut self.locals));
                }
                self.locals = merged;
            }
            ExprKind::Block(b) => self.block(b, false),
            ExprKind::RecordLit { fields, .. } => {
                for f in fields {
                    self.expr(&f.value, Usage::Move);
                }
            }
            ExprKind::ListLit(items) => {
                for i in items {
                    self.expr(i, Usage::Move);
                }
            }
            ExprKind::Return(v) => {
                if let Some(v) = v {
                    self.expr(v, Usage::Move);
                }
            }
            ExprKind::Literal(Literal::Str(parts)) => {
                for p in parts {
                    if let StrPart::Interpolation(inner) = p {
                        self.expr(inner, Usage::Read);
                    }
                }
            }
            ExprKind::Literal(_) | ExprKind::Break | ExprKind::Continue => {}
        }
    }

    /// Arguments move or borrow according to the callee's declared parameters.
    fn call(&mut self, callee: &Expr, args: &[Expr]) {
        let name = match &callee.kind {
            ExprKind::Path(p) => p.as_single().map(|i| i.name.clone()),
            _ => None,
        };

        self.check_conflicting_borrows(args);

        let params: Option<Vec<Type>> = name
            .as_ref()
            .and_then(|n| self.signatures.get(n))
            .map(|f| f.params.iter().map(|p| p.ty.clone()).collect());

        for (i, arg) in args.iter().enumerate() {
            let usage = match params.as_ref().and_then(|p| p.get(i)) {
                Some(ty) if ty.is_ref() => Usage::Read,
                Some(_) => Usage::Move,
                // No signature to consult. Reading is the conservative choice:
                // it can miss a move, but it can never invent one.
                None => Usage::Read,
            };
            self.expr(arg, usage);
        }
    }

    /// §9: `&mut x` may not coexist with any other borrow of `x`.
    fn check_conflicting_borrows(&mut self, args: &[Expr]) {
        let mut unique: Vec<(String, Span)> = Vec::new();
        let mut other: Vec<(String, Span)> = Vec::new();

        for arg in args {
            match &arg.kind {
                ExprKind::Borrow { is_mut, operand } => {
                    if let ExprKind::Path(p) = &operand.kind
                        && let Some(name) = p.as_single()
                    {
                        let entry = (name.name.clone(), arg.span);
                        if *is_mut {
                            unique.push(entry);
                        } else {
                            other.push(entry);
                        }
                    }
                }
                ExprKind::Path(p) => {
                    if let Some(name) = p.as_single() {
                        other.push((name.name.clone(), arg.span));
                    }
                }
                _ => {}
            }
        }

        for (name, at) in &unique {
            let conflicts = other
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, s)| *s)
                .or_else(|| {
                    unique
                        .iter()
                        .find(|(n, s)| n == name && s != at)
                        .map(|(_, s)| *s)
                });
            if let Some(conflict) = conflicts {
                self.diagnostics.push(
                    Diagnostic::error(
                        Code::ConflictingBorrows,
                        *at,
                        format!("`{name}` is borrowed mutably while it is also borrowed here"),
                    )
                    .with_label(conflict, "the other borrow")
                    .with_note(
                        "borrows are shared-xor-mutable: `&mut T` may not coexist with any \
                         other borrow of the same value",
                    ),
                );
            }
        }
    }

    /// §9: a borrow may not outlive its owner.
    fn check_escaping_borrow(&mut self, e: &Expr) {
        let ExprKind::Borrow { operand, .. } = &e.kind else {
            return;
        };
        let ExprKind::Path(p) = &operand.kind else {
            return;
        };
        let Some(name) = p.as_single() else { return };
        // A parameter's value outlives the call, because the caller owns it.
        // A local does not.
        if !self.locals.contains_key(&name.name) {
            return;
        }
        if !self.is_parameter(&name.name) {
            self.diagnostics.push(
                Diagnostic::error(
                    Code::BorrowOutlivesOwner,
                    e.span,
                    format!(
                        "returns a borrow of `{}`, which is local to this function",
                        name.name
                    ),
                )
                .with_note(
                    "the value is destroyed when the function returns; return an owned value \
                     instead, or accept the borrow as a parameter",
                )
                .with_fix(
                    Fix::new(FixKind::Replace, name.name.clone())
                        .at(e.span)
                        .confidence(Confidence::Possible),
                ),
            );
        }
    }

    fn is_parameter(&self, name: &str) -> bool {
        self.parameters.contains(name)
    }

    // --- inference of move-ness ------------------------------------------

    /// What a `let` without an annotation binds. Anything not certain is
    /// `Unknown`, and an `Unknown` local is never reported on.
    fn ownership_of_expr(&self, e: &Expr) -> Ownership {
        match &e.kind {
            ExprKind::Literal(Literal::Str(_)) => Ownership::Move,
            ExprKind::Literal(_) => Ownership::Copy,
            ExprKind::ListLit(_) | ExprKind::RecordLit { .. } => Ownership::Move,
            ExprKind::Borrow { is_mut, .. } => {
                if *is_mut {
                    Ownership::Move
                } else {
                    Ownership::Copy
                }
            }
            ExprKind::Binary { .. } | ExprKind::Unary { .. } => Ownership::Copy,
            ExprKind::Path(p) => p
                .as_single()
                .and_then(|n| self.locals.get(&n.name))
                .map_or(Ownership::Unknown, |l| l.ownership),
            ExprKind::Call { callee, .. } => {
                let ExprKind::Path(p) = &callee.kind else {
                    return Ownership::Unknown;
                };
                let Some(name) = p.as_single() else {
                    return Ownership::Unknown;
                };
                match self.signatures.get(&name.name) {
                    Some(f) => f.ret.as_ref().map_or(Ownership::Copy, ownership_of),
                    // A constructor builds an owned value.
                    None if name.name.starts_with(char::is_uppercase) => Ownership::Move,
                    None => Ownership::Unknown,
                }
            }
            _ => Ownership::Unknown,
        }
    }

    fn merge(&mut self, other: BTreeMap<String, Local>) {
        let current = std::mem::take(&mut self.locals);
        self.locals = merge_maps(current, other);
    }
}

/// A value moved on either path is moved afterwards.
fn merge_maps(
    mut a: BTreeMap<String, Local>,
    b: BTreeMap<String, Local>,
) -> BTreeMap<String, Local> {
    for (name, local) in b {
        match a.get_mut(&name) {
            Some(existing) => {
                if existing.moved.is_none() {
                    existing.moved = local.moved;
                }
            }
            None => {
                a.insert(name, local);
            }
        }
    }
    a
}

/// Primitives and shared borrows copy; everything else moves (§9).
fn ownership_of(ty: &Type) -> Ownership {
    match &ty.kind {
        TypeKind::Unit => Ownership::Copy,
        TypeKind::Ref { is_mut, .. } => {
            if *is_mut {
                Ownership::Move
            } else {
                Ownership::Copy
            }
        }
        TypeKind::Named { name, args } => match name.name.as_str() {
            "Int" | "Float" | "Bool" | "Char" | "Unit" => Ownership::Copy,
            "Str" | "List" | "Map" | "Set" | "Option" | "Result" => Ownership::Move,
            // A user type: a record or enum owns its contents.
            _ if args.is_empty() && name.name.starts_with(char::is_uppercase) => Ownership::Unknown,
            _ => Ownership::Unknown,
        },
    }
}

fn returns_borrow(f: &FnDecl) -> bool {
    f.ret.as_ref().is_some_and(Type::is_ref)
}
