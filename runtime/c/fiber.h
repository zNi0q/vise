/* Cooperative fibers.
 *
 * The scheduling substrate for spec §11: a Vise program's execution order must
 * be a property of its source, not of when the OS chose to preempt it. Fibers
 * switch only where the runtime says so, which is what makes a recorded trace
 * replayable.
 *
 * Cooperative, not preemptive, and deliberately so. A preemptive scheduler
 * would reintroduce exactly the nondeterminism this is here to remove.
 */
#ifndef VISE_FIBER_H
#define VISE_FIBER_H

#include <stddef.h>

typedef struct vise_fiber vise_fiber;

/* What a fiber runs. */
typedef void (*vise_fiber_fn)(void *arg);

/* Create a fiber that will run `entry(arg)` when first resumed.
 * `stack_size` is rounded up to a page; 0 means a sensible default.
 * Returns NULL if the stack could not be mapped or the platform is
 * unsupported. */
vise_fiber *vise_fiber_new(vise_fiber_fn entry, void *arg, size_t stack_size);

/* Resume `fiber`, suspending the caller until the fiber yields or finishes.
 * Returns 1 if the fiber yielded and may be resumed again, 0 if it finished. */
int vise_fiber_resume(vise_fiber *fiber);

/* Suspend the running fiber and return to whoever resumed it. */
void vise_fiber_yield(void);

/* Whether `fiber` has run to completion. */
int vise_fiber_done(const vise_fiber *fiber);

/* Release a finished fiber. Destroying a fiber that has not finished leaks its
 * stack rather than unwinding it, which is reported instead of guessed at. */
void vise_fiber_free(vise_fiber *fiber);

/* Whether this build has a context switch for the running architecture. */
int vise_fiber_supported(void);

#endif /* VISE_FIBER_H */
