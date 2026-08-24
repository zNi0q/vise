/* Cooperative fibers and the assembly context switch.
 *
 * Exits 0 if every case behaved.
 */

#include "fiber.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>

static int failures = 0;

static void fail(const char *what)
{
    fprintf(stderr, "FAIL: %s\n", what);
    failures++;
}

/* Records the order in which things ran, which is the property that matters:
 * a deterministic schedule means this string is the same every time. */
static char order[64];
static void note(char c)
{
    size_t n = strlen(order);
    if (n + 1 < sizeof order) {
        order[n] = c;
        order[n + 1] = '\0';
    }
}

static void counter(void *arg)
{
    char label = (char)(uintptr_t)arg;
    for (int i = 0; i < 3; i++) {
        note(label);
        vise_fiber_yield();
    }
}

static void immediate(void *arg)
{
    (void)arg;
    note('x');
}

/* Enough stack use to be sure the switch really moved to the fiber's own
 * stack rather than scribbling on the caller's. */
static void deep(void *arg)
{
    volatile char scratch[4096];
    memset((void *)scratch, 0x5a, sizeof scratch);
    note((char)(uintptr_t)arg);
    vise_fiber_yield();
    for (size_t i = 0; i < sizeof scratch; i++) {
        if (scratch[i] != 0x5a) {
            fail("a fiber's stack did not survive a yield");
            return;
        }
    }
    note('k');
}

int main(void)
{
    if (!vise_fiber_supported()) {
        printf("fibers: unsupported on this architecture, skipping\n");
        return 0;
    }

    /* A fiber that finishes without yielding reports done, once. */
    order[0] = '\0';
    vise_fiber *once = vise_fiber_new(immediate, NULL, 0);
    if (once == NULL) {
        fail("could not create a fiber");
        return 1;
    }
    if (vise_fiber_resume(once) != 0) {
        fail("a fiber that ran to completion reported that it yielded");
    }
    if (!vise_fiber_done(once)) {
        fail("a finished fiber does not report done");
    }
    if (vise_fiber_resume(once) != 0) {
        fail("a finished fiber was resumable");
    }
    if (strcmp(order, "x") != 0) {
        fail("a fiber that finishes immediately did not run exactly once");
    }
    vise_fiber_free(once);

    /* Two fibers interleaved by the caller. The schedule is the caller's
     * choice, which is the whole point: nothing preempts. */
    order[0] = '\0';
    vise_fiber *a = vise_fiber_new(counter, (void *)(uintptr_t)'a', 0);
    vise_fiber *b = vise_fiber_new(counter, (void *)(uintptr_t)'b', 0);
    if (a == NULL || b == NULL) {
        fail("could not create two fibers");
        return 1;
    }
    for (int round = 0; round < 4; round++) {
        vise_fiber_resume(a);
        vise_fiber_resume(b);
    }
    /* Three notes each: `counter` notes then yields, three times over. */
    if (strcmp(order, "ababab") != 0) {
        fprintf(stderr, "  order was \"%s\"\n", order);
        fail("fibers did not interleave in the order the caller chose");
    }
    if (!vise_fiber_done(a) || !vise_fiber_done(b)) {
        fail("both fibers should have finished after four rounds");
    }
    vise_fiber_free(a);
    vise_fiber_free(b);

    /* A fiber's stack is its own, and survives being switched away from. */
    order[0] = '\0';
    vise_fiber *d = vise_fiber_new(deep, (void *)(uintptr_t)'d', 64 * 1024);
    if (d == NULL) {
        fail("could not create a deep fiber");
        return 1;
    }
    vise_fiber_resume(d);
    vise_fiber_resume(d);
    if (strcmp(order, "dk") != 0) {
        fprintf(stderr, "  order was \"%s\"\n", order);
        fail("a fiber's own stack did not survive the switch");
    }
    vise_fiber_free(d);

    /* Yielding outside a fiber is a no-op rather than a crash. */
    vise_fiber_yield();

    /* The schedule must be identical every run, which is what makes a trace
     * replayable at all. */
    char first[64];
    for (int run = 0; run < 20; run++) {
        order[0] = '\0';
        vise_fiber *p = vise_fiber_new(counter, (void *)(uintptr_t)'p', 0);
        vise_fiber *q = vise_fiber_new(counter, (void *)(uintptr_t)'q', 0);
        while (vise_fiber_resume(p) | vise_fiber_resume(q)) {
            /* run to completion */
        }
        if (run == 0) {
            snprintf(first, sizeof first, "%s", order);
        } else if (strcmp(first, order) != 0) {
            fail("the schedule was not identical between runs");
            break;
        }
        vise_fiber_free(p);
        vise_fiber_free(q);
    }

    if (failures == 0) {
        printf("fibers: all cases behaved\n");
    }
    return failures == 0 ? 0 : 1;
}
