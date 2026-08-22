# Vise Language Specification v0.4

**Vise lets an AI agent write software you don't have to read, because the
compiler — not a human reviewer — guarantees what the code does and what it is
allowed to touch.**

Two goals in tension, both non-negotiable:

- **As easy to write as Python.** Little ceremony, inferred types, readable at
  a glance.
- **As strict as Rust.** Ownership, borrowing, no null, no exceptions,
  exhaustive matching, errors as values, and a compiler that rejects
  plausible-but-wrong code.

These are compatible because Vise separates Rust's *rules* from Rust's
*annotation burden*. The rules are kept in full: single ownership, moves,
shared-xor-mutable borrows, deterministic destruction, no garbage collector.
The annotations are inferred wherever inference is unambiguous. Nothing is
relaxed; there is simply less to type. See §9.

**The whole language is specified here.** If a construct is not in this
document, it does not exist. There is no hidden stdlib surface, no reflection,
no macro layer.

---

## 1. Feel

```
module greet

fn main() {
  let names = ["ada", "alan", "grace"]
  for n in names {
    print("hello, {n}")
  }
}
```

No type annotations. No effect annotations. No lifetimes. No boilerplate. That
is the floor of the language, and most code should look like this.

The strictness shows up when it matters:

```
module payments

use std/http@1:{post}

type UserId = Int
type Cents  = Int

enum ChargeError {
  InsufficientFunds
  CardDeclined(reason: Str)
}

pub fn charge(user: UserId, amount: Cents) -> Result<Receipt, ChargeError> !{net} {
  let quote = post("/quote", user)?
  Ok(Receipt { id: user, amount: quote })
}
```

One signature tells you: it can fail and how, it touches the network and
nothing else, and `user` cannot be passed where a `Cents` is expected.

## 2. Lexical

UTF-8 source. Values are `snake_case`, types are `PascalCase`. Comments are
`--` to end of line. The formatter is canonical and non-configurable, so
formatting is never a decision.

Literals: `42`, `1.5`, `true`, `'a'`, `"text"`. Strings interpolate with
`"hello, {name}"`.

Because a comment opens with `--`, `a --b` is a comment rather than a
subtraction. Write `a - -b`; the formatter inserts the space.

### Statement separation

**A line break ends a statement.** There is no semicolon. A break does *not*
end a statement when the statement is visibly unfinished:

- the line ends on a token that cannot end an expression — an operator, `,`,
  `=`, `->`, or an opening bracket;
- the next line opens with a token that cannot begin a statement — `.`, `)`,
  `]`, `}`, `,`, `else`, or an infix-only operator such as `+`, `*`, or `==`.
  `-`, `&`, and `!` are prefix operators too, so a line starting with one
  begins a new statement; signal continuation from the end of the line above;
- an unclosed `(` or `[` is still open.

Braces are blocks, not brackets, so line breaks inside `{ }` stay significant.
That is what separates record fields, enum variants, and statements:

