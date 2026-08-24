# Vise

**Vise lets an AI agent write software you don't have to read, because the
compiler — not a human reviewer — guarantees what the code does and what it is
allowed to touch.**

As easy to write as Python. As strict as Rust — the rules, not the paperwork.

## Thesis

LLM agents fail differently from human programmers. Humans fail at typos and
off-by-one. Agents fail at calling APIs that do not exist, at plausible code
that passes a weak type checker, at unbounded blast radius from a small edit,
and at not knowing whether they are finished.

Vise is a test of one claim: **if a language is strict in the specific ways
those failures need, the agent's write → compile → repair loop gets measurably
shorter, and "it compiles" becomes strong evidence of "it is correct."**

That claim is falsifiable, and falsifying it is the point of this repo.

**It has not survived its first test.** Across nine tasks, Vise took *more*
repair iterations than TypeScript (1.33 against 1.11) and about 22% more
tokens. See `bench/RESULTS.md` for the numbers, the biases, and why the tasks
were probably too small to test the claim properly. The same run found a
soundness hole in the checker — `.clone()` passed `vise check` and trapped at
runtime — which is the failure the language exists to prevent, in the language
itself.

One claim it *has* survived: compiled Vise runs at the speed of C obeying the
same rules. Across four kernels it is at parity with C written to trap on
overflow and to bounds-check the way §4 requires, and 1.1x to 1.8x slower than
C that does neither — which is what the trapping rule costs, not what the
backend costs. `bench/speed/README.md` has the numbers, the fourth arm that
makes them mean something, and what they do not cover.

## Why "Python-easy and Rust-strict" is achievable here

Not by weakening Rust. Vise keeps the ownership model whole: single ownership,
moves, shared-xor-mutable borrows, deterministic destruction, no garbage
collector, no `unsafe` in user code.

The borrow checker is the strongest mechanical verifier in any mainstream
language — it rejects entire classes of plausible-but-wrong code at compile
time, with crisp, mechanically-repairable errors. That is the Vise thesis
almost word for word. Trading it for a GC would have thrown away the most
Vise-aligned idea in Rust.

What Vise drops is the *annotation burden*, not the rules. Lifetimes are
inferred across a whole function body and elided in signatures; you name one
only where the signature is genuinely ambiguous, and `vise fix` writes it. The
usual objection — that lifetime annotations are hard — is a claim about human
comprehension. Vise's author is a machine, and verbosity costs it tokens, not
understanding.

The same instinct runs through the language: **inferred by default, enforced
when declared.** Types are inferred except on public signatures. Effects are
inferred unless you annotate a row. Contracts are optional. Hello-world has
zero annotations; a production signature still proves its own blast radius.

## What "strict" means here

| Agent failure mode | Language answer |
|---|---|
| Hallucinated APIs | Closed namespace; unknown identifier is a compile error listing what is in scope |
| Unbounded blast radius | Effects in the signature, enforced by the runtime sandbox too |
| Needing to read 10 files to change 1 | 500-line module cap, explicit imports, no transitive visibility |
| Plausible-but-wrong code | Ownership and borrow checking, distinct types, exhaustive matching, no `any`, no null, trapping overflow |
| Not knowing it is done | Optional contracts compiling to runtime checks, static proofs, and property tests |
| Slow repair loops | JSON-first diagnostics carrying scope and ranked candidate fixes |
| "Works on my machine" | Recorded and replayable execution; a regression is a trace diff |

## The corpus problem

Agents are fluent in languages with enormous training corpora. Vise has none.
This is the biggest risk to the project and it is not waved away: the
mitigation is that `spec/spec.md` is the *complete* language and fits in a few
thousand tokens, so an agent works from the authoritative spec in-context
rather than from recalled familiarity. A small, closed language with a perfect
spec may beat a large, open language with fuzzy priors. May. That is the
experiment.

## Stack

- **Rust** — compiler: lex, parse, type/effect/contract check, IR, codegen.
- **C** — runtime: capability gate (syscall filter derived from the effect
  row), allocator, trace record/replay, deterministic softfloat.
- **Assembly** — two justified places only: x86-64 context switch for the
  deterministic scheduler, and the syscall trampoline. Nowhere else.

## Layout

```
spec/spec.md      the entire language, ~1.9k tokens
crates/           compiler (Rust)
runtime/c         capability gate, allocator, trace, softfloat
runtime/asm       context switch, trampoline
bench/            the experiment: agent repair-iteration harness
bench/speed       the other measurement: runtime speed against C and Rust
tools/            programs written in Vise
```

