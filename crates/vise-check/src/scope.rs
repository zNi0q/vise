//! Lexical scopes.
//!
//! Spec §3 gives Vise a closed namespace: a name resolves only if this module
//! defines it, imports it, or `core` provides it. That makes the scope stack
//! the mechanism behind `V0201` — and makes the set of visible names something
//! the compiler can hand back to the author verbatim.

use std::collections::BTreeMap;

use vise_diag::Span;

use crate::prelude::{self, Symbol};

/// A resolved name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub symbol: Symbol,
    /// Where it was declared. `None` for anything `core` provides.
    pub span: Option<Span>,
}

/// A stack of scopes, innermost last.
#[derive(Debug)]
pub struct Scopes {
    stack: Vec<BTreeMap<String, Entry>>,
}

impl Default for Scopes {
    fn default() -> Self {
        Self::new()
    }
}

impl Scopes {
    /// A stack holding just `core`.
    #[must_use]
    pub fn new() -> Self {
        let mut root = BTreeMap::new();
        for (name, symbol) in prelude::all() {
            root.insert(name.to_owned(), Entry { symbol, span: None });
        }
        Self { stack: vec![root] }
    }

    pub fn push(&mut self) {
        self.stack.push(BTreeMap::new());
    }

    pub fn pop(&mut self) {
        // The `core` scope is never popped.
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    /// Declare `name` in the innermost scope.
    ///
    /// Returns the previous declaration's span when it collides *in the same
    /// scope*, which is `V0203`. Shadowing an outer scope is allowed and
    /// returns `None`.
    pub fn declare(&mut self, name: &str, symbol: Symbol, span: Span) -> Option<Span> {
        let scope = self.stack.last_mut().expect("the core scope always exists");
        let previous = scope.get(name).and_then(|e| e.span);
        if previous.is_some() {
            return previous;
        }
        scope.insert(
            name.to_owned(),
            Entry {
                symbol,
                span: Some(span),
            },
        );
        None
    }

    /// Look a name up, innermost scope first.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<Entry> {
        self.stack.iter().rev().find_map(|s| s.get(name).copied())
    }

    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }

    /// Every visible name, sorted. This is what a `V0201` diagnostic reports,
    /// so the author can pick a real name instead of guessing again.
    #[must_use]
    pub fn visible(&self) -> Vec<String> {
        let mut names: Vec<String> = self.stack.iter().flat_map(|s| s.keys().cloned()).collect();
        names.sort();
        names.dedup();
        names
    }

    /// Visible names that look like a typo of `name`, closest first.
    #[must_use]
    pub fn suggestions(&self, name: &str) -> Vec<String> {
        // One edit per three characters, so short names do not match everything.
        let budget = (name.len() / 3).max(1);
        let mut scored: Vec<(usize, String)> = self
            .visible()
            .into_iter()
            .filter_map(|candidate| {
                let d = edit_distance(name, &candidate);
                (d <= budget).then_some((d, candidate))
            })
            .collect();
        scored.sort();
        scored.into_iter().map(|(_, n)| n).collect()
    }
}

/// Levenshtein distance.
#[must_use]
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }

    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];

    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use vise_diag::FileId;

    fn span() -> Span {
        Span::new(FileId(0), 0, 1)
    }

    #[test]
    fn core_is_visible_without_an_import() {
        let s = Scopes::new();
        assert!(s.contains("Result"));
        assert!(s.contains("print"));
        assert!(!s.contains("post"));
    }

    #[test]
    fn an_inner_scope_shadows_an_outer_one() {
        let mut s = Scopes::new();
        s.declare("x", Symbol::Value, span());
        s.push();
        assert!(s.declare("x", Symbol::Value, span()).is_none());
        s.pop();
        assert!(s.contains("x"));
    }

    #[test]
    fn redeclaring_in_one_scope_reports_the_first_declaration() {
        let mut s = Scopes::new();
        let first = Span::new(FileId(0), 10, 12);
        assert!(s.declare("x", Symbol::Value, first).is_none());
        assert_eq!(s.declare("x", Symbol::Value, span()), Some(first));
    }

    #[test]
    fn popping_never_removes_core() {
        let mut s = Scopes::new();
        for _ in 0..5 {
            s.pop();
        }
        assert!(s.contains("Result"));
    }

    #[test]
    fn a_popped_scope_takes_its_names_with_it() {
        let mut s = Scopes::new();
        s.push();
        s.declare("local", Symbol::Value, span());
        assert!(s.contains("local"));
        s.pop();
        assert!(!s.contains("local"));
    }

    #[test]
    fn visible_names_are_sorted_and_deduplicated() {
        let mut s = Scopes::new();
        s.declare("beta", Symbol::Value, span());
        s.push();
        s.declare("alpha", Symbol::Value, span());
        let v = s.visible();
        let mut sorted = v.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(v, sorted);
        assert!(v.contains(&"alpha".to_owned()) && v.contains(&"beta".to_owned()));
    }

    #[test]
    fn suggestions_find_a_near_miss_and_reject_a_stranger() {
        let mut s = Scopes::new();
        s.declare("charge_user", Symbol::Value, span());
        assert_eq!(
            s.suggestions("charge_usr").first().map(String::as_str),
            Some("charge_user")
        );
        assert!(s.suggestions("wibble").is_empty());
    }

    #[test]
    fn short_names_do_not_match_everything() {
        // With a fixed budget of 2, `Ok` would suggest `Err`, `Some`, and more.
        let s = Scopes::new();
        assert!(!s.suggestions("Qk").contains(&"Err".to_owned()));
    }

    #[test]
    fn edit_distance_is_symmetric_and_zero_on_equality() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("sitting", "kitten"), 3);
    }
}
