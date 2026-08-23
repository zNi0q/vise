//! Name resolution.
//!
//! Spec §3: every name must be defined in this module or listed in a `use`.
//! There is no glob import and no transitive visibility, so a name that does
//! not resolve is a compile error — which is what turns a hallucinated API into
//! `V0201` instead of a runtime surprise.
//!
//! The diagnostic carries every visible name, so the reader can pick a real one
//! rather than guessing a second time.

use vise_ast::{
    Binding, Block, Effect, Expr, ExprKind, FnDecl, Ident, Item, ItemKind, Literal, Module,
    Pattern, PatternKind, Stmt, StmtKind, StrPart, Type, TypeKind,
};
use vise_diag::{Code, Confidence, Diagnostic, Fix, FixKind, Span};

use crate::prelude::Symbol;
use crate::scope::Scopes;

/// Resolve every name in `module`, returning what did not resolve.
#[must_use]
pub fn resolve(module: &Module) -> Vec<Diagnostic> {
    let mut r = Resolver {
        scopes: Scopes::new(),
        diagnostics: Vec::new(),
    };
    r.module(module);
    r.diagnostics
}

struct Resolver {
    scopes: Scopes,
    diagnostics: Vec<Diagnostic>,
}

impl Resolver {
    // --- reporting -------------------------------------------------------

    /// Report a name that does not resolve, listing what does.
    fn unresolved(&mut self, ident: &Ident, what: &str) {
        let suggestions = self.scopes.suggestions(&ident.name);
        let mut d = Diagnostic::error(
            Code::UnknownName,
            ident.span,
            format!("`{}` is not in scope", ident.name),
        )
        .with_note(format!(
            "{what} must be defined in this module or listed in a `use`; there is no glob import"
        ))
        .with_scope(self.scopes.visible());

        // One near miss is almost certainly the intended name. Several means
        // the compiler should offer rather than decide.
        let confidence = if suggestions.len() == 1 {
            Confidence::Likely
        } else {
            Confidence::Possible
        };
        for candidate in suggestions.iter().take(3) {
            d = d.with_fix(
                Fix::new(FixKind::Replace, candidate.clone())
                    .at(ident.span)
                    .confidence(confidence),
            );
        }
        self.diagnostics.push(d);
    }

    fn declare(&mut self, ident: &Ident, symbol: Symbol) {
        if let Some(previous) = self.scopes.declare(&ident.name, symbol, ident.span) {
            self.diagnostics.push(
                Diagnostic::error(
                    Code::DuplicateDefinition,
                    ident.span,
                    format!("`{}` is defined twice", ident.name),
                )
                .with_label(previous, "first defined here")
                .with_note("one name resolves to exactly one definition; Vise has no overloading"),
            );
        }
    }

    // --- module ----------------------------------------------------------

    fn module(&mut self, module: &Module) {
        self.scopes.push();

        // Imports and items are declared before any body is walked, so order
        // within a module never matters.
        for u in &module.uses {
            for name in &u.names {
                let symbol = if starts_upper(&name.name) {
                    Symbol::Type
                } else {
                    Symbol::Value
                };
                self.declare(name, symbol);
            }
        }

        for item in &module.items {
            match &item.kind {
                ItemKind::Type(d) => self.declare(&d.name, Symbol::Type),
                ItemKind::Record(d) => self.declare(&d.name, Symbol::Type),
                ItemKind::Enum(d) => {
                    self.declare(&d.name, Symbol::Type);
                    for v in &d.variants {
                        self.declare(&v.name, Symbol::Constructor);
                    }
                }
                ItemKind::Fn(d) => self.declare(&d.name, Symbol::Value),
            }
        }

        for item in &module.items {
            self.item(item);
        }

        self.scopes.pop();
    }

