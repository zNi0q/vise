//! Stable diagnostic codes.
//!
//! A code is a permanent identifier. Messages may be reworded; a code never
//! changes meaning, because agent repair logic and this project's own test
//! suite key off it.
//!
//! Ranges:
//!
//! | Range   | Stage                    |
//! |---------|--------------------------|
//! | `V00xx` | lexical                  |
//! | `V01xx` | module structure, syntax |
//! | `V02xx` | name resolution          |
//! | `V03xx` | types and matching       |
//! | `V04xx` | effects                  |
//! | `V05xx` | error handling           |
//! | `V06xx` | ownership and borrowing  |

use std::fmt;
use std::str::FromStr;

macro_rules! codes {
    ($( $variant:ident = $text:literal, $title:literal, $explain:literal; )*) => {
        /// A stable diagnostic code.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[non_exhaustive]
        pub enum Code {
            $( #[doc = $title] $variant, )*
        }

        impl Code {
            /// Every code, in ascending order. Used to render `vise explain --list`
            /// and to assert uniqueness in tests.
            pub const ALL: &'static [Code] = &[ $( Code::$variant, )* ];

            /// The wire form, such as `"V0401"`.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $( Code::$variant => $text, )* }
            }

            /// A one-line summary.
            #[must_use]
            pub const fn title(self) -> &'static str {
                match self { $( Code::$variant => $title, )* }
            }

            /// Why the rule exists and how to satisfy it.
            #[must_use]
            pub const fn explain(self) -> &'static str {
                match self { $( Code::$variant => $explain, )* }
            }
        }

        impl FromStr for Code {
            type Err = UnknownCode;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $( $text => Ok(Code::$variant), )*
                    _ => Err(UnknownCode(s.to_owned())),
                }
            }
        }
    };
}

codes! {
    // --- lexical -----------------------------------------------------------
    UnknownCharacter = "V0001", "character is not part of Vise",
        "Vise source is UTF-8 but its syntax is ASCII outside of string and \
         character literals and comments. Remove the character, or move it \
         into a string.";
    UnterminatedString = "V0002", "string literal is not closed",
        "A string literal must close with `\"` on the same line. Vise has no \
         multi-line string literal; join lines with `\\n` instead.";
    InvalidEscape = "V0003", "unrecognised escape sequence",
        "Vise supports `\\n`, `\\t`, `\\\\`, `\\\"`, `\\{`, and `\\u{...}`. Any \
         other backslash sequence is an error rather than a literal backslash, \
         so that no escape silently means two different things.";
    MalformedNumber = "V0004", "numeric literal is malformed",
        "Integers are decimal digits with optional `_` separators. Floats need \
         digits on both sides of the point: write `0.5`, not `.5`. There is no \
         octal, hex, or binary literal syntax in v0.";
    UnterminatedInterpolation = "V0005", "string interpolation is not closed",
        "An interpolation opened with `{` must close with `}` inside the same \
         string literal. To write a literal brace, escape it as `\\{`.";
    NonSnakeCase = "V0006", "value name is not snake_case",
        "Values are `[a-z_][a-z0-9_]*`. A camelCase name would otherwise lex as \
         two tokens and fail somewhere confusing, so it is rejected where it is \
         written. Rename it, or capitalise the first letter if a type was meant.";
    NonPascalCase = "V0007", "type name is not PascalCase",
        "Types are `[A-Z][A-Za-z0-9]*`, with no underscores. Casing is how the \
         lexer tells a value from a type, so it is a rule rather than a style \
         preference.";
    UnterminatedChar = "V0008", "character literal is not closed",
        "A character literal holds exactly one character and closes with `'` on \
         the same line. `'a` with no closing quote is read as a lifetime.";

    // --- module structure and syntax ---------------------------------------
    ModuleTooLong = "V0101", "module exceeds the 500-line cap",
        "A module must fit in an agent's working context so it can be edited \
         correctly without reading the rest of the program. Split it: the cap \
         is a design rule, not a tunable.";
    UnexpectedToken = "V0102", "unexpected token",
        "The parser found a token that cannot appear here. The diagnostic \
         lists what would have been valid at this position.";
    MissingModuleHeader = "V0103", "file does not begin with `module`",
        "Every file is a module and must open with `module <name>`, so that a \
         file's identity never depends on its path.";

    // --- name resolution ---------------------------------------------------
    UnknownName = "V0201", "name is not in scope",
        "Vise has a closed namespace: every name must be defined in this module \
         or listed in a `use`. There is no glob import and no transitive \
         visibility. This diagnostic lists the names that *are* in scope, which \
         is what turns a hallucinated API into a compile error.";
    NotExported = "V0202", "name exists but is not `pub`",
        "Only `pub` names leave a module. Mark the definition `pub`, or use a \
         different entry point.";
    DuplicateDefinition = "V0203", "name is defined twice",
        "One name resolves to exactly one definition. Vise has no overloading, \
         so two definitions with the same name are always an error.";

    // --- types and matching ------------------------------------------------
    NonExhaustiveMatch = "V0301", "match does not cover every case",
        "A `match` must handle every constructor. Add the missing arms named in \
         this diagnostic, or add `_` if a catch-all is genuinely intended. \
         Forgetting a case is what this rule prevents; writing `_` on purpose \
         is allowed.";
    TypeMismatch = "V0302", "types do not match",
        "Vise has no implicit conversion. A `type` declaration creates a \
         distinct type, so a `UserId` is not an `Int` even though it is \
         represented as one; convert explicitly.";
    UnknownField = "V0303", "record has no such field",
        "The diagnostic lists the fields the record does declare.";

    // --- effects -----------------------------------------------------------
    UndeclaredEffect = "V0401", "call introduces an effect the signature does not declare",
        "A declared effect row is exact. This diagnostic names the call that \
         introduced the extra effect. Either widen the row — `vise fix` will \
         write it — or stop performing the effect.";
    UnusedDeclaredEffect = "V0402", "declared effect is never used",
        "An effect row is exact rather than an upper bound, so a row that \
         overstates what a function does is an error. This keeps rows honest as \
         code changes instead of decaying into noise.";

    // --- error handling ----------------------------------------------------
    UnusedResult = "V0501", "`Result` is ignored",
        "Vise has no exceptions, so an ignored `Result` is a silently dropped \
         failure. Handle it, propagate it with `?`, or discard it deliberately \
         with `let _ = ...`.";

    // --- ownership ---------------------------------------------------------
    UseAfterMove = "V0601", "value used after it was moved",
        "Assignment and argument passing move by default. This diagnostic names \
         the line the move happened on. Borrow with `&` if the callee only needs \
         to read, or `.clone()` if a second owned copy is genuinely wanted.";
    ConflictingBorrows = "V0602", "borrows conflict",
        "Borrows are shared-xor-mutable: `&T` may be held many times, `&mut T` \
         may not coexist with any other borrow of the same value. Shorten one \
         borrow's scope, or sequence the two uses.";
    BorrowOutlivesOwner = "V0603", "borrow outlives the value it points to",
        "A borrow may not escape the scope that owns the value. Return an owned \
         value instead, or accept the borrow as a parameter so the caller owns \
         it.";
    AmbiguousLifetime = "V0604", "lifetime relationship is ambiguous",
        "Two or more input borrows could be the source of a borrowed output, so \
         elision cannot pick one. Name the lifetime explicitly; `vise fix` will \
         write the annotation.";
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Returned by [`Code::from_str`] when the text is not a known code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownCode(pub String);

impl fmt::Display for UnknownCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown diagnostic code `{}`", self.0)
    }
}

