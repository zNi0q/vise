//! Semantic checks: name resolution, and later types, effects, and ownership.

pub mod borrow;
pub mod effects;
pub mod exhaustive;
pub mod infer;
pub mod prelude;
pub mod resolve;
pub mod results;
pub mod scope;
pub mod types;

pub use borrow::check as check_borrows;
pub use effects::check as check_effects;
pub use exhaustive::check as check_exhaustive;
pub use infer::{TypeMap, check as check_types, check_with_types};
pub use prelude::Symbol;
pub use resolve::{MAX_MODULE_LINES, check_module_length, resolve};
pub use results::check as check_results;
pub use scope::{Entry, Scopes, edit_distance};
pub use types::{Mismatch, Table, Ty};
