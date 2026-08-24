/* Recording and replaying nondeterminism.
 *
 * Spec §11: "Given the same inputs and the same recorded trace, a Vise program
 * produces byte-identical output. `time` and `rand` are effects precisely so
 * they can be captured and replayed."
 *
 * This is the capture. Every value that enters a program from outside is
 * appended to a trace as it is produced; on replay the same values are handed
 * back in the same order. A regression becomes a trace diff rather than a
 * judgement call.
 *
 * Divergence is an error, not a warning. If a replayed program asks for
 * something the recording did not contain, or asks in a different order, it is
 * no longer the same program and the run stops.
 */
#ifndef VISE_TRACE_H
#define VISE_TRACE_H

#include <stddef.h>
#include <stdint.h>

typedef struct vise_trace vise_trace;

/* What kind of external value a record holds. These are the effects that can
 * introduce nondeterminism; `fs` and `net` reads use VISE_TRACE_READ. */
typedef enum {
    VISE_TRACE_TIME = 1,
    VISE_TRACE_RANDOM = 2,
    VISE_TRACE_READ = 3,
    VISE_TRACE_ENV = 4,
} vise_trace_tag;

typedef enum {
    VISE_TRACE_OK = 0,
    VISE_TRACE_IO_ERROR = 1,
    /* The file is not a trace, or its version is not understood. */
    VISE_TRACE_MALFORMED = 2,
    /* Replay asked for something the recording does not contain, or asked in a
     * different order. The program has diverged. */
    VISE_TRACE_DIVERGED = 3,
    /* Replay ran past the end of the recording. */
    VISE_TRACE_EXHAUSTED = 4,
    /* A value larger than the caller's buffer. */
    VISE_TRACE_TOO_LARGE = 5,
} vise_trace_result;

/* Begin recording to `path`, truncating anything already there. */
vise_trace *vise_trace_record(const char *path);

/* Begin replaying from `path`. Returns NULL if it cannot be opened or is not a
 * trace. */
vise_trace *vise_trace_replay(const char *path);

/* Whether this trace is being replayed rather than recorded. */
int vise_trace_is_replay(const vise_trace *trace);

/* How many records have been written or consumed. */
uint64_t vise_trace_position(const vise_trace *trace);

/* Record or replay one external value.
 *
 * While recording, `buffer` holds the value the outside world produced and
 * `len` its length; the value is appended.
 *
 * While replaying, the next record must carry `tag`, and its bytes are copied
 * into `buffer`. `*len` is updated to the recorded length. Anything else is a
 * divergence.
 */
vise_trace_result vise_trace_value(vise_trace *trace, vise_trace_tag tag,
                                   void *buffer, size_t *len);

/* Flush and close. Returns whether everything was written. */
vise_trace_result vise_trace_close(vise_trace *trace);

const char *vise_trace_strerror(vise_trace_result result);

#endif /* VISE_TRACE_H */
