# Speed

`bench/` next door measures whether an agent writes correct Vise in fewer
attempts. This directory measures something separate and much easier to be
honest about: how fast the compiled program runs.

The claim under test:

> Compiled Vise runs at the speed of C obeying the same rules.

"The same rules" is the whole point. Vise's `+` traps on overflow and its `at`
is bounds-checked; C's `+` wraps and its `[]` reads whatever is there. A Vise
column next to a plain C column measures those rules as much as it measures the
backend, so there is a third arm — `c+checks` — which is plain C written to trap
the way §4 requires, using the same `__builtin_*_overflow` the Vise runtime
uses. That is the comparison that says something about the compiler.

## Running it

```
cargo build --release
python3 bench/speed/run.py            # all kernels, best of 5
python3 bench/speed/run.py --runs 9 fib
```

Every kernel takes its size from the command line. A benchmark whose answer is a
constant measures the optimiser's constant folder, not the language — and Rust
folds two of these anyway, which is why its column is not the interesting one.

The runner checks that all four arms print the same answer. Arms that disagree
are not four measurements of one thing.

## Results

Best of seven on one x86-64 laptop, GCC 16.1 and rustc 1.93, `-O2` / `-O`. Times
are wall clock, memory is peak resident.

| kernel | vise | c | c+checks | rust |
|---|---|---|---|---|
| `loop` — 300M checked additions | 0.111s 9MB | 0.071s 9MB | 0.107s 9MB | 0.001s 9MB |
| `fib` — fib(40), recursion | 0.225s 9MB | 0.127s 9MB | 0.381s 9MB | 0.230s 9MB |
| `listbuild` — build and index 20M | 0.064s 158MB | 0.060s 154MB | 0.059s 154MB | 0.063s 155MB |
| `result` — 50M `Result<Int, Str>` | 0.031s 9MB | 0.039s 9MB | 0.031s 9MB | 0.017s 9MB |

Read against `c+checks`, which is the like-for-like column: Vise is at parity on
`loop` and `result`, 8% behind on `listbuild`, and ahead on `fib`. Read against
plain C, it is 1.1× to 1.8× slower, and the difference is the trapping rule
rather than the code generation.

Two caveats, both of which flatter nobody:

- **Rust's `loop` and `result` collapse.** 0.001s is not a loop running fast;
  it is LLVM recognising the closed form of a sum and replacing the loop with
  arithmetic. GCC does not do it here and neither does Vise. It is a fact about
  one optimiser, not about three languages.
- **`fib` beating `c+checks` is not a win to be proud of.** The checked C
  spells the recursion as `add(fib(sub(n, 1)), fib(sub(n, 2)))`, and GCC
  schedules that worse than the shape Vise emits. A different spelling would
  close it.

## What is not measured

Startup, compile time, strings, floating point, and anything that runs long
enough for the arena to matter. The arena is released at exit and never before
(see `runtime/c/value.h`), so a long-running program has a memory profile these
four kernels say nothing about. That is the honest limit of this page.
