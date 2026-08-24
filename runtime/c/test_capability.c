/* Proves the capability gate actually confines, rather than merely installing.
 *
 * Each case runs in a forked child, because a seccomp filter cannot be removed
 * once installed. The parent inspects how the child died.
 *
 * Exits 0 if every case behaved; prints what failed otherwise.
 */

#define _GNU_SOURCE

#include "capability.h"

#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static int failures = 0;

static void fail(const char *what)
{
    fprintf(stderr, "FAIL: %s\n", what);
    failures++;
}

/* What happened to a child. */
typedef enum { CHILD_OK, CHILD_DENIED, CHILD_FAILED } outcome;

/* Run `body` under `effects` in a child and report how it ended. */
static outcome run_confined(uint32_t effects, void (*body)(void))
{
    pid_t pid = fork();
    if (pid < 0) {
        fail("fork");
        return CHILD_FAILED;
    }
    if (pid == 0) {
        vise_caps_result r = vise_caps_apply(effects);
        if (r != VISE_CAPS_OK) {
            /* Report unsupported distinctly from a policy denial. */
            _exit(3);
        }
        body();
        _exit(0);
    }

    int status = 0;
    if (waitpid(pid, &status, 0) < 0) {
        fail("waitpid");
        return CHILD_FAILED;
    }
    if (WIFSIGNALED(status) && WTERMSIG(status) == SIGSYS) {
        return CHILD_DENIED;
    }
    if (WIFEXITED(status) && WEXITSTATUS(status) == 0) {
        return CHILD_OK;
    }
    if (WIFEXITED(status) && WEXITSTATUS(status) == 3) {
        return CHILD_FAILED;
    }
    return CHILD_FAILED;
}

static void body_nothing(void) {}

static void body_open_socket(void)
{
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd >= 0) {
        close(fd);
    }
}

/* Deliberately the raw syscall. glibc's clock_gettime reads the vDSO and never
 * enters the kernel, so it would prove nothing about the filter. See the note
 * in capability.h: the vDSO is a gap this layer cannot close. */
static void body_read_clock(void)
{
    struct timespec ts;
    if (syscall(SYS_clock_gettime, CLOCK_MONOTONIC, &ts) < 0) {
        _exit(1);
    }
}

/* What a program actually gets when it calls the libc wrapper: the vDSO, which
 * no filter can intercept. Recorded as a test so the gap cannot quietly close
 * or quietly widen without someone noticing. */
static void body_read_clock_via_libc(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
}

static void body_getrandom(void)
{
    unsigned char buf[8];
    /* Deliberately the syscall, not the libc fallback, so the filter is what
     * decides rather than glibc quietly reading /dev/urandom. */
    if (syscall(SYS_getrandom, buf, sizeof buf, 0) < 0) {
        _exit(1);
    }
}

int main(void)
{
    /* A pure program must still be able to start and exit. */
    if (run_confined(0, body_nothing) != CHILD_OK) {
        fail("a pure program could not run under an empty effect row");
    }

    /* The point of the whole exercise: no `net`, no sockets. */
    if (run_confined(VISE_EFFECT_IO, body_open_socket) != CHILD_DENIED) {
        fail("socket() was permitted without the `net` effect");
    }

    /* And with `net`, it works. A gate that denies everything is not a gate. */
    if (run_confined(VISE_EFFECT_NET, body_open_socket) != CHILD_OK) {
        fail("socket() was denied despite the `net` effect");
    }

    /* Reading the clock is `time`, and only `time`. */
    if (run_confined(VISE_EFFECT_TIME, body_read_clock) != CHILD_OK) {
        fail("clock_gettime() was denied despite the `time` effect");
    }
    if (run_confined(VISE_EFFECT_NET, body_read_clock) != CHILD_DENIED) {
        fail("the clock_gettime syscall was permitted without the `time` effect");
    }

    /* Documents the vDSO gap rather than pretending it is not there: the libc
     * wrapper succeeds even with no `time` effect, because it never reaches the
     * kernel. If this ever starts being denied, the note in capability.h is out
     * of date and should be revisited. */
    if (run_confined(VISE_EFFECT_NET, body_read_clock_via_libc) != CHILD_OK) {
        fail("the vDSO note in capability.h is stale: libc clock_gettime was denied");
    }

    /* Determinism depends on this one: no `rand`, no entropy. */
    if (run_confined(VISE_EFFECT_RAND, body_getrandom) != CHILD_OK) {
        fail("getrandom() was denied despite the `rand` effect");
    }
    if (run_confined(VISE_EFFECT_TIME, body_getrandom) != CHILD_DENIED) {
        fail("getrandom() was permitted without the `rand` effect");
    }

    /* Widening the row may only add syscalls, never remove them. */
    unsigned pure = vise_caps_syscall_count(0);
    unsigned with_net = vise_caps_syscall_count(VISE_EFFECT_NET);
    unsigned everything = vise_caps_syscall_count(VISE_EFFECT_ALL);
    if (!(pure < with_net && with_net < everything)) {
        fail("the syscall set does not grow with the effect row");
    }
    if (vise_caps_syscall_count(VISE_EFFECT_ENV) != pure) {
        fail("`env` should grant no syscall");
    }

    if (failures == 0) {
        printf("capability gate: all cases behaved\n");
    }
    return failures == 0 ? 0 : 1;
}
