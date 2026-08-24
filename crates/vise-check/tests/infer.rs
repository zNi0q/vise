//! Type inference and checking.

use vise_check::check_types;
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
    check_types(&module(src))
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

fn clean(src: &str) {
    let ds = check_types(&module(src));
    assert!(
        ds.is_empty(),
        "unexpected: {:?}",
        ds.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

fn message(src: &str) -> String {
    check_types(&module(src))
        .into_iter()
        .next()
        .expect("a diagnostic")
        .message
}

// --- distinct types -----------------------------------------------------

#[test]
fn a_distinct_type_is_not_its_representation() {
    // §4: `type UserId = Int` creates a distinct type.
    let src = "module m\ntype UserId = Int\nfn f(u: UserId) -> Int { 0 }\nfn g() -> Int { f(1) }\n";
    assert_eq!(codes(src), ["V0302"]);
    assert!(
        message(src).contains("UserId") && message(src).contains("Int"),
        "{}",
        message(src)
    );
    clean(
        "module m\ntype UserId = Int\nfn f(u: UserId) -> Int { 0 }\nfn g(u: UserId) -> Int { f(u) }\n",
    );
}

#[test]
fn swapped_arguments_are_caught() {
    // The payoff of distinct types: order mistakes become type errors.
    assert_eq!(
        codes(concat!(
            "module m\n",
            "type UserId = Int\n",
            "type Cents = Int\n",
            "fn charge(u: UserId, c: Cents) -> Int { 0 }\n",
            "fn f(u: UserId, c: Cents) -> Int { charge(c, u) }\n"
        )),
        ["V0302", "V0302"]
    );
}

// --- calls --------------------------------------------------------------

#[test]
fn argument_count_is_checked() {
    let src = "module m\nfn f(a: Int, b: Int) -> Int { a }\nfn g() -> Int { f(1) }\n";
    assert_eq!(codes(src), ["V0302"]);
    assert!(
        message(src).contains("takes 2 arguments"),
        "{}",
        message(src)
    );
}

#[test]
fn argument_types_are_checked() {
    assert_eq!(
        codes("module m\nfn f(a: Int) -> Int { a }\nfn g() -> Int { f(\"x\") }\n"),
        ["V0302"]
    );
}

#[test]
fn a_return_type_flows_to_the_call_site() {
    clean("module m\nfn f() -> Str { \"x\" }\nfn g() -> Str { f() }\n");
    assert_eq!(
        codes("module m\nfn f() -> Str { \"x\" }\nfn g() -> Int { f() }\n"),
        ["V0302"]
    );
}

// --- control flow -------------------------------------------------------

#[test]
fn a_condition_must_be_bool() {
    assert_eq!(
        codes("module m\nfn f() -> Int { if 1 { 2 } else { 3 } }\n"),
        ["V0302"]
    );
    clean("module m\nfn f() -> Int { if true { 2 } else { 3 } }\n");
}

#[test]
fn both_branches_of_an_if_must_agree() {
    assert_eq!(
        codes("module m\nfn f() -> Int { if true { 1 } else { \"x\" } }\n"),
        ["V0302"]
    );
}

#[test]
fn every_match_arm_must_agree() {
    assert_eq!(
        codes(
            "module m\nfn f(r: Result<Int, Str>) -> Int {\n  match r {\n    Ok(v) -> v\n    Err(e) -> e\n  }\n}\n"
        ),
        ["V0302"]
    );
}

#[test]
fn a_pattern_must_match_the_scrutinee() {
    assert_eq!(
        codes(
            "module m\nenum C {\n  Red\n}\nfn f(o: Option<Int>) -> Int {\n  match o {\n    Red -> 1\n    _ -> 0\n  }\n}\n"
        ),
        ["V0302"]
    );
}

#[test]
fn a_while_condition_must_be_bool() {
    assert_eq!(codes("module m\nfn f() {\n  while 1 { }\n}\n"), ["V0302"]);
}

#[test]
fn for_binds_the_element_type() {
    clean("module m\nfn f(xs: List<Int>) {\n  for x in xs { g(x) }\n}\nfn g(n: Int) { }\n");
    assert_eq!(
        codes("module m\nfn f(xs: List<Str>) {\n  for x in xs { g(x) }\n}\nfn g(n: Int) { }\n"),
        ["V0302"]
    );
}

// --- operators ----------------------------------------------------------

#[test]
fn arithmetic_needs_numbers() {
    clean("module m\nfn f() -> Int { 1 + 2 }\n");
    assert_eq!(
        codes("module m\nfn f() -> Str { \"a\" + \"b\" }\n"),
        ["V0302"]
    );
}

#[test]
fn arithmetic_operands_must_agree() {
    assert!(!codes("module m\nfn f(a: Int, b: Float) -> Int { a + b }\n").is_empty());
}

#[test]
fn ordering_needs_an_ordered_type() {
    clean("module m\nfn f(a: Int, b: Int) -> Bool { a < b }\n");
    assert_eq!(
        codes("module m\nfn f(a: Bool, b: Bool) -> Bool { a < b }\n"),
        ["V0302"]
    );
}

#[test]
fn equality_works_on_any_matching_pair() {
    clean("module m\nfn f(a: Bool, b: Bool) -> Bool { a == b }\n");
    assert_eq!(
        codes("module m\nfn f(a: Int, b: Str) -> Bool { a == b }\n"),
        ["V0302"]
    );
}

#[test]
fn logical_operators_need_bools() {
    assert_eq!(
        codes("module m\nfn f(a: Int) -> Bool { a && true }\n"),
        ["V0302"]
    );
}

// --- records ------------------------------------------------------------

#[test]
fn a_record_literal_is_checked_field_by_field() {
    clean("module m\nrecord R {\n  n: Int\n  s: Str\n}\nfn f() -> R { R { n: 1, s: \"x\" } }\n");
    assert_eq!(
        codes(
            "module m\nrecord R {\n  n: Int\n  s: Str\n}\nfn f() -> R { R { n: \"x\", s: \"y\" } }\n"
        ),
        ["V0302"]
    );
}

#[test]
fn an_unknown_field_lists_the_real_ones() {
    let d = check_types(&module(
        "module m\nrecord R {\n  n: Int\n}\nfn f() -> R { R { m: 1 } }\n",
    ))
    .into_iter()
    .next()
    .expect("a diagnostic");
    assert_eq!(d.code.as_str(), "V0303");
    assert_eq!(d.in_scope, ["n"]);
}

#[test]
fn a_missing_field_is_reported() {
    let src = "module m\nrecord R {\n  n: Int\n  s: Str\n}\nfn f() -> R { R { n: 1 } }\n";
    assert_eq!(codes(src), ["V0302"]);
    assert!(message(src).contains('s'), "{}", message(src));
}

#[test]
fn field_access_is_typed() {
    clean("module m\nrecord R {\n  n: Int\n}\nfn f(r: R) -> Int { r.n }\n");
    assert_eq!(
        codes("module m\nrecord R {\n  n: Int\n}\nfn f(r: R) -> Str { r.n }\n"),
        ["V0302"]
    );
}

#[test]
fn field_access_reaches_through_a_borrow() {
    clean("module m\nrecord R {\n  n: Int\n}\nfn f(r: &R) -> Int { r.n }\n");
}

#[test]
fn accessing_an_unknown_field_names_the_real_ones() {
    let d = check_types(&module(
        "module m\nrecord R {\n  n: Int\n}\nfn f(r: R) -> Int { r.missing }\n",
    ))
    .into_iter()
    .next()
    .expect("a diagnostic");
    assert_eq!(d.code.as_str(), "V0303");
    assert_eq!(d.in_scope, ["n"]);
}

// --- generics -----------------------------------------------------------

#[test]
fn a_generic_signature_instantiates_per_call_site() {
    clean(concat!(
        "module m\n",
        "fn first<T>(xs: List<T>) -> Option<T> { None }\n",
        "fn a(xs: List<Int>) -> Option<Int> { first(xs) }\n",
        "fn b(xs: List<Str>) -> Option<Str> { first(xs) }\n"
    ));
}

#[test]
fn a_generic_call_still_has_to_line_up() {
    assert_eq!(
        codes(concat!(
            "module m\n",
            "fn first<T>(xs: List<T>) -> Option<T> { None }\n",
            "fn a(xs: List<Int>) -> Option<Str> { first(xs) }\n"
        )),
        ["V0302"]
    );
}

#[test]
fn a_generic_record_instantiates() {
    clean(concat!(
        "module m\n",
        "record Box<T> {\n  value: T\n}\n",
        "fn f() -> Box<Int> { Box { value: 1 } }\n"
    ));
    assert_eq!(
        codes(
            "module m\nrecord Box<T> {\n  value: T\n}\nfn f() -> Box<Int> { Box { value: \"x\" } }\n"
        ),
        ["V0302"]
    );
}

// --- Result and `?` -----------------------------------------------------

#[test]
fn question_mark_unwraps_a_result() {
    clean(concat!(
        "module m\n",
        "fn g() -> Result<Int, Str> { Ok(1) }\n",
        "fn f() -> Result<Int, Str> {\n  let v = g()?\n  Ok(v)\n}\n"
    ));
}

#[test]
fn question_mark_needs_a_result() {
    let src = "module m\nfn f() -> Result<Int, Str> {\n  let v = 1?\n  Ok(v)\n}\n";
    assert_eq!(codes(src), ["V0302"]);
    assert!(message(src).contains("Result"), "{}", message(src));
}

#[test]
fn question_mark_checks_the_error_type() {
    assert_eq!(
        codes(concat!(
            "module m\n",
            "fn g() -> Result<Int, Str> { Ok(1) }\n",
            "fn f() -> Result<Int, Int> {\n  let v = g()?\n  Ok(v)\n}\n"
        )),
        ["V0302"]
    );
}

#[test]
fn constructors_are_typed() {
    clean("module m\nfn f() -> Result<Int, Str> { Ok(1) }\n");
    assert_eq!(
        codes("module m\nfn f() -> Result<Int, Str> { Ok(\"x\") }\n"),
        ["V0302"]
    );
    clean("module m\nfn f() -> Option<Int> { None }\n");
}

// --- misc ---------------------------------------------------------------

#[test]
fn list_elements_must_share_one_type() {
    clean("module m\nfn f() -> List<Int> { [1, 2, 3] }\n");
    assert_eq!(
        codes("module m\nfn f() -> List<Int> { [1, \"x\"] }\n"),
        ["V0302"]
    );
}

#[test]
fn return_must_match_the_signature() {
    assert_eq!(
        codes("module m\nfn f() -> Int {\n  return \"x\"\n}\n"),
        ["V0302"]
    );
}

#[test]
fn contracts_must_be_bool() {
    clean("module m\nfn f(a: Int) -> Int\n  requires a > 0\n  ensures result >= 0\n{ a }\n");
    assert_eq!(
        codes("module m\nfn f(a: Int) -> Int\n  requires a\n{ a }\n"),
        ["V0302"]
    );
}

#[test]
fn a_let_annotation_is_enforced() {
    assert_eq!(
        codes("module m\nfn f() {\n  let x: Int = \"s\"\n}\n"),
        ["V0302"]
    );
}

#[test]
fn one_mistake_produces_one_diagnostic() {
    // The poison type absorbs unification so errors do not cascade.
    assert_eq!(
        codes("module m\nfn f() -> Int {\n  let a = unknown_call()\n  let b = a + 1\n  b\n}\n"),
        Vec::<&str>::new()
    );
}

#[test]
fn imports_are_opaque_not_guessed() {
    clean("module m\nuse std/http@1:{post}\nfn f() -> Int {\n  let r = post(\"/x\")\n  0\n}\n");
}

#[test]
fn clone_returns_the_receivers_type() {
    clean("module m\nfn f(s: Str) -> Str { s.clone() }\n");
    assert_eq!(
        codes("module m\nfn f(s: Str) -> Int { s.clone() }\n"),
        ["V0302"]
    );
}

#[test]
fn an_unknown_method_is_rejected_rather_than_deferred() {
    // Found by the benchmark: returning a poison type here let `s.to_upper()`
    // pass the checker and then trap at runtime, which is precisely the
    // failure this language exists to prevent.
    let d = check_types(&module("module m\nfn f(s: Str) -> Str { s.to_upper() }\n"))
        .into_iter()
        .next()
        .expect("a diagnostic");
    assert_eq!(d.code.as_str(), "V0201");
    assert_eq!(d.in_scope, ["clone"]);
}

// --- the spec's examples ------------------------------------------------

#[test]
fn spec_hello_world_type_checks() {
    clean(
        "module greet\n\nfn main() {\n  let names = [\"ada\", \"alan\"]\n  for n in names {\n    print(n)\n  }\n}\n",
    );
}

#[test]
fn spec_fee_example_type_checks() {
    clean(
        "module m\ntype Cents = Int\nfn fee(amount: Cents) -> Cents\n  requires amount > 0\n  ensures result >= 0\n{\n  amount / 50\n}\n",
    );
}
