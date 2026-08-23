//! Match exhaustiveness.
//!
//! Spec §6: a `match` must cover every constructor, and `V0301` names the ones
//! it misses. `_` is allowed — the rule exists so that *forgetting* a case is
//! impossible, not to ban deliberate catch-alls.
//!
//! # Scope
//!
//! This is a conservative check, not a decision procedure. It descends into a
//! sub-pattern only for single-field constructors — `Ok(x)`, `Some(x)`,
//! `Err(e)` — which is where nesting actually occurs in practice. For a
//! constructor with two or more fields it checks the constructor itself and
//! stops.
//!
//! The bias is deliberate: a missed case is a nuisance, a false positive is a
//! compiler that rejects correct code. Every diagnostic this produces names a
//! constructor that genuinely has no arm.

use std::collections::BTreeMap;

use vise_ast::{
    Block, Expr, ExprKind, ItemKind, Literal, MatchArm, Module, Pattern, PatternKind, Stmt,
    StmtKind, StrPart,
};
use vise_diag::{Code, Confidence, Diagnostic, Fix, FixKind};

/// A constructor: which enum it belongs to, and how many fields it carries.
#[derive(Debug, Clone)]
struct Ctor {
    enum_name: String,
    arity: usize,
}

/// Enums `core` provides, which have no declaration to read.
const BUILTIN_ENUMS: &[(&str, &[(&str, usize)])] = &[
    ("Result", &[("Ok", 1), ("Err", 1)]),
    ("Option", &[("Some", 1), ("None", 0)]),
];

#[derive(Debug, Default)]
struct Enums {
    /// Enum name to its variant names, in declaration order.
    variants: BTreeMap<String, Vec<String>>,
    /// Constructor name to what it constructs.
    ctors: BTreeMap<String, Ctor>,
}

impl Enums {
    fn collect(module: &Module) -> Self {
        let mut e = Self::default();
        for (name, variants) in BUILTIN_ENUMS {
            e.variants.insert(
                (*name).to_owned(),
                variants.iter().map(|(v, _)| (*v).to_owned()).collect(),
            );
            for (v, arity) in *variants {
                e.ctors.insert(
                    (*v).to_owned(),
                    Ctor {
                        enum_name: (*name).to_owned(),
                        arity: *arity,
                    },
                );
            }
        }
        for item in &module.items {
            if let ItemKind::Enum(d) = &item.kind {
                e.variants.insert(
                    d.name.name.clone(),
                    d.variants.iter().map(|v| v.name.name.clone()).collect(),
                );
                for v in &d.variants {
                    e.ctors.insert(
                        v.name.name.clone(),
                        Ctor {
                            enum_name: d.name.name.clone(),
                            arity: v.fields.len(),
                        },
                    );
                }
            }
        }
        e
    }
}

/// Check every `match` in `module`.
#[must_use]
pub fn check(module: &Module) -> Vec<Diagnostic> {
    let enums = Enums::collect(module);
    let mut w = Walker {
        enums,
        diagnostics: Vec::new(),
    };
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(f) => w.block(&f.body),
            ItemKind::Record(d) => {
                for e in &d.invariants {
                    w.expr(e);
                }
            }
            _ => {}
        }
    }
    w.diagnostics
}

struct Walker {
    enums: Enums,
    diagnostics: Vec<Diagnostic>,
}

impl Walker {
    fn check_match(&mut self, expr: &Expr, arms: &[MatchArm]) {
        let patterns: Vec<&Pattern> = arms.iter().map(|a| &a.pattern).collect();
        let missing = self.uncovered(&patterns);
        if missing.is_empty() {
            return;
        }

        let list = missing.join("`, `");
        let arms_text = missing
            .iter()
            .map(|m| format!("{m} -> ..."))
            .collect::<Vec<_>>()
            .join("\n");

        self.diagnostics.push(
            Diagnostic::error(
                Code::NonExhaustiveMatch,
                expr.span,
                format!("match does not cover `{list}`"),
            )
            .with_note("add the missing arms, or `_` if a catch-all is intended")
            .with_fix(Fix::new(FixKind::AddMatchArm, arms_text).confidence(Confidence::Likely)),
        );
    }

    /// Constructors with no arm. Empty means "nothing provably missing".
    fn uncovered(&self, patterns: &[&Pattern]) -> Vec<String> {
        // A wildcard or a binding matches everything.
        if patterns
            .iter()
            .any(|p| matches!(p.kind, PatternKind::Wildcard | PatternKind::Binding(_)))
        {
            return Vec::new();
        }

        // Literal patterns cannot be proven to cover a primitive type, so ask
        // for a catch-all rather than guessing at ranges.
        if patterns
            .iter()
            .any(|p| matches!(p.kind, PatternKind::Literal(_)))
        {
            return vec!["_".to_owned()];
        }

        // Everything left is a constructor. Find the enum they belong to.
        let mut covered: BTreeMap<&str, Vec<&Pattern>> = BTreeMap::new();
        let mut enum_name: Option<&str> = None;
        for p in patterns {
            let PatternKind::Variant { path, fields } = &p.kind else {
                return Vec::new();
            };
            let Some(name) = path.as_single() else {
                return Vec::new();
            };
            let Some(ctor) = self.enums.ctors.get(&name.name) else {
                // An unknown constructor is already `V0201`; do not pile on.
                return Vec::new();
            };
            if enum_name.is_some_and(|e| e != ctor.enum_name) {
                return Vec::new(); // mixed enums: let the type checker speak
            }
            enum_name = Some(&ctor.enum_name);
            covered
                .entry(name.name.as_str())
                .or_default()
                .extend(fields.iter());
        }

        let Some(enum_name) = enum_name else {
            return Vec::new();
        };
        let Some(all) = self.enums.variants.get(enum_name) else {
            return Vec::new();
        };

        let mut missing: Vec<String> = all
            .iter()
            .filter(|v| !covered.contains_key(v.as_str()))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return missing;
        }

        // Every constructor has an arm. Descend into single-field ones, which
        // is where nesting actually happens.
        for (ctor_name, subs) in &covered {
            let Some(ctor) = self.enums.ctors.get(*ctor_name) else {
                continue;
            };
            if ctor.arity != 1 {
                continue;
            }
            // Arity is 1, so each arm for this constructor contributed
            // exactly one sub-pattern.
            for m in self.uncovered(subs) {
                missing.push(format!("{ctor_name}({m})"));
            }
        }
        missing
    }

    // --- traversal -------------------------------------------------------

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
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                self.check_match(e, arms);
                for a in arms {
                    self.expr(&a.body);
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
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::MethodCall { receiver, args, .. } => {
                self.expr(receiver);
                for a in args {
                    self.expr(a);
                }
            }
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
