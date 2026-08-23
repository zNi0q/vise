//! Discarded `Result` values.

use vise_check::check_results;
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
    check_results(&module(src))
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

fn clean(src: &str) {
    let ds = check_results(&module(src));
    assert!(
        ds.is_empty(),
        "unexpected: {:?}",
        ds.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

fn first(src: &str) -> vise_diag::Diagnostic {
    check_results(&module(src))
        .into_iter()
        .next()
        .expect("a diagnostic")
}

/// A module with `save` returning a Result and `plain` returning an Int.
fn with_helpers(body: &str) -> String {
    format!(
        "module m\nfn save(n: Int) -> Result<Int, Str> {{ Ok(n) }}\nfn plain(n: Int) -> Int {{ n }}\nfn f() {{\n{body}\n}}\n"
    )
}

// --- the rule -----------------------------------------------------------

#[test]
fn a_dropped_result_is_an_error() {
    assert_eq!(codes(&with_helpers("  save(1)\n  plain(2)")), ["V0501"]);
}

#[test]
fn a_non_result_call_is_fine() {
    clean(&with_helpers("  plain(1)\n  plain(2)"));
}

#[test]
fn propagating_with_question_mark_consumes_it() {
    clean(
        "module m\nfn save(n: Int) -> Result<Int, Str> { Ok(n) }\nfn f() -> Result<Int, Str> {\n  save(1)?\n  Ok(0)\n}\n",
    );
}

#[test]
fn binding_the_result_consumes_it() {
    clean(&with_helpers("  let r = save(1)\n  plain(2)"));
}

#[test]
fn discarding_deliberately_is_allowed() {
    // §8: `let _ = ...` is the sanctioned discard.
    clean(&with_helpers("  let _ = save(1)\n  plain(2)"));
}

#[test]
fn matching_on_it_consumes_it() {
    clean(
        "module m\nfn save(n: Int) -> Result<Int, Str> { Ok(n) }\nfn f() {\n  match save(1) {\n    Ok(v) -> 1\n    Err(e) -> 0\n  }\n}\n",
    );
}

// --- discard positions --------------------------------------------------

#[test]
fn a_loop_body_discards_its_value() {
    assert_eq!(
        codes(
            "module m\nfn save(n: Int) -> Result<Int, Str> { Ok(n) }\nfn f(xs: List<Int>) {\n  for x in xs { save(x) }\n}\n"
        ),
        ["V0501"]
    );
}

#[test]
fn a_function_returning_unit_discards_its_tail() {
    // No `->` means Unit, so the tail value goes nowhere.
    assert_eq!(
        codes("module m\nfn save(n: Int) -> Result<Int, Str> { Ok(n) }\nfn f() {\n  save(1)\n}\n"),
        ["V0501"]
    );
}

#[test]
fn a_function_returning_result_uses_its_tail() {
    clean(
        "module m\nfn save(n: Int) -> Result<Int, Str> { Ok(n) }\nfn f() -> Result<Int, Str> {\n  save(1)\n}\n",
    );
}

#[test]
fn both_branches_of_a_discarded_if_are_checked() {
    assert_eq!(
        codes(&with_helpers(
            "  if true { save(1) } else { save(2) }\n  plain(0)"
        )),
        ["V0501", "V0501"]
    );
}

#[test]
fn every_arm_of_a_discarded_match_is_checked() {
    assert_eq!(
        codes(&with_helpers(
            "  match plain(0) {\n    0 -> save(1)\n    _ -> save(2)\n  }\n  plain(0)"
        )),
        ["V0501", "V0501"]
    );
}

#[test]
fn a_result_used_as_an_argument_is_not_discarded() {
    clean(
        "module m\nfn save(n: Int) -> Result<Int, Str> { Ok(n) }\nfn take(r: Result<Int, Str>) -> Int { 0 }\nfn f() {\n  take(save(1))\n}\n",
    );
}

// --- suggestions --------------------------------------------------------

#[test]
fn question_mark_is_offered_only_where_it_would_compile() {
    // `?` needs the enclosing function to return Result.
    let inside = first(
        "module m\nfn save(n: Int) -> Result<Int, Str> { Ok(n) }\nfn f() -> Result<Int, Str> {\n  save(1)\n  Ok(0)\n}\n",
    );
    let kinds: Vec<_> = inside.fixes.iter().map(|f| f.kind).collect();
    assert!(
        kinds.contains(&vise_diag::FixKind::HandleResult),
        "{kinds:?}"
    );

    let outside = first(&with_helpers("  save(1)\n  plain(0)"));
    let kinds: Vec<_> = outside.fixes.iter().map(|f| f.kind).collect();
    assert!(
        !kinds.contains(&vise_diag::FixKind::HandleResult),
        "{kinds:?}"
    );
    assert!(kinds.contains(&vise_diag::FixKind::DiscardResult));
}

#[test]
fn no_fix_is_certain_because_the_choice_is_the_authors() {
    let d = first(&with_helpers("  save(1)\n  plain(0)"));
    assert!(
        d.autofix().is_none(),
        "handling versus discarding is not automatable"
    );
}

// --- limits -------------------------------------------------------------

#[test]
fn an_imported_function_is_opaque() {
    // No signature, so nothing can be proven. Documented, not silently wrong.
    clean("module m\nuse std/db@1:{save}\nfn f() {\n  save(1)\n}\n");
}

// --- the spec's example -------------------------------------------------

#[test]
fn spec_payments_example_passes() {
    clean(concat!(
        "module payments\n",
        "use std/http@1:{post}\n",
        "type UserId = Int\n",
        "record Receipt {\n  id: UserId\n  amount: Int\n}\n",
        "enum ChargeError {\n  InsufficientFunds\n}\n",
        "pub fn charge(user: UserId, amount: Int) -> Result<Receipt, ChargeError> !{net} {\n",
        "  let quote = post(\"/quote\", user)?\n",
        "  Ok(Receipt { id: user, amount: quote })\n",
        "}\n"
    ));
}
