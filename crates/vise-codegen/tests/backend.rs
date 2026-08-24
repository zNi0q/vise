//! The C backend.
//!
//! The property that matters is not what C comes out, but that the compiled
//! program behaves like the interpreted one. Every case below is run both ways
//! and the outputs compared: if they ever disagree, one of the two is wrong and
//! the language has two meanings.

use std::path::PathBuf;
use std::process::Command;

use vise_check::check_with_types;
use vise_codegen::emit;
use vise_diag::FileId;
use vise_parse::parse;

fn module(src: &str) -> vise_ast::Module {
    let p = parse(src, FileId(0));
    assert!(
        p.diagnostics.is_empty(),
        "parse errors: {:?}",
        p.diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.message.as_str()))
            .collect::<Vec<_>>()
    );
    p.module.expect("a module")
}

fn c_source(src: &str) -> String {
    let m = module(src);
    let (diagnostics, types) = check_with_types(&m);
    assert!(
        diagnostics.is_empty(),
        "type errors: {:?}",
        diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
    );
    let emitted = emit(&m, &types);
    assert!(
        emitted.is_complete(),
        "unsupported: {:?}",
        emitted
            .unsupported
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
    );
    emitted.c_source
}

/// Compile and run, returning stdout and whether it exited cleanly.
fn compiled_output(name: &str, src: &str) -> (String, bool) {
    let runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../runtime/c");
    let dir = std::env::temp_dir().join(format!("vise-backend-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a build directory");

    let program = dir.join("program.c");
    std::fs::write(&program, c_source(src)).expect("writing the generated C");

    let binary = dir.join("program");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_owned());
    let status = Command::new(&cc)
        .args(["-std=c11", "-O1", "-o"])
        .arg(&binary)
        .arg(&program)
        .arg(runtime.join("value.c"))
        .arg("-I")
        .arg(&runtime)
        .status()
        .expect("running the C compiler");
    assert!(
        status.success(),
        "generated C did not compile; it is at {}",
        program.display()
    );

    let output = Command::new(&binary).output().expect("running the program");
    let _ = std::fs::remove_dir_all(&dir);
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.success(),
    )
}

/// Run the same program through the interpreter.
fn interpreted_output(src: &str) -> (String, bool) {
    let run = vise_interp::run(&module(src));
    let mut text = String::new();
    for line in &run.stdout {
        text.push_str(line);
        text.push('\n');
    }
    (text, run.is_ok())
}

/// The whole point: both paths agree.
fn assert_agrees(name: &str, src: &str) {
    let (compiled, compiled_ok) = compiled_output(name, src);
    let (interpreted, interpreted_ok) = interpreted_output(src);
    assert_eq!(
        compiled, interpreted,
        "{name}: compiled and interpreted output differ"
    );
    assert_eq!(
        compiled_ok, interpreted_ok,
        "{name}: one path trapped and the other did not"
    );
}

#[test]
fn printing_and_interpolation_agree() {
    assert_agrees(
        "print",
        "module t\nfn main() {\n  let n = 6\n  print(\"n is {n * 7}\")\n  print(\"done\")\n}\n",
    );
}

#[test]
fn arithmetic_agrees() {
    assert_agrees(
        "arith",
        "module t\nfn main() {\n  print(\"{1 + 2 * 3}\")\n  print(\"{7 / 2}\")\n  print(\"{7 % 2}\")\n  print(\"{0 - 9}\")\n}\n",
    );
}

#[test]
fn overflow_traps_in_both() {
    // §4 is a language rule, so it has to hold in compiled code too.
    assert_agrees(
        "overflow",
        "module t\nfn main() {\n  print(\"before\")\n  let n = 9223372036854775807 + 1\n  print(\"after\")\n}\n",
    );
}

#[test]
fn division_by_zero_traps_in_both() {
    assert_agrees(
        "divzero",
        "module t\nfn main() {\n  print(\"before\")\n  let n = 1 / 0\n  print(\"after\")\n}\n",
    );
}

#[test]
fn control_flow_agrees() {
    assert_agrees(
        "control",
        "module t\nfn main() {\n  var total = 0\n  for n in [1, 2, 3, 4] {\n    if n == 3 { continue } else { Unit }\n    total = total + n\n  }\n  print(\"{total}\")\n  var i = 0\n  while i < 3 { i = i + 1 }\n  print(\"{i}\")\n}\n",
    );
}

#[test]
fn recursion_agrees() {
    assert_agrees(
        "recursion",
        "module t\nfn fact(n: Int) -> Int {\n  if n <= 1 { 1 } else { n * fact(n - 1) }\n}\nfn main() {\n  print(\"{fact(10)}\")\n}\n",
    );
}

