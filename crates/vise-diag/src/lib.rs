//! Diagnostics, spans, and source management shared by every compiler stage.

pub mod apply;
pub mod code;
pub mod diagnostic;
pub mod json;
pub mod render;
pub mod span;

pub use apply::{Applied, apply};
pub use code::{Code, UnknownCode};
pub use diagnostic::{Confidence, Diagnostic, Fix, FixKind, Label, Severity};
pub use span::{FileId, LineCol, SourceFile, SourceMap, Span};
