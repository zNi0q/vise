//! Expressions, statements, patterns, and string interpolation.

use vise_ast::{
    BinOp, Binding, Block, Expr, ExprKind, FieldInit, Ident, Literal, MatchArm, Path, Pattern,
    PatternKind, Stmt, StmtKind, StrPart, UnOp,
};
use vise_diag::{Code, Diagnostic, Span};
use vise_lex::{Token, TokenKind as T, apply_layout, lex};

use crate::parser::Parser;

/// Whether a record literal may start here.
///
/// `if x { .. }` and `Point { .. }` are ambiguous after `if`, so record
/// literals are banned in the header of `if`, `while`, `for`, and `match`, the
/// same rule Rust uses. Parentheses re-enable them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Struct {
    Allowed,
    Banned,
}

impl Parser<'_> {
    // --- entry points ----------------------------------------------------

    pub(crate) fn expression(&mut self) -> Option<Expr> {
        self.binary(0, Struct::Allowed)
    }

    fn expression_no_struct(&mut self) -> Option<Expr> {
        self.binary(0, Struct::Banned)
    }

    // --- blocks and statements -------------------------------------------

    pub(crate) fn block(&mut self) -> Option<Block> {
        let start = self.current().span;
        if !self.expect(T::LBrace) {
            return None;
        }

        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), T::RBrace | T::Eof) {
                break;
            }

            let before = self.pos;
            match self.stmt() {
                Some(s) => stmts.push(s),
                None => {
                    self.recover_in_block(before);
                    continue;
                }
            }

            if matches!(self.peek(), T::RBrace | T::Eof) {
                break;
            }
            if !self.eat(T::Newline) {
                self.expected("a line break between statements");
                let before = self.pos;
                self.recover_in_block(before);
            }
        }
        self.expect(T::RBrace);

        // Spec §5: the final expression is the block's value.
        let tail = match stmts.last() {
            Some(Stmt {
                kind: StmtKind::Expr(_),
                ..
            }) => match stmts.pop() {
                Some(Stmt {
                    kind: StmtKind::Expr(e),
                    ..
                }) => Some(Box::new(e)),
                _ => unreachable!("just matched an expression statement"),
            },
            _ => None,
        };

        Some(Block {
            stmts,
            tail,
            span: start.to(self.prev_span()),
        })
    }

    /// Skip to the next statement boundary. Always consumes at least one token
    /// when it can, so recovery cannot spin.
    fn recover_in_block(&mut self, before: usize) {
        if self.pos == before && !matches!(self.peek(), T::RBrace | T::Eof) {
            self.bump();
        }
        while !matches!(self.peek(), T::Newline | T::RBrace | T::Eof) {
            self.bump();
        }
        self.eat(T::Newline);
    }

    fn stmt(&mut self) -> Option<Stmt> {
        let start = self.current().span;
        let kind = match self.peek() {
            T::Let | T::Var => {
                let is_var = self.bump().kind == T::Var;
                let name = self.binding()?;
                let ty = if self.eat(T::Colon) {
                    Some(self.ty()?)
                } else {
                    None
                };
                self.expect(T::Eq);
                let value = self.expression()?;
                StmtKind::Let {
                    is_var,
                    name,
                    ty,
                    value,
                }
            }
            T::For => {
                self.bump();
                let binding = self.binding()?;
                self.expect(T::In);
                let iter = self.expression_no_struct()?;
                let body = self.block()?;
                StmtKind::For {
                    binding,
                    iter,
                    body,
                }
            }
            T::While => {
                self.bump();
                let cond = self.expression_no_struct()?;
                let body = self.block()?;
                StmtKind::While { cond, body }
            }
            _ => {
                let e = self.expression()?;
                if self.eat(T::Eq) {
                    let value = self.expression()?;
                    StmtKind::Assign { target: e, value }
                } else {
                    StmtKind::Expr(e)
                }
            }
        };
        Some(Stmt {
            kind,
            span: start.to(self.prev_span()),
        })
    }

    fn binding(&mut self) -> Option<Binding> {
        if self.at(T::Underscore) {
            let t = self.bump();
            return Some(Binding::Wildcard(t.span));
        }
        Some(Binding::Name(self.ident()?))
    }

    // --- operator precedence ---------------------------------------------

    fn peek_binop(&self) -> Option<BinOp> {
        Some(match self.peek() {
            T::Plus => BinOp::Add,
            T::Minus => BinOp::Sub,
            T::Star => BinOp::Mul,
            T::Slash => BinOp::Div,
            T::Percent => BinOp::Rem,
            T::EqEq => BinOp::Eq,
            T::BangEq => BinOp::Ne,
            T::Lt => BinOp::Lt,
            T::Le => BinOp::Le,
            T::Gt => BinOp::Gt,
            T::Ge => BinOp::Ge,
            T::AmpAmp => BinOp::And,
            T::PipePipe => BinOp::Or,
            _ => return None,
        })
    }

    fn binary(&mut self, min_prec: u8, structs: Struct) -> Option<Expr> {
        let mut lhs = self.unary(structs)?;
        let mut prev_cmp: Option<Span> = None;

        while let Some(op) = self.peek_binop() {
            let prec = op.precedence();
            if prec < min_prec {
                break;
            }
            let op_tok = self.bump();

            // Comparisons do not chain: `a < b < c` would otherwise quietly
            // mean `(a < b) < c`, which compares a Bool with a number.
            if op.is_comparison() {
                if let Some(first) = prev_cmp {
                    self.diagnostics.push(
                        Diagnostic::error(
                            Code::UnexpectedToken,
                            op_tok.span,
                            format!(
                                "`{}` cannot be chained with another comparison",
                                op.as_str()
                            ),
                        )
                        .with_label(first, "the first comparison is here")
                        .with_note("write `a < b && b < c` instead"),
                    );
                }
                prev_cmp = Some(op_tok.span);
            } else {
                prev_cmp = None;
            }

            let rhs = self.binary(prec + 1, structs)?;
            let span = lhs.span.to(rhs.span);
            lhs = Expr::new(
                ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }
        Some(lhs)
    }

    fn unary(&mut self, structs: Struct) -> Option<Expr> {
        let start = self.current().span;
        match self.peek() {
            T::Minus => {
                self.bump();
                let operand = Box::new(self.unary(structs)?);
                Some(Expr::new(
                    ExprKind::Unary {
                        op: UnOp::Neg,
                        operand,
                    },
                    start.to(self.prev_span()),
                ))
            }
            T::Bang => {
                self.bump();
                let operand = Box::new(self.unary(structs)?);
                Some(Expr::new(
                    ExprKind::Unary {
                        op: UnOp::Not,
                        operand,
                    },
                    start.to(self.prev_span()),
                ))
            }
            T::Amp => {
                self.bump();
                let is_mut = self.eat(T::Mut);
                let operand = Box::new(self.unary(structs)?);
                Some(Expr::new(
                    ExprKind::Borrow { is_mut, operand },
                    start.to(self.prev_span()),
                ))
            }
            _ => self.postfix(structs),
        }
    }

    fn postfix(&mut self, structs: Struct) -> Option<Expr> {
        let mut e = self.primary(structs)?;
        loop {
            match self.peek() {
                T::LParen => {
                    self.bump();
                    let args = self.call_args()?;
                    let span = e.span.to(self.prev_span());
                    e = Expr::new(
                        ExprKind::Call {
                            callee: Box::new(e),
                            args,
                        },
                        span,
                    );
                }
                T::Dot => {
                    self.bump();
                    let name = self.ident()?;
                    if self.eat(T::LParen) {
                        let args = self.call_args()?;
                        let span = e.span.to(self.prev_span());
                        e = Expr::new(
                            ExprKind::MethodCall {
                                receiver: Box::new(e),
                                method: name,
                                args,
                            },
                            span,
                        );
                    } else {
                        let span = e.span.to(self.prev_span());
                        e = Expr::new(
                            ExprKind::Field {
                                base: Box::new(e),
                                name,
                            },
                            span,
                        );
                    }
                }
                T::LBracket => {
                    self.bump();
                    let index = Box::new(self.expression()?);
                    self.expect(T::RBracket);
                    let span = e.span.to(self.prev_span());
                    e = Expr::new(
                        ExprKind::Index {
                            base: Box::new(e),
                            index,
                        },
                        span,
                    );
                }
                T::Question => {
                    self.bump();
                    let span = e.span.to(self.prev_span());
                    e = Expr::new(ExprKind::Try(Box::new(e)), span);
                }
                _ => return Some(e),
            }
        }
    }

    fn call_args(&mut self) -> Option<Vec<Expr>> {
        let mut args = Vec::new();
        loop {
            if matches!(self.peek(), T::RParen | T::Eof) {
                break;
            }
            args.push(self.expression()?);
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect(T::RParen);
        Some(args)
    }

    fn primary(&mut self, structs: Struct) -> Option<Expr> {
        let start = self.current().span;
        match self.peek() {
            T::Int | T::Float | T::Str | T::Char | T::True | T::False => {
                let tok = self.bump();
                let lit = self.literal(tok);
                Some(Expr::new(ExprKind::Literal(lit), tok.span))
            }
            T::Ident => {
                let t = self.bump();
                let ident = Ident::new(self.text_of(t), t.span);
                Some(Expr::new(
                    ExprKind::Path(Path {
                        segments: vec![ident],
                        span: t.span,
                    }),
                    t.span,
                ))
            }
            T::TypeIdent => {
                let t = self.bump();
                let name = Ident::new(self.text_of(t), t.span);
                if structs == Struct::Allowed && self.at(T::LBrace) {
                    return self.record_literal(name, start);
                }
                Some(Expr::new(
                    ExprKind::Path(Path {
                        segments: vec![name],
                        span: t.span,
                    }),
                    t.span,
                ))
            }
            T::LParen => {
                self.bump();
                // Parentheses restore record literals, since the ambiguity that
                // banned them cannot arise inside them.
                let inner = self.binary(0, Struct::Allowed)?;
                self.expect(T::RParen);
                Some(inner)
            }
            T::LBracket => {
                self.bump();
                let mut items = Vec::new();
                loop {
                    self.skip_newlines();
                    if matches!(self.peek(), T::RBracket | T::Eof) {
                        break;
                    }
                    items.push(self.expression()?);
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
                self.skip_newlines();
                self.expect(T::RBracket);
                Some(Expr::new(
                    ExprKind::ListLit(items),
                    start.to(self.prev_span()),
                ))
            }
            T::LBrace => {
                let b = self.block()?;
                let span = b.span;
                Some(Expr::new(ExprKind::Block(b), span))
            }
            T::If => self.if_expr(),
            T::Match => self.match_expr(),
            T::Return => {
                self.bump();
                let value = if matches!(self.peek(), T::Newline | T::RBrace | T::Eof) {
                    None
                } else {
                    Some(Box::new(self.expression()?))
                };
                Some(Expr::new(
                    ExprKind::Return(value),
                    start.to(self.prev_span()),
                ))
            }
            T::Break => {
                self.bump();
                Some(Expr::new(ExprKind::Break, start))
            }
            T::Continue => {
                self.bump();
                Some(Expr::new(ExprKind::Continue, start))
            }
            _ => {
                self.expected("an expression");
                None
            }
        }
    }

    fn record_literal(&mut self, name: Ident, start: Span) -> Option<Expr> {
        self.expect(T::LBrace);
        let mut fields = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), T::RBrace | T::Eof) {
                break;
            }
            let fstart = self.current().span;
            let fname = self.ident()?;
            self.expect(T::Colon);
            let value = self.expression()?;
            fields.push(FieldInit {
                name: fname,
                value,
                span: fstart.to(self.prev_span()),
            });
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.skip_newlines();
        self.expect(T::RBrace);
        Some(Expr::new(
            ExprKind::RecordLit { name, fields },
            start.to(self.prev_span()),
        ))
    }

    fn if_expr(&mut self) -> Option<Expr> {
        let start = self.current().span;
        self.expect(T::If);
        let cond = Box::new(self.expression_no_struct()?);
        let then = self.block()?;

        // Spec §6: both branches are required.
        if !self.at(T::Else) {
            let span = self.prev_span();
            self.diagnostics.push(
                Diagnostic::error(Code::UnexpectedToken, span, "`if` needs an `else` branch")
                    .with_note("`if` is an expression, so both branches must produce a value")
                    .with_fix(
                        vise_diag::Fix::new(vise_diag::FixKind::Replace, " else { }")
                            .at(Span::new(self.file, span.end, span.end))
                            .confidence(vise_diag::Confidence::Possible),
                    ),
            );
            return None;
        }
        self.bump();

        let otherwise = if self.at(T::If) {
            Box::new(self.if_expr()?)
        } else {
            let b = self.block()?;
            let span = b.span;
            Box::new(Expr::new(ExprKind::Block(b), span))
        };

        Some(Expr::new(
            ExprKind::If {
                cond,
                then,
                otherwise,
            },
            start.to(self.prev_span()),
        ))
    }

    fn match_expr(&mut self) -> Option<Expr> {
        let start = self.current().span;
        self.expect(T::Match);
        let scrutinee = Box::new(self.expression_no_struct()?);
        self.expect(T::LBrace);

        let mut arms = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), T::RBrace | T::Eof) {
                break;
            }
            let astart = self.current().span;
            let Some(pattern) = self.pattern() else { break };
            self.expect(T::Arrow);
            let Some(body) = self.expression() else { break };
            arms.push(MatchArm {
                pattern,
                body,
                span: astart.to(self.prev_span()),
            });
            if !matches!(self.peek(), T::Newline | T::RBrace | T::Eof) {
                self.expected("a line break between match arms");
                break;
            }
        }
        self.expect(T::RBrace);

        Some(Expr::new(
            ExprKind::Match { scrutinee, arms },
            start.to(self.prev_span()),
        ))
    }

    // --- patterns --------------------------------------------------------

    fn pattern(&mut self) -> Option<Pattern> {
        let start = self.current().span;
        match self.peek() {
            T::Underscore => {
                self.bump();
                Some(Pattern {
                    kind: PatternKind::Wildcard,
                    span: start,
                })
            }
            T::Ident => {
                let t = self.bump();
                Some(Pattern {
                    kind: PatternKind::Binding(Ident::new(self.text_of(t), t.span)),
                    span: t.span,
                })
            }
            T::Int | T::Float | T::Str | T::Char | T::True | T::False => {
                let t = self.bump();
                let lit = self.literal(t);
                Some(Pattern {
                    kind: PatternKind::Literal(lit),
                    span: t.span,
                })
            }
            T::TypeIdent => {
                let t = self.bump();
                let path = Path {
                    segments: vec![Ident::new(self.text_of(t), t.span)],
                    span: t.span,
                };
                let mut fields = Vec::new();
                if self.eat(T::LParen) {
                    loop {
                        if matches!(self.peek(), T::RParen | T::Eof) {
                            break;
                        }
                        fields.push(self.pattern()?);
                        if !self.eat(T::Comma) {
                            break;
                        }
                    }
                    self.expect(T::RParen);
                }
                Some(Pattern {
                    kind: PatternKind::Variant { path, fields },
                    span: start.to(self.prev_span()),
                })
            }
            _ => {
                self.expected("a pattern");
                None
            }
        }
    }

    // --- literals --------------------------------------------------------

    fn literal(&mut self, tok: Token) -> Literal {
        let raw = self.text_of(tok);
        match tok.kind {
            T::True => Literal::Bool(true),
            T::False => Literal::Bool(false),
            T::Int => {
                let digits = raw.replace('_', "");
                match digits.parse::<i64>() {
                    Ok(v) => Literal::Int(v),
                    Err(_) => {
                        self.error(
                            Code::MalformedNumber,
                            tok.span,
                            format!("`{raw}` does not fit in Int"),
                        );
                        Literal::Int(0)
                    }
                }
            }
            T::Float => Literal::Float(raw.replace('_', "")),
            T::Char => Literal::Char(decode_char(raw)),
            T::Str => Literal::Str(self.string_parts(tok)),
            _ => unreachable!("literal() called on {:?}", tok.kind),
        }
    }

    /// Split a string literal into text and interpolated expressions.
    ///
    /// The lexer validated escapes and brace balance, so this decodes rather
    /// than re-validates.
    fn string_parts(&mut self, tok: Token) -> Vec<StrPart> {
        let raw = self.text_of(tok);
        let inner = raw
            .strip_prefix('"')
            .map_or(raw, |s| s.strip_suffix('"').unwrap_or(s));
        let base = tok.span.start + 1;

        let mut parts = Vec::new();
        let mut buf = String::new();
        let bytes = inner.as_bytes();
        let mut i = 0usize;

        while i < bytes.len() {
            match bytes[i] {
                b'\\' => {
                    let (decoded, len) = decode_escape(&inner[i..]);
                    buf.push(decoded);
                    i += len;
                }
                b'{' => {
                    if !buf.is_empty() {
                        parts.push(StrPart::Text(std::mem::take(&mut buf)));
                    }
                    let Some(close) = matching_brace(inner, i) else {
                        break; // already reported as V0005
                    };
                    let src = &inner[i + 1..close];
                    let offset = base + u32::try_from(i + 1).unwrap_or(0);
                    if let Some(e) = self.sub_expression(src, offset) {
                        parts.push(StrPart::Interpolation(Box::new(e)));
                    }
                    i = close + 1;
                }
                _ => {
                    let ch = inner[i..].chars().next().unwrap_or('\u{fffd}');
                    buf.push(ch);
                    i += ch.len_utf8();
                }
            }
        }
        if !buf.is_empty() {
            parts.push(StrPart::Text(buf));
        }
        parts
    }

    /// Parse an interpolated expression, shifting its spans so they point at
    /// the real file rather than the extracted fragment.
    fn sub_expression(&mut self, src: &str, offset: u32) -> Option<Expr> {
        let lexed = lex(src, self.file);
        let shift = |s: Span| Span::new(self.file, s.start + offset, s.end + offset);

        for mut d in lexed.diagnostics {
            d.span = shift(d.span);
            for l in &mut d.labels {
                l.span = shift(l.span);
            }
            for f in &mut d.fixes {
                f.span = f.span.map(shift);
            }
            self.diagnostics.push(d);
        }

        let tokens: Vec<Token> = apply_layout(&lexed.tokens)
            .into_iter()
            .map(|t| Token::new(t.kind, shift(t.span)))
            .collect();

        let mut sub = Parser {
            text: self.text,
            tokens,
            pos: 0,
            file: self.file,
            diagnostics: Vec::new(),
        };
        let e = sub.expression();
        self.diagnostics.append(&mut sub.diagnostics);
        e
    }
}

