//! The canonical formatter.
//!
//! Spec §2: formatting is canonical and non-configurable, so it is never a
//! decision. There are no options here on purpose.
//!
//! Two properties matter more than any particular layout choice, and both are
//! tested: formatting is **idempotent**, and formatted source **re-parses to
//! the same thing**. Together they mean a diff between two Vise files reflects
//! a difference in the program, never a difference in habit — which is what
//! makes a diff worth showing to a reviewer, or to a model.
//!
//! Long lines are not wrapped yet. Wrapping is where formatters acquire their
//! difficulty, and doing it badly would break idempotence.

use std::fmt::Write as _;

use vise_ast::{
    Binding, Block, EnumDecl, Expr, ExprKind, Field, FnDecl, GenericParam, Item, ItemKind, Literal,
    Module, Pattern, PatternKind, RecordDecl, Stmt, StmtKind, StrPart, Type, TypeKind, Use,
};

/// Indent width. Not configurable, by design.
const INDENT: usize = 2;

/// Precedence used when an expression stands alone.
const TOP: u8 = 0;
/// Precedence of prefix operators, above every binary operator.
const PREFIX: u8 = 6;
/// Precedence of postfix operators.
const POSTFIX: u8 = 7;

/// Render `module` as canonical Vise source.
#[must_use]
pub fn format(module: &Module) -> String {
    let mut p = Printer {
        out: String::new(),
        depth: 0,
    };
    p.module(module);
    p.out
}

struct Printer {
    out: String,
    depth: usize,
}

impl Printer {
    fn indent(&mut self) {
        for _ in 0..self.depth * INDENT {
            self.out.push(' ');
        }
    }

    fn line(&mut self, text: &str) {
        self.indent();
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn blank(&mut self) {
        self.out.push('\n');
    }

    // --- items -----------------------------------------------------------

    fn module(&mut self, m: &Module) {
        let _ = writeln!(self.out, "module {}", m.name);

        if !m.uses.is_empty() {
            self.blank();
            for u in &m.uses {
                self.use_decl(u);
            }
        }

        for item in &m.items {
            self.blank();
            self.item(item);
        }
    }

    fn use_decl(&mut self, u: &Use) {
        let mut path = u.path.joined();
        if let Some(v) = u.path.version {
            let _ = write!(path, "@{v}");
        }
        let names: Vec<&str> = u.names.iter().map(|n| n.name.as_str()).collect();
        let _ = writeln!(self.out, "use {path}:{{{}}}", names.join(", "));
    }

    fn item(&mut self, item: &Item) {
        let vis = if item.is_pub() { "pub " } else { "" };
        match &item.kind {
            ItemKind::Type(d) => {
                let mut s = format!("{vis}type {} = ", d.name);
                self.push_type(&mut s, &d.underlying);
                self.line(&s);
            }
            ItemKind::Record(d) => self.record(vis, d),
            ItemKind::Enum(d) => self.enum_decl(vis, d),
            ItemKind::Fn(d) => self.function(vis, d),
        }
    }

    fn record(&mut self, vis: &str, d: &RecordDecl) {
        let generics = type_params(
            &d.generics
                .iter()
                .map(|g| g.name.clone())
                .collect::<Vec<_>>(),
        );
        self.line(&format!("{vis}record {}{generics} {{", d.name));
        self.depth += 1;
        for f in &d.fields {
            self.field(f);
        }
        for inv in &d.invariants {
            let mut s = String::from("invariant ");
            self.push_expr(&mut s, inv, TOP, self.depth);
            self.line(&s);
        }
        self.depth -= 1;
        self.line("}");
    }

    fn field(&mut self, f: &Field) {
        let mut s = format!("{}: ", f.name);
        self.push_type(&mut s, &f.ty);
        self.line(&s);
    }

    fn enum_decl(&mut self, vis: &str, d: &EnumDecl) {
        let generics = type_params(
            &d.generics
                .iter()
                .map(|g| g.name.clone())
                .collect::<Vec<_>>(),
        );
        self.line(&format!("{vis}enum {}{generics} {{", d.name));
        self.depth += 1;
        for v in &d.variants {
            if v.fields.is_empty() {
                self.line(&v.name.name);
            } else {
                let mut s = format!("{}(", v.name);
                for (i, f) in v.fields.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    let _ = write!(s, "{}: ", f.name);
                    self.push_type(&mut s, &f.ty);
                }
                s.push(')');
                self.line(&s);
            }
        }
        self.depth -= 1;
        self.line("}");
    }

