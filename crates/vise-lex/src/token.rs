//! Token definitions.
//!
//! A [`Token`] is a kind plus a span and nothing else, so it is `Copy` and the
//! lexer allocates nothing. Identifier and literal *text* is recovered from the
//! span when a later stage needs it.

use vise_diag::Span;

/// The lexical class of a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TokenKind {
    // --- literals ---
    Int,
    Float,
    /// A string literal, interpolation included. The lexer validates escapes
    /// and brace balance; splitting the literal into text and interpolated
    /// expressions is left to the parser, which re-scans the span.
    Str,
    Char,

    // --- names ---
    /// `snake_case` — a value.
    Ident,
    /// `PascalCase` — a type.
    TypeIdent,
    /// `'a` — a lifetime.
    Lifetime,

    // --- keywords ---
    Module,
    Use,
    Pub,
    Fn,
    Let,
    Var,
    Type,
    Record,
    Enum,
    Match,
    If,
    Else,
    For,
    In,
    While,
    Break,
    Continue,
    Return,
    Requires,
    Ensures,
    Invariant,
    Mut,
    True,
    False,

    // --- punctuation ---
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Dot,
    Colon,
    ColonColon,
    Question,
    Arrow,
    FatArrow,
    Eq,
    EqEq,
    BangEq,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Amp,
    AmpAmp,
    Pipe,
    PipePipe,
    Underscore,
    At,

    /// End of a source line.
    ///
    /// The spec calls layout insignificant, but Vise has no statement
    /// terminator, so the parser needs the option of treating a line break as
    /// one. Emitting the token keeps that decision in the parser instead of
    /// baking it into the lexer. See `TRACKER.md`.
    Newline,

    /// A character the lexer could not classify. Lexing continues so a run can
    /// report more than one problem.
    Error,

    Eof,
}

impl TokenKind {
    /// How the token is written in source, for use in "expected one of" lists.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Int => "an integer",
            Self::Float => "a float",
            Self::Str => "a string",
            Self::Char => "a character",
            Self::Ident => "an identifier",
            Self::TypeIdent => "a type name",
            Self::Lifetime => "a lifetime",
            Self::Module => "module",
            Self::Use => "use",
            Self::Pub => "pub",
            Self::Fn => "fn",
            Self::Let => "let",
            Self::Var => "var",
            Self::Type => "type",
            Self::Record => "record",
            Self::Enum => "enum",
            Self::Match => "match",
            Self::If => "if",
            Self::Else => "else",
            Self::For => "for",
            Self::In => "in",
            Self::While => "while",
            Self::Break => "break",
            Self::Continue => "continue",
            Self::Return => "return",
            Self::Requires => "requires",
            Self::Ensures => "ensures",
            Self::Invariant => "invariant",
            Self::Mut => "mut",
            Self::True => "true",
            Self::False => "false",
            Self::LParen => "(",
            Self::RParen => ")",
            Self::LBrace => "{",
            Self::RBrace => "}",
            Self::LBracket => "[",
            Self::RBracket => "]",
            Self::Comma => ",",
            Self::Dot => ".",
            Self::Colon => ":",
            Self::ColonColon => "::",
            Self::Question => "?",
            Self::Arrow => "->",
            Self::FatArrow => "=>",
            Self::Eq => "=",
            Self::EqEq => "==",
            Self::BangEq => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Plus => "+",
            Self::Minus => "-",
            Self::Star => "*",
            Self::Slash => "/",
            Self::Percent => "%",
            Self::Bang => "!",
            Self::Amp => "&",
            Self::AmpAmp => "&&",
            Self::Pipe => "|",
            Self::PipePipe => "||",
            Self::Underscore => "_",
            Self::At => "@",
            Self::Newline => "a line break",
            Self::Error => "an unrecognised character",
            Self::Eof => "end of file",
        }
    }

    /// Whether this kind is a reserved word.
    #[must_use]
    pub const fn is_keyword(self) -> bool {
        matches!(
            self,
            Self::Module
                | Self::Use
                | Self::Pub
                | Self::Fn
                | Self::Let
                | Self::Var
                | Self::Type
                | Self::Record
                | Self::Enum
                | Self::Match
                | Self::If
                | Self::Else
                | Self::For
                | Self::In
                | Self::While
                | Self::Break
                | Self::Continue
                | Self::Return
                | Self::Requires
                | Self::Ensures
                | Self::Invariant
                | Self::Mut
                | Self::True
                | Self::False
        )
    }
}

/// Every reserved word, paired with its kind.
///
/// Kept as a sorted table so the lexer can binary search it and so a test can
/// assert the list matches [`TokenKind::is_keyword`].
pub const KEYWORDS: &[(&str, TokenKind)] = &[
    ("break", TokenKind::Break),
    ("continue", TokenKind::Continue),
    ("else", TokenKind::Else),
    ("ensures", TokenKind::Ensures),
    ("enum", TokenKind::Enum),
    ("false", TokenKind::False),
    ("fn", TokenKind::Fn),
    ("for", TokenKind::For),
    ("if", TokenKind::If),
    ("in", TokenKind::In),
    ("invariant", TokenKind::Invariant),
    ("let", TokenKind::Let),
    ("match", TokenKind::Match),
    ("module", TokenKind::Module),
    ("mut", TokenKind::Mut),
    ("pub", TokenKind::Pub),
    ("record", TokenKind::Record),
    ("requires", TokenKind::Requires),
    ("return", TokenKind::Return),
    ("true", TokenKind::True),
    ("type", TokenKind::Type),
    ("use", TokenKind::Use),
    ("var", TokenKind::Var),
    ("while", TokenKind::While),
];

/// Look up a reserved word.
#[must_use]
pub fn keyword(text: &str) -> Option<TokenKind> {
    KEYWORDS
        .binary_search_by_key(&text, |&(k, _)| k)
        .ok()
        .map(|i| KEYWORDS[i].1)
}

/// A lexed token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    #[must_use]
    pub const fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_table_is_sorted_for_binary_search() {
        let mut sorted = KEYWORDS.to_vec();
        sorted.sort_by_key(|&(k, _)| k);
        assert_eq!(sorted, KEYWORDS.to_vec());
    }

    #[test]
    fn every_keyword_is_findable() {
        for &(text, kind) in KEYWORDS {
            assert_eq!(keyword(text), Some(kind), "{text}");
        }
    }

    #[test]
    fn keyword_table_and_is_keyword_agree() {
        for &(text, kind) in KEYWORDS {
            assert!(
                kind.is_keyword(),
                "{text} is in the table but not is_keyword"
            );
            assert_eq!(kind.as_str(), text, "{text} should render as itself");
        }
    }

    #[test]
    fn ordinary_words_are_not_keywords() {
        for word in ["name", "modules", "iff", "returns", "", "Fn"] {
            assert_eq!(keyword(word), None, "{word}");
        }
    }

    #[test]
    fn spec_absent_features_have_no_keyword() {
        // §13 lists these as deliberately absent; reserving a word for them
        // would be the first step toward accidentally supporting them.
        for word in [
            "trait", "impl", "class", "unsafe", "async", "throw", "null", "macro",
        ] {
            assert_eq!(keyword(word), None, "{word} must not be reserved");
        }
    }
}
