//! Parser behaviour, driven from the spec's own examples.

use vise_ast::{BinOp, Block, Effect, Expr, ExprKind, ItemKind, Literal, StmtKind, StrPart};
use vise_diag::FileId;
use vise_parse::parse;

fn ok(src: &str) -> vise_ast::Module {
    let p = parse(src, FileId(0));
    assert!(
        p.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        p.diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.message.as_str()))
            .collect::<Vec<_>>()
    );
    p.module.expect("a module")
}

fn codes(src: &str) -> Vec<&'static str> {
    parse(src, FileId(0))
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

/// Wrap an expression in a module so it can be parsed on its own.
fn expr_of(src: &str) -> Expr {
    let m = ok(&format!("module t\nfn f() {{\n  {src}\n}}\n"));
    let ItemKind::Fn(f) = &m.items[0].kind else {
        panic!("expected a function")
    };
    *f.body.tail.clone().expect("a tail expression")
}

fn body_of(src: &str) -> Block {
    let m = ok(&format!("module t\nfn f() {{\n{src}\n}}\n"));
    let ItemKind::Fn(f) = &m.items[0].kind else {
        panic!("expected a function")
    };
    f.body.clone()
}

// --- the spec's examples ------------------------------------------------

#[test]
fn spec_hello_world_parses() {
    let m = ok(
        "module greet\n\nfn main() {\n  let names = [\"ada\", \"alan\", \"grace\"]\n  for n in names {\n    print(\"hello, {n}\")\n  }\n}\n",
    );
    assert_eq!(m.name.name, "greet");
    assert_eq!(m.items.len(), 1);
}