```
let total = subtotal +      -- continues: the line ends on `+`
            shipping
let names = [
  "ada",                    -- continues: inside an unclosed `[`
  "alan",
]
```

A terminator is required because the alternative is ambiguity: with no
separator at all, `f` followed by `(x)` on the next line is either one call or
two statements, and §13 exists to forbid exactly that.

## 3. Modules

One file is one module, opened by `module <name>`. A module is capped at **500
lines** (`V0101`) — that cap is what lets an agent edit a module correctly
while reading only that module.

Imports are explicit and versioned. There is no glob import and no transitive
visibility:

```
use core:{Result, Ok, Err, Option, Some, None}   -- implicit; shown for clarity
use std/http@1:{post}
```

An undeclared identifier is `V0201`, and the diagnostic lists what *is* in
scope. A hallucinated API cannot reach runtime. Only `pub` names leave a
module.

## 4. Types

Primitives: `Int` (64-bit signed, traps on overflow), `Float`, `Bool`, `Char`,
`Str`, `Unit`. One integer type, one float type.

Built-ins: `List<T>`, `Map<K, V>`, `Set<T>`, `Option<T>`, `Result<T, E>`.

**There is no `null`** — absence is `Option<T>`. There is no `any`, no dynamic
type, no unchecked cast.

```
type UserId = Int          -- distinct type, not an alias

record Receipt {
  id:     UserId
  amount: Cents
}

enum ChargeError {
  InsufficientFunds
  CardDeclined(reason: Str)
}
```

`type` creates a **distinct** type: a `UserId` is not an `Int`, so swapped
arguments are a type error rather than a silent bug.

Generics are parametric — `fn first<T>(xs: List<T>) -> Option<T>`. No traits,
no overloading: one name resolves to exactly one definition.

**Types are inferred everywhere except public signatures.** Locals, closures,
and private functions need no annotations. Public functions annotate their
signature, because that signature is the module's contract.

## 5. Functions

```
fn fee(amount: Cents) -> Cents {
  amount / 50
}
```

`let` binds immutably; `var` binds mutably. There are no mutable globals.
Parameters are taken by value (moved), by shared borrow `&T`, or by unique
borrow `&mut T` — see §9.

`return` works normally. The final expression is also the return value.

No default arguments, no varargs, no operator overloading.

## 6. Control flow

`if` is an expression; both branches must yield the same type.

`match` is an expression and must be exhaustive (`V0301` names the uncovered
cases). `_` is allowed — the point is that *forgetting* a case is impossible,
not that catch-alls are banned.

```
match result {
  Ok(receipt)            -> log(receipt)
  Err(CardDeclined(why)) -> retry(why)
  Err(InsufficientFunds) -> Unit
}
```

Loops are `for x in xs` and `while cond`, with `break` and `continue`.
Recursion is unrestricted.

*(Termination proofs are not part of the language. `vise verify` can prove
loops terminate when asked; requiring it everywhere cost far more in ergonomics
than it bought in safety.)*

## 7. Effects

A function's effect row states everything it can touch outside itself:

| Effect | Grants                        |
|--------|-------------------------------|
| `io`   | stdin, stdout, stderr         |
| `fs`   | filesystem read/write         |
| `net`  | sockets                       |
| `time` | clock, sleep                  |
| `rand` | non-deterministic bytes       |
| `env`  | environment, arguments        |
| `proc` | spawning processes            |

Effects are primitive capabilities, never domains: a database client is a
library whose functions carry `!{net}`. That keeps the set small, enumerable,
and enforceable.

**Effects are inferred. You only write a row when you want to constrain one.**
An unannotated function gets whatever its body implies. Annotate a public
function and the compiler enforces the row exactly, reporting the call site
that introduced any extra effect (`V0401`). `vise fix` writes the row for you.

This is where the two goals meet: hello-world has no annotations, and a
production signature still proves its own blast radius.

The runtime enforces the row independently — the process is sandboxed to the
syscalls its effects imply. Static checking and runtime confinement agree, or
the program does not run.

## 8. Errors

No exceptions, no panics in user code, no unwinding. Fallible functions return
`Result<T, E>`. The `?` operator propagates `Err` and is the only implicit
control flow in the language.

An ignored `Result` is `V0501`. Discarding one is `let _ = ...`, deliberately.

## 9. Ownership and memory

Vise keeps Rust's ownership model without alteration. There is no garbage
collector, no runtime, and no hidden allocation.

- **Every value has exactly one owner.** Assignment and argument passing
  **move** by default. Using a value after it moved is `V0601`, and the
  diagnostic names the line the move happened on.
- **Borrows are shared-xor-mutable.** `&T` may be held many times at once;
  `&mut T` may not coexist with any other borrow. Violation is `V0602`.
- **A borrow may not outlive its owner** (`V0603`).
- **Destruction is deterministic**, at the end of the owning scope, in reverse
  declaration order. No pauses, no finalizer queue, no nondeterminism.
- **Copies are explicit.** Primitives copy implicitly; everything else requires
  `.clone()`, so an expensive copy is always visible at the call site.

```
fn total(items: &List<Item>) -> Cents {      -- borrows, does not consume
  var sum = 0
  for it in items { sum = sum + it.price }
  sum
}

fn consume(items: List<Item>) -> Receipt { ... }   -- takes ownership

fn main() {
  let items = load()
  let t = total(&items)      -- borrowed, still ours
  let r = consume(items)     -- moved; `items` is unusable past this line
}
```

**Lifetimes are inferred, not relaxed.** The checker solves lifetime
constraints across a whole function body, and signatures use elision rules
covering the common cases. Where a signature is genuinely ambiguous — two input
borrows, one borrowed output — you name it, exactly as in Rust:

```
fn longest<'a>(a: &'a Str, b: &'a Str) -> &'a Str { ... }
```

The compiler never guesses: if it cannot infer, it errors with `V0604` and
`vise fix` writes the annotation. This is the one place Vise's design bets
specifically on its author being a machine — annotation verbosity costs an
agent tokens, not comprehension, and the borrow checker is the strongest
mechanical verifier available. Trading it away for ergonomics would have
discarded the most Vise-aligned idea in Rust.

**There is no `unsafe`.** Data structures that require it live below the
language, in the stdlib, implemented in Rust and C. User code cannot opt out of
the rules.

## 10. Contracts

Optional. When present, they are checked at runtime in dev builds, discharged
statically where possible, and used to generate property tests.

```
fn fee(amount: Cents) -> Cents
  requires amount > 0
  ensures  result >= 0
{ amount / 50 }
```

`vise verify` reports which contracts are proven, which are tested, and which
are neither — a machine-checkable answer to "am I done?"

## 11. Determinism

Same inputs plus same recorded trace produces byte-identical output. `time` and
`rand` are effects precisely so they can be captured and replayed. Map
iteration order is specified. Float transcendentals come from a bundled
softfloat rather than platform libm, which is not reproducible across machines.

Deterministic destruction (§9) is part of this: with no collector, allocation
and teardown order is a property of the source, not of runtime timing.

`vise run --record t.trace`, then `vise replay t.trace`. A regression is a
trace diff, not a judgement call.

## 12. Diagnostics

The compiler's primary output is JSON; human text renders it. Every diagnostic
carries a stable code, a span, the cause, what is in scope, and ranked
candidate fixes.

```json
{ "code": "V0401",
  "span": {"file": "payments.vise", "line": 22, "col": 11},
  "message": "call introduces effect `net`, not declared by `charge`",
  "introduced_by": "post",
  "fixes": [{"kind": "add_effect", "edit": "!{net}"}] }
```

The design target: a correct repair follows from the message alone, without
re-reading the source.

## 13. Deliberately absent

Inheritance, traits, overloading, macros, reflection, exceptions, `null`,
implicit conversion, global mutable state, varargs, default arguments, operator
overloading, shared-memory threading, `unsafe`, conditional compilation, and
automatic version resolution.

Each is a place where two readings of the same code are possible. Ambiguity is
where a machine author guesses, and a guess that type-checks is the most
expensive failure this language exists to prevent.
