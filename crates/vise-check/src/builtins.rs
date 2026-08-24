//! What `core` provides.
//!
//! Spec issue 5 was that `core` had never been enumerated anywhere. This table
//! is that enumeration, and it is deliberately the *only* one: the name
//! resolver, the type checker, the effect checker, and the interpreter all read
//! it, so none of them can drift from the others.
//!
//! Every entry earns its place by being impossible to write in Vise itself.
//! Anything expressible in the language belongs in a library written in it.

use vise_ast::Effect;

use crate::types::Ty;

/// One `core` function.
#[derive(Debug, Clone)]
pub struct Builtin {
    pub name: &'static str,
    /// Type parameters, instantiated fresh at each call site.
    pub generics: Vec<String>,
    pub params: Vec<Ty>,
    pub ret: Ty,
    /// What it does to the outside world (§7). `None` is pure.
    pub effect: Option<Effect>,
    /// One line, for diagnostics that list what exists.
    pub doc: &'static str,
}

fn t(name: &str) -> Ty {
    Ty::con(name)
}

fn list(inner: Ty) -> Ty {
    Ty::app("List", vec![inner])
}

fn result(ok: Ty, err: Ty) -> Ty {
    Ty::app("Result", vec![ok, err])
}

fn option(inner: Ty) -> Ty {
    Ty::app("Option", vec![inner])
}

fn f(
    name: &'static str,
    generics: &[&str],
    params: Vec<Ty>,
    ret: Ty,
    effect: Option<Effect>,
    doc: &'static str,
) -> Builtin {
    Builtin {
        name,
        generics: generics.iter().map(|g| (*g).to_owned()).collect(),
        params,
        ret,
        effect,
        doc,
    }
}

/// Every name `core` defines.
#[must_use]
pub fn all() -> Vec<Builtin> {
    let s = || t("Str");
    let i = || t("Int");

    vec![
        // --- output ------------------------------------------------------
        f(
            "print",
            &[],
            vec![s()],
            t("Unit"),
            Some(Effect::Io),
            "write a line to standard output",
        ),
        // --- lists -------------------------------------------------------
        f(
            "length",
            &["T"],
            vec![list(t("T"))],
            i(),
            None,
            "how many elements a list holds",
        ),
        f(
            "append",
            &["T"],
            vec![list(t("T")), t("T")],
            list(t("T")),
            None,
            "a new list with one more element on the end",
        ),
        f(
            "at",
            &["T"],
            vec![list(t("T")), i()],
            option(t("T")),
            None,
            "the element at an index, or None when out of range",
        ),
        // --- strings -----------------------------------------------------
        f(
            "str_length",
            &[],
            vec![s()],
            i(),
            None,
            "how many characters a string holds",
        ),
        f(
            "lines",
            &[],
            vec![s()],
            list(s()),
            None,
            "split on line breaks, dropping a trailing empty line",
        ),
        f(
            "split",
            &[],
            vec![s(), s()],
            list(s()),
            None,
            "split on every occurrence of a separator",
        ),
        f(
            "join",
            &[],
            vec![list(s()), s()],
            s(),
            None,
            "join strings with a separator between them",
        ),
        f(
            "starts_with",
            &[],
            vec![s(), s()],
            t("Bool"),
            None,
            "whether a string begins with another",
        ),
        f(
            "contains",
            &[],
            vec![s(), s()],
            t("Bool"),
            None,
            "whether a string contains another",
        ),
        f(
            "parse_int",
            &[],
            vec![s()],
            option(i()),
            None,
            "read an integer from a string, or None if it is not one",
        ),
        // --- filesystem --------------------------------------------------
        f(
            "read_file",
            &[],
            vec![s()],
            result(s(), s()),
            Some(Effect::Fs),
            "the whole contents of a file",
        ),
        f(
            "write_file",
            &[],
            vec![s(), s()],
            result(t("Unit"), s()),
            Some(Effect::Fs),
            "replace a file's contents",
        ),
        f(
            "list_dir",
            &[],
            vec![s()],
            result(list(s()), s()),
            Some(Effect::Fs),
            "the names directly inside a directory, sorted",
        ),
        f(
            "is_dir",
            &[],
            vec![s()],
            t("Bool"),
            Some(Effect::Fs),
            "whether a path is a directory; a link to one is not",
        ),
        f(
            "file_size",
            &[],
            vec![s()],
            result(i(), s()),
            Some(Effect::Fs),
            "a file's size in bytes",
        ),
        // --- process -----------------------------------------------------
        f(
            "args",
            &[],
            vec![],
            list(s()),
            Some(Effect::Env),
            "the arguments the program was started with, excluding its own name",
        ),
        f(
            "now",
            &[],
            vec![],
            i(),
            Some(Effect::Time),
            "seconds since the Unix epoch",
        ),
        f(
            "exit",
            &[],
            vec![i()],
            t("Unit"),
            Some(Effect::Proc),
            "stop the program with a status code",
        ),
    ]
}

/// Look one up by name.
#[must_use]
pub fn find(name: &str) -> Option<Builtin> {
    all().into_iter().find(|b| b.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn names_are_unique() {
        let names: BTreeSet<_> = all().iter().map(|b| b.name).collect();
        assert_eq!(names.len(), all().len());
    }

    #[test]
    fn every_builtin_documents_itself() {
        for b in all() {
            assert!(b.doc.len() > 10, "{} needs a real doc", b.name);
        }
    }

    #[test]
    fn effects_match_what_the_function_touches() {
        // §7: effects are primitive capabilities, so a reader should be able to
        // predict these from the name alone.
        for b in all() {
            let expected = match b.name {
                "print" => Some(Effect::Io),
                "read_file" | "write_file" | "list_dir" | "is_dir" | "file_size" => {
                    Some(Effect::Fs)
                }
                "args" => Some(Effect::Env),
                "now" => Some(Effect::Time),
                "exit" => Some(Effect::Proc),
                _ => None,
            };
            assert_eq!(b.effect, expected, "{}", b.name);
        }
    }

    #[test]
    fn generic_parameters_appear_in_the_signature() {
        for b in all() {
            for g in &b.generics {
                let used = b
                    .params
                    .iter()
                    .chain(std::iter::once(&b.ret))
                    .any(|ty| format!("{ty}").contains(g.as_str()));
                assert!(used, "{} declares {g} but never uses it", b.name);
            }
        }
    }

    #[test]
    fn anything_fallible_returns_a_result() {
        // §8: no exceptions, so a builtin that can fail says so in its type.
        for b in all() {
            if matches!(
                b.name,
                "read_file" | "write_file" | "list_dir" | "file_size"
            ) {
                assert!(
                    format!("{}", b.ret).starts_with("Result<"),
                    "{} can fail and must return a Result",
                    b.name
                );
            }
        }
    }
}
