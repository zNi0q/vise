//! Lexical analysis for Vise.

pub mod lexer;
pub mod token;

pub use lexer::{Lexed, lex};
pub use token::{KEYWORDS, Token, TokenKind, keyword};