/// Decode one escape, returning the character and how many bytes it spanned.
fn decode_escape(s: &str) -> (char, usize) {
    let mut it = s.chars();
    it.next(); // the backslash
    match it.next() {
        Some('n') => ('\n', 2),
        Some('t') => ('\t', 2),
        Some('\\') => ('\\', 2),
        Some('"') => ('"', 2),
        Some('{') => ('{', 2),
        Some('u') => {
            let Some(open) = s.find('{') else {
                return ('u', 2);
            };
            let Some(close) = s[open..].find('}').map(|i| open + i) else {
                return ('u', 2);
            };
            let ch = u32::from_str_radix(&s[open + 1..close], 16)
                .ok()
                .and_then(char::from_u32)
                .unwrap_or('\u{fffd}');
            (ch, close + 1)
        }
        Some(other) => (other, 1 + other.len_utf8()),
        None => ('\\', 1),
    }
}

/// Decode a character literal, quotes included.
fn decode_char(raw: &str) -> char {
    let inner = raw
        .strip_prefix('\'')
        .map_or(raw, |s| s.strip_suffix('\'').unwrap_or(s));
    if inner.starts_with('\\') {
        decode_escape(inner).0
    } else {
        inner.chars().next().unwrap_or('\u{fffd}')
    }
}

/// Index of the `}` closing the `{` at `open`, accounting for nesting and
/// escapes.
fn matching_brace(s: &str, open: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0u32;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}
