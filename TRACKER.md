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
- [ ] CI: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`

## M1 — Front end

- [x] Token definitions and spans
- [x] Lexer (incl. string interpolation, `--` comments)
- [x] Lexer test suite (33 tests)
- [ ] AST definitions
- [ ] Parser: module header, `use`, `type`, `record`, `enum`
- [ ] Parser: functions, effect rows, contracts
- [ ] Parser: expressions with precedence, `match`, `for`, `while`, `?`
- [ ] Parser error recovery producing multiple diagnostics per run
- [x] Diagnostic type with stable codes, spans, in-scope names, ranked fixes
- [x] JSON diagnostic emitter (`V0101`, `V0201`, …)
- [ ] Canonical formatter (`vise fmt`), non-configurable

## M2 — Types and effects

- [ ] Name resolution over the closed namespace; `V0201` lists what is in scope
- [ ] Module 500-line cap (`V0101`)
- [ ] Hindley–Milner inference for locals and private functions
- [ ] Distinct `type` declarations (nominal, not aliases)
- [ ] Parametric generics
- [ ] Exhaustiveness checking for `match` (`V0301`)
- [ ] Unused-`Result` detection (`V0501`)
- [ ] Effect inference, bottom-up
- [ ] Effect row checking against declarations (`V0401`)
- [ ] `vise fix` writes inferred effect rows into signatures

## M3 — Falsification gate

**This milestone decides whether the project continues.**

- [ ] Tree-walking interpreter (throwaway; exists to run the benchmark early)
- [ ] `core` builtins: `List`, `Map`, `Set`, `Option`, `Result`, `print`
- [ ] Benchmark harness: N tasks solved by an agent in Vise and in TypeScript
- [ ] Metrics: iterations-to-green, hallucinated-API rate, tokens per solve
- [ ] Task set covering graph-shaped data — the case most likely to fail
- [ ] Write up results, including a negative result if that is what we get

Kill criteria: if Vise does not beat TypeScript on iterations-to-green, and the
gap is not explained by a fixable diagnostic quality problem, the thesis is
wrong and the repo should say so.

## M4 — Ownership

- [ ] Move checking; use-after-move (`V0601`) names the move site
- [ ] Borrow checking, shared-xor-mutable (`V0602`)
- [ ] Borrow-outlives-owner (`V0603`)
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
| 2026-08-22 | Lexer emits `Newline`; the parser decides whether it matters | **Open spec issue**: §2 calls layout insignificant, but Vise has no statement terminator. See "Open spec issues" below. |

## Open spec issues

Found while implementing; each needs a decision before the parser lands.

1. **`--` comments make `a --b` a comment, not a subtraction.** Inherited
   from Haskell's identical problem. A space disambiguates (`a - -b` is
   arithmetic) but the silent reading is the wrong one. Either accept and
   document it, or take a different comment syntax. Currently accepted, with a
   test pinning the behaviour.
2. **No statement terminator, yet layout is called insignificant** (§2, §5).
   Both cannot hold. `f\n(x)` is either one call or two statements, and that
   is exactly the ambiguity §13 exists to forbid. Three ways out: make a line
   break a terminator (Go's rule, no visible ceremony), require semicolons
   (explicit, more typing), or keep layout insignificant and restrict the
   grammar so no expression can continue across a line. The lexer currently
   emits `Newline` so any of the three stays available.

## Open questions

Tracked in `README.md`. The load-bearing one is whether the borrow checker
fights agents on graph-shaped data, which M3 must measure directly.
