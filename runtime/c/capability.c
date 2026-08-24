/* The capability gate: a seccomp-bpf filter derived from an effect row. */

#define _GNU_SOURCE

#include "capability.h"

#include <errno.h>
#include <stddef.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <unistd.h>

#include <linux/audit.h>
#include <linux/filter.h>
#include <linux/seccomp.h>

#if defined(__x86_64__)
#define VISE_AUDIT_ARCH AUDIT_ARCH_X86_64
#elif defined(__aarch64__)
#define VISE_AUDIT_ARCH AUDIT_ARCH_AARCH64
#else
#define VISE_AUDIT_ARCH 0
#endif

/* Two filter instructions per allowed syscall, plus a fixed prologue and
 * epilogue. BPF programs are limited to 4096 instructions; this is far below
 * that, and the bound is checked rather than assumed. */
#define VISE_MAX_SYSCALLS 128

/* Syscalls every Vise program needs regardless of its effects: returning from
 * main, growing and releasing the heap, and the signal machinery the kernel
 * itself uses. Without these a pure function could not even exit. */
static const int BASE_SYSCALLS[] = {
    __NR_exit,        __NR_exit_group, __NR_brk,     __NR_mmap,
    __NR_munmap,      __NR_mprotect,   __NR_madvise, __NR_rt_sigreturn,
    __NR_rt_sigprocmask, __NR_futex,   __NR_sched_yield,
    /* Closing a descriptor is not an effect of its own: a program cannot close
     * what it was never allowed to open. Keeping it here means `net` alone is
     * enough to open and close a socket, rather than also requiring `fs`. */
    __NR_close,
};

/* stdin, stdout, stderr. `read` and `write` are also needed by `fs`, and the
 * kernel cannot tell the two apart by syscall number alone, which is noted in
 * the header as a known limit of filtering at this level. */
static const int IO_SYSCALLS[] = {
    __NR_read, __NR_write, __NR_readv, __NR_writev, __NR_fsync,
};

static const int FS_SYSCALLS[] = {
    __NR_openat, __NR_lseek,  __NR_statx,
    __NR_fstat,  __NR_ftruncate, __NR_getdents64, __NR_unlinkat,
    __NR_mkdirat, __NR_renameat2, __NR_fchmodat,
};

static const int NET_SYSCALLS[] = {
    __NR_socket,   __NR_connect,  __NR_accept4, __NR_bind,
    __NR_listen,   __NR_sendto,   __NR_recvfrom, __NR_sendmsg,
    __NR_recvmsg,  __NR_shutdown, __NR_setsockopt, __NR_getsockopt,
    __NR_getsockname, __NR_getpeername, __NR_ppoll,
};

static const int TIME_SYSCALLS[] = {
    __NR_clock_gettime, __NR_clock_nanosleep, __NR_nanosleep, __NR_clock_getres,
};

static const int RAND_SYSCALLS[] = {
    __NR_getrandom,
};

static const int PROC_SYSCALLS[] = {
    __NR_clone, __NR_execve, __NR_wait4, __NR_kill, __NR_pipe2,
};

struct group {
    uint32_t effect;
    const int *syscalls;
    unsigned count;
};

#define GROUP(bit, table) { (bit), (table), (unsigned)(sizeof(table) / sizeof((table)[0])) }

static const struct group GROUPS[] = {
    GROUP(VISE_EFFECT_IO, IO_SYSCALLS),
    GROUP(VISE_EFFECT_FS, FS_SYSCALLS),
    GROUP(VISE_EFFECT_NET, NET_SYSCALLS),
    GROUP(VISE_EFFECT_TIME, TIME_SYSCALLS),
    GROUP(VISE_EFFECT_RAND, RAND_SYSCALLS),
    GROUP(VISE_EFFECT_PROC, PROC_SYSCALLS),
    /* `env` grants no syscall: the environment is handed to the process on its
     * initial stack, so reading it is memory access, not a call into the
     * kernel. It is enforced by the compiler alone, which the header records as
     * a deliberate gap rather than an oversight. */
};

#define GROUP_COUNT (sizeof(GROUPS) / sizeof(GROUPS[0]))

/* Collect the allowed syscall numbers for `effects`, deduplicated.
 * Returns the count, or -1 if it would exceed VISE_MAX_SYSCALLS. */
