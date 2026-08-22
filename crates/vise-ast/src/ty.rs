//! Types and effect rows.

use vise_diag::Span;

use crate::Ident;

/// A type as written in source. Nothing here is resolved; `Named` holds
/// whatever the author wrote, and resolution happens later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Type {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeKind {
    /// `Int`, `List<T>`, `Result<Receipt, ChargeError>`.
    Named { name: Ident, args: Vec<Type> },
    /// `&T`, `&mut T`, `&'a Str`.
    ///
    /// The lifetime is `None` when elided, which is the common case; §9 says
    /// elision is the default and an explicit lifetime is written only where a
    /// signature is genuinely ambiguous.
    Ref {
        lifetime: Option<Ident>,
        is_mut: bool,
        inner: Box<Type>,
    },
    /// `Unit`, written `()` in a return position.
    Unit,
}

impl Type {
    #[must_use]
    pub const fn new(kind: TypeKind, span: Span) -> Self {
        Self { kind, span }
    }

    /// Whether this type borrows, at the top level.
    #[must_use]
    pub const fn is_ref(&self) -> bool {
        matches!(self.kind, TypeKind::Ref { .. })
    }
}

/// The primitive capabilities a function may use. Spec §7: the set is closed,
/// so it is an enum rather than a name to resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effect {
    Io,
    Fs,
    Net,
    Time,
    Rand,
    Env,
    Proc,
}

impl Effect {
    /// Every effect, in the order the spec's table lists them.
    pub const ALL: &'static [Effect] = &[
        Effect::Io,
        Effect::Fs,
        Effect::Net,
        Effect::Time,
        Effect::Rand,
        Effect::Env,
        Effect::Proc,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Io => "io",
            Self::Fs => "fs",
            Self::Net => "net",
            Self::Time => "time",
            Self::Rand => "rand",
            Self::Env => "env",
            Self::Proc => "proc",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|e| e.as_str() == name)
    }
}

/// An effect row as written: `!{net, time}`.
///
/// Absent rows are `None` on the declaration, not an empty row here. Spec §7
/// makes those different things: an omitted row is inferred, an empty one
/// asserts purity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectRow {
    /// Parsed effects, deduplicated and sorted.
    pub effects: Vec<Effect>,
    /// Names that are not effects, kept so the checker can report them with a
    /// span rather than the parser guessing.
    pub unknown: Vec<Ident>,
    pub span: Span,
}

impl EffectRow {
    #[must_use]
    pub fn is_pure(&self) -> bool {
        self.effects.is_empty() && self.unknown.is_empty()
    }

    #[must_use]
    pub fn contains(&self, effect: Effect) -> bool {
        self.effects.contains(&effect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_names_round_trip() {
        for &e in Effect::ALL {
            assert_eq!(Effect::from_name(e.as_str()), Some(e));
        }
    }

    #[test]
    fn domains_are_not_effects() {
        // Spec §7: effects are primitive capabilities. A database client is a
        // library carrying `!{net}`, not an effect of its own.
        for name in ["db", "http", "log", "sql", "print", "io_uring"] {
            assert_eq!(Effect::from_name(name), None, "{name}");
        }
    }

    #[test]
    fn the_row_matches_the_spec_table_exactly() {
        let names: Vec<_> = Effect::ALL.iter().map(|e| e.as_str()).collect();
        assert_eq!(names, ["io", "fs", "net", "time", "rand", "env", "proc"]);
    }
}
