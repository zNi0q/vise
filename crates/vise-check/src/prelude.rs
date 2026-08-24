//! The `core` prelude.
//!
//! Spec §3 shows `use core:{Result, Ok, Err, Option, Some, None}` marked
//! "implicit; shown for clarity", but never enumerates `core` anywhere. §1 then
//! calls `print` without importing it. That is a gap: the closed namespace is
//! the mechanism behind `V0201`, and "what is in scope" cannot be answered
//! without knowing exactly what `core` holds.
//!
//! What follows is provisional — the smallest core that the spec's own worked
//! examples require. See open spec issue 5 in `TRACKER.md`.

/// Built-in type names, from §4.
pub const TYPES: &[&str] = &[
    "Int", "Float", "Bool", "Char", "Str", "Unit", "List", "Map", "Set", "Option", "Result",
];

/// Constructors of the built-in sum types, from §4 and §8.
pub const CONSTRUCTORS: &[&str] = &["Ok", "Err", "Some", "None"];

/// Every name `core` puts in scope. Free functions come from
/// [`crate::builtins`], the single enumeration every stage reads.
pub fn all() -> impl Iterator<Item = (&'static str, Symbol)> {
    TYPES
        .iter()
        .map(|n| (*n, Symbol::Type))
        .chain(CONSTRUCTORS.iter().map(|n| (*n, Symbol::Constructor)))
        .chain(
            crate::builtins::all()
                .into_iter()
                .map(|b| (b.name, Symbol::Value)),
        )
}

/// What a name refers to.
///
/// One namespace suffices: §2's casing rule means a value name can never
/// collide with a type or constructor name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    Type,
    /// An enum variant, or a built-in like `Ok`.
    Constructor,
    /// A binding, parameter, or function.
    Value,
    /// A `<T>` or `<'a>` parameter.
    Generic,
}

impl Symbol {
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Type => "a type",
            Self::Constructor => "a constructor",
            Self::Value => "a value",
            Self::Generic => "a generic parameter",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_name_the_spec_imports_implicitly_is_present() {
        // §3: `use core:{Result, Ok, Err, Option, Some, None}`.
        let names: BTreeSet<_> = all().map(|(n, _)| n).collect();
        for n in ["Result", "Ok", "Err", "Option", "Some", "None"] {
            assert!(names.contains(n), "core is missing {n}");
        }
    }

    #[test]
    fn every_name_the_spec_examples_use_is_present() {
        let names: BTreeSet<_> = all().map(|(n, _)| n).collect();
        for n in ["print", "Int", "Str", "List", "Map", "Set"] {
            assert!(names.contains(n), "core is missing {n}");
        }
    }

    #[test]
    fn core_has_no_duplicates() {
        let names: Vec<_> = all().map(|(n, _)| n).collect();
        let unique: BTreeSet<_> = names.iter().collect();
        assert_eq!(names.len(), unique.len());
    }
}
