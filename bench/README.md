# The experiment

`README.md` states the claim this repository exists to test:

> If a language is strict in the specific ways an agent's failures need, the
> agent's write → compile → repair loop gets measurably shorter, and "it
> compiles" becomes strong evidence of "it is correct."

This directory is the attempt to falsify it.

## Method

Each task is a short program with an exactly specified output. An agent is
given the task and one language, and loops: write the file, run the checker,
read the diagnostics, edit, run again — until the program produces the expected
output, or it reaches the attempt limit.

The measurement is **iterations-to-green**: how many times the agent invoked
the checker before the program was correct. One means it was right first time.

Both arms are measured the same way, on the same tasks, with an equivalent
toolchain:

| | Vise | TypeScript |
|---|---|---|
| check | `vise check task.vise` | `tsc --strict --noEmit task.ts` |
| run | `vise run task.vise` | `bun run task.ts` |
| reference | `spec/spec.md`, the whole language | none: the model already knows it |

That asymmetry in the reference column *is* the thesis. Vise has no training
corpus, and the mitigation is that its entire specification fits in a context
window. If handing an agent a complete small spec does not beat its fluency in
a large familiar language, the idea does not work.

## What this measurement cannot tell you

Stated plainly, because a benchmark designed by the author of one of the two
languages is worth exactly as much as its disclosed biases.

1. **The tasks fit Vise's stdlib.** Vise's `core` is a handful of names
   (`prelude.rs`), and it has no imports, no string methods, and no `Map`. Tasks
   had to avoid all of those. TypeScript's much larger library is therefore
   never an advantage here, and on a real program it usually would be. This
   biases toward Vise.
2. **The tasks were chosen by someone who knows Vise's diagnostics.** They lean
   on exhaustive matching, error handling, and preconditions — the cases Vise
   was built to catch. That is the honest place to look first, and it is still
   a biased sample.
3. **One model, one trial per cell, five tasks.** Iterations-to-green is a small
   integer with real variance. Five tasks cannot separate a genuine effect from
   noise. Treat any difference as directional, not as a result.
4. **Compiling is not correctness.** Green here means *produces the expected
   output*, which is stronger than compiling, but the programs are small enough
   that the distinction rarely bites.

A negative result is a real result and belongs in `RESULTS.md` either way.

## Layout

```
tasks/         one file per task, language-neutral
RESULTS.md     what happened, including if nothing did
```
