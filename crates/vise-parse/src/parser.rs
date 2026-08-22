//! The parser: modules, imports, items, and types.
//!
//! Recovery is a first-class concern. A parse error skips to the next item
//! boundary and keeps going, so one compile reports every broken item rather
//! than the first. A machine author pays a full round trip per compile, which
//! makes batching worth more than an early exit.

use vise_ast::{
    Effect, EffectRow, EnumDecl, Expr, Field, FnDecl, GenericParam, Ident, Item, ItemKind, Module,
    Param, RecordDecl, Type, TypeDecl, TypeKind, Use, UsePath, Variant,
};
use vise_diag::{Code, Diagnostic, FileId, Fix, FixKind, Span};
use vise_lex::{Token, TokenKind as T, apply_layout, lex};

/// The result of parsing one file.
#[derive(Debug)]
pub struct Parsed {
    /// `None` only when the file has no usable `module` header.
    pub module: Option<Module>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Parsed {
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }
}

/// Parse `text` as the contents of `file`.
#[must_use]
pub fn parse(text: &str, file: FileId) -> Parsed {
    let lexed = lex(text, file);
    let tokens = apply_layout(&lexed.tokens);
    let mut p = Parser {
        text,
        tokens,
        pos: 0,
        file,
        diagnostics: lexed.diagnostics,
    };
    let module = p.module();
    Parsed {
        module,
        diagnostics: p.diagnostics,
    }
}

