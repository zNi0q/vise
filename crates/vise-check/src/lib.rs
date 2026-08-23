//! Semantic checks: name resolution, and later types, effects, and ownership.

pub mod effects;
pub mod exhaustive;
pub mod prelude;
pub mod resolve;
pub mod results;
pub mod scope;

pub use effects::check as check_effects;
pub use exhaustive::check as check_exhaustive;
pub use prelude::Symbol;
pub use resolve::{MAX_MODULE_LINES, check_module_length, resolve};
pub use results::check as check_results;
pub use scope::{Entry, Scopes, edit_distance};
