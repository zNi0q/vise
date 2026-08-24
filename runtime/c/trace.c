/* Trace record and replay.
 *
 * The format is deliberately plain: a header, then a sequence of records, each
 * a tag, a length, and that many bytes. Everything is little-endian and fixed
 * width, so a trace recorded on one machine reads identically on another --
 * which is the whole point of recording one.
 */

#define _GNU_SOURCE

#include "trace.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* "VISETRC\0", little-endian. Version travels separately so a format change is
 * a clean refusal rather than a misread. */
static const unsigned char MAGIC[8] = {'V', 'I', 'S', 'E', 'T', 'R', 'C', 0};
#define TRACE_VERSION 1u

/* A single record may not exceed this. Traces hold clock reads, random bytes,
 * and short IO results; anything larger is a mistake rather than a value. */
#define MAX_RECORD (1u << 20)

struct vise_trace {
    FILE *file;
    int replaying;
    uint64_t position;
    vise_trace_result failure;
};

static int write_u32(FILE *f, uint32_t v)
{
    unsigned char b[4] = {
        (unsigned char)(v & 0xffu),
        (unsigned char)((v >> 8) & 0xffu),
        (unsigned char)((v >> 16) & 0xffu),
        (unsigned char)((v >> 24) & 0xffu),
    };
    return fwrite(b, 1, sizeof b, f) == sizeof b;
}

static int read_u32(FILE *f, uint32_t *out)
{
    unsigned char b[4];
    if (fread(b, 1, sizeof b, f) != sizeof b) {
        return 0;
    }
    *out = (uint32_t)b[0] | ((uint32_t)b[1] << 8) | ((uint32_t)b[2] << 16) |
           ((uint32_t)b[3] << 24);
    return 1;
}

const char *vise_trace_strerror(vise_trace_result result)
{
    switch (result) {
    case VISE_TRACE_OK:         return "ok";
    case VISE_TRACE_IO_ERROR:   return "the trace could not be read or written";
    case VISE_TRACE_MALFORMED:  return "the file is not a trace this build understands";
    case VISE_TRACE_DIVERGED:   return "the program diverged from its recording";
    case VISE_TRACE_EXHAUSTED:  return "the recording ended before the program did";
    case VISE_TRACE_TOO_LARGE:  return "a recorded value is larger than the buffer for it";
    }
    return "unknown";
}

vise_trace *vise_trace_record(const char *path)
{
    FILE *f = fopen(path, "wb");
    if (f == NULL) {
        return NULL;
    }
    if (fwrite(MAGIC, 1, sizeof MAGIC, f) != sizeof MAGIC || !write_u32(f, TRACE_VERSION)) {
        fclose(f);
        return NULL;
    }
    vise_trace *t = calloc(1, sizeof *t);
    if (t == NULL) {
        fclose(f);
        return NULL;
    }
    t->file = f;
    t->replaying = 0;
    return t;
}

vise_trace *vise_trace_replay(const char *path)
{
    FILE *f = fopen(path, "rb");
    if (f == NULL) {
        return NULL;
    }
    unsigned char magic[sizeof MAGIC];
    uint32_t version = 0;
    if (fread(magic, 1, sizeof magic, f) != sizeof magic ||
        memcmp(magic, MAGIC, sizeof MAGIC) != 0 || !read_u32(f, &version) ||
        version != TRACE_VERSION) {
        fclose(f);
        return NULL;
    }
    vise_trace *t = calloc(1, sizeof *t);
    if (t == NULL) {
        fclose(f);
        return NULL;
    }
    t->file = f;
    t->replaying = 1;
    return t;
}

int vise_trace_is_replay(const vise_trace *trace)
{
    return trace != NULL && trace->replaying;
}

uint64_t vise_trace_position(const vise_trace *trace)
{
    return trace == NULL ? 0u : trace->position;
}

vise_trace_result vise_trace_value(vise_trace *trace, vise_trace_tag tag,
                                   void *buffer, size_t *len)
{
    if (trace == NULL || buffer == NULL || len == NULL) {
        return VISE_TRACE_IO_ERROR;
    }
    /* Once a trace has failed it stays failed: continuing would produce a
     * recording that cannot be replayed, or a replay that is no longer
     * faithful. */
    if (trace->failure != VISE_TRACE_OK) {
        return trace->failure;
    }
    if (*len > MAX_RECORD) {
        trace->failure = VISE_TRACE_TOO_LARGE;
        return trace->failure;
    }

    if (!trace->replaying) {
        if (!write_u32(trace->file, (uint32_t)tag) ||
            !write_u32(trace->file, (uint32_t)*len) ||
            fwrite(buffer, 1, *len, trace->file) != *len) {
            trace->failure = VISE_TRACE_IO_ERROR;
            return trace->failure;
        }
        trace->position++;
        return VISE_TRACE_OK;
    }

    uint32_t recorded_tag = 0;
    uint32_t recorded_len = 0;
    if (!read_u32(trace->file, &recorded_tag)) {
        trace->failure = VISE_TRACE_EXHAUSTED;
        return trace->failure;
    }
    if (!read_u32(trace->file, &recorded_len)) {
        trace->failure = VISE_TRACE_MALFORMED;
        return trace->failure;
    }
    /* Asking for a different kind of value than was recorded means the program
     * took a different path. That is divergence, not a recoverable mismatch. */
    if (recorded_tag != (uint32_t)tag) {
        trace->failure = VISE_TRACE_DIVERGED;
        return trace->failure;
    }
    if (recorded_len > MAX_RECORD) {
        trace->failure = VISE_TRACE_MALFORMED;
        return trace->failure;
    }
    if (recorded_len > *len) {
        trace->failure = VISE_TRACE_TOO_LARGE;
        return trace->failure;
    }
    if (recorded_len > 0 && fread(buffer, 1, recorded_len, trace->file) != recorded_len) {
        trace->failure = VISE_TRACE_MALFORMED;
        return trace->failure;
    }
    *len = recorded_len;
    trace->position++;
    return VISE_TRACE_OK;
}

vise_trace_result vise_trace_close(vise_trace *trace)
{
    if (trace == NULL) {
        return VISE_TRACE_OK;
    }
    vise_trace_result result = trace->failure;
    if (trace->file != NULL) {
        if (!trace->replaying && fflush(trace->file) != 0 && result == VISE_TRACE_OK) {
            result = VISE_TRACE_IO_ERROR;
        }
        fclose(trace->file);
    }
    free(trace);
    return result;
}
