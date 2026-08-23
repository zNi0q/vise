//! Semantic checks: name resolution, and later types, effects, and ownership.

pub mod prelude;
pub mod scope;

pub use prelude::Symbol;
pub use scope::{Entry, Scopes, edit_distance};
