//! Running Vise programs.

use vise_diag::FileId;
use vise_interp::{Trap, Value, run};
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

/// Run a `main` body and return its value.
fn eval(body: &str) -> Value {
    let src = format!("module t\nfn main() -> Int {{\n{body}\n}}\n");
    run(&module(&src)).value().clone()
}

fn output(src: &str) -> Vec<String> {
    let r = run(&module(src));
    assert!(r.is_ok(), "trapped: {:?}", r.result);
    r.stdout
}

fn trap(src: &str) -> Trap {
    run(&module(src)).result.expect_err("should trap")
}

// --- arithmetic ---------------------------------------------------------

#[test]
fn arithmetic_evaluates() {
    assert_eq!(eval("  1 + 2 * 3"), Value::Int(7));
    assert_eq!(eval("  (1 + 2) * 3"), Value::Int(9));
    assert_eq!(eval("  7 / 2"), Value::Int(3));
    assert_eq!(eval("  7 % 2"), Value::Int(1));
    assert_eq!(eval("  -(3)"), Value::Int(-3));
}

#[test]
fn integer_overflow_traps_and_never_wraps() {
    // §4: this is the rule, not an implementation detail.
    assert_eq!(
        trap("module t\nfn main() -> Int { 9223372036854775807 + 1 }\n"),
        Trap::Overflow("+")
    );
    assert_eq!(
        trap("module t\nfn main() -> Int { 9223372036854775807 * 2 }\n"),
        Trap::Overflow("*")
    );
}

#[test]
fn division_by_zero_traps() {
    assert_eq!(
        trap("module t\nfn main() -> Int { 1 / 0 }\n"),
        Trap::DivideByZero
    );
    assert_eq!(
        trap("module t\nfn main() -> Int { 1 % 0 }\n"),
        Trap::DivideByZero
    );
}

// --- control flow -------------------------------------------------------

#[test]
fn if_selects_a_branch() {
    assert_eq!(eval("  if 1 < 2 { 10 } else { 20 }"), Value::Int(10));
    assert_eq!(eval("  if 1 > 2 { 10 } else { 20 }"), Value::Int(20));
}

#[test]
fn logical_operators_short_circuit() {
    // If `||` evaluated the right side, this would divide by zero.
    assert_eq!(
        eval("  if true || (1 / 0) == 0 { 1 } else { 0 }"),
        Value::Int(1)
    );
    assert_eq!(
        eval("  if false && (1 / 0) == 0 { 1 } else { 0 }"),
        Value::Int(0)
    );
}

#[test]
fn a_loop_accumulates() {
    assert_eq!(
        eval("  var total = 0\n  for n in [1, 2, 3, 4] { total = total + n }\n  total"),
        Value::Int(10)
    );
}

#[test]
fn while_runs_until_its_condition_fails() {
    assert_eq!(
        eval("  var n = 0\n  while n < 5 { n = n + 1 }\n  n"),
        Value::Int(5)
    );
}

#[test]
fn break_and_continue_work() {
    assert_eq!(
        eval(
            "  var total = 0\n  for n in [1, 2, 3, 4] {\n    if n == 3 { break } else { Unit }\n    total = total + n\n  }\n  total"
        ),
        Value::Int(3)
    );
    assert_eq!(
        eval(
            "  var total = 0\n  for n in [1, 2, 3] {\n    if n == 2 { continue } else { Unit }\n    total = total + n\n  }\n  total"
        ),
        Value::Int(4)
    );
}

#[test]
fn return_unwinds_out_of_a_loop() {
    assert_eq!(
        eval("  for n in [1, 2, 3] {\n    if n == 2 { return 99 } else { Unit }\n  }\n  0"),
        Value::Int(99)
    );
}

// --- functions ----------------------------------------------------------

#[test]
fn functions_call_each_other() {
    let src = "module t\nfn double(n: Int) -> Int { n * 2 }\nfn main() -> Int { double(21) }\n";
    assert_eq!(run(&module(src)).value().clone(), Value::Int(42));
}

#[test]
fn recursion_works() {
    let src = "module t\nfn fact(n: Int) -> Int {\n  if n <= 1 { 1 } else { n * fact(n - 1) }\n}\nfn main() -> Int { fact(10) }\n";
    assert_eq!(run(&module(src)).value().clone(), Value::Int(3_628_800));
}

#[test]
fn runaway_recursion_traps_instead_of_crashing() {
    // A native stack overflow would abort the process, which a benchmark
    // harness cannot tell apart from a bug in the harness.
    let t = trap("module t\nfn go(n: Int) -> Int { go(n + 1) }\nfn main() -> Int { go(0) }\n");
    assert!(
        matches!(t, Trap::Unsupported(ref m) if m.contains("recursion")),
        "{t}"
    );
}

