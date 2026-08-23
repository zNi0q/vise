//! Name resolution against the closed namespace.

use vise_check::resolve;
use vise_diag::FileId;
use vise_parse::parse;

fn module(src: &str) -> vise_ast::Module {
    let p = parse(src, FileId(0));
    assert!(
        p.diagnostics.is_empty(),
        "parse errors: {:?}",
        p.diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>()
    );
    p.module.expect("a module")
}

fn codes(src: &str) -> Vec<&'static str> {
    resolve(&module(src))
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

fn clean(src: &str) {
    let ds = resolve(&module(src));
    assert!(
        ds.is_empty(),
        "unexpected: {:?}",
        ds.iter()
            .map(|d| (d.code.as_str(), d.message.as_str()))
            .collect::<Vec<_>>()
    );
}

fn first(src: &str) -> vise_diag::Diagnostic {
    resolve(&module(src))
        .into_iter()
        .next()
        .expect("a diagnostic")
}

// --- the spec's examples ------------------------------------------------

#[test]
fn spec_hello_world_resolves() {
    clean(
        "module greet\n\nfn main() {\n  let names = [\"ada\", \"alan\"]\n  for n in names {\n    print(\"hello, {n}\")\n  }\n}\n",
    );
}

#[test]
fn spec_payments_example_resolves() {
    clean(concat!(
        "module payments\n\n",
        "use std/http@1:{post}\n\n",
        "type UserId = Int\n",
        "type Cents = Int\n\n",
        "record Receipt {\n  id: UserId\n  amount: Cents\n}\n\n",
        "enum ChargeError {\n  InsufficientFunds\n  CardDeclined(reason: Str)\n}\n\n",
        "pub fn charge(user: UserId, amount: Cents) -> Result<Receipt, ChargeError> !{net} {\n",
        "  let quote = post(\"/quote\", user)?\n",
        "  Ok(Receipt { id: user, amount: quote })\n",
        "}\n"
    ));
}

#[test]
fn spec_match_example_resolves() {
    clean(concat!(
        "module m\n",
        "enum ChargeError {\n  InsufficientFunds\n  CardDeclined(reason: Str)\n}\n",
        "fn handle(result: Result<Int, ChargeError>) {\n",
        "  match result {\n",
        "    Ok(receipt) -> log(receipt)\n",
        "    Err(CardDeclined(why)) -> retry(why)\n",
        "    Err(InsufficientFunds) -> Unit\n",
        "  }\n",
        "}\n",
        "fn log(x: Int) { }\n",
        "fn retry(x: Str) { }\n"
    ));
}

// --- the point of the feature -------------------------------------------

#[test]
fn a_hallucinated_api_is_a_compile_error() {
    assert_eq!(codes("module m\nfn f() { fetch_user(1) }\n"), ["V0201"]);
}

#[test]
fn the_diagnostic_lists_what_is_actually_in_scope() {
    // This is what makes V0201 self-repairing: the reader picks a real name
    // rather than guessing a second time.
    let d = first("module m\nfn f() { nope() }\nfn charge() { }\n");
    assert!(
        d.in_scope.contains(&"charge".to_owned()),
        "{:?}",
        d.in_scope
    );
    assert!(d.in_scope.contains(&"print".to_owned()));
    assert!(d.in_scope.contains(&"Result".to_owned()));
}

#[test]
fn a_near_miss_is_suggested() {
    let d = first("module m\nfn charge_user() { }\nfn f() { charge_usr() }\n");
    assert_eq!(d.fixes[0].edit, "charge_user");
    assert_eq!(d.fixes[0].confidence, vise_diag::Confidence::Likely);
}

#[test]
fn an_unrelated_name_gets_no_suggestion() {
    let d = first("module m\nfn charge_user() { }\nfn f() { wibble() }\n");
    assert!(d.fixes.is_empty(), "{:?}", d.fixes);
}

#[test]
fn an_import_brings_a_name_into_scope() {
    clean("module m\nuse std/http@1:{post}\nfn f() { post(\"/x\") }\n");
    assert_eq!(codes("module m\nfn f() { post(\"/x\") }\n"), ["V0201"]);
}

#[test]
fn core_needs_no_import() {
    clean("module m\nfn f() -> Result<Int, Str> { Ok(1) }\n");
}

// --- ordering and scoping -----------------------------------------------

#[test]
fn item_order_within_a_module_does_not_matter() {
    clean("module m\nfn a() { b() }\nfn b() { }\n");
}

#[test]
fn a_type_must_resolve() {
    assert_eq!(codes("module m\nfn f(x: Widget) { }\n"), ["V0201"]);
    clean("module m\nrecord Widget { n: Int }\nfn f(x: Widget) { }\n");
}

