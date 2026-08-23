//! Applying fixes to source text.
//!
//! Only fixes that [`Diagnostic::autofix`] returns are applied: a lone
//! `Certain` suggestion. A diagnostic offering two equally confident edits is
//! deliberately skipped, because choosing between them is the author's call.
//!
//! Edits are applied right to left so that earlier spans keep their offsets,
//! and an edit overlapping one already applied is skipped rather than
//! producing text neither fix intended.

use crate::{Diagnostic, Span};

/// The outcome of an application pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The rewritten source.
    pub text: String,
    /// How many fixes were applied.
    pub applied: usize,
    /// How many were skipped because they overlapped an applied edit.
    pub skipped: usize,
}

impl Applied {
    #[must_use]
    pub fn changed(&self) -> bool {
        self.applied > 0
    }
}

/// Apply every unambiguous fix in `diagnostics` to `text`.
#[must_use]
pub fn apply(text: &str, diagnostics: &[Diagnostic]) -> Applied {
    let mut edits: Vec<(Span, &str)> = diagnostics
        .iter()
        .filter_map(|d| {
            let fix = d.autofix()?;
            Some((fix.span.unwrap_or(d.span), fix.edit.as_str()))
        })
        .collect();

    // Right to left, so applying one edit does not move the next.
    edits.sort_by_key(|(span, _)| std::cmp::Reverse((span.start, span.end)));

    let mut out = text.to_owned();
    let mut applied = 0usize;
    let mut skipped = 0usize;
    // Start of the leftmost edit applied so far; anything reaching past it
    // would be operating on text that has already changed.
    let mut boundary = usize::MAX;

    for (span, edit) in edits {
        let start = span.start as usize;
        let end = span.end as usize;

        if start > end || end > out.len() || end > boundary {
            skipped += 1;
            continue;
        }
        if !out.is_char_boundary(start) || !out.is_char_boundary(end) {
            skipped += 1;
            continue;
        }

        out.replace_range(start..end, edit);
        boundary = start;
        applied += 1;
    }

    Applied {
        text: out,
        applied,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Code, Confidence, Diagnostic, FileId, Fix, FixKind};

    fn span(start: u32, end: u32) -> Span {
        Span::new(FileId(0), start, end)
    }

    fn certain(at: Span, edit: &str) -> Diagnostic {
        Diagnostic::error(Code::UndeclaredEffect, at, "m").with_fix(
            Fix::new(FixKind::AddEffect, edit)
                .at(at)
                .confidence(Confidence::Certain),
        )
    }

    #[test]
    fn a_certain_fix_is_applied() {
        let out = apply("fn f() !{} { }", &[certain(span(7, 10), "!{io}")]);
        assert_eq!(out.text, "fn f() !{io} { }");
        assert_eq!((out.applied, out.skipped), (1, 0));
        assert!(out.changed());
    }

    #[test]
    fn an_ambiguous_diagnostic_is_left_alone() {
        let d = Diagnostic::error(Code::UnusedResult, span(0, 2), "m")
            .with_fix(Fix::new(FixKind::Borrow, "a").confidence(Confidence::Certain))
            .with_fix(Fix::new(FixKind::Clone, "b").confidence(Confidence::Certain));
        let out = apply("xy", &[d]);
        assert_eq!(out.text, "xy");
        assert_eq!(out.applied, 0);
    }

    #[test]
    fn a_merely_likely_fix_is_left_alone() {
        let d = Diagnostic::error(Code::NonSnakeCase, span(0, 5), "m")
            .with_fix(Fix::new(FixKind::Replace, "my_var").confidence(Confidence::Likely));
        assert_eq!(apply("myVar", &[d]).applied, 0);
    }

    #[test]
    fn several_edits_apply_without_disturbing_each_other() {
        // Left to right in the source, applied right to left.
        let out = apply(
            "aaa bbb ccc",
            &[
                certain(span(0, 3), "X"),
                certain(span(4, 7), "YY"),
                certain(span(8, 11), "ZZZ"),
            ],
        );
        assert_eq!(out.text, "X YY ZZZ");
        assert_eq!(out.applied, 3);
    }

    #[test]
    fn an_overlapping_edit_is_skipped_rather_than_corrupting_the_text() {
        let out = apply(
            "abcdef",
            &[certain(span(0, 4), "X"), certain(span(2, 6), "Y")],
        );
        // The rightmost applies; the one reaching into it is dropped.
        assert_eq!(out.text, "abY");
        assert_eq!((out.applied, out.skipped), (1, 1));
    }

    #[test]
    fn an_empty_span_inserts() {
        let out = apply("save(1)", &[certain(span(0, 0), "let _ = ")]);
        assert_eq!(out.text, "let _ = save(1)");
    }

    #[test]
    fn an_out_of_range_span_is_skipped() {
        let out = apply("abc", &[certain(span(2, 99), "X")]);
        assert_eq!(out.text, "abc");
        assert_eq!(out.skipped, 1);
    }

    #[test]
    fn an_edit_splitting_a_character_is_skipped() {
        // "é" is two bytes; offset 1 is inside it.
        let out = apply("aéb", &[certain(span(1, 2), "X")]);
        assert_eq!(out.text, "aéb");
        assert_eq!(out.skipped, 1);
    }

    #[test]
    fn nothing_to_do_leaves_the_text_untouched() {
        let out = apply("abc", &[]);
        assert_eq!(out.text, "abc");
        assert!(!out.changed());
    }
}
