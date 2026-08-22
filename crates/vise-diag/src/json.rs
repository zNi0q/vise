//! JSON rendering of diagnostics.
//!
//! Per the spec, JSON is the compiler's *primary* output and the human text is
//! a rendering of it. This module has no dependencies: JSON is small enough to
//! emit by hand, and an empty supply chain is worth more here than the
//! convenience of a derive macro.

use crate::{Diagnostic, Fix, Label, SourceMap, Span};
use std::fmt::Write as _;

/// Schema version of the report envelope. Bumped only on a breaking change to
/// the shape, so consumers can pin.
pub const SCHEMA_VERSION: u32 = 1;

/// Append `s` to `out` as a quoted JSON string.
fn push_quoted(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            // Remaining C0 controls have no short form and must be escaped.
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            // Everything else, including non-ASCII, is legal JSON text as-is.
            c => out.push(c),
        }
    }
    out.push('"');
}

fn push_field_str(out: &mut String, key: &str, value: &str) {
    push_quoted(out, key);
    out.push(':');
    push_quoted(out, value);
}

fn push_array<T>(out: &mut String, items: &[T], mut each: impl FnMut(&mut String, &T)) {
    out.push('[');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        each(out, item);
    }
    out.push(']');
}

fn push_span(out: &mut String, span: Span, map: &SourceMap) {
    let file = map.file(span.file);
    let pos = file.line_col(span.start);
    out.push('{');
    push_field_str(out, "file", file.name());
    let _ = write!(
        out,
        ",\"line\":{},\"col\":{},\"start\":{},\"end\":{}",
        pos.line, pos.col, span.start, span.end
    );
    out.push('}');
}

fn push_label(out: &mut String, label: &Label, map: &SourceMap) {
    out.push('{');
    out.push_str("\"span\":");
    push_span(out, label.span, map);
    out.push(',');
    push_field_str(out, "message", &label.message);
    out.push('}');
}

fn push_fix(out: &mut String, fix: &Fix, diag_span: Span, map: &SourceMap) {
    out.push('{');
    push_field_str(out, "kind", fix.kind.as_str());
    out.push(',');
    push_field_str(out, "edit", &fix.edit);
    out.push(',');
    push_field_str(out, "confidence", fix.confidence.as_str());
    out.push_str(",\"span\":");
    push_span(out, fix.span.unwrap_or(diag_span), map);
    out.push('}');
}

/// Render one diagnostic as a JSON object.
#[must_use]
pub fn diagnostic(d: &Diagnostic, map: &SourceMap) -> String {
    let mut out = String::new();
    out.push('{');
    push_field_str(&mut out, "code", d.code.as_str());
    out.push(',');
    push_field_str(&mut out, "severity", d.severity.as_str());
    out.push(',');
    push_field_str(&mut out, "message", &d.message);
    out.push_str(",\"span\":");
    push_span(&mut out, d.span, map);

    out.push_str(",\"labels\":");
    push_array(&mut out, &d.labels, |o, l| push_label(o, l, map));

    out.push_str(",\"notes\":");
    push_array(&mut out, &d.notes, |o, n| push_quoted(o, n));

    out.push_str(",\"in_scope\":");
    push_array(&mut out, &d.in_scope, |o, n| push_quoted(o, n));

    out.push_str(",\"fixes\":");
    push_array(&mut out, &d.fixes, |o, f| push_fix(o, f, d.span, map));

    out.push('}');
    out
}

