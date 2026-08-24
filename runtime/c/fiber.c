/* Cooperative fibers over the assembly context switch. */

#define _GNU_SOURCE

#include "fiber.h"

#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

#if defined(__x86_64__)
#define VISE_HAVE_CONTEXT 1
#else
#define VISE_HAVE_CONTEXT 0
#endif

#if VISE_HAVE_CONTEXT
/* Defined in runtime/asm/switch_x86_64.S. */
extern void vise_context_switch(void **save_sp, void *resume_sp);
extern void vise_context_entry(void);
#endif

/* Called from the entry trampoline when a fiber's function returns. */
void vise_fiber_finished(void);

#define DEFAULT_STACK (128u * 1024u)

struct vise_fiber {
    /* The fiber's suspended stack pointer. */
    void *sp;
    /* Where to return on yield or completion. */
    void *resumer_sp;
    void *stack;
    size_t stack_size;
    int finished;
};

/* The fiber currently executing, if any. Single-threaded by design: a
 * deterministic scheduler that could be entered from two threads at once would
 * not be deterministic. */
static vise_fiber *current = NULL;

int vise_fiber_supported(void)
{
    return VISE_HAVE_CONTEXT;
}

int vise_fiber_done(const vise_fiber *fiber)
{
    return fiber == NULL || fiber->finished;
}

#if VISE_HAVE_CONTEXT

/* Lay out a stack so the first switch lands in `vise_context_entry`.
 *
 * The switch pops six registers and then returns, so the slots below are read
 * in that order: r15, r14, r13, r12, rbx, rbp, and finally the return address.
 * The argument and entry point travel in r15 and r14 because `ret` cannot put
 * anything in rdi.
 */
static void *prepare(void *stack, size_t size, vise_fiber_fn entry, void *arg)
{
    uintptr_t top = (uintptr_t)stack + size;
    top &= ~(uintptr_t)15;

    /* The System V ABI wants rsp % 16 == 8 on entry to a function, which is
     * what it would be just after a `call`. Working backwards through six pops
     * and a `ret`, that means the saved stack pointer must be 16-aligned, so
     * the top is nudged to 8 mod 16 first. */
    top -= 8;

    uint64_t *sp = (uint64_t *)(top - 56);
    sp[0] = (uint64_t)(uintptr_t)arg;                 /* -> r15 */
    sp[1] = (uint64_t)(uintptr_t)entry;               /* -> r14 */
    sp[2] = 0;                                        /* -> r13 */
    sp[3] = 0;                                        /* -> r12 */
    sp[4] = 0;                                        /* -> rbx */
    sp[5] = 0;                                        /* -> rbp */
    sp[6] = (uint64_t)(uintptr_t)vise_context_entry;  /* return address */
    return sp;
}

vise_fiber *vise_fiber_new(vise_fiber_fn entry, void *arg, size_t stack_size)
{
    if (entry == NULL) {
        return NULL;
    }
    size_t page = (size_t)sysconf(_SC_PAGESIZE);
    size_t size = stack_size == 0 ? DEFAULT_STACK : stack_size;
    size = ((size + page - 1) / page) * page;

    /* One guard page below the stack, so running off the end faults instead of
     * quietly writing over whatever the allocator put there. */
    size_t mapped = size + page;
    void *base = mmap(NULL, mapped, PROT_READ | PROT_WRITE,
                      MAP_PRIVATE | MAP_ANONYMOUS | MAP_STACK, -1, 0);
    if (base == MAP_FAILED) {
        return NULL;
    }
    if (mprotect(base, page, PROT_NONE) != 0) {
        munmap(base, mapped);
        return NULL;
    }

    vise_fiber *fiber = calloc(1, sizeof *fiber);
    if (fiber == NULL) {
        munmap(base, mapped);
        return NULL;
    }
    fiber->stack = base;
    fiber->stack_size = mapped;
    fiber->finished = 0;
    fiber->sp = prepare((char *)base + page, size, entry, arg);
    return fiber;
}

int vise_fiber_resume(vise_fiber *fiber)
{
    if (fiber == NULL || fiber->finished) {
        return 0;
    }
    vise_fiber *previous = current;
    current = fiber;
    vise_context_switch(&fiber->resumer_sp, fiber->sp);
    current = previous;
    return fiber->finished ? 0 : 1;
}

void vise_fiber_yield(void)
{
    vise_fiber *fiber = current;
    if (fiber == NULL) {
        return; /* Not in a fiber: yielding has nothing to return to. */
    }
    vise_context_switch(&fiber->sp, fiber->resumer_sp);
}

void vise_fiber_finished(void)
{
    vise_fiber *fiber = current;
    if (fiber == NULL) {
        abort();
    }
    fiber->finished = 1;
    /* Switch away for the last time. The saved stack pointer is never read
     * again, so it goes to a scratch slot rather than back into the fiber. */
    void *discard = NULL;
    vise_context_switch(&discard, fiber->resumer_sp);
    /* A finished fiber is never resumed, so this cannot be reached. */
    abort();
}

#else /* !VISE_HAVE_CONTEXT */

vise_fiber *vise_fiber_new(vise_fiber_fn entry, void *arg, size_t stack_size)
{
    (void)entry; (void)arg; (void)stack_size;
    return NULL;
}

int vise_fiber_resume(vise_fiber *fiber) { (void)fiber; return 0; }
void vise_fiber_yield(void) {}
void vise_fiber_finished(void) { abort(); }

#endif

void vise_fiber_free(vise_fiber *fiber)
{
    if (fiber == NULL) {
        return;
    }
    if (fiber->stack != NULL) {
        munmap(fiber->stack, fiber->stack_size);
    }
    free(fiber);
}
