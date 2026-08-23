//! Effect inference and row checking.

use vise_check::check_effects;
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
    check_effects(&module(src))
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

fn clean(src: &str) {
    let ds = check_effects(&module(src));
    assert!(
        ds.is_empty(),
        "unexpected: {:?}",
        ds.iter()
            .map(|d| (d.code.as_str(), d.message.as_str()))
            .collect::<Vec<_>>()
    );
}

fn first(src: &str) -> vise_diag::Diagnostic {
    check_effects(&module(src))
        .into_iter()
        .next()
        .expect("a diagnostic")
}

// --- omitted versus empty -----------------------------------------------

#[test]
fn an_omitted_row_is_inferred_and_never_wrong() {
    // §7: omitting the row means "whatever the body implies".
    clean("module m\nfn f() { print(\"x\") }\n");
}

#[test]
fn an_empty_row_asserts_purity() {
    // `!{}` is a claim, so performing io breaks it.
    assert_eq!(codes("module m\nfn f() !{} { print(\"x\") }\n"), ["V0401"]);
    clean("module m\nfn f() !{} { }\n");
}

#[test]
fn a_correct_row_passes() {
    clean("module m\nfn f() !{io} { print(\"x\") }\n");
}

// --- the row is exact ---------------------------------------------------

#[test]
fn an_effect_that_is_never_performed_is_reported() {
    // §7: the row is exact, not an upper bound.
    let d = first("module m\nfn f() !{net} { }\n");
    assert_eq!(d.code.as_str(), "V0402");
    assert_eq!(d.severity, vise_diag::Severity::Warning);
    assert_eq!(d.fixes[0].edit, "!{}");
}

#[test]
fn main_may_declare_effects_it_only_passes_through() {
    // §7 names `main` as the one exemption.
    clean("module m\nfn main() !{net, fs} { }\n");
}

#[test]
fn the_fix_widens_the_row_to_exactly_what_is_needed() {
    let d = first("module m\nfn f() !{net} { print(\"x\") }\n");
    assert_eq!(d.code.as_str(), "V0401");
    assert_eq!(d.fixes[0].edit, "!{io, net}");
    assert_eq!(d.fixes[0].confidence, vise_diag::Confidence::Certain);
}

#[test]
fn the_diagnostic_names_the_call_that_introduced_the_effect() {
    let d = first("module m\nfn f() !{} { print(\"x\") }\n");
    assert_eq!(d.labels.len(), 1);
    assert!(d.labels[0].message.contains("print"), "{:?}", d.labels[0]);
}

// --- propagation --------------------------------------------------------

#[test]
fn effects_propagate_from_callee_to_caller() {
    let d = first("module m\nfn low() { print(\"x\") }\nfn high() !{} { low() }\n");
    assert_eq!(d.code.as_str(), "V0401");
    assert!(d.labels[0].message.contains("low"));
}

#[test]
fn a_declared_row_is_the_interface_callers_see() {
    // The caller trusts `low`'s signature rather than re-deriving it.
    let d = first("module m\nfn low() !{io} { print(\"x\") }\nfn high() !{} { low() }\n");
    assert_eq!(d.code.as_str(), "V0401");
    assert!(d.fixes[0].edit.contains("io"));
}

#[test]
fn effects_propagate_through_a_chain() {
    assert_eq!(
        codes("module m\nfn a() { print(\"x\") }\nfn b() { a() }\nfn c() !{} { b() }\n"),
        ["V0401"]
    );
}

#[test]
fn mutual_recursion_terminates() {
    clean("module m\nfn a() !{} { b() }\nfn b() !{} { a() }\n");
    assert_eq!(
        codes("module m\nfn a() { b() }\nfn b() { a()\n  print(\"x\") }\nfn c() !{} { a() }\n"),
        ["V0401"]
    );
}

#[test]
fn self_recursion_terminates() {
    clean("module m\nfn a() !{} { a() }\n");
}

// --- effects found in every position ------------------------------------

#[test]
fn an_effect_inside_a_loop_body_is_found() {
    assert_eq!(
        codes("module m\nfn f(xs: List<Int>) !{} {\n  for x in xs { print(x) }\n}\n"),
        ["V0401"]
    );
}

#[test]
fn an_effect_inside_a_match_arm_is_found() {
    assert_eq!(
        codes(
            "module m\nfn f(r: Result<Int, Str>) !{} {\n  match r {\n    Ok(v) -> print(v)\n    Err(e) -> Unit\n  }\n}\n"
        ),
        ["V0401"]
    );
}

#[test]
fn an_effect_inside_a_string_interpolation_is_found() {
    // A pure call inside `{...}` is fine.
    clean("module m\nfn g() -> Int { 1 }\nfn f() !{} {\n  let s = \"v={g()}\"\n  s\n}\n");
    // An effectful one is not, even though it sits inside a literal.
    assert_eq!(
        codes(concat!(
            "module m\n",
            "fn g() -> Int { print(\"x\")\n  1 }\n",
            "fn f() !{} {\n  let s = \"v={g()}\"\n  s\n}\n"
        )),
        ["V0401"]
    );
}

// --- imports are opaque -------------------------------------------------

#[test]
fn an_import_makes_unused_unprovable_so_v0402_is_suppressed() {
    // `post` has no signature: Vise has no module system yet. Absence of proof
    // is not proof of absence, so the row is not called unused.
    clean("module m\nuse std/http@1:{post}\nfn f() !{net} { post(\"/x\") }\n");
}

#[test]
fn an_import_does_not_suppress_effects_that_are_known() {
    assert_eq!(
        codes(
            "module m\nuse std/http@1:{post}\nfn f() !{net} {\n  post(\"/x\")\n  print(\"done\")\n}\n"
        ),
        ["V0401"]
    );
}

#[test]
fn unknown_propagates_only_through_undeclared_functions() {
    // `wrapper` declares a row, so callers trust it and unused stays provable.
    assert_eq!(
        codes(concat!(
            "module m\n",
            "use std/http@1:{post}\n",
            "fn wrapper() !{net} { post(\"/x\") }\n",
            "fn f() !{fs} { wrapper() }\n"
        )),
        ["V0401", "V0402"]
    );
}

// --- the spec's example -------------------------------------------------

#[test]
fn spec_payments_example_passes() {
    clean(concat!(
        "module payments\n",
        "use std/http@1:{post}\n",
        "type UserId = Int\n",
        "type Cents = Int\n",
        "record Receipt {\n  id: UserId\n  amount: Cents\n}\n",
        "enum ChargeError {\n  InsufficientFunds\n  CardDeclined(reason: Str)\n}\n",
        "pub fn charge(user: UserId, amount: Cents) -> Result<Receipt, ChargeError> !{net} {\n",
        "  let quote = post(\"/quote\", user)?\n",
        "  Ok(Receipt { id: user, amount: quote })\n",
        "}\n"
    ));
}