#[test]
fn spec_payments_example_parses() {
    let m = ok(concat!(
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

    assert_eq!(m.uses.len(), 1);
    assert_eq!(m.uses[0].path.joined(), "std/http");
    assert_eq!(m.uses[0].path.version, Some(1));
    assert_eq!(m.uses[0].names[0].name, "post");

    let ItemKind::Fn(charge) = &m.item("charge").expect("charge").kind else {
        panic!("charge should be a function")
    };
    assert!(charge.is_pub);
    assert_eq!(charge.params.len(), 2);
    let row = charge.effects.as_ref().expect("an effect row");
    assert_eq!(row.effects, [Effect::Net]);

    let ItemKind::Enum(e) = &m.item("ChargeError").expect("ChargeError").kind else {
        panic!("ChargeError should be an enum")
    };
    assert_eq!(e.variants.len(), 2);
    assert_eq!(e.variants[1].fields[0].name.name, "reason");
}

#[test]
fn spec_contract_example_parses() {
    let m = ok(concat!(
        "module m\n",
        "fn fee(amount: Cents) -> Cents\n",
        "  requires amount > 0\n",
        "  ensures result >= 0\n",
        "{\n  amount / 50\n}\n"
    ));
    let ItemKind::Fn(f) = &m.items[0].kind else {
        panic!()
    };
    assert_eq!(f.requires.len(), 1);
    assert_eq!(f.ensures.len(), 1);
    assert!(f.has_contracts());
}

#[test]
fn spec_ownership_example_parses() {
    let m = ok(concat!(
        "module m\n",
        "fn longest<'a>(a: &'a Str, b: &'a Str) -> &'a Str { a }\n",
        "fn total(items: &List<Item>) -> Cents {\n",
        "  var sum = 0\n",
        "  for it in items { sum = sum + it.price }\n",
        "  sum\n",
        "}\n"
    ));
    let ItemKind::Fn(longest) = &m.items[0].kind else {
        panic!()
    };
    assert_eq!(longest.generics.len(), 1);
    assert!(longest.params[0].ty.is_ref());
    assert!(longest.ret.as_ref().expect("a return type").is_ref());
}

#[test]
fn spec_match_example_parses() {
    let e = expr_of(
        "match result {\n    Ok(receipt) -> log(receipt)\n    Err(CardDeclined(why)) -> retry(why)\n    Err(InsufficientFunds) -> Unit\n  }",
    );
    let ExprKind::Match { arms, .. } = e.kind else {
        panic!("expected a match")
    };
    assert_eq!(arms.len(), 3);
}

// --- effects ------------------------------------------------------------

#[test]
fn an_omitted_effect_row_is_not_an_empty_one() {
    // Spec §7: omitted means inferred, `!{}` asserts purity.
    let m = ok("module m\nfn f() { }\n");
    let ItemKind::Fn(f) = &m.items[0].kind else {
        panic!()
    };
    assert!(!f.declares_effects());

    let m = ok("module m\nfn f() !{} { }\n");
    let ItemKind::Fn(f) = &m.items[0].kind else {
        panic!()
    };
    assert!(f.declares_effects());
    assert!(f.effects.as_ref().expect("a row").is_pure());
}

#[test]
fn effect_rows_are_sorted_and_deduplicated() {
    let m = ok("module m\nfn f() !{time, net, net} { }\n");
    let ItemKind::Fn(f) = &m.items[0].kind else {
        panic!()
    };
    assert_eq!(
        f.effects.as_ref().unwrap().effects,
        [Effect::Net, Effect::Time]
    );
}

#[test]
fn an_unknown_effect_is_kept_for_the_checker_to_report() {
    // The parser does not guess what `db` meant; §7 says a database client is
    // a library carrying `!{net}`.
    let m = ok("module m\nfn f() !{db} { }\n");
    let ItemKind::Fn(f) = &m.items[0].kind else {
        panic!()
    };
    let row = f.effects.as_ref().unwrap();
    assert!(row.effects.is_empty());
    assert_eq!(row.unknown[0].name, "db");
}

// --- expressions --------------------------------------------------------

#[test]
fn precedence_follows_the_table() {
    let ExprKind::Binary { op, rhs, .. } = expr_of("a + b * c").kind else {
        panic!()
    };
    assert_eq!(op, BinOp::Add);
    assert!(matches!(rhs.kind, ExprKind::Binary { op: BinOp::Mul, .. }));
}

#[test]
fn arithmetic_is_left_associative() {
    let ExprKind::Binary { op, lhs, .. } = expr_of("a - b - c").kind else {
        panic!()
    };
    assert_eq!(op, BinOp::Sub);
    assert!(matches!(lhs.kind, ExprKind::Binary { op: BinOp::Sub, .. }));
}

#[test]
fn parentheses_override_precedence() {
    let ExprKind::Binary { op, .. } = expr_of("(a + b) * c").kind else {
        panic!()
    };
    assert_eq!(op, BinOp::Mul);
}

#[test]
fn comparisons_do_not_chain() {
    // `a < b < c` would otherwise quietly compare a Bool with a number.
    assert_eq!(codes("module t\nfn f() { a < b < c }\n"), ["V0102"]);
    assert!(codes("module t\nfn f() { a < b && b < c }\n").is_empty());
}

#[test]
fn postfix_operators_chain() {
    let e = expr_of("a.b().c[0]?");
    assert!(matches!(e.kind, ExprKind::Try(_)));
}

#[test]
fn borrows_parse_shared_and_unique() {
    assert!(matches!(
        expr_of("&x").kind,
        ExprKind::Borrow { is_mut: false, .. }
    ));
    assert!(matches!(
        expr_of("&mut x").kind,
        ExprKind::Borrow { is_mut: true, .. }
    ));
}

#[test]
fn if_requires_both_branches() {
    assert!(codes("module t\nfn f() { if c { a } else { b } }\n").is_empty());
    assert_eq!(codes("module t\nfn f() { if c { a } }\n"), ["V0102"]);
}

#[test]
fn else_if_chains() {
    let ExprKind::If { otherwise, .. } = expr_of("if a { x } else if b { y } else { z }").kind
    else {
        panic!()
    };
    assert!(matches!(otherwise.kind, ExprKind::If { .. }));
}

#[test]
fn a_record_literal_is_banned_in_a_condition_but_allowed_in_parentheses() {
    // Without the ban, `if x { .. }` cannot be told from a record literal.
    let b = body_of("  if flag { a } else { b }");
    assert!(b.tail.is_some());
    assert!(codes("module t\nfn f() { if (P { x: 1 }).ok { a } else { b } }\n").is_empty());
}

// --- statements ---------------------------------------------------------

#[test]
fn let_and_var_are_distinguished() {
    let b = body_of("  let a = 1\n  var b = 2");
    let StmtKind::Let { is_var, .. } = &b.stmts[0].kind else {
        panic!()
    };
    assert!(!is_var);
    let StmtKind::Let { is_var, .. } = &b.stmts[1].kind else {
        panic!()
    };
    assert!(is_var);
}

#[test]
fn a_discarding_let_parses() {
    let b = body_of("  let _ = f()");
    assert!(matches!(&b.stmts[0].kind, StmtKind::Let { .. }));
}

#[test]
fn assignment_is_a_statement_not_an_expression() {
    let b = body_of("  sum = sum + 1");
    assert!(matches!(&b.stmts[0].kind, StmtKind::Assign { .. }));
}

#[test]
fn the_final_expression_becomes_the_block_value() {
    let b = body_of("  let a = 1\n  a + 1");
    assert_eq!(b.stmts.len(), 1);
    assert!(b.tail.is_some());
}

#[test]
fn a_block_ending_in_a_loop_has_no_value() {
    let b = body_of("  for x in xs { g(x) }");
    assert!(b.tail.is_none());
    assert!(matches!(&b.stmts[0].kind, StmtKind::For { .. }));
}

// --- string interpolation -----------------------------------------------

#[test]
fn interpolation_splits_into_text_and_expressions() {
    let ExprKind::Literal(Literal::Str(parts)) = expr_of(r#""hello, {name}!""#).kind else {
        panic!()
    };
    assert_eq!(parts.len(), 3);
    assert!(matches!(&parts[0], StrPart::Text(t) if t == "hello, "));
    assert!(matches!(&parts[1], StrPart::Interpolation(_)));
    assert!(matches!(&parts[2], StrPart::Text(t) if t == "!"));
}

#[test]
fn an_interpolated_expression_is_fully_parsed() {
    let ExprKind::Literal(Literal::Str(parts)) = expr_of(r#""{a + b * 2}""#).kind else {
        panic!()
    };
    let StrPart::Interpolation(e) = &parts[0] else {
        panic!()
    };
    assert!(matches!(e.kind, ExprKind::Binary { op: BinOp::Add, .. }));
}

#[test]
fn interpolation_spans_point_at_the_real_file() {
    let src = "module t\nfn f() {\n  \"x{name}\"\n}\n";
    let m = ok(src);
    let ItemKind::Fn(f) = &m.items[0].kind else {
        panic!()
    };
    let Some(tail) = &f.body.tail else { panic!() };
    let ExprKind::Literal(Literal::Str(parts)) = &tail.kind else {
        panic!()
    };
    let StrPart::Interpolation(e) = &parts[1] else {
        panic!()
    };
    assert_eq!(&src[e.span.start as usize..e.span.end as usize], "name");
}

#[test]
fn escapes_are_decoded_and_an_escaped_brace_is_not_an_interpolation() {
    let ExprKind::Literal(Literal::Str(parts)) = expr_of(r#""a\nb\{c""#).kind else {
        panic!()
    };
    assert_eq!(parts.len(), 1);
    assert!(matches!(&parts[0], StrPart::Text(t) if t == "a\nb{c"));
}

// --- errors and recovery ------------------------------------------------

#[test]
fn a_file_must_open_with_a_module_header() {
    assert_eq!(codes("fn main() { }\n"), ["V0103"]);
    assert!(parse("fn main() { }\n", FileId(0)).module.is_none());
}

#[test]
fn a_glob_import_is_rejected() {
    assert!(!codes("module t\nuse std/http@1:{}\n").is_empty());
}

#[test]
fn one_broken_item_does_not_swallow_the_rest_of_the_file() {
    let p = parse("module t\nfn a( { }\nfn b() { }\nfn c() { }\n", FileId(0));
    assert!(p.has_errors());
    let m = p.module.expect("a module");
    // Recovery reaches the later items rather than giving up at the first.
    assert!(
        m.item("b").is_some() && m.item("c").is_some(),
        "recovered items: {:?}",
        m.items
            .iter()
            .map(|i| i.name().name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn parsing_terminates_on_garbage() {
    // Recovery must always consume a token, or the parser spins.
    for src in [
        "module t\n}\n",
        "module t\nfn f() { ] }\n",
        "module t\n@@@\n",
    ] {
        let _ = parse(src, FileId(0));
    }
}

#[test]
fn an_integer_too_large_for_int_is_reported() {
    assert_eq!(
        codes("module t\nfn f() { 99999999999999999999 }\n"),
        ["V0004"]
    );
}