#[test]
fn a_local_leaves_scope_with_its_block() {
    assert_eq!(
        codes(
            "module m\nfn f() {\n  if c() {\n    let inner = 1\n  } else { }\n  inner\n}\nfn c() -> Bool { true }\n"
        ),
        ["V0201"]
    );
}

#[test]
fn a_let_value_sees_the_outer_binding_not_itself() {
    // `let x = x` must read the outer `x`, so this resolves.
    clean("module m\nfn f() {\n  let x = 1\n  let g = { let x = x\n    x }\n  g\n}\n");
}

#[test]
fn a_loop_binding_is_scoped_to_the_loop() {
    assert_eq!(
        codes("module m\nfn f(xs: List<Int>) {\n  for n in xs { print(n) }\n  n\n}\n"),
        ["V0201"]
    );
}

#[test]
fn match_arm_bindings_do_not_leak_between_arms() {
    assert_eq!(
        codes(concat!(
            "module m\n",
            "fn f(r: Result<Int, Str>) {\n",
            "  match r {\n",
            "    Ok(v) -> print(v)\n",
            "    Err(e) -> print(v)\n",
            "  }\n",
            "}\n"
        )),
        ["V0201"]
    );
}

#[test]
fn a_discarding_let_introduces_nothing() {
    // `let _ = ...` binds nothing (§8), so repeating it is not a redefinition.
    clean("module m\nfn f() {\n  let _ = g()\n  let _ = g()\n}\nfn g() -> Int { 1 }\n");
}

// --- contracts ----------------------------------------------------------

#[test]
fn ensures_can_speak_about_result() {
    clean("module m\nfn fee(a: Int) -> Int\n  requires a > 0\n  ensures result >= 0\n{ a / 50 }\n");
}

#[test]
fn requires_cannot_speak_about_result() {
    // `result` exists only in `ensures`.
    assert_eq!(
        codes("module m\nfn fee(a: Int) -> Int\n  requires result > 0\n{ a }\n"),
        ["V0201"]
    );
}

#[test]
fn a_record_invariant_sees_its_own_fields() {
    clean("module m\nrecord R {\n  n: Int\n  invariant n > 0\n}\n");
}

// --- generics and lifetimes ---------------------------------------------

#[test]
fn generic_parameters_are_in_scope_in_the_signature() {
    clean("module m\nfn first<T>(xs: List<T>) -> Option<T> { None }\n");
    assert_eq!(
        codes("module m\nfn first(xs: List<T>) -> Option<T> { None }\n").len(),
        2
    );
}

#[test]
fn a_lifetime_must_be_declared() {
    clean("module m\nfn longest<'a>(a: &'a Str, b: &'a Str) -> &'a Str { a }\n");
    assert_eq!(
        codes("module m\nfn longest(a: &'a Str) -> &'a Str { a }\n").len(),
        2
    );
}

// --- duplicates ---------------------------------------------------------

#[test]
fn two_definitions_of_one_name_are_rejected() {
    let d = first("module m\nfn f() { }\nfn f() { }\n");
    assert_eq!(d.code.as_str(), "V0203");
    assert_eq!(d.labels.len(), 1, "should point at the first definition");
}

#[test]
fn an_import_colliding_with_an_item_is_rejected() {
    assert_eq!(
        codes("module m\nuse std/http@1:{post}\nfn post() { }\n"),
        ["V0203"]
    );
}

#[test]
fn shadowing_an_outer_scope_is_allowed() {
    clean(
        "module m\nfn f() {\n  let x = 1\n  if x > 0 {\n    let x = 2\n    print(x)\n  } else { }\n}\n",
    );
}

// --- effects ------------------------------------------------------------

#[test]
fn effects_resolve_against_the_fixed_table_not_the_scope() {
    clean("module m\nfn f() !{net, time} { }\n");
    let d = first("module m\nfn f() !{db} { }\n");
    assert_eq!(d.code.as_str(), "V0201");
    // `in_scope` is sorted, so compare against the sorted effect table.
    assert_eq!(
        d.in_scope,
        ["env", "fs", "io", "net", "proc", "rand", "time"]
    );
}

#[test]
fn a_misspelled_effect_is_suggested() {
    let d = first("module m\nfn f() !{nett} { }\n");
    assert_eq!(d.fixes[0].edit, "net");
}

// --- module cap ---------------------------------------------------------

#[test]
fn the_module_line_cap_is_enforced() {
    use vise_check::{MAX_MODULE_LINES, check_module_length};
    let span = vise_diag::Span::new(FileId(0), 0, 1);
    assert!(check_module_length(MAX_MODULE_LINES, span).is_none());
    let d = check_module_length(MAX_MODULE_LINES + 1, span).expect("a diagnostic");
    assert_eq!(d.code.as_str(), "V0101");
    assert_eq!(d.fixes[0].kind, vise_diag::FixKind::SplitModule);
}
