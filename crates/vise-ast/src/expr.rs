//! Expressions, statements, and patterns.

use vise_diag::Span;

use crate::{Ident, Type};

// --- literals -----------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    Int(i64),
    /// Held as source text. Spec §11 requires reproducible floats, so the
    /// decimal form is preserved rather than round-tripped through a parse.
    Float(String),
    /// Interpolation is not split here; see [`StrPart`].
    Str(Vec<StrPart>),
    Char(char),
    Bool(bool),
}

/// One piece of a string literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrPart {
    Text(String),
    /// `{name}` — the expression between the braces.
    Interpolation(Box<Expr>),
}

// --- operators ----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::And => "&&",
            Self::Or => "||",
        }
    }

    /// Binding power. Higher binds tighter.
    ///
    /// Comparisons all share one level and are non-associative, so `a < b < c`
    /// is a parse error rather than something that quietly means `(a < b) < c`.
    #[must_use]
    pub const fn precedence(self) -> u8 {
        match self {
            Self::Or => 1,
            Self::And => 2,
            Self::Eq | Self::Ne | Self::Lt | Self::Le | Self::Gt | Self::Ge => 3,
            Self::Add | Self::Sub => 4,
            Self::Mul | Self::Div | Self::Rem => 5,
        }
    }

    /// Comparisons do not chain.
    #[must_use]
    pub const fn is_comparison(self) -> bool {
        matches!(
            self,
            Self::Eq | Self::Ne | Self::Lt | Self::Le | Self::Gt | Self::Ge
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// `-x`
    Neg,
    /// `!x`
    Not,
}

impl UnOp {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Neg => "-",
            Self::Not => "!",
        }
    }
}

// --- expressions --------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    #[must_use]
    pub const fn new(kind: ExprKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    Literal(Literal),
    /// A bare name, or `Type::Variant`.
    Path(Path),
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    MethodCall {
        receiver: Box<Expr>,
        method: Ident,
        args: Vec<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: Ident,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Unary {
        op: UnOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// `&x` or `&mut x`.
    Borrow {
        is_mut: bool,
        operand: Box<Expr>,
    },
    /// `expr?` — the only implicit control flow in the language (§8).
    Try(Box<Expr>),
    /// `if c { a } else { b }`. Both branches are required (§6), so `otherwise`
    /// is not optional.
    If {
        cond: Box<Expr>,
        then: Block,
        otherwise: Box<Expr>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    Block(Block),
    /// `Receipt { id: user, amount: quote }`
    RecordLit {
        name: Ident,
        fields: Vec<FieldInit>,
    },
    /// `["ada", "alan"]`
    ListLit(Vec<Expr>),
    Return(Option<Box<Expr>>),
    Break,
    Continue,
}

/// A dotted or `::`-separated name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    pub segments: Vec<Ident>,
    pub span: Span,
}

impl Path {
    /// The name, when the path is a single segment.
    #[must_use]
    pub fn as_single(&self) -> Option<&Ident> {
        match self.segments.as_slice() {
            [only] => Some(only),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInit {
    pub name: Ident,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

// --- patterns -----------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternKind {
    /// `_`
    Wildcard,
    /// `receipt` — binds the whole value.
    Binding(Ident),
    Literal(Literal),
    /// `Ok(receipt)`, `Err(CardDeclined(why))`, `InsufficientFunds`.
    ///
    /// Sub-patterns are positional even though variant fields are declared with
    /// names, matching the spec's own example.
    Variant {
        path: Path,
        fields: Vec<Pattern>,
    },
}

// --- statements ---------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// The final expression, which is the block's value (§5).
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StmtKind {
    /// `let x = e`, `var x = e`, `let x: T = e`, `let _ = e`.
    Let {
        /// `var` rather than `let`.
        is_var: bool,
        name: Binding,
        ty: Option<Type>,
        value: Expr,
    },
    /// `x = e` or `xs[i] = e`. Only a `var` binding is a legal target, which
    /// the checker enforces.
    Assign {
        target: Expr,
        value: Expr,
    },
    /// `for x in xs { .. }`
    For {
        binding: Binding,
        iter: Expr,
        body: Block,
    },
    /// `while cond { .. }`
    While {
        cond: Expr,
        body: Block,
    },
    Expr(Expr),
}

/// The name a `let` or `for` introduces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    Name(Ident),
    /// `let _ = ...` — a deliberate discard (§8).
    Wildcard(Span),
}

impl Binding {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Name(ident) => ident.span,
            Self::Wildcard(span) => *span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_orders_arithmetic_above_comparison_above_logic() {
        assert!(BinOp::Mul.precedence() > BinOp::Add.precedence());
        assert!(BinOp::Add.precedence() > BinOp::Lt.precedence());
        assert!(BinOp::Lt.precedence() > BinOp::And.precedence());
        assert!(BinOp::And.precedence() > BinOp::Or.precedence());
    }

    #[test]
    fn every_comparison_shares_one_precedence_level() {
        // They are non-associative, so `a < b < c` must be a parse error rather
        // than something that quietly means `(a < b) < c`.
        let levels: Vec<_> = [
            BinOp::Eq,
            BinOp::Ne,
            BinOp::Lt,
            BinOp::Le,
            BinOp::Gt,
            BinOp::Ge,
        ]
        .iter()
        .map(|op| op.precedence())
        .collect();
        assert!(levels.windows(2).all(|w| w[0] == w[1]), "{levels:?}");
        assert!([BinOp::Eq, BinOp::Lt].iter().all(|op| op.is_comparison()));
        assert!(![BinOp::Add, BinOp::And].iter().any(|op| op.is_comparison()));
    }

    #[test]
    fn operator_text_matches_the_lexer() {
        assert_eq!(BinOp::Ne.as_str(), "!=");
        assert_eq!(BinOp::And.as_str(), "&&");
        assert_eq!(UnOp::Neg.as_str(), "-");
    }
}
