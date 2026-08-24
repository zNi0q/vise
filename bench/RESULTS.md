# Results

## Round 1 — five tasks, no signal

Run 2026-08-24. Five tasks, two languages, one trial each, all ten cells solved
by a fresh agent with no prior knowledge of this repository.

| Task | Vise checks | TypeScript checks | Both correct |
|---|---|---|---|
| classify | 1 | 1 | yes |
| grades | 1 | 1 | yes |
| divide | 1 | 1 | yes |
| orders | 1 | 1 | yes |
| fee | 1 | 1 | yes |

Every cell hit **1**. The outputs were verified independently rather than taken
from the agents' own reports.

### This is a failed experiment, not a confirmed thesis

One is the floor. A metric that counts repair iterations cannot show a
difference when neither arm ever needed to repair anything. The tasks were too
small: every agent wrote a correct program on its first attempt in both
languages, so the measurement had nothing to measure.

The claim in `README.md` is therefore **untested**, not supported. Saying
anything else would be reading a result into an absence of one.

### What did get measured

The cost side came out clearly, because it does not depend on anyone failing:

| | Vise | TypeScript | Difference |
|---|---|---|---|
| tokens, mean | 36,056 | 29,648 | **+21.6%** |
| tool calls | 4 | 3 | +1 |
| wall clock, mean | 32.0s | 22.2s | **+44%** |

The extra tool call is reading the specification, and the extra tokens are
mostly its contents. So on tasks this size the honest summary is: **Vise costs
about a fifth more tokens and buys nothing measurable.** That is what the
evidence says. Whether it buys something on work where first attempts actually
fail is the question round 1 failed to ask.

### One qualitative finding, and it is a real one

The agent solving `classify` reported:

> Spec never defines a modulo operator, so I used a toggled boolean instead of
> `n % 2`.

It was right. The specification lists `%` only in prose about line
continuation; it has no operator table anywhere. Section 0 says "if a construct
is not in this document, it does not exist", and the agent believed it rather
than reaching for the operator every other language has.

That is the closed-world discipline working exactly as intended — and it caught
a genuine hole in the specification, which is now issue 10 in `TRACKER.md`. It
also cost that agent a worse program. Both halves are the point.

### What round 2 has to change

Tasks where a competent first attempt is *likely to be wrong*: several
interacting cases, an error that has to travel through more than one function,
a precondition that one input violates. If the floor is still 1 on those, the
thesis is in real trouble.


## Round 2 — four harder tasks, and the thesis loses

Same protocol, tasks chosen so a first attempt could plausibly be wrong.

| Task | Vise checks | TypeScript checks | Both correct |
|---|---|---|---|
| ledger | 1 | 1 | yes |
| runs | **2** | 1 | yes |
| door | **3** | **2** | yes |
| chain | 1 | 1 | yes |
| **mean** | **1.75** | **1.25** | |

Across both rounds, nine tasks:

| | Vise | TypeScript |
|---|---|---|
| mean iterations-to-green | **1.33** | **1.11** |
| tokens, mean | ~36,900 | ~30,000 |

**Vise took more iterations, not fewer.** The claim predicted the opposite. On
this evidence — nine tasks, one model, one trial each — the thesis is not
supported, and the cost is real: about a fifth more tokens and, now,
more repair cycles too.

That is the result. It is small and it is noisy, and it is still the direction
the numbers point.

### Why Vise needed the extra cycles

Both extra Vise iterations came from Vise's own design or its own defects, not
from the agent misunderstanding the task.

- `runs`: "A trailing `if` with no `else` is rejected since `if` is an
  expression requiring both branches." That is §6 working as written. It cost a
  cycle and bought nothing here.
- `door`: "`.clone()` type-checks but traps at runtime as an unsupported method
  call."

The second one is not friction. It is the language failing its own premise.

### The most important thing this benchmark found

`vise check` accepted a program that trapped when run:

```
$ vise check clone.vise
module clone: 0 import(s), 1 item(s), checks pass
$ vise run clone.vise
trap: unsupported: method calls
```

`.clone()` is in the specification. The type checker returned a poison type for
every method call, which absorbed unification and let anything through; the
interpreter then refused it. "It compiles" meant nothing at all for that
program — which is the single claim this whole repository rests on.

Fixed: `.clone()` now type-checks to the receiver's type, and any other method
is `V0201` at check time with `clone` listed as what does exist. Regression
tests in `vise-check` and `vise-interp` hold the line.

An agent found in one afternoon a hole that 363 tests and every design review
in this repository had missed. That is the argument for running the benchmark,
independent of what the benchmark measured.

### The one place Vise's design demonstrably won

The TypeScript `door` agent lost its extra cycle to this:

> My `Event` type name collided with the DOM lib's global `Event` under
> `--strict`, so I renamed it to `DoorEvent`.

An ambient global nobody imported broke a correct program. Vise's closed
namespace (§3) makes that failure impossible: there are no ambient names. One
data point, but it is exactly the mechanism the design argues for, and it is
the only place in nine tasks where the strictness paid.

### What would actually test this

The tasks remain far too small. Both languages solve them in one or two passes,
so the measurement is dominated by whether the agent happened to read a rule
carefully. The thesis is about *blast radius* and *not knowing when you are
done* — failure modes that appear in programs with many interacting parts, not
in forty-line ones. Testing it properly needs tasks large enough that an agent
cannot hold the whole thing in its head, which is where a closed namespace,
exact effect rows, and enforced module size would begin to matter.

Until someone runs that, the honest summary of this repository is: an
interesting design, an argument that has not been demonstrated, and a benchmark
that found a real bug in the implementation.
