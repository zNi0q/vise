//! The diagnostic type.
//!
//! Vise treats a diagnostic as a data structure that an agent repairs against,
//! not as a sentence for a person to read. The design target from the spec is
//! that a correct repair follows from the diagnostic alone, without re-reading
//! the source, so a diagnostic carries four things beyond its message:
//!
//! - **labels** — secondary spans, so `V0601` can point at the use *and* the
//!   move that invalidated it;
//! - **notes** — prose that does not belong to any one span;
//! - **in-scope names** — what the author could have written instead, which is
//!   what makes `V0201` self-repairing;
//! - **fixes** — concrete edits, ranked by confidence.

use crate::{Code, Span};

/// How much a diagnostic matters. Vise has no lint tiers: an error stops the
/// build, and a warning is something the compiler will fix for you.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl Severity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
        }
    }
}

/// How much the compiler trusts a suggested fix.
///
/// Fixes are ranked so that a repair loop can apply the top one without
/// deliberating. `Certain` means the compiler would have written this edit
/// itself, and `vise fix` applies only fixes at this level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Confidence {
    /// One reading of the author's intent. Safe to apply unattended.
    Certain,
    /// The most likely of several readings.
    Likely,
    /// Plausible, offered so the agent does not have to guess blindly.
    Possible,
}

impl Confidence {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Certain => "certain",
            Self::Likely => "likely",
            Self::Possible => "possible",
        }
    }
}

/// The category of edit a fix performs. A machine consumer switches on this
/// rather than pattern-matching prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FixKind {
    AddEffect,
    RemoveEffect,
    AddLifetime,
    AddImport,
    AddMatchArm,
    MakePublic,
    Borrow,
    BorrowMut,
    Clone,
    HandleResult,
    DiscardResult,
    Replace,
    SplitModule,
}

impl FixKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AddEffect => "add_effect",
            Self::RemoveEffect => "remove_effect",
            Self::AddLifetime => "add_lifetime",
            Self::AddImport => "add_import",
            Self::AddMatchArm => "add_match_arm",
            Self::MakePublic => "make_public",
            Self::Borrow => "borrow",
            Self::BorrowMut => "borrow_mut",
            Self::Clone => "clone",
            Self::HandleResult => "handle_result",
            Self::DiscardResult => "discard_result",
            Self::Replace => "replace",
            Self::SplitModule => "split_module",
        }
    }
}

/// A concrete edit that would resolve the diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub kind: FixKind,
    /// Replacement text.
    pub edit: String,
    /// The range `edit` replaces. `None` means the diagnostic's own span.
    pub span: Option<Span>,
    pub confidence: Confidence,
}

impl Fix {
    #[must_use]
    pub fn new(kind: FixKind, edit: impl Into<String>) -> Self {
        Self {
            kind,
            edit: edit.into(),
            span: None,
            confidence: Confidence::Likely,
        }
    }

    #[must_use]
    pub fn at(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    #[must_use]
    pub fn confidence(mut self, c: Confidence) -> Self {
        self.confidence = c;
        self
    }
}

/// A secondary span that explains the primary one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

/// A single compiler diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    /// Names in scope at the point of the error. Populated for resolution
    /// failures, where knowing the alternatives *is* the fix.
    pub in_scope: Vec<String>,
    /// Candidate edits, most confident first once [`Diagnostic::rank`] has run.
    pub fixes: Vec<Fix>,
}