// --- contracts ----------------------------------------------------------

#[test]
fn a_requires_clause_is_checked() {
    // §10: contracts are checked at runtime in dev builds.
    let src = "module t\nfn fee(a: Int) -> Int\n  requires a > 0\n{ a / 50 }\nfn main() -> Int { fee(0) }\n";
    assert_eq!(
        trap(src),
        Trap::Requires {
            function: "fee".to_owned()
        }
    );
}

#[test]
fn an_ensures_clause_is_checked_against_result() {
    let src = "module t\nfn bad(a: Int) -> Int\n  ensures result > 0\n{ 0 - a }\nfn main() -> Int { bad(5) }\n";
    assert_eq!(
        trap(src),
        Trap::Ensures {
            function: "bad".to_owned()
        }
    );
}

#[test]
fn satisfied_contracts_do_not_interfere() {
    let src = "module t\nfn fee(a: Int) -> Int\n  requires a > 0\n  ensures result >= 0\n{ a / 50 }\nfn main() -> Int { fee(500) }\n";
    assert_eq!(run(&module(src)).value().clone(), Value::Int(10));
}

// --- data ---------------------------------------------------------------

#[test]
fn records_construct_and_project() {
    let src = "module t\nrecord P {\n  x: Int\n  y: Int\n}\nfn main() -> Int {\n  let p = P { x: 3, y: 4 }\n  p.x + p.y\n}\n";
    assert_eq!(run(&module(src)).value().clone(), Value::Int(7));
}

#[test]
fn lists_index_and_trap_out_of_bounds() {
    assert_eq!(eval("  [10, 20, 30][1]"), Value::Int(20));
    assert_eq!(
        trap("module t\nfn main() -> Int { [1, 2][5] }\n"),
        Trap::IndexOutOfBounds { index: 5, len: 2 }
    );
}

#[test]
fn match_binds_and_selects() {
    let src = concat!(
        "module t\n",
        "enum C {\n  Red\n  Tagged(n: Int)\n}\n",
        "fn pick(c: C) -> Int {\n  match c {\n    Red -> 1\n    Tagged(n) -> n\n  }\n}\n",
        "fn main() -> Int { pick(Tagged(42)) + pick(Red) }\n"
    );
    assert_eq!(run(&module(src)).value().clone(), Value::Int(43));
}

#[test]
fn nested_patterns_match() {
    let src = concat!(
        "module t\n",
        "enum E {\n  A(n: Int)\n  B\n}\n",
        "fn f(r: Result<Int, E>) -> Int {\n",
        "  match r {\n    Ok(v) -> v\n    Err(A(n)) -> n\n    Err(B) -> 0\n  }\n}\n",
        "fn main() -> Int { f(Err(A(7))) }\n"
    );
    assert_eq!(run(&module(src)).value().clone(), Value::Int(7));
}

// --- Result and `?` -----------------------------------------------------

#[test]
fn question_mark_unwraps_ok() {
    let src = concat!(
        "module t\n",
        "fn get() -> Result<Int, Str> { Ok(5) }\n",
        "fn use_it() -> Result<Int, Str> {\n  let v = get()?\n  Ok(v + 1)\n}\n",
        "fn main() -> Result<Int, Str> { use_it() }\n"
    );
    assert_eq!(
        run(&module(src)).value().clone(),
        Value::variant("Ok", vec![Value::Int(6)])
    );
}

#[test]
fn question_mark_propagates_err_immediately() {
    let src = concat!(
        "module t\n",
        "fn get() -> Result<Int, Str> { Err(\"boom\") }\n",
        "fn use_it() -> Result<Int, Str> {\n  let v = get()?\n  Ok(v + 1)\n}\n",
        "fn main() -> Result<Int, Str> { use_it() }\n"
    );
    assert_eq!(
        run(&module(src)).value().clone(),
        Value::variant("Err", vec![Value::str("boom")])
    );
}

// --- core -----------------------------------------------------------------

#[test]
fn the_interpreter_implements_every_builtin() {
    // The table in vise-check is the single enumeration of `core`. If an entry
    // is added there and not here, this fails rather than the program trapping
    // in front of whoever ran it.
    for b in vise_check::builtins() {
        let missing = matches!(
            run(&module(&format!(
                "module t\nfn main() {{ let _ = {}\n}}\n",
                stub_call(&b)
            )))
            .result,
            Err(Trap::Unsupported(ref m)) if m.contains("does not implement")
        );
        assert!(!missing, "`{}` is in core but not implemented", b.name);
    }
}

