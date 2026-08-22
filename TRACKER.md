# Vise — Project Tracker

Status legend: `[ ]` todo · `[~]` in progress · `[x]` done · `[!]` blocked

Work proceeds one task at a time, one commit per task. Commit messages follow
[Conventional Commits](https://www.conventionalcommits.org/).

---

## M0 — Foundations

- [x] Language specification v0.3 (`spec/spec.md`)
- [x] Repository, `.gitignore`, commit conventions
- [ ] Cargo workspace with the seven compiler crates
- [ ] `vise` CLI skeleton with `--json` diagnostics flag
- [ ] CI: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`

## M1 — Front end

- [ ] Token definitions and spans
- [ ] Lexer (incl. string interpolation, `--` comments)
- [ ] Lexer test suite
- [ ] AST definitions
- [ ] Parser: module header, `use`, `type`, `record`, `enum`
- [ ] Parser: functions, effect rows, contracts
- [ ] Parser: expressions with precedence, `match`, `for`, `while`, `?`
- [ ] Parser error recovery producing multiple diagnostics per run
- [ ] Diagnostic type with stable codes, spans, in-scope names, ranked fixes
- [ ] JSON diagnostic emitter (`V0101`, `V0201`, …)
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

## Open questions

Tracked in `README.md`. The load-bearing one is whether the borrow checker
fights agents on graph-shaped data, which M3 must measure directly.
