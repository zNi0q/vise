//! Lexical analysis for Vise.

pub mod layout;
pub mod lexer;
pub mod token;

pub use layout::apply as apply_layout;
pub use lexer::{Lexed, lex};
pub use token::{KEYWORDS, Token, TokenKind, keyword};
