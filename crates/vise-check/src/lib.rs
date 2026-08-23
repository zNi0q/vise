//! Semantic checks: name resolution, and later types, effects, and ownership.

pub mod effects;
pub mod prelude;
pub mod resolve;
pub mod scope;

pub use effects::check as check_effects;
pub use prelude::Symbol;
pub use resolve::{MAX_MODULE_LINES, check_module_length, resolve};
pub use scope::{Entry, Scopes, edit_distance};