impl Diagnostic {
    fn new(severity: Severity, code: Code, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            severity,
            message: message.into(),
            span,
            labels: Vec::new(),
            notes: Vec::new(),
            in_scope: Vec::new(),
            fixes: Vec::new(),
        }
    }

    #[must_use]
    pub fn error(code: Code, span: Span, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, span, message)
    }

    #[must_use]
    pub fn warning(code: Code, span: Span, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, span, message)
    }

    #[must_use]
    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
        });
        self
    }

    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    #[must_use]
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fixes.push(fix);
        self
    }

    /// Record what was in scope, so the reader can pick a real name.
    #[must_use]
    pub fn with_scope<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.in_scope = names.into_iter().map(Into::into).collect();
        self.in_scope.sort();
        self.in_scope.dedup();
        self
    }

    /// Sort fixes most-confident first. Stable, so equally confident fixes keep
    /// the order the compiler produced them in.
    pub fn rank(&mut self) {
        self.fixes.sort_by_key(|f| f.confidence);
    }

    /// The single fix `vise fix` would apply unattended, if there is one.
    #[must_use]
    pub fn autofix(&self) -> Option<&Fix> {
        let mut certain = self
            .fixes
            .iter()
            .filter(|f| f.confidence == Confidence::Certain);
        let first = certain.next()?;
        // Ambiguity is not automatable: if two certain fixes disagree, a human
        // or a model has to choose.
        certain.next().is_none().then_some(first)
    }

    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self.severity, Severity::Error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileId;

    fn span(start: u32, end: u32) -> Span {
        Span::new(FileId(0), start, end)
    }

    fn diag() -> Diagnostic {
        Diagnostic::error(Code::UndeclaredEffect, span(0, 4), "boom")
    }

    #[test]
    fn a_new_diagnostic_is_an_error_with_no_extras() {
        let d = diag();
        assert!(d.is_error());
        assert!(d.labels.is_empty() && d.notes.is_empty() && d.fixes.is_empty());
    }

    #[test]
    fn ranking_puts_the_most_confident_fix_first() {
        let mut d = diag()
            .with_fix(Fix::new(FixKind::Clone, "c").confidence(Confidence::Possible))
            .with_fix(Fix::new(FixKind::Borrow, "b").confidence(Confidence::Certain))
            .with_fix(Fix::new(FixKind::Replace, "r").confidence(Confidence::Likely));
        d.rank();
        let order: Vec<_> = d.fixes.iter().map(|f| f.kind).collect();
        assert_eq!(order, [FixKind::Borrow, FixKind::Replace, FixKind::Clone]);
    }

    #[test]
    fn ranking_is_stable_among_equal_confidence() {
        let mut d = diag()
            .with_fix(Fix::new(FixKind::Borrow, "b").confidence(Confidence::Likely))
            .with_fix(Fix::new(FixKind::Clone, "c").confidence(Confidence::Likely));
        d.rank();
        let order: Vec<_> = d.fixes.iter().map(|f| f.kind).collect();
        assert_eq!(order, [FixKind::Borrow, FixKind::Clone]);
    }

    #[test]
    fn a_lone_certain_fix_is_applied_unattended() {
        let d = diag()
            .with_fix(Fix::new(FixKind::Borrow, "&x").confidence(Confidence::Certain))
            .with_fix(Fix::new(FixKind::Clone, "x.clone()").confidence(Confidence::Likely));
        assert_eq!(d.autofix().map(|f| f.kind), Some(FixKind::Borrow));
    }

    #[test]
    fn two_certain_fixes_are_not_automatable() {
        let d = diag()
            .with_fix(Fix::new(FixKind::Borrow, "&x").confidence(Confidence::Certain))
            .with_fix(Fix::new(FixKind::Clone, "x.clone()").confidence(Confidence::Certain));
        assert!(d.autofix().is_none(), "ambiguity must not be auto-applied");
    }

    #[test]
    fn no_certain_fix_means_no_autofix() {
        let d = diag().with_fix(Fix::new(FixKind::Clone, "c").confidence(Confidence::Likely));
        assert!(d.autofix().is_none());
    }

    #[test]
    fn scope_names_are_sorted_and_deduplicated() {
        let d = diag().with_scope(["post", "get", "post"]);
        assert_eq!(d.in_scope, ["get", "post"]);
    }

    #[test]
    fn a_fix_defaults_to_the_diagnostic_span() {
        assert_eq!(Fix::new(FixKind::AddEffect, "!{net}").span, None);
        let s = span(3, 9);
        assert_eq!(Fix::new(FixKind::AddEffect, "!{net}").at(s).span, Some(s));
    }
}
