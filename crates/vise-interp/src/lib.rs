//! A tree-walking interpreter for Vise.
//!
//! Throwaway by design: it exists so the M3 benchmark can measure whether a
//! program *works*, not merely whether it compiles, before a real backend
//! exists.

pub mod eval;
pub mod value;

pub use eval::{Run, call, run};
pub use value::{Trap, Value};
