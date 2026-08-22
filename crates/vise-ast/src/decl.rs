//! Modules, imports, and item declarations.

use vise_diag::Span;

use crate::{Block, EffectRow, Expr, Ident, Type};

/// One file. Spec §3: a file is a module, opened by `module <name>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub name: Ident,
    pub uses: Vec<Use>,
    pub items: Vec<Item>,
    pub span: Span,
}

impl Module {
    /// Find an item by name. Spec §3 forbids duplicate definitions, so at most
    /// one can match in a well-formed module.
    #[must_use]
    pub fn item(&self, name: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.name().name == name)
    }
}

/// `use std/http@1:{post, Response}`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use {
    pub path: UsePath,
    /// Every imported name, listed explicitly. Spec §3 has no glob import, so
    /// this is never empty in valid source.
    pub names: Vec<Ident>,
    pub span: Span,
}

/// The `std/http@1` part of an import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsePath {
    pub segments: Vec<Ident>,
    /// The `@1`. Absent only for `core`, which is implicit and unversioned.
    pub version: Option<u32>,
    pub span: Span,
}

impl UsePath {
    /// The path as written, without the version.
    #[must_use]
    pub fn joined(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join("/")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub kind: ItemKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    Type(TypeDecl),
    Record(RecordDecl),
    Enum(EnumDecl),
    /// Boxed: a function carries params, contracts, and a whole body, and
    /// would otherwise set the size of every `Item` in the module.
    Fn(Box<FnDecl>),
}

impl Item {
    #[must_use]
    pub const fn name(&self) -> &Ident {
        match &self.kind {
            ItemKind::Type(d) => &d.name,
            ItemKind::Record(d) => &d.name,
            ItemKind::Enum(d) => &d.name,
            ItemKind::Fn(d) => &d.name,
        }
    }

    /// Spec §3: only `pub` names leave a module.
    #[must_use]
    pub const fn is_pub(&self) -> bool {
        match &self.kind {
            ItemKind::Type(d) => d.is_pub,
            ItemKind::Record(d) => d.is_pub,
            ItemKind::Enum(d) => d.is_pub,
            ItemKind::Fn(d) => d.is_pub,
        }
    }
}

/// `type UserId = Int`
///
/// Spec §4: this creates a **distinct** type, not an alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDecl {
    pub is_pub: bool,
    pub name: Ident,
    pub underlying: Type,
    pub span: Span,
}

/// `record Receipt { id: UserId }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordDecl {
    pub is_pub: bool,
    pub name: Ident,
    pub generics: Vec<Ident>,
    pub fields: Vec<Field>,
    /// `invariant` clauses, which hold at every construction (§10).
    pub invariants: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: Ident,
    pub ty: Type,
    pub span: Span,
}

/// `enum ChargeError { InsufficientFunds  CardDeclined(reason: Str) }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDecl {
    pub is_pub: bool,
    pub name: Ident,
    pub generics: Vec<Ident>,
    pub variants: Vec<Variant>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub name: Ident,
    /// Payload fields, declared with names even though patterns bind them
    /// positionally.
    pub fields: Vec<Field>,
    pub span: Span,
}

/// A function, with its effect row and contracts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnDecl {
    pub is_pub: bool,
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    /// Absent means `Unit`.
    pub ret: Option<Type>,
    /// Absent means *inferred*, which is not the same as empty. Spec §7: an
    /// omitted row is whatever the body implies; `!{}` asserts purity.
    pub effects: Option<EffectRow>,
    pub requires: Vec<Expr>,
    pub ensures: Vec<Expr>,
    pub body: Block,
    pub span: Span,
}

impl FnDecl {
    /// Whether the author constrained the effects, rather than leaving them to
    /// inference.
    #[must_use]
    pub const fn declares_effects(&self) -> bool {
        self.effects.is_some()
    }

    #[must_use]
    pub fn has_contracts(&self) -> bool {
        !self.requires.is_empty() || !self.ensures.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenericParam {
    /// `<T>`
    Type(Ident),
    /// `<'a>`
    Lifetime(Ident),
}

impl GenericParam {
    #[must_use]
    pub const fn name(&self) -> &Ident {
        match self {
            Self::Type(i) | Self::Lifetime(i) => i,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: Ident,
    pub ty: Type,
    pub span: Span,
}
