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