impl std::error::Error for UnknownCode {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn codes_are_unique() {
        let seen: BTreeSet<_> = Code::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(seen.len(), Code::ALL.len(), "duplicate code text");
    }

    #[test]
    fn codes_round_trip_through_text() {
        for &c in Code::ALL {
            assert_eq!(c.as_str().parse::<Code>(), Ok(c));
        }
    }

    #[test]
    fn unknown_text_does_not_parse() {
        assert!("V9999".parse::<Code>().is_err());
        assert!("".parse::<Code>().is_err());
    }

    #[test]
    fn every_code_is_well_formed() {
        for &c in Code::ALL {
            let s = c.as_str();
            assert_eq!(s.len(), 5, "{s} should be V then four digits");
            assert!(s.starts_with('V'), "{s} should start with V");
            assert!(s[1..].chars().all(|ch| ch.is_ascii_digit()), "{s}");
        }
    }

    #[test]
    fn every_code_documents_itself() {
        for &c in Code::ALL {
            assert!(!c.title().is_empty(), "{c} has no title");
            assert!(c.explain().len() > 40, "{c} needs a real explanation");
        }
    }

    #[test]
    fn codes_are_listed_in_ascending_order() {
        let texts: Vec<_> = Code::ALL.iter().map(|c| c.as_str()).collect();
        let mut sorted = texts.clone();
        sorted.sort_unstable();
        assert_eq!(texts, sorted);
    }

    #[test]
    fn spec_codes_are_present() {
        // Codes named directly in spec/spec.md must never disappear.
        for s in [
            "V0101", "V0201", "V0301", "V0401", "V0501", "V0601", "V0602", "V0603", "V0604",
        ] {
            assert!(s.parse::<Code>().is_ok(), "{s} is referenced by the spec");
        }
    }
}
