//! Move and borrow checking.

use vise_check::check_borrows;
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
    check_borrows(&module(src))
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

fn clean(src: &str) {
    let ds = check_borrows(&module(src));
    assert!(
        ds.is_empty(),
        "unexpected: {:?}",
        ds.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}

fn first(src: &str) -> vise_diag::Diagnostic {
    check_borrows(&module(src))
        .into_iter()
        .next()
        .expect("a diagnostic")
}

/// Helpers: `make` produces an owned List, `eat` consumes one, `peek` borrows.
fn with_helpers(body: &str) -> String {
    format!(
        concat!(
            "module m\n",
            "fn make() -> List<Int> {{ [1, 2] }}\n",
            "fn eat(xs: List<Int>) -> Int {{ 0 }}\n",
            "fn peek(xs: &List<Int>) -> Int {{ 0 }}\n",
            "fn main() {{\n{}\n}}\n"
        ),
        body
    )
}

// --- moves --------------------------------------------------------------

#[test]
fn using_a_value_after_moving_it_is_an_error() {
    let src = with_helpers("  let xs = make()\n  eat(xs)\n  eat(xs)");
    assert_eq!(codes(&src), ["V0601"]);
    let d = first(&src);
    assert_eq!(d.labels.len(), 1, "should point at the move");
    assert!(d.labels[0].message.contains("moved"), "{:?}", d.labels[0]);
}

#[test]
fn the_fix_offers_borrowing_or_cloning() {
    let d = first(&with_helpers("  let xs = make()\n  eat(xs)\n  eat(xs)"));
    let kinds: Vec<_> = d.fixes.iter().map(|f| f.kind).collect();
    assert!(kinds.contains(&vise_diag::FixKind::Borrow), "{kinds:?}");
    assert!(kinds.contains(&vise_diag::FixKind::Clone), "{kinds:?}");
    assert!(
        d.autofix().is_none(),
        "borrow versus clone is the author's choice"
    );
}

#[test]
fn primitives_copy_rather_than_move() {
    // §9: primitives copy implicitly.
    clean(
        "module m\nfn use_it(n: Int) -> Int { n }\nfn main() {\n  let a = 1\n  use_it(a)\n  use_it(a)\n}\n",
    );
}

#[test]
fn borrowing_does_not_consume() {
    // The spec's own shape: borrow first, then hand ownership over.
    clean(&with_helpers("  let xs = make()\n  peek(&xs)\n  eat(xs)"));
}

#[test]
fn a_shared_borrow_parameter_does_not_consume_its_argument() {
    clean(&with_helpers(
        "  let xs = make()\n  peek(&xs)\n  peek(&xs)\n  eat(xs)",
    ));
}

#[test]
fn reassignment_makes_a_moved_name_usable_again() {
    clean(&with_helpers(
        "  var xs = make()\n  eat(xs)\n  xs = make()\n  eat(xs)",
    ));
}

#[test]
fn a_move_into_a_record_counts() {
    let src = concat!(
        "module m\n",
        "record Holder {\n  items: List<Int>\n}\n",
        "fn make() -> List<Int> { [1] }\n",
        "fn eat(xs: List<Int>) -> Int { 0 }\n",
        "fn main() {\n  let xs = make()\n  let h = Holder { items: xs }\n  eat(xs)\n}\n"
    );
    assert_eq!(codes(src), ["V0601"]);
}

// --- loops --------------------------------------------------------------

#[test]
fn moving_an_outer_value_inside_a_loop_is_an_error() {
    // The second iteration would use a value that is already gone.
    let src = with_helpers("  let xs = make()\n  for n in [1, 2] { eat(xs) }");
    assert_eq!(codes(&src), ["V0601"]);
    assert!(first(&src).message.contains("inside a loop"));
}

#[test]
fn moving_a_value_declared_inside_the_loop_is_fine() {
    clean(&with_helpers(
        "  for n in [1, 2] {\n    let xs = make()\n    eat(xs)\n  }",
    ));
}

// --- branches -----------------------------------------------------------

#[test]
fn a_move_in_one_branch_counts_afterwards() {
    // Conservative merge: moved on either path is moved after.
    let src = with_helpers("  let xs = make()\n  if true { eat(xs) } else { 0 }\n  eat(xs)");
    assert_eq!(codes(&src), ["V0601"]);
}

#[test]
fn branches_do_not_move_for_each_other() {
    // Each arm starts from the same state, so this is one move, not two.
    clean(&with_helpers(
        "  let xs = make()\n  if true { eat(xs) } else { eat(xs) }",
    ));
}

#[test]
fn match_arms_do_not_move_for_each_other() {
    clean(concat!(
        "module m\n",
        "fn make() -> List<Int> { [1] }\n",
        "fn eat(xs: List<Int>) -> Int { 0 }\n",
        "fn main() {\n",
        "  let xs = make()\n",
        "  match 1 {\n    1 -> eat(xs)\n    _ -> eat(xs)\n  }\n",
        "}\n"
    ));
}

// --- borrow conflicts ---------------------------------------------------

#[test]
fn a_unique_borrow_may_not_coexist_with_a_shared_one() {
    let src = concat!(
        "module m\n",
        "fn make() -> List<Int> { [1] }\n",
        "fn both(a: &mut List<Int>, b: &List<Int>) -> Int { 0 }\n",
        "fn main() {\n  var xs = make()\n  both(&mut xs, &xs)\n}\n"
    );
    assert_eq!(codes(src), ["V0602"]);
    assert_eq!(first(src).labels.len(), 1);
}

#[test]
fn two_unique_borrows_of_one_value_conflict() {
    let src = concat!(
        "module m\n",
        "fn make() -> List<Int> { [1] }\n",
        "fn both(a: &mut List<Int>, b: &mut List<Int>) -> Int { 0 }\n",
        "fn main() {\n  var xs = make()\n  both(&mut xs, &mut xs)\n}\n"
    );
    assert!(!codes(src).is_empty());
}

#[test]
fn two_shared_borrows_are_fine() {
    clean(concat!(
        "module m\n",
        "fn make() -> List<Int> { [1] }\n",
        "fn both(a: &List<Int>, b: &List<Int>) -> Int { 0 }\n",
        "fn main() {\n  let xs = make()\n  both(&xs, &xs)\n}\n"
    ));
}

#[test]
fn borrows_of_different_values_are_fine() {
    clean(concat!(
        "module m\n",
        "fn make() -> List<Int> { [1] }\n",
        "fn both(a: &mut List<Int>, b: &List<Int>) -> Int { 0 }\n",
        "fn main() {\n  var xs = make()\n  let ys = make()\n  both(&mut xs, &ys)\n}\n"
    ));
}

// --- escaping borrows ---------------------------------------------------

#[test]
fn a_borrow_of_a_local_may_not_be_returned() {
    let src = concat!(
        "module m\n",
        "fn make() -> List<Int> { [1] }\n",
        "fn escape() -> &List<Int> {\n  let xs = make()\n  &xs\n}\n"
    );
    assert_eq!(codes(src), ["V0603"]);
}

#[test]
fn a_borrow_of_a_parameter_may_be_returned() {
    // The caller owns it, so it outlives the call.
    clean("module m\nfn pass(xs: &List<Int>) -> &List<Int> { xs }\n");
}

// --- conservatism -------------------------------------------------------

#[test]
fn a_value_of_unknown_type_is_never_reported_on() {
    // `load` is imported, so nothing about its result is knowable.
    clean(concat!(
        "module m\n",
        "use std/db@1:{load}\n",
        "fn eat(xs: List<Int>) -> Int { 0 }\n",
        "fn main() {\n  let xs = load()\n  eat(xs)\n  eat(xs)\n}\n"
    ));
}

#[test]
fn an_unknown_callee_is_assumed_to_read_not_move() {
    // Guessing a move would invent an error the author cannot argue with.
    clean(concat!(
        "module m\n",
        "use std/io@1:{send}\n",
        "fn make() -> List<Int> { [1] }\n",
        "fn main() {\n  let xs = make()\n  send(xs)\n  send(xs)\n}\n"
    ));
}

// --- the spec's example -------------------------------------------------

#[test]
fn spec_ownership_example_passes() {
    clean(concat!(
        "module m\n",
        "record Item {\n  price: Int\n}\n",
        "fn load() -> List<Item> { [] }\n",
        "fn total(items: &List<Item>) -> Int { 0 }\n",
        "fn consume(items: List<Item>) -> Int { 0 }\n",
        "fn main() {\n",
        "  let items = load()\n",
        "  let t = total(&items)\n",
        "  let r = consume(items)\n",
        "}\n"
    ));
}
