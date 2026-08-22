//! Statement separation.
//!
//! Spec §2: a line break ends a statement unless the statement is visibly
//! unfinished. Deciding that here, in one pass over the token stream, keeps the
//! rule in a single testable place instead of spreading it through the parser.
//!
//! A `Newline` survives only when all three hold:
//!
//! 1. no `(` or `[` is open — braces are blocks, so breaks inside them count;
//! 2. the previous token can end an expression;
//! 3. the next token can begin a statement.
//!
//! Runs of newlines collapse to one, and blank lines at the start and end of a
//! file disappear, so blank lines never carry meaning.

use crate::token::{Token, TokenKind};

/// Can a statement end on this token?
const fn can_end_statement(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Ident
            | TokenKind::TypeIdent
            | TokenKind::Lifetime
            | TokenKind::Int
            | TokenKind::Float
            | TokenKind::Str
            | TokenKind::Char
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Underscore
            | TokenKind::RParen
            | TokenKind::RBracket
            | TokenKind::RBrace
            | TokenKind::Question
            | TokenKind::Break
            | TokenKind::Continue
            | TokenKind::Return
    )
}

/// Can a statement begin with this token?
///
/// `-`, `&`, and `!` are prefix operators, so they can. The infix-only
/// operators cannot, which is what lets an expression continue onto a line that
/// starts with `+` or `==`.
const fn can_begin_statement(kind: TokenKind) -> bool {
    !matches!(
        kind,
        TokenKind::Dot
            | TokenKind::Comma
            | TokenKind::Colon
            | TokenKind::ColonColon
            | TokenKind::RParen
            | TokenKind::RBracket
            | TokenKind::RBrace
            | TokenKind::Else
            | TokenKind::In
            | TokenKind::Arrow
            | TokenKind::FatArrow
            | TokenKind::Eq
            | TokenKind::EqEq
            | TokenKind::BangEq
            | TokenKind::Lt
            | TokenKind::Le
            | TokenKind::Gt
            | TokenKind::Ge
            | TokenKind::Plus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Percent
            | TokenKind::AmpAmp
            | TokenKind::Pipe
            | TokenKind::PipePipe
            | TokenKind::Question
            | TokenKind::At
            | TokenKind::Newline
            | TokenKind::Eof
    )
}

/// Drop the line breaks that do not separate statements.
#[must_use]
pub fn apply(tokens: &[Token]) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    // Depth of `(` and `[` only. Braces delimit blocks, so breaks inside a
    // brace are exactly what separates record fields and statements.
    let mut depth = 0u32;

    for (i, &tok) in tokens.iter().enumerate() {
        match tok.kind {
            TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RParen | TokenKind::RBracket => depth = depth.saturating_sub(1),
            TokenKind::Newline => {
                if depth > 0 {
                    continue;
                }
                // A run of newlines is one separator, and a leading one is none.
                let Some(prev) = out.last().map(|t| t.kind) else {
                    continue;
                };
                if !can_end_statement(prev) {
                    continue;
                }
                let next = tokens[i + 1..]
                    .iter()
                    .map(|t| t.kind)
                    .find(|k| *k != TokenKind::Newline)
                    .unwrap_or(TokenKind::Eof);
                if !can_begin_statement(next) {
                    continue;
                }
                out.push(tok);
                continue;
            }
            _ => {}
        }
        out.push(tok);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex;
    use vise_diag::FileId;

    /// Statement separators as `|`, everything else as its source text.
    fn shape(src: &str) -> String {
        let out = lex(src, FileId(0));
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        apply(&out.tokens)
            .iter()
            .filter(|t| t.kind != TokenKind::Eof)
            .map(|t| {
                if t.kind == TokenKind::Newline {
                    "|".to_owned()
                } else {
                    src[t.span.start as usize..t.span.end as usize].to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn a_line_break_separates_statements() {
        assert_eq!(shape("let a = 1\nlet b = 2"), "let a = 1 | let b = 2");
    }

    #[test]
    fn a_line_ending_on_an_operator_continues() {
        assert_eq!(shape("let a = b +\n  c"), "let a = b + c");
    }

    #[test]
    fn a_line_opening_on_an_infix_operator_continues() {
        assert_eq!(shape("let a = b\n  + c"), "let a = b + c");
        assert_eq!(shape("x\n  .foo()"), "x . foo ( )");
    }

    #[test]
    fn a_line_opening_on_a_prefix_operator_is_a_new_statement() {
        // `-`, `&`, and `!` can start an expression, so the break stands.
        assert_eq!(shape("a\n-b"), "a | - b");
        assert_eq!(shape("a\n&b"), "a | & b");
    }

    #[test]
    fn breaks_inside_parens_and_brackets_are_ignored() {
        assert_eq!(shape("f(\n  a,\n  b\n)"), "f ( a , b )");
        assert_eq!(shape("let x = [\n  1,\n  2,\n]"), "let x = [ 1 , 2 , ]");
    }

    #[test]
    fn breaks_inside_braces_are_significant() {
        // This is what separates record fields and enum variants.
        assert_eq!(
            shape("record R {\n  a: Int\n  b: Int\n}"),
            "record R { a : Int | b : Int }"
        );
    }

    #[test]
    fn no_separator_is_emitted_next_to_a_brace() {
        assert_eq!(shape("fn f() {\n  g()\n}"), "fn f ( ) { g ( ) }");
    }

    #[test]
    fn blank_lines_never_carry_meaning() {
        assert_eq!(shape("let a = 1\n\n\n\nlet b = 2"), "let a = 1 | let b = 2");
        assert_eq!(shape("\n\nlet a = 1\n\n"), "let a = 1");
    }

    #[test]
    fn else_continues_onto_the_previous_line() {
        assert_eq!(
            shape("if c {\n  a\n}\nelse {\n  b\n}"),
            "if c { a } else { b }"
        );
    }

    #[test]
    fn a_trailing_break_before_eof_is_dropped() {
        let out = lex("let a = 1\n", FileId(0));
        let toks = apply(&out.tokens);
        assert_eq!(toks.last().map(|t| t.kind), Some(TokenKind::Eof));
        assert_eq!(
            toks.iter().filter(|t| t.kind == TokenKind::Newline).count(),
            0
        );
    }

    #[test]
    fn the_spec_hello_world_separates_into_statements() {
        let src = "module greet\n\nfn main() {\n  let names = [\"ada\"]\n  for n in names {\n    print(\"hello, {n}\")\n  }\n}\n";
        assert_eq!(
            shape(src),
            "module greet | fn main ( ) { let names = [ \"ada\" ] | for n in names { print ( \"hello, {n}\" ) } }"
        );
    }

    #[test]
    fn the_spec_continuation_example_works() {
        assert_eq!(
            shape("let total = subtotal +\n            shipping\nlet x = 1"),
            "let total = subtotal + shipping | let x = 1"
        );
    }
}
