//! The interactive session.
//!
//! Driven by piping input to the real binary, because the behaviour worth
//! pinning is what someone typing at it actually sees.

use std::io::Write;
use std::process::{Command, Stdio};

/// Feed `input` to `vise repl` and return everything it printed, with the
/// prompts removed so the assertions are about content.
fn repl(input: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_vise"))
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning vise repl");

    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("writing to the repl");

    let output = child.wait_with_output().expect("waiting for the repl");
    String::from_utf8_lossy(&output.stdout)
        .replace("vise> ", "")
        .replace("  ... ", "")
}

/// The lines a session produced, ignoring the banner.
fn lines(input: &str) -> Vec<String> {
    repl(input)
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

#[test]
fn an_expression_prints_its_value() {
    assert_eq!(lines("1 + 2 * 3\n"), ["7"]);
}

#[test]
fn a_unit_expression_prints_only_what_it_produced() {
    // `print` returns Unit; a second line saying "Unit" would be noise.
    assert_eq!(lines("print(\"hi\")\n"), ["hi"]);
}

#[test]
fn bindings_persist_across_inputs() {
    assert_eq!(lines("let a = 40\na + 2\n"), ["42"]);
}

#[test]
fn declarations_persist_across_inputs() {
    assert_eq!(
        lines("fn double(n: Int) -> Int { n * 2 }\ndouble(21)\n"),
        ["42"]
    );
}

#[test]
fn a_loop_can_mutate_a_binding_made_earlier() {
    assert_eq!(
        lines("var total = 0\nfor n in [1, 2, 3] { total = total + n }\ntotal\n"),
        ["6"]
    );
}

#[test]
fn output_from_earlier_inputs_is_not_repeated() {
    // The session replays from the start every time, which is only invisible
    // because §11 makes the replay identical. If the skipping were wrong, the
    // earlier line would appear twice.
    assert_eq!(lines("var n = 1\nprint(\"once\")\nn + 1\n"), ["once", "2"]);
}

#[test]
fn an_expression_is_not_remembered_as_a_statement() {
    // A bug this test exists for: a trailing expression becomes the block's
    // tail rather than a statement, and treating it as one corrupted the
    // bookkeeping so later output vanished.
    assert_eq!(lines("var n = 5\nn + 1\nn\nn + 2\n"), ["6", "5", "7"]);
}

#[test]
fn a_rejected_input_leaves_the_session_intact() {
    let out = lines("let a = 1\nnot_a_name()\na + 1\n");
    assert!(out.iter().any(|l| l.contains("V0201")), "{out:?}");
    assert_eq!(out.last().map(String::as_str), Some("2"));
}

#[test]
fn a_trap_leaves_the_session_intact() {
    let out = lines("let a = 1\n1 / 0\na + 1\n");
    assert!(
        out.iter().any(|l| l.contains("division by zero")),
        "{out:?}"
    );
    assert_eq!(out.last().map(String::as_str), Some("2"));
}

#[test]
fn a_definition_may_span_lines() {
    assert_eq!(
        lines("fn add(a: Int, b: Int) -> Int {\n  a + b\n}\nadd(2, 3)\n"),
        ["5"]
    );
}

#[test]
fn type_reports_without_running() {
    let out = lines("let s = \"x\"\n:type s\n");
    assert_eq!(out, ["s : Str"]);
}

#[test]
fn list_shows_the_session_as_a_module() {
    let out = repl("let a = 1\n:list\n");
    assert!(out.contains("module repl"), "{out}");
    assert!(out.contains("fn main()"), "{out}");
    assert!(out.contains("let a = 1"), "{out}");
}

#[test]
fn reset_forgets_everything() {
    let out = lines("let a = 1\n:reset\na\n");
    assert!(out.iter().any(|l| l.contains("cleared")), "{out:?}");
    assert!(out.iter().any(|l| l.contains("V0201")), "{out:?}");
}

#[test]
fn quit_exits_cleanly() {
    let status = Command::new(env!("CARGO_BIN_EXE_vise"))
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .and_then(|mut c| {
            c.stdin
                .as_mut()
                .expect("stdin")
                .write_all(b":quit\n")
                .and_then(|()| c.wait())
        })
        .expect("running the repl");
    assert!(status.success());
}
