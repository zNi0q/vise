//! Effect inference and checking.
//!
//! Spec §7: effects are inferred bottom-up, and a *declared* row is exact — it
//! is neither a lower nor an upper bound. Omitting a row means "whatever the
//! body implies"; writing `!{}` asserts purity.
//!
//! A declared row is treated as the function's interface, so callers propagate
//! what the signature says rather than re-deriving it from the body. That is
//! what makes a signature worth reading.
//!
//! # What cannot be checked yet
//!
//! An imported function's effects are unknown: Vise has no module system, so
//! `use std/http@1:{post}` brings in a name with no signature. A function that
//! calls one therefore has an unknown component, and `V0402` — "declared but
//! never used" — is suppressed for it, because absence of proof is not proof
//! of absence. `V0401` still applies to effects that *are* known. See open
//! spec issue 7 in `TRACKER.md`.

use std::collections::{BTreeMap, BTreeSet};

use vise_ast::{
    Block, Effect, Expr, ExprKind, FnDecl, ItemKind, Literal, Module, Stmt, StmtKind, StrPart,
};
use vise_diag::{Code, Confidence, Diagnostic, Fix, FixKind, Span};

/// Where an effect entered a function.
#[derive(Debug, Clone)]
struct Origin {
    /// The call that introduced it.
    span: Span,
    /// What was called.
    callee: String,
}

type Origins = BTreeMap<Effect, Origin>;

