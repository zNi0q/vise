//! Diagnostics, spans, and source management shared by every compiler stage.

pub mod code;
pub mod diagnostic;
pub mod json;
pub mod span;

pub use code::{Code, UnknownCode};
pub use diagnostic::{Confidence, Diagnostic, Fix, FixKind, Label, Severity};
pub use span::{FileId, LineCol, SourceFile, SourceMap, Span};
