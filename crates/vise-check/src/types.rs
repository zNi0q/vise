//! Types and unification.
//!
//! Vise's type system is nominal and first-order: a type is a constructor
//! applied to arguments, a borrow, or an inference variable. There are no
//! traits and no overloading (§4), so solving is plain equality — no constraint
//! sets, no coherence, no ambiguity.
//!
//! `type UserId = Int` produces `Con("UserId", [])`, which is a *different*
//! type from `Con("Int", [])`. Distinctness falls out of nominal equality
//! rather than needing a rule of its own, which is what makes swapped
//! arguments a type error.

use std::fmt;

/// A type, possibly containing inference variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// A named type applied to arguments: `Int`, `List<T>`, `Result<T, E>`,
    /// `UserId`, `Receipt`.
    Con(String, Vec<Ty>),
    /// `&T` or `&mut T`.
    Ref { is_mut: bool, inner: Box<Ty> },
    /// An inference variable.
    Var(u32),
    /// A type that has already been reported. It unifies with everything, so
    /// one mistake produces one diagnostic instead of a cascade.
    Error,
}

impl Ty {
    #[must_use]
    pub fn con(name: &str) -> Self {
        Self::Con(name.to_owned(), Vec::new())
    }

    #[must_use]
    pub fn app(name: &str, args: Vec<Ty>) -> Self {
        Self::Con(name.to_owned(), args)
    }

    #[must_use]
    pub fn unit() -> Self {
        Self::con("Unit")
    }

    #[must_use]
    pub fn borrow(inner: Ty, is_mut: bool) -> Self {
        Self::Ref {
            is_mut,
            inner: Box::new(inner),
        }
    }

    /// Whether this type mentions `var`, directly or nested.
    #[must_use]
    pub fn occurs(&self, var: u32) -> bool {
        match self {
            Self::Var(v) => *v == var,
            Self::Con(_, args) => args.iter().any(|a| a.occurs(var)),
            Self::Ref { inner, .. } => inner.occurs(var),
            Self::Error => false,
        }
    }

    /// Replace every variable in `map`'s domain. Used to instantiate a generic
    /// signature at a call site.
    #[must_use]
    pub fn substitute(&self, map: &impl Fn(u32) -> Option<Ty>) -> Self {
        match self {
            Self::Var(v) => map(*v).unwrap_or_else(|| self.clone()),
            Self::Con(name, args) => Self::Con(
                name.clone(),
                args.iter().map(|a| a.substitute(map)).collect(),
            ),
            Self::Ref { is_mut, inner } => Self::Ref {
                is_mut: *is_mut,
                inner: Box::new(inner.substitute(map)),
            },
            Self::Error => Self::Error,
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Con(name, args) if args.is_empty() => f.write_str(name),
            Self::Con(name, args) => {
                write!(f, "{name}<")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{a}")?;
                }
                f.write_str(">")
            }
            Self::Ref { is_mut, inner } => {
                if *is_mut {
                    write!(f, "&mut {inner}")
                } else {
                    write!(f, "&{inner}")
                }
            }
            // Inference variables are never shown to a reader: an unsolved
            // variable in a message is noise, not information.
            Self::Var(_) => f.write_str("_"),
            Self::Error => f.write_str("<error>"),
        }
    }
}

/// Why two types could not be made equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mismatch {
    /// Different types, reported as the outermost pair that disagreed.
    Types { expected: Ty, found: Ty },
    /// A variable would have to contain itself, as in `x = [x]`.
    Infinite,
}

/// The substitution built up during inference.
#[derive(Debug, Default)]
pub struct Table {
    bindings: Vec<Option<Ty>>,
}

impl Table {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A fresh, unbound inference variable.
    pub fn fresh(&mut self) -> Ty {
        let id = u32::try_from(self.bindings.len()).expect("too many type variables");
        self.bindings.push(None);
        Ty::Var(id)
    }

    #[must_use]
    pub fn var_count(&self) -> usize {
        self.bindings.len()
    }

    /// Follow bindings until the head is not a bound variable.
    #[must_use]
    pub fn shallow(&self, ty: &Ty) -> Ty {
        let mut current = ty.clone();
        while let Ty::Var(v) = current {
            match self.bindings.get(v as usize).and_then(Option::as_ref) {
                Some(bound) => current = bound.clone(),
                None => break,
            }
        }
        current
    }

    /// Apply the substitution everywhere, so the result is fit to display.
    #[must_use]
    pub fn resolve(&self, ty: &Ty) -> Ty {
        match self.shallow(ty) {
            Ty::Con(name, args) => Ty::Con(name, args.iter().map(|a| self.resolve(a)).collect()),
            Ty::Ref { is_mut, inner } => Ty::Ref {
                is_mut,
                inner: Box::new(self.resolve(&inner)),
            },
            other => other,
        }
    }