#[test]
fn records_agree() {
    assert_agrees(
        "records",
        "module t\nrecord P {\n  x: Int\n  y: Int\n}\nfn main() {\n  let p = P { x: 3, y: 4 }\n  print(\"{p.x + p.y}\")\n}\n",
    );
}

#[test]
fn enums_and_match_agree() {
    assert_agrees(
        "enums",
        concat!(
            "module t\n",
            "enum C {\n  Red\n  Tagged(n: Int)\n}\n",
            "fn pick(c: C) -> Int {\n  match c {\n    Red -> 1\n    Tagged(n) -> n\n  }\n}\n",
            "fn main() {\n  print(\"{pick(Tagged(42))}\")\n  print(\"{pick(Red)}\")\n}\n"
        ),
    );
}

#[test]
fn result_and_question_mark_agree() {
    assert_agrees(
        "result",
        concat!(
            "module t\n",
            "fn get(fail: Bool) -> Result<Int, Int> {\n  if fail { Err(7) } else { Ok(5) }\n}\n",
            "fn use_it(fail: Bool) -> Result<Int, Int> {\n  let v = get(fail)?\n  Ok(v + 1)\n}\n",
            "fn report(fail: Bool) {\n",
            "  match use_it(fail) {\n    Ok(v) -> print(\"ok {v}\")\n    Err(e) -> print(\"err {e}\")\n  }\n}\n",
            "fn main() {\n  report(false)\n  report(true)\n}\n"
        ),
    );
}

#[test]
fn borrows_lower_transparently() {
    // `&T` has the same type as `T`: values are immutable, so a borrow and its
    // referent lower to the same thing. This used to be refused.
    assert_agrees(
        "borrows",
        concat!(
            "module t\n",
            "fn total(xs: &List<Int>) -> Int {\n",
            "  var sum = 0\n  for x in xs { sum = sum + x }\n  sum\n}\n",
            "fn main() {\n",
            "  let xs = [1, 2, 3]\n",
            "  print(\"{total(&xs)}\")\n",
            "  print(\"{total(&xs)}\")\n",
            "}\n"
        ),
    );
}

#[test]
fn strings_agree() {
    // Comparisons are computed outside the interpolation: a string literal
    // cannot be nested inside one.
    assert_agrees(
        "strings",
        concat!(
            "module t\n",
            "fn main() {\n",
            "  let a = \"x\"\n",
            "  let same = a == \"x\"\n",
            "  let before = a < \"y\"\n",
            "  print(\"{a}y\")\n",
            "  print(\"{same}\")\n",
            "  print(\"{before}\")\n",
            "}\n"
        ),
    );
}

#[test]
fn contracts_trap_in_both() {
    assert_agrees(
        "contracts",
        concat!(
            "module t\n",
            "fn fee(a: Int) -> Int\n  requires a > 0\n{ a / 50 }\n",
            "fn main() {\n  print(\"{fee(500)}\")\n  print(\"{fee(0)}\")\n}\n"
        ),
    );
}

#[test]
fn short_circuit_agrees() {
    // If `||` evaluated its right side, this would divide by zero.
    assert_agrees(
        "shortcircuit",
        "module t\nfn main() {\n  let ok = true || (1 / 0) == 0\n  print(\"{ok}\")\n}\n",
    );
}

#[test]
fn the_spec_hello_world_agrees() {
    assert_agrees(
        "greet",
        "module greet\n\nfn main() {\n  let names = [\"ada\", \"alan\", \"grace\"]\n  for n in names {\n    print(\"hello, {n}\")\n  }\n}\n",
    );
}

// --- what the backend refuses -------------------------------------------

#[test]
fn unsupported_constructs_are_named_not_miscompiled() {
    for (src, expected) in [
        (
            "module t\nfn first<T>(xs: List<T>) -> Option<T> { None }\nfn main() { }\n",
            "generic function",
        ),
        (
            "module t\nuse std/http@1:{post}\nfn main() {\n  let r = post(\"/x\")\n}\n",
            "imported function",
        ),
        // A core function with no C implementation is refused by name, and
        // distinctly from an import, which has no definition at all.
        (
            "module t\nfn main() {\n  let r = read_file(\"x\")\n}\n",
            "read_file",
        ),
    ] {
        let m = module(src);
        let (_, types) = check_with_types(&m);
        let emitted = emit(&m, &types);
        assert!(
            !emitted.is_complete(),
            "expected a refusal mentioning {expected}"
        );
        assert!(
            emitted
                .unsupported
                .iter()
                .any(|d| d.message.contains(expected)),
            "expected a refusal mentioning {expected}, got {:?}",
            emitted
                .unsupported
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
        );
    }
}