/// Render a whole run as a single JSON document.
#[must_use]
pub fn report(diagnostics: &[Diagnostic], map: &SourceMap) -> String {
    let errors = diagnostics.iter().filter(|d| d.is_error()).count();
    let warnings = diagnostics.len() - errors;

    let mut out = String::new();
    let _ = write!(out, "{{\"version\":{SCHEMA_VERSION},\"diagnostics\":");
    push_array(&mut out, diagnostics, |o, d| {
        o.push_str(&diagnostic(d, map));
    });
    let _ = write!(out, ",\"errors\":{errors},\"warnings\":{warnings}}}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Code, Confidence, Diagnostic, FixKind, Span};

    fn quoted(s: &str) -> String {
        let mut out = String::new();
        push_quoted(&mut out, s);
        out
    }

    #[test]
    fn quotes_and_backslashes_are_escaped() {
        assert_eq!(quoted("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }

    #[test]
    fn whitespace_controls_use_short_forms() {
        assert_eq!(quoted("a\nb\tc\rd"), "\"a\\nb\\tc\\rd\"");
        assert_eq!(quoted("\u{08}\u{0c}"), "\"\\b\\f\"");
    }

    #[test]
    fn other_controls_use_unicode_escapes() {
        assert_eq!(quoted("\u{0}\u{1f}"), "\"\\u0000\\u001f\"");
    }

    #[test]
    fn non_ascii_is_passed_through_unescaped() {
        assert_eq!(quoted("héllo ✓"), "\"héllo ✓\"");
    }

    #[test]
    fn an_empty_report_still_carries_the_envelope() {
        let map = SourceMap::new();
        assert_eq!(
            report(&[], &map),
            "{\"version\":1,\"diagnostics\":[],\"errors\":0,\"warnings\":0}"
        );
    }

    #[test]
    fn a_diagnostic_matches_the_documented_shape() {
        let mut map = SourceMap::new();
        let f = map.add("payments.vise", "module payments\nlet x = post()\n");
        let d = Diagnostic::error(
            Code::UndeclaredEffect,
            Span::new(f, 24, 28),
            "call introduces effect `net`, not declared by `charge`",
        )
        .with_fix(crate::Fix::new(FixKind::AddEffect, "!{net}").confidence(Confidence::Certain));

        let json = diagnostic(&d, &map);
        assert!(json.contains("\"code\":\"V0401\""), "{json}");
        assert!(json.contains("\"severity\":\"error\""), "{json}");
        assert!(json.contains("\"line\":2,\"col\":9"), "{json}");
        assert!(json.contains("\"kind\":\"add_effect\""), "{json}");
        assert!(json.contains("\"confidence\":\"certain\""), "{json}");
    }

    #[test]
    fn a_fix_without_a_span_inherits_the_diagnostic_span() {
        let mut map = SourceMap::new();
        let f = map.add("t.vise", "abcdef");
        let d = Diagnostic::error(Code::UnusedResult, Span::new(f, 2, 4), "m")
            .with_fix(crate::Fix::new(FixKind::DiscardResult, "let _ = "));
        let json = diagnostic(&d, &map);
        assert_eq!(json.matches("\"start\":2,\"end\":4").count(), 2);
    }

    #[test]
    fn counts_split_errors_from_warnings() {
        let mut map = SourceMap::new();
        let f = map.add("t.vise", "x");
        let s = Span::new(f, 0, 1);
        let ds = [
            Diagnostic::error(Code::UnusedResult, s, "a"),
            Diagnostic::warning(Code::UnusedDeclaredEffect, s, "b"),
            Diagnostic::error(Code::UnknownName, s, "c"),
        ];
        let json = report(&ds, &map);
        assert!(json.contains("\"errors\":2,\"warnings\":1"), "{json}");
    }

    /// Prints a report containing every awkward character, so an external JSON
    /// parser can confirm the output is well formed. Run with `--nocapture`.
    #[test]
    fn emits_parseable_json_for_external_validation() {
        let mut map = SourceMap::new();
        let f = map.add("w\u{e9}ird\"name.vise", "module m\nlet x = 1\n");
        let s = Span::new(f, 9, 12);
        let d = Diagnostic::error(
            Code::UnknownName,
            s,
            "quote \" backslash \\ tab \t nul \u{0}",
        )
        .with_label(Span::new(f, 0, 6), "declared\nhere")
        .with_note("a note with \u{fc}nicode \u{2713}")
        .with_scope(["post", "get"])
        .with_fix(crate::Fix::new(FixKind::AddImport, "use std/http@1:{post}"));
        println!("JSON_SAMPLE {}", report(&[d], &map));
    }
}
