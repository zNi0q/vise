//! The Vise abstract syntax tree.
//!
//! Every node is a `kind` plus a `span`, so a span is always available without
//! a variant-by-variant match. Nothing here is resolved or checked: the AST
//! records what the author wrote, including what is wrong with it, so later
//! stages can report against real source.

pub mod decl;
pub mod expr;
pub mod ty;

pub use decl::{
    EnumDecl, Field, FnDecl, GenericParam, Item, ItemKind, Module, Param, RecordDecl, TypeDecl,
    Use, UsePath, Variant,
};
pub use expr::{
    BinOp, Binding, Block, Expr, ExprKind, FieldInit, Literal, MatchArm, Path, Pattern,
    PatternKind, Stmt, StmtKind, StrPart, UnOp,
};
pub use ty::{Effect, EffectRow, Type, TypeKind};

use vise_diag::Span;

/// A name as written, with where it was written.
///
/// Text is owned rather than interned. Interning is worth doing once modules
/// are large enough to measure, and not before.
///
/// Deliberately not `Hash`: two occurrences of one name have different spans,
/// so an `Ident`-keyed map would miss lookups. Key maps by the name itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

impl Ident {
    #[must_use]
    pub fn new(name: impl Into<String>, span: Span) -> Self {
        Self {
            name: name.into(),
            span,
        }
    }
}

impl std::fmt::Display for Ident {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}