    /// Make two types equal, or say why that is impossible.
    ///
    /// # Errors
    /// Returns the outermost disagreement, or [`Mismatch::Infinite`] when the
    /// occurs check fails.
    pub fn unify(&mut self, expected: &Ty, found: &Ty) -> Result<(), Mismatch> {
        let a = self.shallow(expected);
        let b = self.shallow(found);

        match (&a, &b) {
            // Poison absorbs everything, so one error stays one error.
            (Ty::Error, _) | (_, Ty::Error) => Ok(()),

            (Ty::Var(x), Ty::Var(y)) if x == y => Ok(()),
            (Ty::Var(x), other) | (other, Ty::Var(x)) => {
                if other.occurs(*x) {
                    return Err(Mismatch::Infinite);
                }
                self.bindings[*x as usize] = Some(other.clone());
                Ok(())
            }

            (Ty::Con(n1, a1), Ty::Con(n2, a2)) => {
                // Nominal: `UserId` and `Int` disagree even though one is
                // declared in terms of the other.
                if n1 != n2 || a1.len() != a2.len() {
                    return Err(Mismatch::Types {
                        expected: self.resolve(&a),
                        found: self.resolve(&b),
                    });
                }
                for (x, y) in a1.iter().zip(a2) {
                    self.unify(x, y)?;
                }
                Ok(())
            }

            (
                Ty::Ref {
                    is_mut: m1,
                    inner: i1,
                },
                Ty::Ref {
                    is_mut: m2,
                    inner: i2,
                },
            ) => {
                if m1 != m2 {
                    return Err(Mismatch::Types {
                        expected: self.resolve(&a),
                        found: self.resolve(&b),
                    });
                }
                self.unify(i1, i2)
            }

            _ => Err(Mismatch::Types {
                expected: self.resolve(&a),
                found: self.resolve(&b),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int() -> Ty {
        Ty::con("Int")
    }

    #[test]
    fn identical_types_unify() {
        let mut t = Table::new();
        assert!(t.unify(&int(), &int()).is_ok());
    }

    #[test]
    fn a_variable_binds_to_a_concrete_type() {
        let mut t = Table::new();
        let v = t.fresh();
        assert!(t.unify(&v, &int()).is_ok());
        assert_eq!(t.resolve(&v), int());
    }

    #[test]
    fn two_variables_unify_transitively() {
        let mut t = Table::new();
        let (a, b) = (t.fresh(), t.fresh());
        assert!(t.unify(&a, &b).is_ok());
        assert!(t.unify(&b, &int()).is_ok());
        assert_eq!(t.resolve(&a), int());
    }

    #[test]
    fn a_distinct_type_does_not_unify_with_its_representation() {
        // §4: `type UserId = Int` creates a distinct type. This is what makes
        // swapped arguments a type error.
        let mut t = Table::new();
        let err = t.unify(&Ty::con("UserId"), &int()).unwrap_err();
        assert!(matches!(err, Mismatch::Types { .. }));
    }

    #[test]
    fn constructors_unify_argument_wise() {
        let mut t = Table::new();
        let v = t.fresh();
        assert!(
            t.unify(
                &Ty::app("List", vec![v.clone()]),
                &Ty::app("List", vec![int()])
            )
            .is_ok()
        );
        assert_eq!(t.resolve(&v), int());
    }

    #[test]
    fn different_arities_do_not_unify() {
        let mut t = Table::new();
        assert!(
            t.unify(
                &Ty::app("Map", vec![int()]),
                &Ty::app("Map", vec![int(), int()])
            )
            .is_err()
        );
    }

    #[test]
    fn borrows_unify_only_with_matching_mutability() {
        let mut t = Table::new();
        assert!(
            t.unify(&Ty::borrow(int(), false), &Ty::borrow(int(), false))
                .is_ok()
        );
        assert!(
            t.unify(&Ty::borrow(int(), true), &Ty::borrow(int(), false))
                .is_err()
        );
    }

    #[test]
    fn the_occurs_check_rejects_an_infinite_type() {
        let mut t = Table::new();
        let v = t.fresh();
        let list_of_v = Ty::app("List", vec![v.clone()]);
        assert_eq!(t.unify(&v, &list_of_v), Err(Mismatch::Infinite));
    }

    #[test]
    fn error_absorbs_everything_so_one_mistake_is_one_diagnostic() {
        let mut t = Table::new();
        assert!(t.unify(&Ty::Error, &int()).is_ok());
        assert!(
            t.unify(
                &Ty::app("List", vec![Ty::Error]),
                &Ty::app("List", vec![int()])
            )
            .is_ok()
        );
    }

    #[test]
    fn a_mismatch_reports_resolved_types_not_variables() {
        let mut t = Table::new();
        let v = t.fresh();
        t.unify(&v, &Ty::con("Str")).expect("binds");
        let err = t.unify(&int(), &v).unwrap_err();
        assert_eq!(
            err,
            Mismatch::Types {
                expected: int(),
                found: Ty::con("Str")
            }
        );
    }

    #[test]
    fn substitution_instantiates_a_generic_signature() {
        // `fn first<T>(xs: List<T>) -> Option<T>` at a call site with List<Int>.
        let sig = Ty::app("Option", vec![Ty::Var(0)]);
        let instantiated = sig.substitute(&|v| (v == 0).then(|| Ty::con("Int")));
        assert_eq!(instantiated, Ty::app("Option", vec![Ty::con("Int")]));
    }

    #[test]
    fn display_hides_inference_variables() {
        assert_eq!(
            Ty::app("List", vec![Ty::con("Int")]).to_string(),
            "List<Int>"
        );
        assert_eq!(Ty::borrow(Ty::con("Str"), true).to_string(), "&mut Str");
        assert_eq!(Ty::Var(7).to_string(), "_");
        assert_eq!(
            Ty::app("Result", vec![Ty::con("Int"), Ty::con("Str")]).to_string(),
            "Result<Int, Str>"
        );
    }
}