pub(crate) struct Parser<'a> {
    pub(crate) text: &'a str,
    pub(crate) tokens: Vec<Token>,
    pub(crate) pos: usize,
    pub(crate) file: FileId,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    // --- cursor ----------------------------------------------------------

    pub(crate) fn peek(&self) -> T {
        self.tokens.get(self.pos).map_or(T::Eof, |t| t.kind)
    }

    pub(crate) fn at(&self, kind: T) -> bool {
        self.peek() == kind
    }

    pub(crate) fn current(&self) -> Token {
        self.tokens.get(self.pos).copied().unwrap_or(Token::new(
            T::Eof,
            Span::new(self.file, self.end_offset(), self.end_offset()),
        ))
    }

    fn end_offset(&self) -> u32 {
        u32::try_from(self.text.len()).unwrap_or(u32::MAX)
    }

    pub(crate) fn bump(&mut self) -> Token {
        let t = self.current();
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    pub(crate) fn eat(&mut self, kind: T) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(crate) fn text_of(&self, token: Token) -> &'a str {
        &self.text[token.span.start as usize..token.span.end as usize]
    }

    pub(crate) fn skip_newlines(&mut self) {
        while self.at(T::Newline) {
            self.bump();
        }
    }

    // --- diagnostics -----------------------------------------------------

    pub(crate) fn error(&mut self, code: Code, span: Span, message: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::error(code, span, message.into()));
    }

    /// Report that something else was expected here.
    pub(crate) fn expected(&mut self, what: &str) {
        let found = self.current();
        let span = found.span;
        let msg = format!("expected {what}, found {}", found.kind.as_str());
        self.diagnostics.push(
            Diagnostic::error(Code::UnexpectedToken, span, msg)
                .with_note(format!("this is where {what} should appear")),
        );
    }

    /// Consume `kind` or report and return `false`.
    pub(crate) fn expect(&mut self, kind: T) -> bool {
        if self.eat(kind) {
            return true;
        }
        let found = self.current();
        self.diagnostics.push(
            Diagnostic::error(
                Code::UnexpectedToken,
                found.span,
                format!(
                    "expected `{}`, found {}",
                    kind.as_str(),
                    found.kind.as_str()
                ),
            )
            .with_fix(
                Fix::new(FixKind::Replace, kind.as_str())
                    .at(found.span)
                    .confidence(vise_diag::Confidence::Possible),
            ),
        );
        false
    }

    // --- names -----------------------------------------------------------

    /// A `snake_case` name.
    pub(crate) fn ident(&mut self) -> Option<Ident> {
        if self.at(T::Ident) {
            let t = self.bump();
            Some(Ident::new(self.text_of(t), t.span))
        } else {
            self.expected("an identifier");
            None
        }
    }

    /// A `PascalCase` name.
    pub(crate) fn type_ident(&mut self) -> Option<Ident> {
        if self.at(T::TypeIdent) {
            let t = self.bump();
            Some(Ident::new(self.text_of(t), t.span))
        } else {
            self.expected("a type name");
            None
        }
    }

    // --- module ----------------------------------------------------------

    fn module(&mut self) -> Option<Module> {
        self.skip_newlines();
        let start = self.current().span;

        if !self.at(T::Module) {
            let span = self.current().span;
            self.diagnostics.push(
                Diagnostic::error(
                    Code::MissingModuleHeader,
                    span,
                    "a file must begin with `module <name>`",
                )
                .with_note("every file is a module, so its identity never depends on its path")
                .with_fix(
                    Fix::new(FixKind::Replace, "module <name>")
                        .at(Span::new(self.file, span.start, span.start))
                        .confidence(vise_diag::Confidence::Likely),
                ),
            );
            return None;
        }
        self.bump();
        let name = self.ident()?;

        let mut uses = Vec::new();
        let mut items = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                T::Eof => break,
                T::Use => {
                    if let Some(u) = self.use_decl() {
                        uses.push(u);
                    } else {
                        self.recover_to_item();
                    }
                }
                _ => {
                    if let Some(item) = self.item() {
                        items.push(item);
                    } else {
                        self.recover_to_item();
                    }
                }
            }
        }

        let end = self.current().span;
        Some(Module {
            name,
            uses,
            items,
            span: start.to(end),
        })
    }

    /// Skip to the next token that can start an item, so one bad item does not
    /// swallow the rest of the file.
    fn recover_to_item(&mut self) {
        loop {
            match self.peek() {
                T::Eof => return,
                T::Use | T::Pub | T::Fn | T::Type | T::Record | T::Enum => return,
                _ => {
                    self.bump();
                }
            }
        }
    }

    // --- imports ---------------------------------------------------------

    /// `use std/http@1:{post, Response}`
    fn use_decl(&mut self) -> Option<Use> {
        let start = self.current().span;
        self.expect(T::Use);

        let mut segments = vec![self.ident()?];
        while self.eat(T::Slash) {
            segments.push(self.ident()?);
        }

        let version = if self.eat(T::At) {
            let t = self.current();
            if self.at(T::Int) {
                self.bump();
                self.text_of(t).parse::<u32>().ok()
            } else {
                self.expected("a version number");
                None
            }
        } else {
            None
        };

        let path_span = start.to(self.tokens[self.pos.saturating_sub(1)].span);
        let path = UsePath {
            segments,
            version,
            span: path_span,
        };

        self.expect(T::Colon);
        self.expect(T::LBrace);
        let mut names = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                T::RBrace | T::Eof => break,
                T::Ident => {
                    let t = self.bump();
                    names.push(Ident::new(self.text_of(t), t.span));
                }
                T::TypeIdent => {
                    let t = self.bump();
                    names.push(Ident::new(self.text_of(t), t.span));
                }
                _ => {
                    self.expected("an imported name");
                    break;
                }
            }
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.skip_newlines();
        self.expect(T::RBrace);

        if names.is_empty() {
            self.error(
                Code::UnexpectedToken,
                path.span,
                "an import must list at least one name; there is no glob import",
            );
        }

        let span = start.to(self.current().span);
        Some(Use { path, names, span })
    }

    // --- items -----------------------------------------------------------

    fn item(&mut self) -> Option<Item> {
        let start = self.current().span;
        let is_pub = self.eat(T::Pub);

        let kind = match self.peek() {
            T::Type => ItemKind::Type(self.type_decl(is_pub, start)?),
            T::Record => ItemKind::Record(self.record_decl(is_pub, start)?),
            T::Enum => ItemKind::Enum(self.enum_decl(is_pub, start)?),
            T::Fn => ItemKind::Fn(Box::new(self.fn_decl(is_pub, start)?)),
            _ => {
                self.expected("`type`, `record`, `enum`, or `fn`");
                return None;
            }
        };

        let span = start.to(self.prev_span());
        Some(Item { kind, span })
    }

    pub(crate) fn prev_span(&self) -> Span {
        self.tokens
            .get(self.pos.saturating_sub(1))
            .map_or(self.current().span, |t| t.span)
    }

    /// `type UserId = Int`
    fn type_decl(&mut self, is_pub: bool, start: Span) -> Option<TypeDecl> {
        self.expect(T::Type);
        let name = self.type_ident()?;
        self.expect(T::Eq);
        let underlying = self.ty()?;
        Some(TypeDecl {
            is_pub,
            name,
            underlying,
            span: start.to(self.prev_span()),
        })
    }

    /// `record Receipt { id: UserId }`
    fn record_decl(&mut self, is_pub: bool, start: Span) -> Option<RecordDecl> {
        self.expect(T::Record);
        let name = self.type_ident()?;
        let generics = self.type_generics();
        self.expect(T::LBrace);

        let mut fields = Vec::new();
        let mut invariants = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                T::RBrace | T::Eof => break,
                T::Invariant => {
                    self.bump();
                    if let Some(e) = self.expr() {
                        invariants.push(e);
                    } else {
                        break;
                    }
                }
                _ => {
                    let Some(f) = self.field() else { break };
                    fields.push(f);
                }
            }
        }
        self.expect(T::RBrace);

        Some(RecordDecl {
            is_pub,
            name,
            generics,
            fields,
            invariants,
            span: start.to(self.prev_span()),
        })
    }

    fn field(&mut self) -> Option<Field> {
        let start = self.current().span;
        let name = self.ident()?;
        self.expect(T::Colon);
        let ty = self.ty()?;
        Some(Field {
            name,
            ty,
            span: start.to(self.prev_span()),
        })
    }

    /// `enum ChargeError { InsufficientFunds  CardDeclined(reason: Str) }`
    fn enum_decl(&mut self, is_pub: bool, start: Span) -> Option<EnumDecl> {
        self.expect(T::Enum);
        let name = self.type_ident()?;
        let generics = self.type_generics();
        self.expect(T::LBrace);

        let mut variants = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), T::RBrace | T::Eof) {
                break;
            }
            let vstart = self.current().span;
            let Some(vname) = self.type_ident() else {
                break;
            };
            let mut fields = Vec::new();
            if self.eat(T::LParen) {
                loop {
                    if matches!(self.peek(), T::RParen | T::Eof) {
                        break;
                    }
                    let Some(f) = self.field() else { break };
                    fields.push(f);
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
                self.expect(T::RParen);
            }
            variants.push(Variant {
                name: vname,
                fields,
                span: vstart.to(self.prev_span()),
            });
        }
        self.expect(T::RBrace);

        Some(EnumDecl {
            is_pub,
            name,
            generics,
            variants,
            span: start.to(self.prev_span()),
        })
    }

    /// `<T>` on a record or enum: type parameters only, no lifetimes.
    fn type_generics(&mut self) -> Vec<Ident> {
        let mut out = Vec::new();
        if !self.eat(T::Lt) {
            return out;
        }
        loop {
            if matches!(self.peek(), T::Gt | T::Eof) {
                break;
            }
            let Some(name) = self.type_ident() else { break };
            out.push(name);
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect(T::Gt);
        out
    }

    /// `<T, 'a>` on a function: type parameters and lifetimes.
    fn fn_generics(&mut self) -> Vec<GenericParam> {
        let mut out = Vec::new();
        if !self.eat(T::Lt) {
            return out;
        }
        loop {
            match self.peek() {
                T::Gt | T::Eof => break,
                T::Lifetime => {
                    let t = self.bump();
                    out.push(GenericParam::Lifetime(Ident::new(self.text_of(t), t.span)));
                }
                T::TypeIdent => {
                    let t = self.bump();
                    out.push(GenericParam::Type(Ident::new(self.text_of(t), t.span)));
                }
                _ => {
                    self.expected("a type parameter or lifetime");
                    break;
                }
            }
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect(T::Gt);
        out
    }

    fn fn_decl(&mut self, is_pub: bool, start: Span) -> Option<FnDecl> {
        self.expect(T::Fn);
        let name = self.ident()?;
        let generics = self.fn_generics();

        self.expect(T::LParen);
        let mut params = Vec::new();
        loop {
            if matches!(self.peek(), T::RParen | T::Eof) {
                break;
            }
            let pstart = self.current().span;
            let Some(pname) = self.ident() else { break };
            self.expect(T::Colon);
            let Some(ty) = self.ty() else { break };
            params.push(Param {
                name: pname,
                ty,
                span: pstart.to(self.prev_span()),
            });
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect(T::RParen);

        let ret = if self.eat(T::Arrow) {
            Some(self.ty()?)
        } else {
            None
        };

        // The row, contracts, and body may each start on their own line.
        self.skip_newlines();
        let effects = if self.at(T::Bang) {
            Some(self.effect_row()?)
        } else {
            None
        };

        let mut requires = Vec::new();
        let mut ensures = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                T::Requires => {
                    self.bump();
                    let Some(e) = self.expr() else { break };
                    requires.push(e);
                }
                T::Ensures => {
                    self.bump();
                    let Some(e) = self.expr() else { break };
                    ensures.push(e);
                }
                _ => break,
            }
        }

        self.skip_newlines();
        let body = self.block()?;

        Some(FnDecl {
            is_pub,
            name,
            generics,
            params,
            ret,
            effects,
            requires,
            ensures,
            body,
            span: start.to(self.prev_span()),
        })
    }

    /// `!{net, time}`. An empty row `!{}` asserts purity, which is different
    /// from omitting the row and letting inference decide.
    fn effect_row(&mut self) -> Option<EffectRow> {
        let start = self.current().span;
        self.expect(T::Bang);
        self.expect(T::LBrace);

        let mut effects = Vec::new();
        let mut unknown = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), T::RBrace | T::Eof) {
                break;
            }
            let t = self.bump();
            if t.kind == T::Ident {
                let name = self.text_of(t);
                match Effect::from_name(name) {
                    Some(e) => effects.push(e),
                    // Kept with its span so the checker reports it, rather than
                    // the parser guessing what was meant.
                    None => unknown.push(Ident::new(name, t.span)),
                }
            } else {
                self.expected("an effect name");
                break;
            }
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect(T::RBrace);

        effects.sort_unstable();
        effects.dedup();
        Some(EffectRow {
            effects,
            unknown,
            span: start.to(self.prev_span()),
        })
    }

    // --- types -----------------------------------------------------------

    pub(crate) fn ty(&mut self) -> Option<Type> {
        let start = self.current().span;
        match self.peek() {
            T::Amp => {
                self.bump();
                let lifetime = if self.at(T::Lifetime) {
                    let t = self.bump();
                    Some(Ident::new(self.text_of(t), t.span))
                } else {
                    None
                };
                let is_mut = self.eat(T::Mut);
                let inner = Box::new(self.ty()?);
                Some(Type::new(
                    TypeKind::Ref {
                        lifetime,
                        is_mut,
                        inner,
                    },
                    start.to(self.prev_span()),
                ))
            }
            T::LParen => {
                self.bump();
                if self.eat(T::RParen) {
                    return Some(Type::new(TypeKind::Unit, start.to(self.prev_span())));
                }
                self.expected("`)` — `()` is the only parenthesised type");
                None
            }
            T::TypeIdent => {
                let t = self.bump();
                let name = Ident::new(self.text_of(t), t.span);
                let mut args = Vec::new();
                if self.eat(T::Lt) {
                    loop {
                        if matches!(self.peek(), T::Gt | T::Eof) {
                            break;
                        }
                        args.push(self.ty()?);
                        if !self.eat(T::Comma) {
                            break;
                        }
                    }
                    self.expect(T::Gt);
                }
                Some(Type::new(
                    TypeKind::Named { name, args },
                    start.to(self.prev_span()),
                ))
            }
            _ => {
                self.expected("a type");
                None
            }
        }
    }

    /// Parse contract and invariant expressions. Declared here so the item
    /// parser can reach the expression grammar in `expr.rs`.
    fn expr(&mut self) -> Option<Expr> {
        self.expression()
    }
}
