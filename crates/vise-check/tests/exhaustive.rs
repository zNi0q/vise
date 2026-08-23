//! Match exhaustiveness.

use vise_check::check_exhaustive;
use vise_diag::FileId;
use vise_parse::parse;

fn module(src: &str) -> vise_ast::Module {
    let p = parse(src, FileId(0));
    assert!(
        p.diagnostics.is_empty(),
        "parse errors: {:?}",
        p.diagnostics
    );
    p.module.expect("a module")
}

fn codes(src: &str) -> Vec<&'static str> {
    check_exhaustive(&module(src))
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

fn clean(src: &str) {
    let ds = check_exhaustive(&module(src));
    assert!(
        ds.is_empty(),
        "unexpected: {:?}",
        ds.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

fn message(src: &str) -> String {
    check_exhaustive(&module(src))
        .into_iter()
        .next()
        .expect("a diagnostic")
        .message
}

/// Wrap a match in a module with an enum to match on.
fn with_enum(arms: &str) -> String {
    format!(
        "module m\nenum Colour {{\n  Red\n  Green\n  Blue\n}}\nfn f(c: Colour) {{\n  match c {{\n{arms}\n  }}\n}}\n"
    )
}

// --- enums --------------------------------------------------------------

#[test]
fn a_missing_variant_is_named() {
    let m = message(&with_enum("    Red -> 1\n    Green -> 2"));
    assert!(m.contains("Blue"), "{m}");
}

#[test]
fn covering_every_variant_passes() {
    clean(&with_enum("    Red -> 1\n    Green -> 2\n    Blue -> 3"));
}

#[test]
fn several_missing_variants_are_all_named() {
    let m = message(&with_enum("    Red -> 1"));
    assert!(m.contains("Green") && m.contains("Blue"), "{m}");
}

#[test]
fn a_wildcard_covers_the_rest() {
    // §6: `_` is allowed. The rule prevents forgetting a case, not catch-alls.
    clean(&with_enum("    Red -> 1\n    _ -> 0"));
}

#[test]
fn a_binding_also_covers_the_rest() {
    clean(&with_enum("    Red -> 1\n    other -> 0"));
}

// --- built-in enums -----------------------------------------------------

#[test]
fn result_needs_both_arms() {
    let src = "module m\nfn f(r: Result<Int, Str>) {\n  match r {\n    Ok(v) -> v\n  }\n}\n";
    assert_eq!(codes(src), ["V0301"]);
    assert!(message(src).contains("Err"));
    clean(
        "module m\nfn f(r: Result<Int, Str>) {\n  match r {\n    Ok(v) -> 1\n    Err(e) -> 0\n  }\n}\n",
    );
}

#[test]
fn option_needs_both_arms() {
    let src = "module m\nfn f(o: Option<Int>) {\n  match o {\n    Some(v) -> v\n  }\n}\n";
    assert!(message(src).contains("None"), "{}", message(src));
}

// --- nesting ------------------------------------------------------------

#[test]
fn nesting_inside_a_single_field_constructor_is_checked() {
    // The spec's own example, minus one inner arm.
    let src = concat!(
        "module m\n",
        "enum ChargeError {\n  InsufficientFunds\n  CardDeclined(reason: Str)\n}\n",
        "fn f(r: Result<Int, ChargeError>) {\n",
        "  match r {\n",
        "    Ok(v) -> 1\n",
        "    Err(CardDeclined(why)) -> 2\n",
        "  }\n",
        "}\n"
    );
    let m = message(src);
    assert!(m.contains("InsufficientFunds"), "{m}");
}

#[test]
fn spec_match_example_is_exhaustive() {
    clean(concat!(
        "module m\n",
        "enum ChargeError {\n  InsufficientFunds\n  CardDeclined(reason: Str)\n}\n",
        "fn f(r: Result<Int, ChargeError>) {\n",
        "  match r {\n",
        "    Ok(receipt) -> 1\n",
        "    Err(CardDeclined(why)) -> 2\n",
        "    Err(InsufficientFunds) -> 3\n",
        "  }\n",
        "}\n"
    ));
}

// --- conservatism -------------------------------------------------------

#[test]
fn literal_patterns_ask_for_a_catch_all() {
    // Vise cannot prove that literals cover Int.
    let src = "module m\nfn f(n: Int) {\n  match n {\n    0 -> 1\n    1 -> 2\n  }\n}\n";
    assert_eq!(codes(src), ["V0301"]);
    assert!(message(src).contains('_'));
}

#[test]
fn an_unknown_constructor_produces_no_second_complaint() {
    // Already V0201; piling on would be noise.
    clean("module m\nfn f(x: Int) {\n  match x {\n    Wibble -> 1\n  }\n}\n");
}

#[test]
fn a_multi_field_constructor_is_checked_but_not_descended_into() {
    // Deliberately conservative: no false positive from column-wise guessing.
    clean(concat!(
        "module m\n",
        "enum Pair {\n  Both(a: Int, b: Int)\n}\n",
        "fn f(p: Pair) {\n  match p {\n    Both(x, y) -> 1\n  }\n}\n"
    ));
}

// --- traversal ----------------------------------------------------------

#[test]
fn a_match_nested_in_another_matchs_arm_is_checked() {
    let src = concat!(
        "module m\n",
        "enum Colour {\n  Red\n  Green\n}\n",
        "fn f(o: Option<Colour>) {\n",
        "  match o {\n",
        "    None -> 0\n",
        "    Some(c) -> match c {\n      Red -> 1\n    }\n",
        "  }\n",
        "}\n"
    );
    assert_eq!(codes(src), ["V0301"]);
    assert!(message(src).contains("Green"));
}

#[test]
fn a_match_inside_a_loop_is_checked() {
    assert_eq!(
        codes(concat!(
            "module m\n",
            "enum Colour {\n  Red\n  Green\n}\n",
            "fn f(cs: List<Colour>) {\n",
            "  for c in cs {\n    match c {\n      Red -> 1\n    }\n  }\n",
            "}\n"
        )),
        ["V0301"]
    );
}

#[test]
fn the_fix_offers_the_missing_arms() {
    let d = check_exhaustive(&module(&with_enum("    Red -> 1")))
        .into_iter()
        .next()
        .expect("a diagnostic");
    assert_eq!(d.fixes[0].kind, vise_diag::FixKind::AddMatchArm);
    assert!(
        d.fixes[0].edit.contains("Green -> ..."),
        "{}",
        d.fixes[0].edit
    );
}
