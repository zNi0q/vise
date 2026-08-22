//! The lexer.
//!
//! Lexing never stops at the first problem: an unrecognised character produces
//! an [`TokenKind::Error`] token and scanning continues, so one run reports
//! every lexical fault in the file rather than one per invocation. That matters
//! for a machine author, which pays a full round trip for each compile.

use vise_diag::{Code, Confidence, Diagnostic, FileId, Fix, FixKind, Span};

use crate::token::{Token, TokenKind, keyword};

/// The result of lexing one file.
#[derive(Debug)]
pub struct Lexed {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Lexed {
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }

    /// Tokens with line breaks removed, for consumers that do not care about
    /// layout. See the open spec issue in `TRACKER.md`.
    #[must_use]
    pub fn without_newlines(&self) -> Vec<Token> {
        self.tokens
            .iter()
            .copied()
            .filter(|t| t.kind != TokenKind::Newline)
            .collect()
    }
}

/// Lex `text` as the contents of `file`.
#[must_use]
pub fn lex(text: &str, file: FileId) -> Lexed {
    Lexer::new(text, file).run()
}

struct Lexer<'a> {
    text: &'a str,
    pos: usize,
    file: FileId,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn new(text: &'a str, file: FileId) -> Self {
        Self {
            text,
            pos: 0,
            file,
            tokens: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    // --- cursor ----------------------------------------------------------

    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.text[self.pos..].chars().nth(n)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn span_from(&self, start: usize) -> Span {
        Span::new(
            self.file,
            u32::try_from(start).unwrap_or(u32::MAX),
            u32::try_from(self.pos).unwrap_or(u32::MAX),
        )
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        let span = self.span_from(start);
        self.tokens.push(Token::new(kind, span));
    }

    fn error(&mut self, code: Code, start: usize, message: impl Into<String>) {
        let span = self.span_from(start);
        self.diagnostics
            .push(Diagnostic::error(code, span, message.into()));
    }

    // --- driver ----------------------------------------------------------

    fn run(mut self) -> Lexed {
        loop {
            self.skip_trivia();
            let start = self.pos;
            let Some(c) = self.peek() else {
                self.push(TokenKind::Eof, start);
                break;
            };

            match c {
                '\n' => {
                    self.bump();
                    self.push(TokenKind::Newline, start);
                }
                '"' => self.string(start),
                '\'' => self.quote(start),
                c if c.is_ascii_digit() => self.number(start),
                c if c.is_ascii_lowercase() || c == '_' => self.value_name(start),
                c if c.is_ascii_uppercase() => self.type_name(start),
                _ => self.punctuation(start),
            }
        }

        Lexed {
            tokens: self.tokens,
            diagnostics: self.diagnostics,
        }
    }

    /// Horizontal whitespace and `--` comments. Line breaks are tokens, so they
    /// are not trivia.
    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\r') => {
                    self.bump();
                }
                Some('-') if self.peek_at(1) == Some('-') => {
                    while !matches!(self.peek(), None | Some('\n')) {
                        self.bump();
                    }
                }
                _ => return,
            }
        }
    }

    // --- names -----------------------------------------------------------

    fn value_name(&mut self, start: usize) {
        while matches!(self.peek(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            self.bump();
        }

        // `myVar` would otherwise split into `my` and the type name `Var`, and
        // fail somewhere far from the real mistake.
        if matches!(self.peek(), Some(c) if c.is_ascii_uppercase()) {
            while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
                self.bump();
            }
            let text = &self.text[start..self.pos];
            let snake = to_snake_case(text);
            let span = self.span_from(start);
            self.diagnostics.push(
                Diagnostic::error(
                    Code::NonSnakeCase,
                    span,
                    format!("`{text}` is not snake_case"),
                )
                .with_fix(
                    Fix::new(FixKind::Replace, snake)
                        .at(span)
                        .confidence(Confidence::Likely),
                ),
            );
            self.push(TokenKind::Ident, start);
            return;
        }

        let text = &self.text[start..self.pos];
        if text == "_" {
            self.push(TokenKind::Underscore, start);
        } else if let Some(kw) = keyword(text) {
            self.push(kw, start);
        } else {
            self.push(TokenKind::Ident, start);
        }
    }

    fn type_name(&mut self, start: usize) {
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric()) {
            self.bump();
        }
        // An underscore in a type name is a casing error, not a new token.
        if self.peek() == Some('_') {
            while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
                self.bump();
            }
            let text = self.text[start..self.pos].to_owned();
            self.error(
                Code::NonPascalCase,
                start,
                format!("`{text}` is not PascalCase"),
            );
        }
        self.push(TokenKind::TypeIdent, start);
    }

    // --- numbers ---------------------------------------------------------

    fn number(&mut self, start: usize) {
        self.digits();

        let mut kind = TokenKind::Int;
        // A `.` only continues the literal when a digit follows, so `x.0.max()`
        // and field access keep working.
        if self.peek() == Some('.') && matches!(self.peek_at(1), Some(c) if c.is_ascii_digit()) {
            self.bump();
            self.digits();
            kind = TokenKind::Float;
        }

        if self.text[start..self.pos].ends_with('_') {
            self.error(
                Code::MalformedNumber,
                start,
                "numeric literal ends with `_`",
            );
        }

        // `0x10`, `1abc`, `1.5e3`: trailing word characters are always a
        // mistake, and v0 has no hex, octal, binary, or exponent syntax.
        if matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
            while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
                self.bump();
            }
            let text = &self.text[start..self.pos];
            let msg = if text.starts_with("0x") || text.starts_with("0b") || text.starts_with("0o")
            {
                format!("`{text}` is not a Vise literal; v0 has decimal literals only")
            } else {
                format!("`{text}` is not a valid numeric literal")
            };
            self.error(Code::MalformedNumber, start, msg);
        }

        self.push(kind, start);
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '_') {
            self.bump();
        }
    }

    // --- strings and characters ------------------------------------------

    fn string(&mut self, start: usize) {
        self.bump(); // opening quote
        let mut depth = 0u32;
        let mut closed = false;

        while let Some(c) = self.peek() {
            if c == '\n' {
                break; // do not swallow the line break
            }
            self.bump();
            match c {
                '"' => {
                    closed = true;
                    break;
                }
                '\\' => self.escape(),
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }

        if !closed {
            self.error(
                Code::UnterminatedString,
                start,
                "string literal is not closed",
            );
        } else if depth > 0 {
            let span = self.span_from(start);
            self.diagnostics.push(
                Diagnostic::error(
                    Code::UnterminatedInterpolation,
                    span,
                    "interpolation opened with `{` is never closed",
                )
                .with_note("a string literal may not be nested inside an interpolation")
                .with_fix(Fix::new(FixKind::Replace, "}").confidence(Confidence::Possible)),
            );
        }

        self.push(TokenKind::Str, start);
    }

    /// A backslash has been consumed; validate what follows.
    fn escape(&mut self) {
        let start = self.pos - 1;
        match self.peek() {
            Some('n' | 't' | '\\' | '"' | '{') => {
                self.bump();
            }
            Some('u') => {
                self.bump();
                if !self.eat('{') {
                    self.error(Code::InvalidEscape, start, "`\\u` must be followed by `{`");
                    return;
                }
                let digits_at = self.pos;
                while matches!(self.peek(), Some(c) if c.is_ascii_hexdigit()) {
                    self.bump();
                }
                let digits = &self.text[digits_at..self.pos];
                if digits.is_empty() || digits.len() > 6 {
                    self.error(
                        Code::InvalidEscape,
                        start,
                        "`\\u{...}` takes one to six hex digits",
                    );
                } else if u32::from_str_radix(digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .is_none()
                {
                    self.error(
                        Code::InvalidEscape,
                        start,
                        format!("`\\u{{{digits}}}` is not a character"),
                    );
                }
                if !self.eat('}') {
                    self.error(Code::InvalidEscape, start, "`\\u{...}` is not closed");
                }
            }
            _ => {
                let found = self.peek().map_or(String::new(), |c| format!("`\\{c}` "));
                self.bump();
                self.error(
                    Code::InvalidEscape,
                    start,
                    format!("{found}is not a recognised escape"),
                );
            }
        }
    }

    /// A `'` starts either a character literal or a lifetime.
    fn quote(&mut self, start: usize) {
        // `'a'` is a character; `'a` is a lifetime. One character followed by a
        // closing quote is the only thing that distinguishes them.
        let is_lifetime = matches!(self.peek_at(1), Some(c) if c.is_ascii_alphabetic() || c == '_')
            && self.peek_at(2) != Some('\'');

        if is_lifetime {
            self.bump(); // the quote
            while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
                self.bump();
            }
            self.push(TokenKind::Lifetime, start);
            return;
        }

        self.bump(); // the quote
        match self.peek() {
            None | Some('\n') => {
                self.error(
                    Code::UnterminatedChar,
                    start,
                    "character literal is not closed",
                );
            }
            Some('\\') => {
                self.bump();
                self.escape();
                if !self.eat('\'') {
                    self.error(
                        Code::UnterminatedChar,
                        start,
                        "character literal is not closed",
                    );
                }
            }
            Some(_) => {
                self.bump();
                if !self.eat('\'') {
                    self.error(
                        Code::UnterminatedChar,
                        start,
                        "character literal holds exactly one character",
                    );
                }
            }
        }
        self.push(TokenKind::Char, start);
    }

    // --- punctuation -----------------------------------------------------

    fn punctuation(&mut self, start: usize) {
        let c = self.bump().expect("caller checked for a character");
        let kind = match c {
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ',' => TokenKind::Comma,
            '.' => TokenKind::Dot,
            '@' => TokenKind::At,
            '+' => TokenKind::Plus,
            '*' => TokenKind::Star,
            '/' => TokenKind::Slash,
            '%' => TokenKind::Percent,
            ':' => {
                if self.eat(':') {
                    TokenKind::ColonColon
                } else {
                    TokenKind::Colon
                }
            }
            '?' => TokenKind::Question,
            '-' => {
                if self.eat('>') {
                    TokenKind::Arrow
                } else {
                    TokenKind::Minus
                }
            }
            '=' => {
                if self.eat('>') {
                    TokenKind::FatArrow
                } else if self.eat('=') {
                    TokenKind::EqEq
                } else {
                    TokenKind::Eq
                }
            }
            '!' => {
                if self.eat('=') {
                    TokenKind::BangEq
                } else {
                    TokenKind::Bang
                }
            }
            '<' => {
                if self.eat('=') {
                    TokenKind::Le
                } else {
                    TokenKind::Lt
                }
            }
            '>' => {
                if self.eat('=') {
                    TokenKind::Ge
                } else {
                    TokenKind::Gt
                }
            }
            '&' => {
                if self.eat('&') {
                    TokenKind::AmpAmp
                } else {
                    TokenKind::Amp
                }
            }
            '|' => {
                if self.eat('|') {
                    TokenKind::PipePipe
                } else {
                    TokenKind::Pipe
                }
            }
            other => {
                self.error(
                    Code::UnknownCharacter,
                    start,
                    format!("`{other}` is not part of Vise"),
                );
                TokenKind::Error
            }
        };
        self.push(kind, start);
    }
}

/// Best-effort `camelCase` to `snake_case`, used to suggest a rename.
fn to_snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}
