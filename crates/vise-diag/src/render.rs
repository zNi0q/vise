//! Human-readable rendering.
//!
//! JSON is the compiler's primary output; this is a view over the same data.
//! Output is plain text with no colour, so it is deterministic and can be
//! compared exactly in tests.

use crate::{Diagnostic, SourceMap, Span};
use std::fmt::Write as _;

/// Tabs are expanded to this many spaces so carets line up under the source.
const TAB_WIDTH: usize = 4;

fn expand_tabs(s: &str) -> String {
    s.replace('\t', &" ".repeat(TAB_WIDTH))
}

/// Display width of `s` once tabs are expanded, in characters.
fn width(s: &str) -> usize {
    s.chars()
        .map(|c| if c == '\t' { TAB_WIDTH } else { 1 })
        .sum()
}

/// One `line | source` block with a caret run under `span`.
fn push_snippet(out: &mut String, span: Span, map: &SourceMap, gutter: usize, marker: char) {
    let file = map.file(span.file);
    let pos = file.line_col(span.start);
    let Some(line) = file.line_text(pos.line) else {
        return;
    };

    // Byte offset of this line, so the span can be measured against it.
    let line_start = line_start_offset(file, span.start);
    let before = &line[..(span.start as usize - line_start).min(line.len())];
    let visible_end = (span.end as usize - line_start).min(line.len());
    let covered = &line[before.len()..visible_end.max(before.len())];

    let pad = width(before);
    let caret_len = width(covered).max(1);

    let _ = writeln!(out, "{:>gutter$} |", "", gutter = gutter);
    let _ = writeln!(
        out,
        "{:>gutter$} | {}",
        pos.line,
        expand_tabs(line),
        gutter = gutter
    );
    let _ = writeln!(
        out,
        "{:>gutter$} | {}{}",
        "",
        " ".repeat(pad),
        marker.to_string().repeat(caret_len),
        gutter = gutter
    );
}

/// Byte offset at which `offset`'s line begins.
fn line_start_offset(file: &crate::SourceFile, offset: u32) -> usize {
    let text = file.text();
    let mut start = (offset as usize).min(text.len());
    while start > 0 && text.as_bytes()[start - 1] != b'\n' {
        start -= 1;
    }
    start
}

/// Render one diagnostic.
#[must_use]
pub fn diagnostic(d: &Diagnostic, map: &SourceMap) -> String {
    let file = map.file(d.span.file);
    let pos = file.line_col(d.span.start);
    let gutter = pos.line.to_string().len().max(2);

    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}[{}]: {}",
        d.severity.as_str(),
        d.code.as_str(),
        d.message
    );
    let _ = writeln!(
        out,
        "{:>gutter$}--> {}:{}:{}",
        "",
        file.name(),
        pos.line,
        pos.col,
        gutter = gutter
    );
    push_snippet(&mut out, d.span, map, gutter, '^');

    for label in &d.labels {
        push_snippet(&mut out, label.span, map, gutter, '-');
        let _ = writeln!(out, "{:>gutter$} = {}", "", label.message, gutter = gutter);
    }

    for note in &d.notes {
        let _ = writeln!(out, "{:>gutter$} = note: {note}", "", gutter = gutter);
    }

    if !d.in_scope.is_empty() {
        let _ = writeln!(
            out,
            "{:>gutter$} = in scope: {}",
            "",
            d.in_scope.join(", "),
            gutter = gutter
        );
    }

    for fix in &d.fixes {
        let _ = writeln!(
            out,
            "{:>gutter$} = fix ({}): {} `{}`",
            "",
            fix.confidence.as_str(),
            fix.kind.as_str(),
            fix.edit,
            gutter = gutter
        );
    }

    out
}

/// Render a whole run, with a trailing summary.
#[must_use]
pub fn report(diagnostics: &[Diagnostic], map: &SourceMap) -> String {
    let mut out = String::new();
    for d in diagnostics {
        out.push_str(&diagnostic(d, map));
        out.push('\n');
    }
    let errors = diagnostics.iter().filter(|d| d.is_error()).count();
    let warnings = diagnostics.len() - errors;
    match (errors, warnings) {
        (0, 0) => {}
        (e, 0) => {
            let _ = writeln!(out, "{e} error{}", plural(e));
        }
        (0, w) => {
            let _ = writeln!(out, "{w} warning{}", plural(w));
        }
        (e, w) => {
            let _ = writeln!(out, "{e} error{}, {w} warning{}", plural(e), plural(w));
        }
    }
    out
}

const fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Code, Confidence, Diagnostic, Fix, FixKind, SourceMap, Span};

    fn setup(text: &str) -> (SourceMap, crate::FileId) {
        let mut m = SourceMap::new();
        let f = m.add("t.vise", text);
        (m, f)
    }

    #[test]
    fn renders_header_location_and_caret() {
        let (map, f) = setup("module m\nlet x = post()\n");
        let d = Diagnostic::error(
            Code::UnknownName,
            Span::new(f, 17, 21),
            "`post` is not in scope",
        );
        let text = diagnostic(&d, &map);
        assert_eq!(
            text,
            concat!(
                "error[V0201]: `post` is not in scope\n",
                "  --> t.vise:2:9\n",
                "   |\n",
                " 2 | let x = post()\n",
                "   |         ^^^^\n",
            )
        );
    }

    #[test]
    fn renders_scope_notes_and_fixes() {
        let (map, f) = setup("module m\nlet x = post()\n");
        let d = Diagnostic::error(Code::UnknownName, Span::new(f, 17, 21), "not in scope")
            .with_scope(["get", "put"])
            .with_note("imports must be explicit")
            .with_fix(
                Fix::new(FixKind::AddImport, "use std/http@1:{post}")
                    .confidence(Confidence::Certain),
            );
        let text = diagnostic(&d, &map);
        assert!(text.contains("   = in scope: get, put\n"), "{text}");
        assert!(
            text.contains("   = note: imports must be explicit\n"),
            "{text}"
        );
        assert!(
            text.contains("   = fix (certain): add_import `use std/http@1:{post}`\n"),
            "{text}"
        );
    }

    #[test]
    fn a_label_gets_its_own_snippet_with_dashes() {
        let (map, f) = setup("let a = mk()\nuse(a)\nuse(a)\n");
        let d = Diagnostic::error(Code::UseAfterMove, Span::new(f, 17, 18), "used after move")
            .with_label(Span::new(f, 4, 5), "moved here");
        let text = diagnostic(&d, &map);
        assert!(text.contains(" 1 | let a = mk()\n"), "{text}");
        assert!(text.contains("   |     -\n"), "{text}");
        assert!(text.contains("   = moved here\n"), "{text}");
    }

    #[test]
    fn carets_align_past_a_tab() {
        let (map, f) = setup("\tlet x = 1\n");
        // `x` is at byte 5, after a tab that renders as four spaces.
        let d = Diagnostic::error(Code::UnknownName, Span::new(f, 5, 6), "m");
        let text = diagnostic(&d, &map);
        assert!(text.contains(" 1 |     let x = 1\n"), "{text}");
        assert!(text.contains("   |         ^\n"), "{text}");
    }

    #[test]
    fn an_empty_span_still_gets_one_caret() {
        let (map, f) = setup("let x = 1\n");
        let d = Diagnostic::error(Code::UnexpectedToken, Span::new(f, 9, 9), "expected `;`");
        assert!(
            diagnostic(&d, &map).contains("^"),
            "empty span needs a caret"
        );
    }

    #[test]
    fn gutter_widens_for_large_line_numbers() {
        let text = "x\n".repeat(120);
        let (map, f) = setup(&text);
        // Line 120 begins at byte 238 (each line is "x\n").
        let d = Diagnostic::error(Code::UnknownName, Span::new(f, 238, 239), "m");
        let text = diagnostic(&d, &map);
        assert!(text.contains("120 | x\n"), "{text}");
        assert!(text.contains("    | ^\n"), "{text}");
    }

    #[test]
    fn report_summarises_counts_with_correct_plurals() {
        let (map, f) = setup("x\n");
        let s = Span::new(f, 0, 1);
        assert!(
            report(&[Diagnostic::error(Code::UnknownName, s, "a")], &map).ends_with("1 error\n")
        );
        assert!(
            report(
                &[
                    Diagnostic::error(Code::UnknownName, s, "a"),
                    Diagnostic::warning(Code::UnusedDeclaredEffect, s, "b"),
                    Diagnostic::error(Code::UnusedResult, s, "c"),
                ],
                &map
            )
            .ends_with("2 errors, 1 warning\n")
        );
        assert_eq!(report(&[], &map), "");
    }
}