/// Check every declared effect row in `module`.
#[must_use]
pub fn check(module: &Module) -> Vec<Diagnostic> {
    let fns: Vec<&FnDecl> = module
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::Fn(f) => Some(&**f),
            _ => None,
        })
        .collect();

    let index: BTreeMap<&str, usize> = fns
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.name.as_str(), i))
        .collect();

    // Direct effects and outgoing calls, per function.
    let mut origins: Vec<Origins> = Vec::with_capacity(fns.len());
    let mut calls: Vec<Vec<(String, Span)>> = Vec::with_capacity(fns.len());
    let mut unknown: Vec<bool> = Vec::with_capacity(fns.len());

    for f in &fns {
        let mut collector = Calls::default();
        collector.block(&f.body);
        let mut own = Origins::new();
        let mut has_unknown = false;

        for (callee, span) in &collector.calls {
            if let Some(effect) = builtin_effect(callee) {
                own.entry(effect).or_insert_with(|| Origin {
                    span: *span,
                    callee: callee.clone(),
                });
            } else if !index.contains_key(callee.as_str()) {
                // Imported, or a local binding used as a value. Either way its
                // effects are not knowable from this file.
                has_unknown = true;
            }
        }

        origins.push(own);
        calls.push(collector.calls);
        unknown.push(has_unknown);
    }

    // Effects only grow, and there are seven of them, so this terminates.
    loop {
        let mut changed = false;
        for i in 0..fns.len() {
            for (callee, span) in calls[i].clone() {
                let Some(&j) = index.get(callee.as_str()) else {
                    continue;
                };
                for effect in interface(fns[j], &origins[j]) {
                    if let std::collections::btree_map::Entry::Vacant(slot) =
                        origins[i].entry(effect)
                    {
                        slot.insert(Origin {
                            span,
                            callee: callee.clone(),
                        });
                        changed = true;
                    }
                }
                // Unknown propagates only through functions that did not
                // declare a row; a declared row is authoritative.
                if fns[j].effects.is_none() && unknown[j] && !unknown[i] {
                    unknown[i] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    let mut diagnostics = Vec::new();
    for (i, f) in fns.iter().enumerate() {
        let Some(row) = &f.effects else {
            continue; // omitted rows are inferred, never wrong
        };
        let declared: BTreeSet<Effect> = row.effects.iter().copied().collect();
        let inferred: BTreeSet<Effect> = origins[i].keys().copied().collect();

        for effect in inferred.difference(&declared) {
            let origin = &origins[i][effect];
            let mut widened: Vec<Effect> = declared.iter().copied().collect();
            widened.push(*effect);
            widened.sort_unstable();

            diagnostics.push(
                Diagnostic::error(
                    Code::UndeclaredEffect,
                    row.span,
                    format!(
                        "call introduces effect `{}`, not declared by `{}`",
                        effect.as_str(),
                        f.name
                    ),
                )
                .with_label(origin.span, format!("`{}` performs it here", origin.callee))
                .with_fix(
                    Fix::new(FixKind::AddEffect, render_row(&widened))
                        .at(row.span)
                        .confidence(Confidence::Certain),
                ),
            );
        }

        // §7 exempts `main`, which may declare effects it merely passes through.
        if unknown[i] || f.name.name == "main" {
            continue;
        }
        for effect in declared.difference(&inferred) {
            let mut narrowed: Vec<Effect> = declared.iter().copied().collect();
            narrowed.retain(|e| e != effect);

            diagnostics.push(
                Diagnostic::warning(
                    Code::UnusedDeclaredEffect,
                    row.span,
                    format!(
                        "`{}` declares effect `{}` but never performs it",
                        f.name,
                        effect.as_str()
                    ),
                )
                .with_note(
                    "an effect row is exact rather than an upper bound, so it stays honest as \
                     code changes",
                )
                .with_fix(
                    Fix::new(FixKind::RemoveEffect, render_row(&narrowed))
                        .at(row.span)
                        .confidence(Confidence::Certain),
                ),
            );
        }
    }

    diagnostics
}

/// The effects a caller should assume: the declared row when there is one,
/// otherwise what was inferred.
fn interface(f: &FnDecl, inferred: &Origins) -> Vec<Effect> {
    f.effects
        .as_ref()
        .map_or_else(|| inferred.keys().copied().collect(), |r| r.effects.clone())
}

/// The effect a `core` function performs, from the one table all stages read.
fn builtin_effect(name: &str) -> Option<Effect> {
    crate::builtins::find(name).and_then(|b| b.effect)
}

fn render_row(effects: &[Effect]) -> String {
    let names: Vec<&str> = effects.iter().map(|e| e.as_str()).collect();
    format!("!{{{}}}", names.join(", "))
}

/// Collects every call in a function body.
#[derive(Debug, Default)]
struct Calls {
    calls: Vec<(String, Span)>,
}

impl Calls {
    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            self.stmt(s);
        }
        if let Some(t) = &b.tail {
            self.expr(t);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let { value, .. } => self.expr(value),
            StmtKind::Assign { target, value } => {
                self.expr(target);
                self.expr(value);
            }
            StmtKind::For { iter, body, .. } => {
                self.expr(iter);
                self.block(body);
            }
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            StmtKind::Expr(e) => self.expr(e),
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Call { callee, args } => {
                if let ExprKind::Path(p) = &callee.kind
                    && let Some(name) = p.as_single()
                {
                    self.calls.push((name.name.clone(), e.span));
                } else {
                    self.expr(callee);
                }
                for a in args {
                    self.expr(a);
                }
            }
            // Methods are on core types and are pure in v0; once traits or a
            // module system exist this becomes a real lookup.
            ExprKind::MethodCall { receiver, args, .. } => {
                self.expr(receiver);
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Literal(Literal::Str(parts)) => {
                for p in parts {
                    if let StrPart::Interpolation(inner) = p {
                        self.expr(inner);
                    }
                }
            }
            ExprKind::Literal(_) | ExprKind::Path(_) | ExprKind::Break | ExprKind::Continue => {}
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Unary { operand, .. } | ExprKind::Borrow { operand, .. } => {
                self.expr(operand);
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Try(inner) => self.expr(inner),
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.expr(cond);
                self.block(then);
                self.expr(otherwise);
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                for a in arms {
                    self.expr(&a.body);
                }
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::RecordLit { fields, .. } => {
                for f in fields {
                    self.expr(&f.value);
                }
            }
            ExprKind::ListLit(items) => {
                for i in items {
                    self.expr(i);
                }
            }
            ExprKind::Return(v) => {
                if let Some(v) = v {
                    self.expr(v);
                }
            }
        }
    }
}
