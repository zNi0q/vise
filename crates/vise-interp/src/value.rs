//! Runtime values and traps.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// A runtime value.
///
/// Values are immutable and structurally shared, so cloning one is a refcount
/// bump. That matches §9: copies are cheap because nothing is mutated in place.
///
/// `Arc` rather than `Rc` because the evaluator runs on its own thread with a
/// large stack, so a runaway program traps instead of aborting the process.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    Str(Arc<str>),
    Unit,
    List(Arc<Vec<Value>>),
    /// Map iteration order is specified (§11), so the ordered map is the
    /// representation rather than an implementation detail.
    Map(Arc<BTreeMap<String, Value>>),
    Record {
        name: Arc<str>,
        fields: Arc<BTreeMap<String, Value>>,
    },
    Variant {
        name: Arc<str>,
        fields: Arc<Vec<Value>>,
    },
}

impl Value {
    #[must_use]
    pub fn str(s: impl AsRef<str>) -> Self {
        Self::Str(Arc::from(s.as_ref()))
    }

    #[must_use]
    pub fn variant(name: &str, fields: Vec<Value>) -> Self {
        Self::Variant {
            name: Arc::from(name),
            fields: Arc::new(fields),
        }
    }

    /// The name shown in a trap message.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Int(_) => "Int",
            Self::Float(_) => "Float",
            Self::Bool(_) => "Bool",
            Self::Char(_) => "Char",
            Self::Str(_) => "Str",
            Self::Unit => "Unit",
            Self::List(_) => "List",
            Self::Map(_) => "Map",
            Self::Record { .. } => "a record",
            Self::Variant { .. } => "a variant",
        }
    }

    /// Whether this is `Err(..)`, which `?` propagates.
    #[must_use]
    pub fn is_err(&self) -> bool {
        matches!(self, Self::Variant { name, .. } if &**name == "Err")
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            // Always with a decimal point, so a Float never reads as an Int.
            Self::Float(v) if v.fract() == 0.0 && v.is_finite() => write!(f, "{v:.1}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Bool(v) => write!(f, "{v}"),
            Self::Char(v) => write!(f, "{v}"),
            Self::Str(v) => f.write_str(v),
            Self::Unit => f.write_str("Unit"),
            Self::List(items) => {
                f.write_str("[")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{v}")?;
                }
                f.write_str("]")
            }
            Self::Map(entries) => {
                f.write_str("{")?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                f.write_str("}")
            }
            Self::Record { name, fields } => {
                write!(f, "{name} {{")?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, " {k}: {v}")?;
                }
                f.write_str(" }")
            }
            Self::Variant { name, fields } if fields.is_empty() => f.write_str(name),
            Self::Variant { name, fields } => {
                write!(f, "{name}(")?;
                for (i, v) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{v}")?;
                }
                f.write_str(")")
            }
        }
    }
}

/// A runtime failure. Vise has no exceptions, so a trap stops the program
/// rather than being catchable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trap {
    /// §4: integer arithmetic traps, it never wraps.
    Overflow(&'static str),
    DivideByZero,
    IndexOutOfBounds {
        index: i64,
        len: usize,
    },
    /// §10: a `requires` clause did not hold.
    Requires {
        function: String,
    },
    /// §10: an `ensures` clause did not hold.
    Ensures {
        function: String,
    },
    /// §6 guarantees exhaustiveness statically; reaching this means the checker
    /// was bypassed.
    NoMatchingArm,
    /// A construct the interpreter does not implement. Named rather than
    /// silently producing a wrong answer.
    Unsupported(String),
}

impl fmt::Display for Trap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overflow(op) => write!(f, "integer overflow in `{op}`"),
            Self::DivideByZero => f.write_str("division by zero"),
            Self::IndexOutOfBounds { index, len } => {
                write!(f, "index {index} is out of bounds for a list of {len}")
            }
            Self::Requires { function } => {
                write!(f, "a `requires` clause of `{function}` does not hold")
            }
            Self::Ensures { function } => {
                write!(f, "an `ensures` clause of `{function}` does not hold")
            }
            Self::NoMatchingArm => f.write_str("no match arm applied"),
            Self::Unsupported(what) => write!(f, "unsupported: {what}"),
        }
    }
}

impl std::error::Error for Trap {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_float_never_prints_as_an_integer() {
        assert_eq!(Value::Float(2.0).to_string(), "2.0");
        assert_eq!(Value::Float(2.5).to_string(), "2.5");
        assert_eq!(Value::Int(2).to_string(), "2");
    }

    #[test]
    fn a_nullary_variant_prints_without_parentheses() {
        assert_eq!(Value::variant("None", vec![]).to_string(), "None");
        assert_eq!(
            Value::variant("Ok", vec![Value::Int(1)]).to_string(),
            "Ok(1)"
        );
    }

    #[test]
    fn only_err_propagates() {
        assert!(Value::variant("Err", vec![Value::Unit]).is_err());
        assert!(!Value::variant("Ok", vec![Value::Unit]).is_err());
        assert!(!Value::Int(1).is_err());
    }

    #[test]
    fn a_list_prints_its_elements() {
        let v = Value::List(Arc::new(vec![Value::Int(1), Value::str("a")]));
        assert_eq!(v.to_string(), "[1, a]");
    }

    #[test]
    fn traps_describe_themselves() {
        assert_eq!(Trap::DivideByZero.to_string(), "division by zero");
        assert_eq!(Trap::Overflow("+").to_string(), "integer overflow in `+`");
        assert_eq!(
            Trap::IndexOutOfBounds { index: 5, len: 2 }.to_string(),
            "index 5 is out of bounds for a list of 2"
        );
    }
}
