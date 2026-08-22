//! Diagnostics, spans, and source management shared by every compiler stage.

pub mod span;

pub use span::{FileId, LineCol, SourceFile, SourceMap, Span};
