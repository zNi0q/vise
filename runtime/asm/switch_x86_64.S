/* x86-64 context switch.
 *
 * One of the two places this project uses assembly. There is no way to express
 * "save the callee-saved registers, exchange the stack pointer, and resume
 * somewhere else" in C: the operation is defined entirely in terms of registers
 * the language does not let you name.
 *
 * The other place is the syscall trampoline. Anywhere else, assembly would be
 * decoration.
 *
 * Deterministic scheduling (spec §11) needs this: a fiber must yield at points
 * the runtime chooses, not wherever the OS decides to preempt it.
 */

    .text

/* void vise_context_switch(void **save_sp, void *resume_sp);
 *
 * rdi = where to store the outgoing stack pointer
 * rsi = the stack pointer to resume
 *
 * The System V ABI makes rbx, rbp, and r12-r15 callee-saved, so they are the
 * whole of a context: everything else is either caller-saved or derived.
 */
    .globl vise_context_switch
    .type vise_context_switch, @function
    .align 16
vise_context_switch:
    .cfi_startproc
    pushq %rbp
    pushq %rbx
    pushq %r12
    pushq %r13
    pushq %r14
    pushq %r15

    movq %rsp, (%rdi)          /* hand the outgoing stack back to the caller */
    movq %rsi, %rsp            /* and adopt the incoming one */

    popq %r15
    popq %r14
    popq %r13
    popq %r12
    popq %rbx
    popq %rbp
    ret
    .cfi_endproc
    .size vise_context_switch, . - vise_context_switch

/* Where a freshly created context begins.
 *
 * `vise_fiber_prepare` lays out a stack so that the first switch into it pops
 * the argument into r15 and the entry point into r14, then returns here. This
 * exists because `ret` cannot place an argument in rdi, so a new context has to
 * land somewhere that can.
 */
    .globl vise_context_entry
    .type vise_context_entry, @function
    .align 16
vise_context_entry:
    .cfi_startproc
    /* Nothing above this frame: tell the unwinder to stop rather than walk off
     * the top of a stack it did not create. */
    .cfi_undefined %rip
    movq %r15, %rdi
    callq *%r14
    /* The entry function returned, so this fiber is finished. The runtime
     * switches away and never comes back. */
    callq vise_fiber_finished
    /* Unreachable: vise_fiber_finished does not return. */
    ud2
    .cfi_endproc
    .size vise_context_entry, . - vise_context_entry

/* This object needs no executable stack. Without the note, the linker marks the
 * whole program's stack executable, which is both unnecessary and a hazard. */
    .section .note.GNU-stack,"",@progbits
