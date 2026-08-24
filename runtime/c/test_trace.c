/* Trace record and replay. Exits 0 if every case behaved. */

#define _GNU_SOURCE

#include "trace.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int failures = 0;

static void fail(const char *what)
{
    fprintf(stderr, "FAIL: %s\n", what);
    failures++;
}

static char path[] = "/tmp/vise-trace-testXXXXXX";

static void record_a_run(void)
{
    vise_trace *t = vise_trace_record(path);
    if (t == NULL) {
        fail("could not begin recording");
        return;
    }
    if (vise_trace_is_replay(t)) {
        fail("a recording reported itself as a replay");
    }

    int64_t now = 1700000000;
    size_t n = sizeof now;
    if (vise_trace_value(t, VISE_TRACE_TIME, &now, &n) != VISE_TRACE_OK) {
        fail("recording a clock read");
    }

    unsigned char entropy[4] = {0xde, 0xad, 0xbe, 0xef};
    n = sizeof entropy;
    if (vise_trace_value(t, VISE_TRACE_RANDOM, entropy, &n) != VISE_TRACE_OK) {
        fail("recording random bytes");
    }

    if (vise_trace_position(t) != 2) {
        fail("the recording did not count its records");
    }
    if (vise_trace_close(t) != VISE_TRACE_OK) {
        fail("closing the recording");
    }
}

int main(void)
{
    int fd = mkstemp(path);
    if (fd < 0) {
        fail("could not make a temporary file");
        return 1;
    }
    close(fd);

    record_a_run();

    /* The same values come back, in the same order. */
    vise_trace *t = vise_trace_replay(path);
    if (t == NULL) {
        fail("could not begin replaying");
        goto done;
    }
    if (!vise_trace_is_replay(t)) {
        fail("a replay reported itself as a recording");
    }

    int64_t now = 0;
    size_t n = sizeof now;
    if (vise_trace_value(t, VISE_TRACE_TIME, &now, &n) != VISE_TRACE_OK) {
        fail("replaying a clock read");
    }
    if (now != 1700000000 || n != sizeof now) {
        fail("the replayed clock value differed from the recorded one");
    }

    unsigned char entropy[4] = {0};
    n = sizeof entropy;
    if (vise_trace_value(t, VISE_TRACE_RANDOM, entropy, &n) != VISE_TRACE_OK) {
        fail("replaying random bytes");
    }
    if (memcmp(entropy, "\xde\xad\xbe\xef", 4) != 0) {
        fail("the replayed random bytes differed from the recorded ones");
    }

    /* Running past the end of a recording is an error, not silence. */
    n = sizeof now;
    if (vise_trace_value(t, VISE_TRACE_TIME, &now, &n) != VISE_TRACE_EXHAUSTED) {
        fail("reading past the end of a recording was not reported");
    }
    vise_trace_close(t);

    /* Asking for a different kind of value than was recorded means the program
     * took a different path, which is divergence. */
    t = vise_trace_replay(path);
    if (t == NULL) {
        fail("could not reopen the recording");
        goto done;
    }
    n = sizeof entropy;
    if (vise_trace_value(t, VISE_TRACE_RANDOM, entropy, &n) != VISE_TRACE_DIVERGED) {
        fail("a divergent request was accepted");
    }
    /* And once diverged, it stays diverged rather than resynchronising. */
    n = sizeof now;
    if (vise_trace_value(t, VISE_TRACE_TIME, &now, &n) != VISE_TRACE_DIVERGED) {
        fail("a diverged trace resynchronised");
    }
    if (vise_trace_close(t) != VISE_TRACE_DIVERGED) {
        fail("closing a diverged trace reported success");
    }

    /* A buffer too small for a recorded value is reported rather than
     * truncated: a silently short value would replay as a different program. */
    t = vise_trace_replay(path);
    if (t != NULL) {
        unsigned char tiny[2];
        n = sizeof tiny;
        if (vise_trace_value(t, VISE_TRACE_TIME, tiny, &n) != VISE_TRACE_TOO_LARGE) {
            fail("an oversized value was not reported");
        }
        vise_trace_close(t);
    }

    /* Anything that is not a trace is refused rather than misread. */
    FILE *junk = fopen(path, "wb");
    if (junk != NULL) {
        fputs("not a trace at all", junk);
        fclose(junk);
        if (vise_trace_replay(path) != NULL) {
            fail("a file that is not a trace was accepted");
        }
    }

done:
    unlink(path);
    if (failures == 0) {
        printf("trace: all cases behaved\n");
    }
    return failures == 0 ? 0 : 1;
}
