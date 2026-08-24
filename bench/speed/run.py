#!/usr/bin/env python3
"""Build every kernel in every language and report the fastest run of each.

Fastest rather than mean: a slow run is a machine doing something else, and
that is noise about this laptop rather than about the language.

Usage: python3 bench/speed/run.py [--runs N] [kernel ...]
"""

import os
import pathlib
import subprocess
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
VISE = HERE / "../../target/release/vise"
OUT = HERE / "build"

# Each kernel and the size it runs at. The sizes are chosen so every arm takes
# long enough to measure and short enough to sit through.
KERNELS = {
    "loop": "300000000",
    "fib": "40",
    "listbuild": "20000000",
    "result": "50000000",
}

# `c+checks` is plain C written to trap on overflow the way §4 requires, so
# that the Vise column can be read against a C that obeys the same rule. `c`
# and `rust` do not check, and Rust's release mode does not either.
ARMS = ("vise", "c", "c+checks", "rust")


def build(kernel):
    """Compile all three arms. Returns arm -> binary, omitting any that fail."""
    OUT.mkdir(exist_ok=True)
    binaries = {}
    recipes = {
        "vise": [str(VISE), "build", str(HERE / f"{kernel}.vise"), "-o",
                 str(OUT / f"{kernel}.vise.bin")],
        "c": ["cc", "-std=c11", "-O2", "-o", str(OUT / f"{kernel}.c.bin"),
              str(HERE / f"{kernel}.c")],
        "c+checks": ["cc", "-std=c11", "-O2", "-I", str(HERE), "-o",
                     str(OUT / f"{kernel}.c+checks.bin"),
                     str(HERE / f"{kernel}.checked.c")],
        "rust": ["rustc", "-O", "-o", str(OUT / f"{kernel}.rust.bin"),
                 str(HERE / f"{kernel}.rs")],
    }
    for arm, command in recipes.items():
        done = subprocess.run(command, capture_output=True, text=True, cwd=OUT)
        if done.returncode == 0:
            binaries[arm] = OUT / f"{kernel}.{arm}.bin"
        else:
            print(f"  {arm}: did not build: {done.stderr.strip().splitlines()[-1:]}")
    return binaries


def measure(binary, size, runs):
    """The fastest of `runs` runs: seconds, peak megabytes, and the output.

    `os.wait4` is what reports the peak resident size of *this* child.
    `getrusage(RUSAGE_CHILDREN)` is a running maximum over every child the
    process has ever reaped, so it would report the largest arm's memory for
    every arm after it.
    """
    best, peak, output = float("inf"), 0, None
    for _ in range(runs):
        read, write = os.pipe()
        started = time.perf_counter()
        pid = os.fork()
        if pid == 0:  # the child
            os.close(read)
            os.dup2(write, 1)
            os.close(write)
            os.execv(str(binary), [str(binary), size])
            os._exit(127)
        os.close(write)
        with os.fdopen(read) as stdout:
            output = stdout.read().strip()
        _, _, usage = os.wait4(pid, 0)
        best = min(best, time.perf_counter() - started)
        peak = max(peak, usage.ru_maxrss)
    return best, peak / 1024, output


def main():
    argv = sys.argv[1:]
    runs = 5
    if "--runs" in argv:
        at = argv.index("--runs")
        runs = int(argv[at + 1])
        del argv[at:at + 2]
    wanted = argv or list(KERNELS)

    if not VISE.exists():
        sys.exit(f"{VISE} does not exist; run `cargo build --release` first")

    print(f"best of {runs}, sizes from the command line so nothing folds\n")
    header = f"{'kernel':<12}" + "".join(f"{a:>20}" for a in ARMS)
    print(header)
    print("-" * len(header))

    for kernel in wanted:
        size = KERNELS[kernel]
        binaries = build(kernel)
        row, answers = f"{kernel:<12}", {}
        for arm in ARMS:
            if arm not in binaries:
                row += f"{'--':>20}"
                continue
            seconds, megabytes, answer = measure(binaries[arm], size, runs)
            answers[arm] = answer
            row += f"{seconds:>13.3f}s{megabytes:>5.0f}MB"
        print(row)

        # Three arms that disagree are not three measurements of one thing.
        distinct = set(answers.values())
        if len(distinct) > 1:
            print(f"  !! the arms printed different answers: {answers}")


if __name__ == "__main__":
    main()