## Installing

Needs Rust and a C compiler: the build compiles the C runtime, and `vise build`
shells out to a C compiler to produce a binary.

```sh
git clone https://github.com/zNi0q/vise && cd vise
./install.sh                    # to ~/.local/bin
PREFIX=/usr/local ./install.sh  # or somewhere else
./install.sh --uninstall
```

`make install` does the same thing. On Arch, `packaging/PKGBUILD` builds a
package with `makepkg -si`.

## Using it

```
vise repl                  an interactive session
vise run file.vise         check, then interpret
vise build file.vise       check, then compile to a native binary
vise check file.vise       all checks; --json for machine-readable diagnostics
vise fmt file.vise         rewrite in canonical form
vise fix file.vise         apply every unambiguous fix
vise explain V0401         why a rule exists
```

The REPL rebuilds a whole module around each input and replays the session from
the start, which is invisible only because a Vise program is deterministic
(§11):

```
vise> fn double(n: Int) -> Int { n * 2 }
vise> var total = 0
vise> for n in [1, 2, 3] { total = total + n }
vise> double(total)
12
vise> :type total
total : Int
```

## Licence

Dual licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your
option.

## Status

Spec v0.5. The compiler runs end to end: `vise check` resolves names and checks
types, effects, exhaustiveness, dropped `Result`s, and ownership; `vise run`
interprets; `vise build` compiles to a native binary; `vise fmt` canonicalises;
`vise fix` applies the unambiguous repairs.

```
$ vise build examples/greet.vise && ./greet
hello, ada
hello, alan
hello, grace
```

The central claim, working:

```
$ vise check examples/hallucinated.vise
error[V0201]: `fetch_user` is not in scope
  --> examples/hallucinated.vise:6:18
   |
 6 |   let response = fetch_user(id)
   |                  ^^^^^^^^^^
   = note: a name must be defined in this module or listed in a `use`;
           there is no glob import
   = in scope: Bool, Char, Err, Float, Int, List, Map, None, Ok, Option,
               Result, Set, Some, Str, Unit, charge_user, id, post, print
   = fix (likely): replace `charge_user`
```

A hallucinated API is a compile error, and the diagnostic hands back every name
that does exist rather than refusing the one that does not.

`vise fix` applies the unambiguous ones — a lone `Certain` suggestion, such as
the exactly-widened effect row. It deliberately declines anything else: the
rename above is offered to a reader but never applied unattended, because
guessing which name was meant is the author's decision.

## Roadmap

1. **Spec** — v0.4, and the thing to argue with first.
2. **Benchmark harness** — N tasks, an agent solves each in Vise and in
   TypeScript, measure iterations-to-green and hallucinated-API rate. Build a
   tree-walking interpreter first so this can run before the real backend
   exists. *If the numbers are flat here, the thesis is wrong and we should
   know that in week two, not month six.*
3. **Compiler front end** — ~~lexer~~, ~~parser~~, ~~name resolution~~, type
   inference, borrow checker, effect inference.
4. **Runtime** — capability gate, allocator, trace record/replay, scheduler.
5. **Native backend** — ~~C emission~~. `vise build` produces a native binary;
   a differential suite runs each program compiled *and* interpreted and fails
   if the two ever disagree.

## Open questions

- **No traits at all.** Parametric-only generics may make collection and
  numeric code repetitive enough to hurt. The cheapest fix if it bites is
  monomorphised traits with no inheritance, but that is real complexity.
- **Does the borrow checker fight agents on graph-shaped data?** This is the
  real cost of keeping ownership. Trees and linked structures are where Rust
  users reach for `Rc`/`RefCell` or arena-plus-index. Vise has no `unsafe`
  escape hatch, so the stdlib must ship an arena that makes the common shapes
  pleasant, or agents will thrash. Highest-priority thing to measure.
- **Distinct `type` aliases** are the strongest bug-catcher in the language and
  also the most conversion boilerplate. Watch for agents writing wrapper
  functions to escape them.

## Resolved

- ~~Termination proofs on every loop~~ — dropped in v0.2. Correct in theory,
  and it cost more in ergonomics than it bought in safety. Moved to opt-in
  `vise verify`.
- ~~Exact effect rows on every function~~ — v0.2 infers effects by default.
  Annotation is opt-in and enforced exactly when present.
- ~~Refcounting GC instead of ownership~~ — reversed in v0.3. Dropping the
  borrow checker was justified by human ergonomics, which is the wrong
  yardstick for a language whose author is an agent. Ownership is back, whole.
