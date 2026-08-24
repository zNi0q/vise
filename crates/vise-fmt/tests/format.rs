//! The canonical formatter.
//!
//! Two properties matter more than any layout choice: formatting is idempotent,
//! and formatted source re-parses to the same program. Together they mean a
//! diff between two Vise files reflects a difference in the program rather than
//! a difference in habit.

use std::path::PathBuf;

use vise_diag::FileId;
use vise_fmt::format;
use vise_parse::parse;

fn module(src: &str) -> vise_ast::Module {
    let p = parse(src, FileId(0));
    assert!(
        p.diagnostics.is_empty(),
        "parse errors in\n{src}\n{:?}",
        p.diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.message.as_str()))
            .collect::<Vec<_>>()
    );
    p.module.expect("a module")
}

fn fmt(src: &str) -> String {
    format(&module(src))
}

/// Format, re-parse, format again. The two must agree.
fn assert_stable(src: &str) {
    let once = fmt(src);
    let twice = fmt(&once);
    assert_eq!(once, twice, "formatting is not idempotent for:\n{src}");
}

// --- the properties -----------------------------------------------------

#[test]
fn formatting_is_idempotent_and_reparses() {
    for src in [
        "module m\nfn f() { }\n",
        "module m\nfn f(a: Int, b: Str) -> Bool { true }\n",
        "module m\ntype UserId = Int\n",
        "module m\nrecord R {\n  a: Int\n  b: List<Str>\n}\n",
        "module m\nenum E {\n  A\n  B(n: Int, s: Str)\n}\n",
        "module m\nuse std/http@1:{post, Response}\nfn f() { }\n",
        "module m\nfn f<T>(xs: List<T>) -> Option<T> { None }\n",
        "module m\nfn f<'a>(a: &'a Str, b: &'a mut Str) -> &'a Str { a }\n",
        "module m\nfn f() !{io, net} { }\n",
        "module m\nfn f(a: Int) -> Int\n  requires a > 0\n  ensures result >= 0\n{ a }\n",
        "module m\nfn f() {\n  let a = 1\n  var b = 2\n  b = a + b\n}\n",
        "module m\nfn f(xs: List<Int>) {\n  for x in xs { g(x) }\n  while false { }\n}\nfn g(n: Int) { }\n",
        "module m\nfn f(r: Result<Int, Str>) -> Int {\n  match r {\n    Ok(v) -> v\n    Err(e) -> 0\n  }\n}\n",
        "module m\nfn f() -> Int { if true { 1 } else { 2 } }\n",
        "module m\nrecord P {\n  x: Int\n}\nfn f() -> P { P { x: 1 } }\n",
        "module m\nfn f() -> List<Int> { [1, 2, 3] }\n",
        "module m\nfn f(s: Str) -> Str { \"a{s}b\" }\n",
        "module m\nfn f() -> Result<Int, Str> {\n  let v = g()?\n  Ok(v)\n}\nfn g() -> Result<Int, Str> { Ok(1) }\n",
    ] {
        assert_stable(src);
        // Formatted output must itself be valid Vise.
        let _ = module(&fmt(src));
    }
}

#[test]
fn every_example_file_formats_stably() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for entry in std::fs::read_dir(&dir).expect("examples/ should exist") {
        let path = entry.expect("a readable entry").path();
        if path.extension().is_none_or(|e| e != "vise") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("readable");
        // Some examples are deliberately broken; only format the ones that parse.
        if parse(&text, FileId(0)).has_errors() {
            continue;
        }
        assert_stable(&text);
    }
}

// --- layout -------------------------------------------------------------

#[test]
fn the_spec_hello_world_formats_as_written() {
    let src = "module greet\nfn main() {\nlet names = [\"ada\", \"alan\"]\nfor n in names {\nprint(\"hello, {n}\")\n}\n}\n";
    assert_eq!(
        fmt(src),
        concat!(
            "module greet\n",
            "\n",
            "fn main() {\n",
            "  let names = [\"ada\", \"alan\"]\n",
            "  for n in names {\n",
            "    print(\"hello, {n}\")\n",
            "  }\n",
            "}\n"
        )
    );
}

#[test]
fn contracts_put_the_brace_on_its_own_line() {
    // Matching the spec's own example.
    let src = "module m\nfn fee(a: Int) -> Int requires a > 0 ensures result >= 0 { a / 50 }\n";
    assert_eq!(
        fmt(src),
        concat!(
            "module m\n",
            "\n",
            "fn fee(a: Int) -> Int\n",
            "  requires a > 0\n",
            "  ensures result >= 0\n",
            "{\n",
            "  a / 50\n",
            "}\n"
        )
    );
}

#[test]
fn imports_sit_between_the_header_and_the_items() {
    assert_eq!(
        fmt("module m\nuse std/http@1:{post}\nfn f() { }\n"),
        "module m\n\nuse std/http@1:{post}\n\nfn f() {\n}\n"
    );
}

// --- parenthesisation ---------------------------------------------------

#[test]
fn only_necessary_parentheses_survive() {
    let body = |src: &str| {
        let out = fmt(&format!("module m\nfn f() -> Int {{ {src} }}\n"));
        out.lines().nth(3).unwrap_or_default().trim().to_owned()
    };
    assert_eq!(body("(1 + 2) * 3"), "(1 + 2) * 3");
    assert_eq!(body("1 + (2 * 3)"), "1 + 2 * 3");
    assert_eq!(body("(1 + 2) + 3"), "1 + 2 + 3");
    assert_eq!(body("1 + (2 + 3)"), "1 + (2 + 3)");
    assert_eq!(body("((1))"), "1");
}

#[test]
fn a_unary_minus_after_a_binary_one_stays_readable() {
    // `--` opens a comment, so the printer must not emit two adjacent minuses.
    let out = fmt("module m\nfn f(a: Int, b: Int) -> Int { a - (-b) }\n");
    assert!(!out.contains("--"), "{out}");
    let _ = module(&out); // and it must re-parse
}

// --- escaping -----------------------------------------------------------

#[test]
fn string_escapes_survive_a_round_trip() {
    for src in [
        r#""a\nb""#,
        r#""quote \" here""#,
        r#""back \\ slash""#,
        r#""brace \{ here""#,
        r#""tab\there""#,
    ] {
        let program = format!("module m\nfn f() -> Str {{ {src} }}\n");
        assert_stable(&program);
    }
}

#[test]
fn a_literal_brace_does_not_become_an_interpolation() {
    let once = fmt(r#"module m
fn f() -> Str { "a \{ b" }
"#);
    assert!(once.contains("\\{"), "{once}");
    assert_stable(&once);
}
