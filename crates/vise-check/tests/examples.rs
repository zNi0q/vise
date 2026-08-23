//! End-to-end: run every stage over every file in `examples/`.
//!
//! The unit suites cover each stage in isolation. This pins what the compiler
//! says about whole files, which is what a reader of the repository actually
//! runs, and it fails if an example is added without deciding what it should
//! report.

use std::collections::BTreeSet;
use std::path::PathBuf;

use vise_diag::{FileId, SourceMap};

/// Every example, and the diagnostic codes it should produce.
const EXPECTED: &[(&str, &[&str])] = &[
    ("greet.vise", &[]),
    ("payments.vise", &[]),
    // Lexical faults, all reported in one run. The trailing V0102 is a real
    // cascade: the unterminated string swallowed the closing paren, so the
    // call is genuinely unfinished.
    ("broken.vise", &["V0006", "V0004", "V0002", "V0102"]),
    // Two calls to functions that do not exist.
    ("hallucinated.vise", &["V0201", "V0201"]),
    // `!{}` claims purity, but `print` performs io.
    ("effects.vise", &["V0401"]),
    // A nested constructor with no arm.
    ("incomplete.vise", &["V0301"]),
    // A `Result` thrown away in a loop body.
    ("dropped.vise", &["V0501"]),
    // Swapped arguments, caught by distinct types.
    ("swapped.vise", &["V0302", "V0302"]),
];

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// The whole pipeline, in the order `vise check` runs it.
fn diagnose(path: &str, text: &str) -> Vec<String> {
    let mut map = SourceMap::new();
    let file = map.add(path, text.to_owned());
    let parsed = vise_parse::parse(text, file);
    let mut out = parsed.diagnostics;

    if let Some(module) = &parsed.module {
        let lines = map.file(file).line_count();
        if let Some(d) = vise_check::check_module_length(lines, module.name.span) {
            out.push(d);
        }
        out.extend(vise_check::resolve(module));
        out.extend(vise_check::check_effects(module));
        out.extend(vise_check::check_exhaustive(module));
        out.extend(vise_check::check_results(module));
        out.extend(vise_check::check_types(module));
    }
    out.iter().map(|d| d.code.as_str().to_owned()).collect()
}

fn read(name: &str) -> String {
    let path = examples_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

#[test]
fn every_example_reports_exactly_what_is_expected() {
    for (name, expected) in EXPECTED {
        let got = diagnose(name, &read(name));
        assert_eq!(&got, expected, "{name}");
    }
}

#[test]
fn the_clean_examples_stay_clean() {
    // These two are the ones a reader is invited to trust.
    for name in ["greet.vise", "payments.vise"] {
        assert!(
            diagnose(name, &read(name)).is_empty(),
            "{name} should check clean"
        );
    }
}

#[test]
fn every_example_file_is_accounted_for() {
    // Adding an example without deciding what it should report is an omission,
    // not a pass.
    let listed: BTreeSet<&str> = EXPECTED.iter().map(|(n, _)| *n).collect();
    let mut found = BTreeSet::new();
    for entry in std::fs::read_dir(examples_dir()).expect("examples/ should exist") {
        let entry = entry.expect("a readable entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".vise") {
            found.insert(name);
        }
    }
    let found: BTreeSet<&str> = found.iter().map(String::as_str).collect();
    assert_eq!(found, listed, "examples/ and the EXPECTED table disagree");
}

#[test]
fn a_broken_file_still_yields_a_module() {
    // Recovery matters: later stages need a tree even when parsing failed.
    let text = read("broken.vise");
    let parsed = vise_parse::parse(&text, FileId(0));
    assert!(parsed.has_errors());
    assert!(
        parsed.module.is_some(),
        "recovery should still produce a module"
    );
}