    fn item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Type(d) => self.ty(&d.underlying),
            ItemKind::Record(d) => {
                self.scopes.push();
                for g in &d.generics {
                    self.declare(g, Symbol::Generic);
                }
                for f in &d.fields {
                    self.ty(&f.ty);
                }
                // An invariant sees the record's own fields by name.
                self.scopes.push();
                for f in &d.fields {
                    self.declare(&f.name, Symbol::Value);
                }
                for e in &d.invariants {
                    self.expr(e);
                }
                self.scopes.pop();
                self.scopes.pop();
            }
            ItemKind::Enum(d) => {
                self.scopes.push();
                for g in &d.generics {
                    self.declare(g, Symbol::Generic);
                }
                for v in &d.variants {
                    for f in &v.fields {
                        self.ty(&f.ty);
                    }
                }
                self.scopes.pop();
            }
            ItemKind::Fn(d) => self.function(d),
        }
    }

    fn function(&mut self, d: &FnDecl) {
        self.scopes.push();
        for g in &d.generics {
            self.declare(g.name(), Symbol::Generic);
        }
        for p in &d.params {
            self.ty(&p.ty);
        }
        if let Some(ret) = &d.ret {
            self.ty(ret);
        }
        self.effects(d);

        for p in &d.params {
            self.declare(&p.name, Symbol::Value);
        }
        for e in &d.requires {
            self.expr(e);
        }

        // §10: `ensures` speaks about `result`, which exists only there.
        if !d.ensures.is_empty() {
            self.scopes.push();
            let result = Ident::new("result", d.name.span);
            self.scopes
                .declare(&result.name, Symbol::Value, result.span);
            for e in &d.ensures {
                self.expr(e);
            }
            self.scopes.pop();
        }

        self.block(&d.body);
        self.scopes.pop();
    }

    /// Effect names resolve against §7's fixed table, not the scope stack.
    fn effects(&mut self, d: &FnDecl) {
        let Some(row) = &d.effects else { return };
        for name in &row.unknown {
            let known: Vec<String> = Effect::ALL.iter().map(|e| e.as_str().to_owned()).collect();
            let mut diag = Diagnostic::error(
                Code::UnknownName,
                name.span,
                format!("`{}` is not an effect", name.name),
            )
            .with_note(
                "effects are primitive capabilities, never domains; a database client is a \
                 library whose functions carry `!{net}`",
            )
            .with_scope(known.clone());

            if let Some(best) = known
                .iter()
                .map(|k| (crate::scope::edit_distance(&name.name, k), k))
                .filter(|(d, _)| *d <= 2)
                .min()
                .map(|(_, k)| k.clone())
            {
                diag = diag.with_fix(
                    Fix::new(FixKind::Replace, best)
                        .at(name.span)
                        .confidence(Confidence::Possible),
                );
            }
            self.diagnostics.push(diag);
        }
    }

    // --- types -----------------------------------------------------------

    fn ty(&mut self, ty: &Type) {
        match &ty.kind {
            TypeKind::Named { name, args } => {
                if !self.scopes.contains(&name.name) {
                    self.unresolved(name, "a type");
                }
                for a in args {
                    self.ty(a);
                }
            }
            TypeKind::Ref {
                lifetime, inner, ..
            } => {
                if let Some(lt) = lifetime
                    && !self.scopes.contains(&lt.name)
                {
                    self.unresolved(lt, "a lifetime");
                }
                self.ty(inner);
            }
            TypeKind::Unit => {}
        }
    }

    // --- statements ------------------------------------------------------

    fn block(&mut self, block: &Block) {
        self.scopes.push();
        for s in &block.stmts {
            self.stmt(s);
        }
        if let Some(tail) = &block.tail {
            self.expr(tail);
        }
        self.scopes.pop();
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Let {
                name, ty, value, ..
            } => {
                if let Some(t) = ty {
                    self.ty(t);
                }
                // The value is resolved first, so `let x = x` reads the outer
                // `x` rather than itself.
                self.expr(value);
                self.bind(name);
            }
            StmtKind::Assign { target, value } => {
                self.expr(target);
                self.expr(value);
            }
            StmtKind::For {
                binding,
                iter,
                body,
            } => {
                self.expr(iter);
                self.scopes.push();
                self.bind(binding);
                self.block(body);
                self.scopes.pop();
            }
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            StmtKind::Expr(e) => self.expr(e),
        }
    }

    fn bind(&mut self, binding: &Binding) {
        match binding {
            Binding::Name(ident) => self.declare(ident, Symbol::Value),
            // `let _ = ...` introduces nothing, deliberately (§8).
            Binding::Wildcard(_) => {}
        }
    }

    // --- expressions -----------------------------------------------------

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Literal(Literal::Str(parts)) => {
                for p in parts {
                    if let StrPart::Interpolation(e) = p {
                        self.expr(e);
                    }
                }
            }
            ExprKind::Literal(_) | ExprKind::Break | ExprKind::Continue => {}
            ExprKind::Path(path) => {
                if let Some(name) = path.as_single()
                    && !self.scopes.contains(&name.name)
                {
                    self.unresolved(name, "a name");
                }
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
            }
            // The method and field names need types to resolve, so they are
            // left to the type checker.
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
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                for arm in arms {
                    self.scopes.push();
                    self.pattern(&arm.pattern);
                    self.expr(&arm.body);
                    self.scopes.pop();
                }
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::RecordLit { name, fields } => {
                if !self.scopes.contains(&name.name) {
                    self.unresolved(name, "a type");
                }
                // Field names are checked against the record once types exist.
                for f in fields {
                    self.expr(&f.value);
                }
            }
            ExprKind::ListLit(items) => {
                for i in items {
                    self.expr(i);
                }
            }
            ExprKind::Return(value) => {
                if let Some(v) = value {
                    self.expr(v);
                }
            }
        }
    }

    fn pattern(&mut self, pattern: &Pattern) {
        match &pattern.kind {
            PatternKind::Wildcard | PatternKind::Literal(_) => {}
            PatternKind::Binding(ident) => self.declare(ident, Symbol::Value),
            PatternKind::Variant { path, fields } => {
                if let Some(name) = path.as_single()
                    && !self.scopes.contains(&name.name)
                {
                    self.unresolved(name, "a constructor");
                }
                for f in fields {
                    self.pattern(f);
                }
            }
        }
    }
}

fn starts_upper(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

/// Enforce the module line cap of §3.
///
/// The cap exists so a module fits in an agent's working context and can be
/// edited correctly without reading the rest of the program.
pub const MAX_MODULE_LINES: u32 = 500;

/// Report `V0101` when a file exceeds the cap.
#[must_use]
pub fn check_module_length(lines: u32, span: Span) -> Option<Diagnostic> {
    (lines > MAX_MODULE_LINES).then(|| {
        Diagnostic::error(
            Code::ModuleTooLong,
            span,
            format!("module is {lines} lines; the cap is {MAX_MODULE_LINES}"),
        )
        .with_note(
            "a module must fit in an agent's working context so it can be edited without \
             reading the rest of the program",
        )
        .with_fix(
            Fix::new(FixKind::SplitModule, String::new())
                .at(span)
                .confidence(Confidence::Possible),
        )
    })
}
