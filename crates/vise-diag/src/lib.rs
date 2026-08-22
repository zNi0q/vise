//! Diagnostics, spans, and source management shared by every compiler stage.

pub mod code;
pub mod span;

pub use code::{Code, UnknownCode};
pub use span::{FileId, LineCol, SourceFile, SourceMap, Span};