    fn function(&mut self, vis: &str, d: &FnDecl) {
        let mut head = format!("{vis}fn {}", d.name);

        if !d.generics.is_empty() {
            head.push('<');
            for (i, g) in d.generics.iter().enumerate() {
                if i > 0 {
                    head.push_str(", ");
                }
                match g {
                    GenericParam::Type(n) | GenericParam::Lifetime(n) => {
                        head.push_str(&n.name);
                    }
                }
            }
            head.push('>');
        }

        head.push('(');
        for (i, p) in d.params.iter().enumerate() {
            if i > 0 {
                head.push_str(", ");
            }
            let _ = write!(head, "{}: ", p.name);
            self.push_type(&mut head, &p.ty);
        }
        head.push(')');

        if let Some(ret) = &d.ret {
            head.push_str(" -> ");
            self.push_type(&mut head, ret);
        }

        if let Some(row) = &d.effects {
            let names: Vec<&str> = row.effects.iter().map(|e| e.as_str()).collect();
            let unknown: Vec<&str> = row.unknown.iter().map(|n| n.name.as_str()).collect();
            let all = [names, unknown].concat();
            let _ = write!(head, " !{{{}}}", all.join(", "));
        }

        // Contracts push the brace onto its own line, which is what the spec's
        // own example does.
        if d.has_contracts() {
            self.line(&head);
            self.depth += 1;
            for c in &d.requires {
                let mut s = String::from("requires ");
                self.push_expr(&mut s, c, TOP, self.depth);
                self.line(&s);
            }
            for c in &d.ensures {
                let mut s = String::from("ensures ");
                self.push_expr(&mut s, c, TOP, self.depth);
                self.line(&s);
            }
            self.depth -= 1;
            self.line("{");
        } else {
            head.push_str(" {");
            self.line(&head);
        }

        self.depth += 1;
        self.block_body(&d.body);
        self.depth -= 1;
        self.line("}");
    }

    // --- statements ------------------------------------------------------

    fn block_body(&mut self, b: &Block) {
        let depth = self.depth;
        let mut out = String::new();
        self.push_block_body(&mut out, b, depth);
        self.out.push_str(&out);
    }

    /// Statements, one per line, each already indented.
    fn push_block_body(&self, out: &mut String, b: &Block, depth: usize) {
        for s in &b.stmts {
            pad(out, depth);
            self.push_stmt(out, s, depth);
            out.push('\n');
        }
        if let Some(t) = &b.tail {
            pad(out, depth);
            self.push_expr(out, t, TOP, depth);
            out.push('\n');
        }
    }

    /// One statement, starting at the caller's cursor and ending without a
    /// newline.
    fn push_stmt(&self, out: &mut String, stmt: &Stmt, depth: usize) {
        match &stmt.kind {
            StmtKind::Let {
                is_var,
                name,
                ty,
                value,
            } => {
                let _ = write!(
                    out,
                    "{} {}",
                    if *is_var { "var" } else { "let" },
                    binding_name(name)
                );
                if let Some(t) = ty {
                    out.push_str(": ");
                    self.push_type(out, t);
                }
                out.push_str(" = ");
                self.push_expr(out, value, TOP, depth);
            }
            StmtKind::Assign { target, value } => {
                self.push_expr(out, target, TOP, depth);
                out.push_str(" = ");
                self.push_expr(out, value, TOP, depth);
            }
            StmtKind::For {
                binding,
                iter,
                body,
            } => {
                let _ = write!(out, "for {} in ", binding_name(binding));
                self.push_expr(out, iter, TOP, depth);
                out.push_str(" {");
                self.push_braced(out, body, depth);
            }
            StmtKind::While { cond, body } => {
                out.push_str("while ");
                self.push_expr(out, cond, TOP, depth);
                out.push_str(" {");
                self.push_braced(out, body, depth);
            }
            StmtKind::Expr(e) => self.push_expr(out, e, TOP, depth),
        }
    }