/// A call to `b` with plausible arguments, used only to reach the dispatch.
fn stub_call(b: &vise_check::Builtin) -> String {
    let args: Vec<&str> = b
        .params
        .iter()
        .map(|p| match format!("{p}").as_str() {
            "Str" => "\"\"",
            "Int" => "0",
            "Bool" => "true",
            _ => "[]",
        })
        .collect();
    format!("{}({})", b.name, args.join(", "))
}

#[test]
fn list_builtins_work() {
    assert_eq!(eval("  length([1, 2, 3])"), Value::Int(3));
    assert_eq!(
        output("module t\nfn main() {\n  let xs = append([1], 2)\n  print(\"{length(xs)}\")\n}\n"),
        ["2"]
    );
}

#[test]
fn string_builtins_work() {
    assert_eq!(eval("  str_length(\"hello\")"), Value::Int(5));
    assert_eq!(eval("  length(lines(\"a\\nb\\nc\"))"), Value::Int(3));
    // A trailing newline ends the last line rather than starting an empty one.
    assert_eq!(eval("  length(lines(\"a\\nb\\n\"))"), Value::Int(2));
    assert_eq!(eval("  length(split(\"a,b,c\", \",\"))"), Value::Int(3));
    assert_eq!(
        output("module t\nfn main() {\n  print(join([\"a\", \"b\"], \"-\"))\n}\n"),
        ["a-b"]
    );
    assert_eq!(
        eval("  if starts_with(\"abc\", \"ab\") { 1 } else { 0 }"),
        Value::Int(1)
    );
    assert_eq!(
        eval("  if contains(\"abc\", \"bc\") { 1 } else { 0 }"),
        Value::Int(1)
    );
}

#[test]
fn parse_int_reports_failure_as_a_value() {
    // §8: no exceptions, so a parse that can fail returns an Option.
    // The calls are bound first: a string literal cannot be nested inside an
    // interpolation.
    let src = concat!(
        "module t\n",
        "fn f(s: Str) -> Int {\n",
        "  match parse_int(s) {\n    Some(n) -> n\n    None -> 0 - 1\n  }\n}\n",
        "fn main() {\n",
        "  let good = f(\"42\")\n",
        "  let bad = f(\"nope\")\n",
        "  print(\"{good} {bad}\")\n",
        "}\n"
    );
    assert_eq!(output(src), ["42 -1"]);
}

#[test]
fn exit_stops_the_program_with_a_status() {
    let r = run(&module(
        "module t\nfn main() {\n  print(\"before\")\n  exit(3)\n  print(\"after\")\n}\n",
    ));
    assert_eq!(r.stdout, ["before"]);
    assert_eq!(r.result, Err(Trap::Exit(3)));
}

// --- methods ------------------------------------------------------------

#[test]
fn clone_yields_the_same_value() {
    // §9's one method. Values are immutable and shared, so a clone is the value
    // itself; what it buys is ownership, which the borrow checker cares about
    // and the evaluator does not.
    assert_eq!(
        output("module t\nfn main() {\n  let a = \"x\"\n  let b = a.clone()\n  print(b)\n}\n"),
        ["x"]
    );
}

// --- output -------------------------------------------------------------

#[test]
fn output_written_before_a_trap_is_kept() {
    // The most useful thing about a failure is usually what ran before it.
    let r = run(&module(
        "module t\nfn main() {\n  print(\"before\")\n  let x = 1 / 0\n  print(\"after\")\n}\n",
    ));
    assert_eq!(r.stdout, ["before"]);
    assert_eq!(r.result, Err(Trap::DivideByZero));
}

#[test]
fn print_captures_lines_in_order() {
    assert_eq!(
        output("module t\nfn main() {\n  print(\"a\")\n  print(\"b\")\n}\n"),
        ["a", "b"]
    );
}

#[test]
fn interpolation_renders_values() {
    assert_eq!(
        output("module t\nfn main() {\n  let n = 6\n  print(\"n is {n * 7}\")\n}\n"),
        ["n is 42"]
    );
}

#[test]
fn spec_hello_world_runs() {
    assert_eq!(
        output(
            "module greet\n\nfn main() {\n  let names = [\"ada\", \"alan\", \"grace\"]\n  for n in names {\n    print(\"hello, {n}\")\n  }\n}\n"
        ),
        ["hello, ada", "hello, alan", "hello, grace"]
    );
}

// --- determinism --------------------------------------------------------

#[test]
fn the_same_program_produces_the_same_output_every_time() {
    // §11: same inputs, byte-identical output.
    let src = "module t\nfn main() {\n  for n in [3, 1, 2] { print(\"{n}\") }\n}\n";
    let m = module(src);
    let first = run(&m);
    for _ in 0..20 {
        assert_eq!(run(&m), first);
    }
}
