//! Lexer behaviour, driven from the spec's own examples.

use vise_diag::FileId;
use vise_lex::{TokenKind as T, lex};

fn kinds(src: &str) -> Vec<T> {
    lex(src, FileId(0))
        .tokens
        .into_iter()
        .map(|t| t.kind)
        .filter(|k| *k != T::Newline && *k != T::Eof)
        .collect()
}

fn codes(src: &str) -> Vec<&'static str> {
    lex(src, FileId(0))
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

fn clean(src: &str) -> Vec<T> {
    let out = lex(src, FileId(0));
    assert!(
        out.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        out.diagnostics.iter().map(|d| d.code).collect::<Vec<_>>()
    );
    out.tokens
        .into_iter()
        .map(|t| t.kind)
        .filter(|k| *k != T::Newline && *k != T::Eof)
        .collect()
}

// --- the spec's examples ------------------------------------------------

#[test]
fn spec_hello_world_lexes_cleanly() {
    let src = r#"module greet

fn main() {
  let names = ["ada", "alan", "grace"]
  for n in names {
    print("hello, {n}")
  }
}
"#;
    let out = lex(src, FileId(0));
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(out.tokens.last().map(|t| t.kind), Some(T::Eof));
}

#[test]
fn spec_payments_example_lexes_cleanly() {
    let src = r#"module payments

use std/http@1:{post}

type UserId = Int

enum ChargeError {
  InsufficientFunds
  CardDeclined(reason: Str)
}

pub fn charge(user: UserId, amount: Cents) -> Result<Receipt, ChargeError> !{net} {
  let quote = post("/quote", user)?
  Ok(Receipt { id: user, amount: quote })
}
"#;
    let out = lex(src, FileId(0));
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn spec_ownership_example_lexes_cleanly() {
    let src = r#"fn longest<'a>(a: &'a Str, b: &'a Str) -> &'a Str { a }
fn total(items: &List<Item>) -> Cents {
  var sum = 0
  for it in items { sum = sum + it.price }
  sum
}
"#;
    let out = lex(src, FileId(0));
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

// --- names --------------------------------------------------------------

#[test]
fn keywords_are_distinguished_from_identifiers() {
    assert_eq!(clean("let x"), [T::Let, T::Ident]);
    assert_eq!(clean("lets"), [T::Ident]);
    assert_eq!(clean("returns"), [T::Ident]);
}

#[test]
fn casing_decides_value_from_type() {
    assert_eq!(clean("user_id"), [T::Ident]);
    assert_eq!(clean("UserId"), [T::TypeIdent]);
    assert_eq!(clean("_"), [T::Underscore]);
    assert_eq!(clean("_x"), [T::Ident]);
}

#[test]
fn camel_case_is_rejected_where_it_is_written() {
    // Without this rule `myVar` would split into `my` and the type `Var`, and
    // fail somewhere far from the mistake.
    assert_eq!(codes("let myVar = 1"), ["V0006"]);
    let d = &lex("let myVar = 1", FileId(0)).diagnostics[0];
    assert_eq!(d.fixes[0].edit, "my_var");
}

#[test]
fn underscores_in_type_names_are_rejected() {
    assert_eq!(codes("type My_Type = Int"), ["V0007"]);
}

// --- trivia -------------------------------------------------------------

#[test]
fn comments_run_to_end_of_line() {
    assert_eq!(
        clean("let x -- a comment\nlet y"),
        [T::Let, T::Ident, T::Let, T::Ident]
    );
}

#[test]
fn line_breaks_are_tokens_but_horizontal_space_is_not() {
    let ks: Vec<_> = lex("a\n b", FileId(0))
        .tokens
        .iter()
        .map(|t| t.kind)
        .collect();
    assert_eq!(ks, [T::Ident, T::Newline, T::Ident, T::Eof]);
}

#[test]
fn adjacent_minuses_are_a_comment_not_a_subtraction() {
    // Documented hazard, inherited from `--` comments: `a --b` is `a` followed
    // by a comment, not `a - (-b)`. A space between the two minuses is enough
    // to disambiguate, and both spaced forms lex as arithmetic.
    assert_eq!(clean("a --b"), [T::Ident]);
    assert_eq!(clean("a - -b"), [T::Ident, T::Minus, T::Minus, T::Ident]);
    assert_eq!(
        clean("a - (-b)"),
        [T::Ident, T::Minus, T::LParen, T::Minus, T::Ident, T::RParen]
    );
}

// --- numbers ------------------------------------------------------------

#[test]
fn integers_and_floats_are_distinguished() {
    assert_eq!(clean("42"), [T::Int]);
    assert_eq!(clean("1.5"), [T::Float]);
    assert_eq!(clean("1_000_000"), [T::Int]);
}

#[test]
fn a_dot_only_continues_a_number_when_a_digit_follows() {
    // Field access must keep working.
    assert_eq!(clean("x.0"), [T::Ident, T::Dot, T::Int]);
    assert_eq!(clean("1.max"), [T::Int, T::Dot, T::Ident]);
}

#[test]
fn v0_has_no_hex_octal_or_binary_literals() {
    assert_eq!(codes("0x10"), ["V0004"]);
    assert_eq!(codes("0b1010"), ["V0004"]);
    assert_eq!(codes("0o17"), ["V0004"]);
}

#[test]
fn malformed_numbers_are_reported() {
    assert_eq!(codes("1abc"), ["V0004"]);
    assert_eq!(codes("1_"), ["V0004"]);
    assert_eq!(codes("1.5e3"), ["V0004"]);
}

// --- strings ------------------------------------------------------------

#[test]
fn strings_and_interpolation_lex_as_one_token() {
    assert_eq!(clean(r#""hello""#), [T::Str]);
    assert_eq!(clean(r#""hello, {name}""#), [T::Str]);
    assert_eq!(clean(r#""{a}{b}""#), [T::Str]);
}

#[test]
fn every_documented_escape_is_accepted() {
    assert_eq!(clean(r#""a\nb\tc\\d\"e\{f\u{1F600}""#), [T::Str]);
}

#[test]
fn undocumented_escapes_are_rejected() {
    assert_eq!(codes(r#""a\qb""#), ["V0003"]);
    assert_eq!(codes(r#""a\rb""#), ["V0003"]);
}

#[test]
fn malformed_unicode_escapes_are_rejected() {
    assert_eq!(codes(r#""\u1234""#), ["V0003"]); // missing braces
    assert_eq!(codes(r#""\u{}""#), ["V0003"]); // no digits
    assert_eq!(codes(r#""\u{1234567}""#), ["V0003"]); // too many digits
    assert_eq!(codes(r#""\u{110000}""#), ["V0003"]); // past the last code point
    assert_eq!(codes(r#""\u{D800}""#), ["V0003"]); // lone surrogate
}

#[test]
fn a_string_may_not_span_a_line() {
    assert_eq!(codes("\"abc\ndef\""), ["V0002", "V0002"]);
}

#[test]
fn unbalanced_interpolation_is_reported() {
    assert_eq!(codes(r#""hello, {name""#), ["V0005"]);
}

#[test]
fn an_escaped_brace_is_not_an_interpolation() {
    assert_eq!(codes(r#""a \{ b""#), Vec::<&str>::new());
}

#[test]
fn nested_strings_inside_interpolation_are_rejected() {
    assert_eq!(codes(r#""{f("x")}""#), ["V0005"]);
}

// --- characters and lifetimes -------------------------------------------

#[test]
fn a_quote_starts_a_char_or_a_lifetime() {
    assert_eq!(clean("'a'"), [T::Char]);
    assert_eq!(clean("'a"), [T::Lifetime]);
    assert_eq!(clean("'static"), [T::Lifetime]);
    assert_eq!(clean("'_'"), [T::Char]);
    assert_eq!(clean(r"'\n'"), [T::Char]);
    assert_eq!(clean("&'a Str"), [T::Amp, T::Lifetime, T::TypeIdent]);
}

#[test]
fn unterminated_char_literals_are_reported() {
    assert_eq!(codes("'ab'"), ["V0008"]);
    assert_eq!(codes("'"), ["V0008"]);
}

// --- punctuation --------------------------------------------------------

#[test]
fn multi_character_operators_win_over_single() {
    assert_eq!(
        clean(":: -> => == != <= >= && ||"),
        [
            T::ColonColon,
            T::Arrow,
            T::FatArrow,
            T::EqEq,
            T::BangEq,
            T::Le,
            T::Ge,
            T::AmpAmp,
            T::PipePipe
        ]
    );
    assert_eq!(
        clean(": - = ! < > & |"),
        [
            T::Colon,
            T::Minus,
            T::Eq,
            T::Bang,
            T::Lt,
            T::Gt,
            T::Amp,
            T::Pipe
        ]
    );
}

#[test]
fn an_effect_row_lexes_as_bang_then_a_brace_group() {
    assert_eq!(
        clean("!{net, time}"),
        [T::Bang, T::LBrace, T::Ident, T::Comma, T::Ident, T::RBrace]
    );
}

#[test]
fn a_versioned_import_path_lexes() {
    assert_eq!(
        clean("use std/http@1:{post}"),
        [
            T::Use,
            T::Ident,
            T::Slash,
            T::Ident,
            T::At,
            T::Int,
            T::Colon,
            T::LBrace,
            T::Ident,
            T::RBrace
        ]
    );
}

// --- error recovery -----------------------------------------------------

#[test]
fn lexing_continues_past_an_unknown_character() {
    // One run must report every lexical fault: a machine author pays a full
    // round trip for each compile.
    assert_eq!(codes("a # b $ c"), ["V0001", "V0001"]);
    assert_eq!(clean("a b"), [T::Ident, T::Ident]);
}

#[test]
fn an_unknown_character_still_produces_a_token() {
    assert_eq!(kinds("a # b"), [T::Ident, T::Error, T::Ident]);
}

// --- spans --------------------------------------------------------------

#[test]
fn spans_cover_exactly_the_token_text() {
    let src = "let name = 42";
    let out = lex(src, FileId(0));
    let text: Vec<&str> = out
        .tokens
        .iter()
        .filter(|t| t.kind != T::Eof)
        .map(|t| &src[t.span.start as usize..t.span.end as usize])
        .collect();
    assert_eq!(text, ["let", "name", "=", "42"]);
}

#[test]
fn a_string_span_includes_both_quotes() {
    let src = r#"("hi")"#;
    let out = lex(src, FileId(0));
    let s = out
        .tokens
        .iter()
        .find(|t| t.kind == T::Str)
        .expect("a string token");
    assert_eq!(&src[s.span.start as usize..s.span.end as usize], r#""hi""#);
}

#[test]
fn an_empty_file_yields_only_eof() {
    let out = lex("", FileId(0));
    assert_eq!(
        out.tokens.iter().map(|t| t.kind).collect::<Vec<_>>(),
        [T::Eof]
    );
    assert!(out.diagnostics.is_empty());
}

#[test]
fn without_newlines_drops_layout() {
    let out = lex("a\nb\n", FileId(0));
    assert_eq!(
        out.without_newlines()
            .iter()
            .map(|t| t.kind)
            .collect::<Vec<_>>(),
        [T::Ident, T::Ident, T::Eof]
    );
}