    /// The body of a `{ ... }` that opened on the current line, plus its
    /// closing brace on a line of its own.
    fn push_braced(&self, out: &mut String, b: &Block, depth: usize) {
        out.push('\n');
        self.push_block_body(out, b, depth + 1);
        pad(out, depth);
        out.push('}');
    }

    // --- types -----------------------------------------------------------

    fn push_type(&self, out: &mut String, ty: &Type) {
        match &ty.kind {
            TypeKind::Unit => out.push_str("()"),
            TypeKind::Ref {
                lifetime,
                is_mut,
                inner,
            } => {
                out.push('&');
                if let Some(lt) = lifetime {
                    let _ = write!(out, "{lt} ");
                }
                if *is_mut {
                    out.push_str("mut ");
                }
                self.push_type(out, inner);
            }
            TypeKind::Named { name, args } => {
                out.push_str(&name.name);
                if !args.is_empty() {
                    out.push('<');
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        self.push_type(out, a);
                    }
                    out.push('>');
                }
            }
        }
    }

    // --- expressions -----------------------------------------------------

    /// Print `e`, parenthesising only where precedence requires it.
    fn push_expr(&self, out: &mut String, e: &Expr, parent: u8, depth: usize) {
        match &e.kind {
            ExprKind::Literal(lit) => self.push_literal(out, lit, depth),
            ExprKind::Path(p) => {
                let names: Vec<&str> = p.segments.iter().map(|s| s.name.as_str()).collect();
                out.push_str(&names.join("::"));
            }
            ExprKind::Call { callee, args } => {
                self.push_expr(out, callee, POSTFIX, depth);
                out.push('(');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    self.push_expr(out, a, TOP, depth);
                }
                out.push(')');
            }
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                self.push_expr(out, receiver, POSTFIX, depth);
                let _ = write!(out, ".{method}(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    self.push_expr(out, a, TOP, depth);
                }
                out.push(')');
            }
            ExprKind::Field { base, name } => {
                self.push_expr(out, base, POSTFIX, depth);
                let _ = write!(out, ".{name}");
            }
            ExprKind::Index { base, index } => {
                self.push_expr(out, base, POSTFIX, depth);
                out.push('[');
                self.push_expr(out, index, TOP, depth);
                out.push(']');
            }
            ExprKind::Unary { op, operand } => {
                let wrap = parent > PREFIX;
                if wrap {
                    out.push('(');
                }
                out.push_str(op.as_str());
                self.push_expr(out, operand, PREFIX, depth);
                if wrap {
                    out.push(')');
                }
            }
            ExprKind::Borrow { is_mut, operand } => {
                let wrap = parent > PREFIX;
                if wrap {
                    out.push('(');
                }
                out.push('&');
                if *is_mut {
                    out.push_str("mut ");
                }
                self.push_expr(out, operand, PREFIX, depth);
                if wrap {
                    out.push(')');
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let prec = op.precedence();
                let wrap = prec < parent;
                if wrap {
                    out.push('(');
                }
                self.push_expr(out, lhs, prec, depth);
                let _ = write!(out, " {} ", op.as_str());
                // Right operand at prec + 1 keeps left associativity explicit.
                self.push_expr(out, rhs, prec + 1, depth);
                if wrap {
                    out.push(')');
                }
            }
            ExprKind::Try(inner) => {
                self.push_expr(out, inner, POSTFIX, depth);
                out.push('?');
            }
            ExprKind::RecordLit { name, fields } => {
                let _ = write!(out, "{name} {{");
                for (i, f) in fields.iter().enumerate() {
                    out.push_str(if i > 0 { ", " } else { " " });
                    let _ = write!(out, "{}: ", f.name);
                    self.push_expr(out, &f.value, TOP, depth);
                }
                out.push_str(if fields.is_empty() { "}" } else { " }" });
            }
            ExprKind::ListLit(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    self.push_expr(out, item, TOP, depth);
                }
                out.push(']');
            }
            ExprKind::Return(v) => {
                out.push_str("return");
                if let Some(v) = v {
                    out.push(' ');
                    self.push_expr(out, v, TOP, depth);
                }
            }
            ExprKind::Break => out.push_str("break"),
            ExprKind::Continue => out.push_str("continue"),
            // Block-shaped expressions always span lines. A `match` cannot do
            // otherwise: its arms are separated by line breaks, so printing
            // them on one line would not re-parse.
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                out.push_str("if ");
                self.push_expr(out, cond, TOP, depth);
                out.push_str(" {");
                out.push('\n');
                self.push_block_body(out, then, depth + 1);
                pad(out, depth);
                out.push_str("} else ");
                match &otherwise.kind {
                    ExprKind::Block(b) => {
                        out.push('{');
                        self.push_braced(out, b, depth);
                    }
                    // `else if` chains stay on the same line.
                    _ => self.push_expr(out, otherwise, TOP, depth),
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                out.push_str("match ");
                self.push_expr(out, scrutinee, TOP, depth);
                out.push_str(" {\n");
                for arm in arms {
                    pad(out, depth + 1);
                    self.push_pattern(out, &arm.pattern);
                    out.push_str(" -> ");
                    self.push_expr(out, &arm.body, TOP, depth + 1);
                    out.push('\n');
                }
                pad(out, depth);
                out.push('}');
            }
            ExprKind::Block(b) => {
                out.push('{');
                self.push_braced(out, b, depth);
            }
        }
    }

    fn push_pattern(&self, out: &mut String, p: &Pattern) {
        match &p.kind {
            PatternKind::Wildcard => out.push('_'),
            PatternKind::Binding(i) => out.push_str(&i.name),
            PatternKind::Literal(l) => self.push_literal(out, l, 0),
            PatternKind::Variant { path, fields } => {
                let names: Vec<&str> = path.segments.iter().map(|s| s.name.as_str()).collect();
                out.push_str(&names.join("::"));
                if !fields.is_empty() {
                    out.push('(');
                    for (i, f) in fields.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        self.push_pattern(out, f);
                    }
                    out.push(')');
                }
            }
        }
    }

    fn push_literal(&self, out: &mut String, lit: &Literal, depth: usize) {
        match lit {
            Literal::Int(v) => {
                let _ = write!(out, "{v}");
            }
            Literal::Float(text) => out.push_str(text),
            Literal::Bool(v) => {
                let _ = write!(out, "{v}");
            }
            Literal::Char(c) => {
                let _ = write!(out, "'{}'", escape_char(*c));
            }
            Literal::Str(parts) => {
                out.push('"');
                for p in parts {
                    match p {
                        StrPart::Text(t) => out.push_str(&escape_text(t)),
                        StrPart::Interpolation(e) => {
                            out.push('{');
                            self.push_expr(out, e, TOP, depth);
                            out.push('}');
                        }
                    }
                }
                out.push('"');
            }
        }
    }
}

/// Write `depth` levels of indentation.
fn pad(out: &mut String, depth: usize) {
    for _ in 0..depth * INDENT {
        out.push(' ');
    }
}

fn binding_name(b: &Binding) -> String {
    match b {
        Binding::Name(i) => i.name.clone(),
        Binding::Wildcard(_) => "_".to_owned(),
    }
}

fn type_params(names: &[String]) -> String {
    if names.is_empty() {
        String::new()
    } else {
        format!("<{}>", names.join(", "))
    }
}

/// Re-escape text so that printing then re-lexing yields the same string.
fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            // A bare `{` would open an interpolation on the way back in.
            '{' => out.push_str("\\{"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

fn escape_char(c: char) -> String {
    match c {
        '\'' => "\\u{27}".to_owned(),
        '\\' => "\\\\".to_owned(),
        '\n' => "\\n".to_owned(),
        '\t' => "\\t".to_owned(),
        c if (c as u32) < 0x20 => format!("\\u{{{:x}}}", c as u32),
        c => c.to_string(),
    }
}
