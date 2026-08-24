# Vise — Project Tracker

Status legend: `[ ]` todo · `[~]` in progress · `[x]` done · `[!]` blocked

Work proceeds one task at a time, one commit per task. Commit messages follow
[Conventional Commits](https://www.conventionalcommits.org/).

---

## M0 — Foundations

- [x] Language specification v0.3 (`spec/spec.md`)
- [x] Repository, `.gitignore`, commit conventions
- [x] Cargo workspace, zero third-party dependencies (`vise-diag` added
      for shared diagnostics, so eight crates rather than seven)
- [x] `vise` CLI skeleton: `lex`, `explain`, `--json`
- [x] CI: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, examples

## M1 — Front end

- [x] Token definitions and spans
- [x] Lexer (incl. string interpolation, `--` comments)
- [x] Lexer test suite (33 tests)
- [x] Layout pass: line break terminates a statement (spec v0.4)
- [x] AST definitions
- [x] Parser: module header, `use`, `type`, `record`, `enum`
- [x] Parser: functions, effect rows, contracts
- [x] Parser: expressions with precedence, `match`, `for`, `while`, `?`
- [x] Parser error recovery producing multiple diagnostics per run
- [x] Diagnostic type with stable codes, spans, in-scope names, ranked fixes
- [x] JSON diagnostic emitter (`V0101`, `V0201`, …)
- [x] Canonical formatter (`vise fmt`), non-configurable; idempotent and
      round-trip tested

## M2 — Types and effects

- [x] Name resolution over the closed namespace; `V0201` lists what is in scope
- [x] Module 500-line cap (`V0101`)
- [x] Type inference for locals and private functions (unification)
- [x] Distinct `type` declarations (nominal, not aliases)
- [x] Parametric generics
- [x] Exhaustiveness checking for `match` (`V0301`) — conservative: descends
      into single-field constructors only, never reports a false positive
- [x] Unused-`Result` detection (`V0501`) — limited to calls whose return
      type is declared in this module
- [x] Effect inference, bottom-up (call-graph fixpoint)
- [x] Effect row checking against declarations (`V0401`, `V0402`)
- [x] `vise fix` applies every unambiguous fix, effect rows included

## M3 — Falsification gate

**This milestone decides whether the project continues.**

- [x] Tree-walking interpreter (throwaway; exists to run the benchmark early)
- [~] `core` builtins: `List`, `Option`, `Result`, `print` done; `Map` and
      `Set` have no literal syntax yet
- [ ] Benchmark harness: N tasks solved by an agent in Vise and in TypeScript
- [ ] Metrics: iterations-to-green, hallucinated-API rate, tokens per solve
- [ ] Task set covering graph-shaped data — the case most likely to fail
- [ ] Write up results, including a negative result if that is what we get

Kill criteria: if Vise does not beat TypeScript on iterations-to-green, and the
gap is not explained by a fixable diagnostic quality problem, the thesis is
wrong and the repo should say so.

## M4 — Ownership

- [x] Move checking; use-after-move (`V0601`) names the move site
- [x] Borrow checking, shared-xor-mutable (`V0602`) — within one call
- [x] Borrow-outlives-owner (`V0603`) — returning a borrow of a local
- [ ] Lifetime inference across function bodies
- [ ] Signature elision rules; `V0604` when genuinely ambiguous
- [ ] `vise fix` writes lifetime annotations
- [ ] Arena in `core` for graph-shaped data (see open question in README)
- [ ] Re-run M3 benchmark with ownership enabled

## M5 — Runtime (C + Assembly)

- [ ] Allocator
- [ ] Capability gate: seccomp-bpf filter derived from the effect row
- [ ] Syscall trampoline (asm)
- [ ] Deterministic scheduler; x86-64 context switch (asm)
- [ ] Trace record / replay (`vise run --record`, `vise replay`)
- [ ] Deterministic softfloat for transcendentals
- [ ] Proof that static effect rows and runtime confinement agree

## M6 — Native backend

- [ ] IR definition
- [ ] Lowering from checked AST
- [ ] Cranelift or C emission (decide once IR exists)
- [ ] Deterministic destruction ordering in codegen
- [ ] End-to-end: `vise build` producing a native binary

---

## Decisions

| Date | Decision | Rationale |
|---|---|---|
| 2026-08-22 | Own compiler in Rust, runtime in C, asm only for context switch and trampoline | Assembly elsewhere would be decoration |
| 2026-08-22 | Effects inferred by default, enforced when declared | Keeps hello-world annotation-free without weakening the guarantee |
| 2026-08-22 | Dropped mandatory termination proofs | Cost more in ergonomics than it bought in safety |
| 2026-08-22 | Kept Rust ownership in full, reversing the GC decision | The borrow checker is the strongest mechanical verifier available, and annotation cost is tokens, not comprehension, for a machine author |
| 2026-08-22 | Tokens are `Copy`: kind plus span, no payload | Text is recovered from the span, so the lexer allocates nothing |
| 2026-08-22 | String interpolation is validated by the lexer but split by the parser | Keeps the token stream flat and avoids a brace-depth stack in the lexer |
| 2026-08-22 | Lexer emits `Newline`; a layout pass decides which ones matter | Keeps the rule in one testable place instead of spread through the parser |
| 2026-08-22 | A line break terminates a statement; no semicolons | Resolves the §2 contradiction with no visible ceremony, and every spec example works unchanged |
| 2026-08-22 | Record literals are banned in `if`/`while`/`for`/`match` headers | `if x { .. }` cannot otherwise be told from a record literal; parentheses re-enable them (Rust's rule) |
| 2026-08-22 | Comparisons are non-associative | `a < b < c` would quietly compare a Bool with a number |
| 2026-08-22 | Paths are a single segment; `::` is not parsed | The spec's examples use bare constructors (`Ok`, `CardDeclined`). See spec issue 4. |
| 2026-08-23 | A numeric literal adopts the other operand's type | Resolves a contradiction between §4 (distinct types) and §10 (whose `fee` example did arithmetic between `Cents` and integer literals). Narrowest rule that works: named types still never mix. |

## Open spec issues

Found while implementing; each needs a decision before the parser lands.

1. **`--` comments make `a --b` a comment, not a subtraction.** Inherited
   from Haskell's identical problem. A space disambiguates (`a - -b` is
   arithmetic) but the silent reading is the wrong one. Either accept and
   document it, or take a different comment syntax. Currently accepted, with a
   test pinning the behaviour.
2. ~~No statement terminator, yet layout is called insignificant~~ —
   **resolved in spec v0.4**: a line break ends a statement, with continuation
   when the line is visibly unfinished. Go's rule, no visible ceremony, and
   every example already in the spec works unchanged.

3. **Closures are referenced but never defined.** §4 says types are inferred
   for "locals, closures, and private functions", but no closure syntax appears
   anywhere in the spec, and §13 does not list them as absent. Either give them
   syntax or strike the word. The AST omits them until this is decided rather
   than guessing a form.

4. **How are enum variants qualified?** The spec writes bare `Ok(..)` and
   `CardDeclined(..)`, relying on imports and inference, and never uses `::`.
   The lexer produces a `::` token that nothing parses. Either define qualified
   paths or drop the token.

5. **`core` is never enumerated.** §3 shows six names marked "implicit; shown
   for clarity" and §1 calls `print` without importing it, but the spec never
   says what `core` contains. The closed namespace is the mechanism behind
   `V0201`, and "what is in scope" has no answer without this list.
   `crates/vise-check/src/prelude.rs` holds a provisional core covering exactly
   what the worked examples use.
6. **Is shadowing allowed?** The implementation currently follows Rust: an
   inner scope may shadow an outer one, and redeclaring within one scope is
   `V0203`. The spec says nothing. Shadowing is a real source of
   plausible-but-wrong reads, so forbidding it outright is defensible.

7. **How does a caller learn an imported function's effects?** There is no
   module system, so `use std/http@1:{post}` brings in a name with no
   signature. Effect checking is therefore one-sided for any function that
   calls an import: `V0401` still fires for effects that are known, but
   `V0402` ("declared but never performed") is suppressed, because absence of
   proof is not proof of absence. Needs a module system, or signatures
   published alongside imports.
8. **Method calls are assumed pure.** `.clone()` is, but nothing enforces that
   for any other method. Becomes a real lookup once types exist.

## Open questions

Tracked in `README.md`. The load-bearing one is whether the borrow checker
fights agents on graph-shaped data, which M3 must measure directly.
