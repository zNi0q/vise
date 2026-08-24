/* Trapping arithmetic in C, for the arm that compares like with like.
 *
 * Vise's `+` traps on overflow; C's wraps. Comparing them measures the check as
 * much as the code generation, so the `c+checks` arm is plain C written the way
 * the language rule requires -- the same `__builtin_*_overflow` the Vise
 * runtime uses, inlined the same way. The gap that remains after that is the
 * backend's, and it is the one worth closing.
 */
#ifndef BENCH_CHECKED_H
#define BENCH_CHECKED_H

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static inline int64_t add(int64_t a, int64_t b)
{
    int64_t out;
    if (__builtin_add_overflow(a, b, &out)) {
        fputs("trap: integer overflow in `+`\n", stderr);
        exit(1);
    }
    return out;
}

static inline int64_t sub(int64_t a, int64_t b)
{
    int64_t out;
    if (__builtin_sub_overflow(a, b, &out)) {
        fputs("trap: integer overflow in `-`\n", stderr);
        exit(1);
    }
    return out;
}

#endif /* BENCH_CHECKED_H */
