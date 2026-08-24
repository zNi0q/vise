/* The capability gate.
 *
 * Spec section 7: "The runtime enforces the row independently of the compiler:
 * the process is sandboxed to exactly the syscalls its effect row implies.
 * Static checking and runtime confinement agree, or the program does not run."
 *
 * This is that second half. The compiler proves what a program *may* do; this
 * makes the kernel refuse everything else, so a bug in the effect checker
 * cannot become a bug in the sandbox.
 *
 * WHAT THIS CANNOT ENFORCE
 *
 * Syscall filtering only sees calls that reach the kernel. Linux exposes some
 * operations through the vDSO, a page of code mapped into every process, and
 * those never become syscalls at all. `clock_gettime` is the important case:
 * glibc reads the clock from the vDSO, so a process denied the `time` effect
 * can still read it. `getcpu` and `time` are the same.
 *
 * The gap is real and is not closed here. Closing it means running without a
 * vDSO, which has to be arranged before the process starts rather than from
 * inside it. Until then `time` is enforced by the compiler alone, and only the
 * effects whose operations are true syscalls -- `fs`, `net`, `rand`, `proc`,
 * and the descriptor side of `io` -- are enforced twice.
 *
 * `env` grants no syscall at all: the environment arrives on the initial stack,
 * so reading it is memory access. It is a compiler-only effect by nature.
 */
#ifndef VISE_CAPABILITY_H
#define VISE_CAPABILITY_H

#include <stdint.h>

/* Effect bits. These mirror the order of `Effect` in vise-ast, and the two
 * must not drift: a test asserts they agree. */
#define VISE_EFFECT_IO   (1u << 0)
#define VISE_EFFECT_FS   (1u << 1)
#define VISE_EFFECT_NET  (1u << 2)
#define VISE_EFFECT_TIME (1u << 3)
#define VISE_EFFECT_RAND (1u << 4)
#define VISE_EFFECT_ENV  (1u << 5)
#define VISE_EFFECT_PROC (1u << 6)

#define VISE_EFFECT_ALL  0x7fu

/* Outcomes of installing the gate. */
typedef enum {
    VISE_CAPS_OK = 0,
    /* The kernel does not support seccomp filtering. */
    VISE_CAPS_UNSUPPORTED = 1,
    /* PR_SET_NO_NEW_PRIVS failed. */
    VISE_CAPS_NO_NEW_PRIVS_FAILED = 2,
    /* The filter was rejected. */
    VISE_CAPS_FILTER_REJECTED = 3,
    /* More syscalls than the filter can hold. */
    VISE_CAPS_TOO_MANY = 4,
    /* This build has no gate for the running architecture. */
    VISE_CAPS_UNSUPPORTED_ARCH = 5,
} vise_caps_result;

/* Confine this process to the syscalls `effects` implies.
 *
 * Irreversible: a seccomp filter cannot be removed, only narrowed. Call it once,
 * immediately before running user code.
 *
 * A denied syscall kills the process with SIGSYS rather than returning an
 * error, because a program that has escaped its declared effects should not get
 * to decide what to do about it. */
vise_caps_result vise_caps_apply(uint32_t effects);

/* Human-readable form of a result, for diagnostics. */
const char *vise_caps_strerror(vise_caps_result result);

/* How many syscalls `effects` permits. Exposed so a test can check that adding
 * an effect widens the set and removing one narrows it, without installing a
 * filter the test could not then undo. */
unsigned vise_caps_syscall_count(uint32_t effects);

#endif /* VISE_CAPABILITY_H */