static int collect(uint32_t effects, int *out)
{
    unsigned n = 0;

    for (unsigned i = 0; i < sizeof(BASE_SYSCALLS) / sizeof(BASE_SYSCALLS[0]); i++) {
        if (n >= VISE_MAX_SYSCALLS) return -1;
        out[n++] = BASE_SYSCALLS[i];
    }

    for (unsigned g = 0; g < GROUP_COUNT; g++) {
        if ((effects & GROUPS[g].effect) == 0) continue;
        for (unsigned i = 0; i < GROUPS[g].count; i++) {
            int nr = GROUPS[g].syscalls[i];
            int seen = 0;
            for (unsigned j = 0; j < n; j++) {
                if (out[j] == nr) { seen = 1; break; }
            }
            if (seen) continue;
            if (n >= VISE_MAX_SYSCALLS) return -1;
            out[n++] = nr;
        }
    }
    return (int)n;
}

unsigned vise_caps_syscall_count(uint32_t effects)
{
    int allowed[VISE_MAX_SYSCALLS];
    int n = collect(effects, allowed);
    return n < 0 ? 0u : (unsigned)n;
}

const char *vise_caps_strerror(vise_caps_result result)
{
    switch (result) {
    case VISE_CAPS_OK:                   return "ok";
    case VISE_CAPS_UNSUPPORTED:          return "the kernel does not support seccomp filtering";
    case VISE_CAPS_NO_NEW_PRIVS_FAILED:  return "could not set no-new-privs";
    case VISE_CAPS_FILTER_REJECTED:      return "the kernel rejected the filter";
    case VISE_CAPS_TOO_MANY:             return "too many syscalls for one filter";
    case VISE_CAPS_UNSUPPORTED_ARCH:     return "no capability gate for this architecture";
    }
    return "unknown";
}

vise_caps_result vise_caps_apply(uint32_t effects)
{
    if (VISE_AUDIT_ARCH == 0) {
        return VISE_CAPS_UNSUPPORTED_ARCH;
    }

    int allowed[VISE_MAX_SYSCALLS];
    int n = collect(effects, allowed);
    if (n < 0) {
        return VISE_CAPS_TOO_MANY;
    }

    /* Prologue (3) + load nr (1) + two per syscall + epilogue (1). */
    struct sock_filter program[4 + 2 * VISE_MAX_SYSCALLS + 1];
    unsigned p = 0;

    /* Refuse to run under a different personality than the one whose syscall
     * numbers this filter was built from. Without this check a 32-bit entry
     * point could reach a different call under the same number. */
    program[p++] = (struct sock_filter)BPF_STMT(
        BPF_LD | BPF_W | BPF_ABS, (uint32_t)offsetof(struct seccomp_data, arch));
    program[p++] = (struct sock_filter)BPF_JUMP(
        BPF_JMP | BPF_JEQ | BPF_K, VISE_AUDIT_ARCH, 1, 0);
    program[p++] = (struct sock_filter)BPF_STMT(
        BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS);

    program[p++] = (struct sock_filter)BPF_STMT(
        BPF_LD | BPF_W | BPF_ABS, (uint32_t)offsetof(struct seccomp_data, nr));

    /* One compare per syscall: equal falls through to ALLOW, otherwise skip it.
     * Keeping the jump offsets constant avoids arithmetic that could silently
     * go wrong as the tables change. */
    for (int i = 0; i < n; i++) {
        program[p++] = (struct sock_filter)BPF_JUMP(
            BPF_JMP | BPF_JEQ | BPF_K, (uint32_t)allowed[i], 0, 1);
        program[p++] = (struct sock_filter)BPF_STMT(
            BPF_RET | BPF_K, SECCOMP_RET_ALLOW);
    }

    /* Anything not named above ends the process. A program that has escaped its
     * declared effects does not get to decide what to do about it. */
    program[p++] = (struct sock_filter)BPF_STMT(
        BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS);

    struct sock_fprog fprog = {
        .len = (unsigned short)p,
        .filter = program,
    };

    /* Required before a filter may be installed by an unprivileged process:
     * without it, a filtered process could still gain privileges through a
     * setuid binary. */
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0) {
        return VISE_CAPS_NO_NEW_PRIVS_FAILED;
    }

    if (syscall(__NR_seccomp, SECCOMP_SET_MODE_FILTER, 0u, &fprog) != 0) {
        if (errno == EINVAL || errno == ENOSYS) {
            /* Older kernels only offer the prctl entry point. */
            if (prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &fprog) == 0) {
                return VISE_CAPS_OK;
            }
            return errno == EINVAL ? VISE_CAPS_UNSUPPORTED : VISE_CAPS_FILTER_REJECTED;
        }
        return VISE_CAPS_FILTER_REJECTED;
    }
    return VISE_CAPS_OK;
}
