//! Discarded `Result` values.
//!
//! Spec §8: Vise has no exceptions, so an ignored `Result` is a silently
//! dropped failure. Handle it, propagate it with `?`, or discard it
//! deliberately with `let _ = ...`.
//!
//! # Scope
//!
//! Without type inference this can only see calls to functions declared in this
//! module, whose return type is written down. An imported function is opaque,
//! and a `Result` reaching a discard position through a variable or a method is
//! invisible here. Every diagnostic it does produce names a call that provably
//! returns a `Result` and provably has its value thrown away.

use std::collections::BTreeMap;

use vise_ast::{
    Block, Expr, ExprKind, FnDecl, ItemKind, Literal, Module, Stmt, StmtKind, StrPart, Type,
    TypeKind,
};
use vise_diag::{Code, Confidence, Diagnostic, Fix, FixKind, Span};

/// Check every discard position in `module`.
#[must_use]
pub fn check(module: &Module) -> Vec<Diagnostic> {
    let returns_result: BTreeMap<String, bool> = module
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::Fn(f) => Some((f.name.name.clone(), f.ret.as_ref().is_some_and(is_result))),
            _ => None,
        })
        .collect();

    let mut w = Walker {
        returns_result,
        in_result_fn: false,
        diagnostics: Vec::new(),
    };

    for item in &module.items {
        if let ItemKind::Fn(f) = &item.kind {
            w.function(f);
        }
    }
    w.diagnostics
}

fn is_result(ty: &Type) -> bool {
    matches!(&ty.kind, TypeKind::Named { name, .. } if name.name == "Result")
}

struct Walker {
    returns_result: BTreeMap<String, bool>,
    /// Whether the function being walked returns `Result`, which decides
    /// whether `?` is a legal suggestion.
    in_result_fn: bool,
    diagnostics: Vec<Diagnostic>,
}

impl Walker {
    fn function(&mut self, f: &FnDecl) {
        self.in_result_fn = f.ret.as_ref().is_some_and(is_result);
        // A function with no `->` returns Unit, so its tail value is thrown
        // away like any other statement.
        let tail_used = f.ret.is_some();
        self.block(&f.body, tail_used);
    }

    fn block(&mut self, b: &Block, tail_used: bool) {
        for s in &b.stmts {
            self.stmt(s);
        }
        if let Some(t) = &b.tail {
            if !tail_used {
                self.discarded(t);
            }
            self.walk(t);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let { value, .. } => self.walk(value),
            StmtKind::Assign { target, value } => {
                self.walk(target);
                self.walk(value);
            }
            StmtKind::For { iter, body, .. } => {
                self.walk(iter);
                // A loop body's value goes nowhere.
                self.block(body, false);
            }
            StmtKind::While { cond, body } => {
                self.walk(cond);
                self.block(body, false);
            }
            StmtKind::Expr(e) => {
                self.discarded(e);
                self.walk(e);
            }
        }
    }

    /// `e`'s value is thrown away. Report it if it is provably a `Result`, and
    /// descend through constructs that pass the discard along.
    fn discarded(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Call { callee, .. } => {
                if let ExprKind::Path(p) = &callee.kind
                    && let Some(name) = p.as_single()
                    && self.returns_result.get(&name.name) == Some(&true)
                {
                    self.report(e.span, &name.name);
                }
            }
            // `if` and `match` in statement position discard every branch.
            ExprKind::If {
                then, otherwise, ..
            } => {
                self.block(then, false);
                self.discarded(otherwise);
            }
            ExprKind::Match { arms, .. } => {
                for a in arms {
                    self.discarded(&a.body);
                }
            }
            ExprKind::Block(b) => self.block(b, false),
            // `?` consumes the Result, which is the point of it.
            _ => {}
        }
    }

    fn report(&mut self, span: Span, callee: &str) {
        let mut d = Diagnostic::error(
            Code::UnusedResult,
            span,
            format!("the `Result` from `{callee}` is ignored"),
        )
        .with_note("Vise has no exceptions, so an ignored `Result` is a silently dropped failure");

        // `?` only type-checks inside a function that itself returns `Result`.
        if self.in_result_fn {
            d = d.with_fix(
                Fix::new(FixKind::HandleResult, "?")
                    .at(Span::new(span.file, span.end, span.end))
                    .confidence(Confidence::Possible),
            );
        }
        d = d.with_fix(
            Fix::new(FixKind::DiscardResult, "let _ = ")
                .at(Span::new(span.file, span.start, span.start))
                .confidence(Confidence::Possible),
        );
        self.diagnostics.push(d);
    }

    /// Ordinary traversal: everything here is in value position.
    fn walk(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Literal(Literal::Str(parts)) => {
                for p in parts {
                    if let StrPart::Interpolation(inner) = p {
                        self.walk(inner);
                    }
                }
            }
            ExprKind::Literal(_) | ExprKind::Path(_) | ExprKind::Break | ExprKind::Continue => {}
            ExprKind::Call { callee, args } => {
                self.walk(callee);
                for a in args {
                    self.walk(a);
                }
            }
            ExprKind::MethodCall { receiver, args, .. } => {
                self.walk(receiver);
                for a in args {
                    self.walk(a);
                }
            }
            ExprKind::Field { base, .. } => self.walk(base),
            ExprKind::Index { base, index } => {
                self.walk(base);
                self.walk(index);
            }
            ExprKind::Unary { operand, .. } | ExprKind::Borrow { operand, .. } => {
                self.walk(operand);
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.walk(lhs);
                self.walk(rhs);
            }
            ExprKind::Try(inner) => self.walk(inner),
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.walk(cond);
                self.block(then, true);
                self.walk(otherwise);
            }
            ExprKind::Match { scrutinee, arms } => {
                self.walk(scrutinee);
                for a in arms {
                    self.walk(&a.body);
                }
            }
            ExprKind::Block(b) => self.block(b, true),
            ExprKind::RecordLit { fields, .. } => {
                for f in fields {
                    self.walk(&f.value);
                }
            }
            ExprKind::ListLit(items) => {
                for i in items {
                    self.walk(i);
                }
            }
            ExprKind::Return(v) => {
                if let Some(v) = v {
                    self.walk(v);
                }
            }
        }
    }
}
