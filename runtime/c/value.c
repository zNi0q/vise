/* Runtime values for compiled Vise. See value.h for the memory note. */

#define _GNU_SOURCE

#include <dirent.h>

#include "value.h"

#include <errno.h>
#include <inttypes.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

/* --- arena -------------------------------------------------------------- */

/* Blocks are chained so a long-running program grows rather than failing, and
 * the whole chain is released at once on exit. */
typedef struct block {
    struct block *next;
    size_t used;
    size_t capacity;
    unsigned char data[];
} block;

#define BLOCK_MIN (64u * 1024u)

static block *arena = NULL;

static block *new_block(size_t need)
{
    size_t capacity = need > BLOCK_MIN ? need : BLOCK_MIN;
    block *b = malloc(sizeof *b + capacity);
    if (b == NULL) {
        vise_trap("out of memory");
    }
    b->next = arena;
    b->used = 0;
    b->capacity = capacity;
    arena = b;
    return b;
}

void *vise_alloc(size_t bytes)
{
    /* Round up so every allocation is suitably aligned for any scalar. */
    bytes = (bytes + 15u) & ~(size_t)15u;
    if (arena == NULL || arena->capacity - arena->used < bytes) {
        new_block(bytes);
    }
    void *p = arena->data + arena->used;
    arena->used += bytes;
    return p;
}

void vise_runtime_shutdown(void)
{
    vise_list_unmap_all();
    block *b = arena;
    while (b != NULL) {
        block *next = b->next;
        free(b);
        b = next;
    }
    arena = NULL;
}

_Noreturn void vise_trap(const char *message)
{
    fflush(stdout);
    fprintf(stderr, "trap: %s\n", message);
    exit(1);
}

/* --- strings ------------------------------------------------------------ */

vise_str vise_str_literal(const char *bytes, size_t len)
{
    vise_str s = {bytes, len};
    return s;
}

vise_str vise_str_concat(vise_str a, vise_str b)
{
    char *out = vise_alloc(a.len + b.len + 1);
    memcpy(out, a.bytes, a.len);
    memcpy(out + a.len, b.bytes, b.len);
    out[a.len + b.len] = '\0';
    vise_str s = {out, a.len + b.len};
    return s;
}

/* Format into the arena. The buffer is sized for the widest of these. */
static vise_str formatted(const char *format, ...)
{
    char scratch[64];
    va_list args;
    va_start(args, format);
    int n = vsnprintf(scratch, sizeof scratch, format, args);
    va_end(args);
    if (n < 0) {
        vise_trap("could not format a value");
    }
    size_t len = (size_t)n < sizeof scratch ? (size_t)n : sizeof scratch - 1;
    char *out = vise_alloc(len + 1);
    memcpy(out, scratch, len);
    out[len] = '\0';
    vise_str s = {out, len};
    return s;
}

vise_str vise_str_from_int(int64_t value)
{
    return formatted("%" PRId64, value);
}

vise_str vise_str_from_float(double value)
{
    /* Always with a decimal point, so a Float never reads as an Int. This
     * matches how the interpreter prints, so a program gives the same output
     * whether it was interpreted or compiled. */
    if (value == (double)(int64_t)value && value > -1e18 && value < 1e18) {
        return formatted("%.1f", value);
    }
    return formatted("%.17g", value);
}

vise_str vise_str_from_bool(bool value)
{
    return vise_str_literal(value ? "true" : "false", value ? 4u : 5u);
}

vise_str vise_str_from_char(uint32_t value)
{
    /* UTF-8, so a Char round-trips through a Str unchanged. */
    char buffer[4];
    size_t len;
    if (value < 0x80u) {
        buffer[0] = (char)value;
        len = 1;
    } else if (value < 0x800u) {
        buffer[0] = (char)(0xc0u | (value >> 6));
        buffer[1] = (char)(0x80u | (value & 0x3fu));
        len = 2;
    } else if (value < 0x10000u) {
        buffer[0] = (char)(0xe0u | (value >> 12));
        buffer[1] = (char)(0x80u | ((value >> 6) & 0x3fu));
        buffer[2] = (char)(0x80u | (value & 0x3fu));
        len = 3;
    } else {
        buffer[0] = (char)(0xf0u | (value >> 18));
        buffer[1] = (char)(0x80u | ((value >> 12) & 0x3fu));
        buffer[2] = (char)(0x80u | ((value >> 6) & 0x3fu));
        buffer[3] = (char)(0x80u | (value & 0x3fu));
        len = 4;
    }
    char *out = vise_alloc(len + 1);
    memcpy(out, buffer, len);
    out[len] = '\0';
    vise_str s = {out, len};
    return s;
}

bool vise_str_eq(vise_str a, vise_str b)
{
    return a.len == b.len && memcmp(a.bytes, b.bytes, a.len) == 0;
}

int vise_str_cmp(vise_str a, vise_str b)
{
    size_t shorter = a.len < b.len ? a.len : b.len;
    int order = memcmp(a.bytes, b.bytes, shorter);
    if (order != 0) {
        return order;
    }
    if (a.len == b.len) {
        return 0;
    }
    return a.len < b.len ? -1 : 1;
}

/* --- lists -------------------------------------------------------------- */

/* Every list buffer carries this immediately before its items.
 *
 * `capacity` is how many slots the buffer holds; `filled` is how many of them
 * some list has already claimed. A list is a pointer and a length, and several
 * lists can share one buffer, so neither number can live in the list itself.
 *
 * The pair is what makes append cheap without making it lie. §9 requires the
 * original list to be untouched by an append, and it is: appending to a list
 * whose length equals `filled` writes into a slot no list has claimed yet, and
 * every existing list keeps exactly the elements it had. Appending to a shorter
 * list -- one someone kept a copy of and then appended to twice -- would
 * overwrite a claimed slot, so that case copies instead. */
typedef struct {
    int64_t capacity;
    int64_t filled;
    /* Bytes of mapping this buffer owns, or zero if it came from the arena.
     * See reserve() for why a large buffer gets a mapping of its own. */
    size_t reserved;
} list_header;

/* Buffers at least this large are mapped rather than taken from the arena. It
 * is the arena's block size, so nothing that fits comfortably in a block pays
 * for a system call. */
#define LIST_MAPPED_AT (64u * 1024u)

/* Every mapping made below, so shutdown can return them. */
typedef struct mapping {
    struct mapping *next;
    void *base;
    size_t bytes;
} mapping;

static mapping *mappings = NULL;

/* A list that is appended to repeatedly outgrows whatever buffer it has. Moving
 * it to a larger one each time costs the copy, and costs the abandoned buffer
 * too, which the arena never reclaims: built that way, a list of twenty million
 * elements touches 410MB to end up holding 160MB.
 *
 * So a large buffer is mapped, with far more address space reserved than it
 * needs. Reserved pages are not committed until something writes to them, so
 * the reservation costs address space and nothing else, and the buffer grows
 * into it without ever moving. Not moving is the point: other lists may share
 * this buffer, and §9 promises they still hold what they held.
 *
 * The reservation is sixty-four times what is asked for, which covers six
 * doublings, and is capped so that one list cannot reserve without limit. */
static size_t reserve(size_t bytes)
{
    const size_t cap = (size_t)1 << 34; /* 16GB of address space, no memory */
    size_t want = bytes > cap / 64 ? cap : bytes * 64;
    return want < LIST_MAPPED_AT ? LIST_MAPPED_AT : want;
}

static list_header *map_buffer(size_t bytes, size_t *reserved)
{
    size_t want = reserve(bytes);
    void *base = mmap(NULL, want, PROT_READ | PROT_WRITE,
                      MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
    if (base == MAP_FAILED) {
        return NULL;
    }
    /* Recorded so shutdown can unmap it. The record itself is arena memory. */
    mapping *record = vise_alloc(sizeof *record);
    record->next = mappings;
    record->base = base;
    record->bytes = want;
    mappings = record;
    *reserved = want;
    return base;
}

void vise_list_unmap_all(void)
{
    for (mapping *m = mappings; m != NULL; m = m->next) {
        munmap(m->base, m->bytes);
    }
    mappings = NULL;
}

static vise_list list_with_capacity(int64_t len, int64_t capacity)
{
    size_t bytes = sizeof(list_header) + (size_t)capacity * sizeof(vise_slot);
    size_t reserved = 0;
    list_header *header = NULL;
    if (bytes >= LIST_MAPPED_AT) {
        header = map_buffer(bytes, &reserved);
    }
    if (header == NULL) {
        header = vise_alloc(bytes);
        reserved = 0;
    }
    header->capacity = capacity;
    header->filled = len;
    header->reserved = reserved;

    vise_list list;
    list.len = len;
    list.items = (vise_slot *)(header + 1);
    return list;
}

vise_list vise_list_new(int64_t len)
{
    return list_with_capacity(len, len);
}

/* --- output ------------------------------------------------------------- */

void vise_print(vise_str text)
{
    fwrite(text.bytes, 1, text.len, stdout);
    fputc('\n', stdout);
}

/* --- enums -------------------------------------------------------------- */

static vise_enum tagged(uint32_t tag)
{
    vise_enum e;
    memset(&e, 0, sizeof e);
    e.tag = tag;
    return e;
}

/* A list slot is one slot wide whatever it holds, so anything larger goes on
 * the heap -- exactly as generated code does it. */
static void *boxed(const void *value, size_t size)
{
    void *p = vise_alloc(size);
    memcpy(p, value, size);
    return p;
}

/* An enum payload is not indexed by element, so a value wider than one slot
 * spreads across as many as it needs instead of being boxed. Generated code
 * lays it out the same way, so a Result built here is indistinguishable from
 * one built in Vise. `memcpy` rather than a cast, because writing a vise_str
 * through a vise_slot is not something the aliasing rules allow. */
static void wide(vise_enum *e, const void *value, size_t size)
{
    memcpy(&e->f[0], value, size);
}

vise_enum vise_ok_unit(void)
{
    return tagged(VISE_TAG_OK);
}

vise_enum vise_ok_int(int64_t value)
{
    vise_enum e = tagged(VISE_TAG_OK);
    e.f[0].as_int = value;
    return e;
}

vise_enum vise_ok_str(vise_str value)
{
    vise_enum e = tagged(VISE_TAG_OK);
    wide(&e, &value, sizeof value);
    return e;
}

vise_enum vise_ok_list(vise_list value)
{
    vise_enum e = tagged(VISE_TAG_OK);
    wide(&e, &value, sizeof value);
    return e;
}

vise_enum vise_err_str(vise_str message)
{
    vise_enum e = tagged(VISE_TAG_ERR);
    wide(&e, &message, sizeof message);
    return e;
}

vise_enum vise_some_int(int64_t value)
{
    vise_enum e = tagged(VISE_TAG_SOME);
    e.f[0].as_int = value;
    return e;
}

vise_enum vise_none(void)
{
    return tagged(VISE_TAG_NONE);
}

/* --- lists -------------------------------------------------------------- */

vise_list vise_list_append(vise_list list, vise_slot item)
{
    /* The common shape -- a list built by appending, one element at a time --
     * extends the buffer in place and costs nothing but the write. Without
     * this, building a list of n elements copies n(n-1)/2 slots and allocates
     * every one of them, which is what a directory walk cannot survive. */
    list_header *header = (list_header *)list.items - 1;
    if (list.len == header->filled && header->filled < header->capacity) {
        list.items[list.len] = item;
        header->filled = list.len + 1;
        list.len += 1;
        return list;
    }

    int64_t capacity = list.len < 4 ? 4 : list.len * 2;

    /* A buffer that is full but has reservation left grows into it, which is
     * the whole reason large buffers are mapped: no copy, no second buffer. */
    if (list.len == header->filled && header->reserved > 0) {
        size_t want = sizeof(list_header) + (size_t)capacity * sizeof(vise_slot);
        if (want <= header->reserved) {
            header->capacity = capacity;
            list.items[list.len] = item;
            header->filled = list.len + 1;
            list.len += 1;
            return list;
        }
    }

    /* Otherwise copy. Doubling keeps the appends that follow this one cheap. */
    vise_list out = list_with_capacity(list.len + 1, capacity);
    if (list.len > 0) {
        memcpy(out.items, list.items, (size_t)list.len * sizeof(vise_slot));
    }
    out.items[list.len] = item;
    return out;
}

/* Put a string into a list slot, which means boxing it. */
static vise_slot slot_of_str(vise_str s)
{
    vise_slot slot;
    slot.as_ptr = boxed(&s, sizeof s);
    return slot;
}

static vise_str owned(const char *bytes, size_t len)
{
    char *copy = vise_alloc(len + 1);
    memcpy(copy, bytes, len);
    copy[len] = '\0';
    vise_str s = {copy, len};
    return s;
}

/* --- strings ------------------------------------------------------------ */

int64_t vise_str_length(vise_str text)
{
    /* Characters, not bytes: a Str is UTF-8 and its length should not depend on
     * which characters it happens to hold. */
    int64_t count = 0;
    for (size_t i = 0; i < text.len; i++) {
        if ((text.bytes[i] & 0xc0) != 0x80) {
            count++;
        }
    }
    return count;
}

vise_list vise_lines(vise_str text)
{
    /* A trailing newline ends the last line; it does not begin an empty one. */
    int64_t count = 0;
    for (size_t i = 0; i < text.len; i++) {
        if (text.bytes[i] == '\n') {
            count++;
        }
    }
    if (text.len > 0 && text.bytes[text.len - 1] != '\n') {
        count++;
    }

    vise_list out = vise_list_new(count);
    int64_t n = 0;
    size_t start = 0;
    for (size_t i = 0; i < text.len; i++) {
        if (text.bytes[i] != '\n') {
            continue;
        }
        size_t end = i;
        /* Tolerate CRLF, so a file written on another system reads the same. */
        if (end > start && text.bytes[end - 1] == '\r') {
            end--;
        }
        out.items[n++] = slot_of_str(owned(text.bytes + start, end - start));
        start = i + 1;
    }
    if (start < text.len) {
        out.items[n++] = slot_of_str(owned(text.bytes + start, text.len - start));
    }
    out.len = n;
    return out;
}

/* First occurrence of `needle` in `haystack` at or after `from`, or -1. */
static long find_from(vise_str haystack, vise_str needle, size_t from)
{
    if (needle.len == 0 || needle.len > haystack.len) {
        return -1;
    }
    for (size_t i = from; i + needle.len <= haystack.len; i++) {
        if (memcmp(haystack.bytes + i, needle.bytes, needle.len) == 0) {
            return (long)i;
        }
    }
    return -1;
}

vise_list vise_split(vise_str text, vise_str separator)
{
    if (separator.len == 0) {
        /* Splitting on nothing yields the characters, matching the
         * interpreter. */
        vise_list out = vise_list_new(vise_str_length(text));
        int64_t n = 0;
        size_t i = 0;
        while (i < text.len) {
            size_t width = 1;
            while (i + width < text.len && (text.bytes[i + width] & 0xc0) == 0x80) {
                width++;
            }
            out.items[n++] = slot_of_str(owned(text.bytes + i, width));
            i += width;
        }
        out.len = n;
        return out;
    }

    int64_t count = 1;
    long at = find_from(text, separator, 0);
    while (at >= 0) {
        count++;
        at = find_from(text, separator, (size_t)at + separator.len);
    }

    vise_list out = vise_list_new(count);
    int64_t n = 0;
    size_t start = 0;
    at = find_from(text, separator, 0);
    while (at >= 0) {
        out.items[n++] = slot_of_str(owned(text.bytes + start, (size_t)at - start));
        start = (size_t)at + separator.len;
        at = find_from(text, separator, start);
    }
    out.items[n++] = slot_of_str(owned(text.bytes + start, text.len - start));
    out.len = n;
    return out;
}

vise_str vise_join(vise_list parts, vise_str separator)
{
    if (parts.len <= 0) {
        return vise_str_literal("", 0);
    }
    size_t total = separator.len * (size_t)(parts.len - 1);
    for (int64_t i = 0; i < parts.len; i++) {
        total += ((const vise_str *)parts.items[i].as_ptr)->len;
    }

    char *out = vise_alloc(total + 1);
    size_t at = 0;
    for (int64_t i = 0; i < parts.len; i++) {
        if (i > 0) {
            memcpy(out + at, separator.bytes, separator.len);
            at += separator.len;
        }
        const vise_str *piece = parts.items[i].as_ptr;
        memcpy(out + at, piece->bytes, piece->len);
        at += piece->len;
    }
    out[at] = '\0';
    vise_str s = {out, at};
    return s;
}

bool vise_starts_with(vise_str text, vise_str prefix)
{
    return prefix.len <= text.len && memcmp(text.bytes, prefix.bytes, prefix.len) == 0;
}

bool vise_contains(vise_str text, vise_str needle)
{
    return needle.len == 0 || find_from(text, needle, 0) >= 0;
}

vise_enum vise_parse_int(vise_str text)
{
    /* Trim, then require the whole of what is left to be the number: a string
     * that is only partly a number is not one. */
    size_t start = 0;
    size_t end = text.len;
    while (start < end && (text.bytes[start] == ' ' || text.bytes[start] == '\t' ||
                           text.bytes[start] == '\n' || text.bytes[start] == '\r')) {
        start++;
    }
    while (end > start && (text.bytes[end - 1] == ' ' || text.bytes[end - 1] == '\t' ||
                           text.bytes[end - 1] == '\n' || text.bytes[end - 1] == '\r')) {
        end--;
    }
    if (start == end) {
        return vise_none();
    }

    size_t i = start;
    bool negative = false;
    if (text.bytes[i] == '-' || text.bytes[i] == '+') {
        negative = text.bytes[i] == '-';
        i++;
    }
    if (i == end) {
        return vise_none();
    }

    int64_t value = 0;
    for (; i < end; i++) {
        char c = text.bytes[i];
        if (c < '0' || c > '9') {
            return vise_none();
        }
        int64_t digit = c - '0';
        /* Overflow is not a number this Int can hold, so it is None rather
         * than a trap: parsing is allowed to fail. */
        if (__builtin_mul_overflow(value, (int64_t)10, &value) ||
            __builtin_add_overflow(value, digit, &value)) {
            return vise_none();
        }
    }
    return vise_some_int(negative ? -value : value);
}

/* --- filesystem --------------------------------------------------------- */

/* A path arrives as a Str, which is not guaranteed to be NUL-terminated. */
static const char *as_path(vise_str path)
{
    char *out = vise_alloc(path.len + 1);
    memcpy(out, path.bytes, path.len);
    out[path.len] = '\0';
    return out;
}

static vise_enum errno_error(void)
{
    const char *message = strerror(errno);
    return vise_err_str(owned(message, strlen(message)));
}

vise_enum vise_read_file(vise_str path)
{
    FILE *f = fopen(as_path(path), "rb");
    if (f == NULL) {
        return errno_error();
    }
    size_t capacity = 4096;
    size_t len = 0;
    char *buffer = vise_alloc(capacity);
    for (;;) {
        if (len == capacity) {
            size_t bigger = capacity * 2;
            char *grown = vise_alloc(bigger);
            memcpy(grown, buffer, len);
            buffer = grown;
            capacity = bigger;
        }
        size_t got = fread(buffer + len, 1, capacity - len, f);
        len += got;
        if (got == 0) {
            break;
        }
    }
    int failed = ferror(f);
    fclose(f);
    if (failed) {
        return errno_error();
    }
    vise_str s = {buffer, len};
    return vise_ok_str(s);
}

vise_enum vise_write_file(vise_str path, vise_str contents)
{
    FILE *f = fopen(as_path(path), "wb");
    if (f == NULL) {
        return errno_error();
    }
    size_t written = fwrite(contents.bytes, 1, contents.len, f);
    int failed = ferror(f) || written != contents.len;
    if (fclose(f) != 0) {
        failed = 1;
    }
    return failed ? errno_error() : vise_ok_unit();
}

vise_enum vise_list_dir(vise_str path)
{
    DIR *dir = opendir(as_path(path));
    if (dir == NULL) {
        return errno_error();
    }
    vise_list out = vise_list_new(0);
    struct dirent *entry;
    while ((entry = readdir(dir)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }
        out = vise_list_append(out, slot_of_str(owned(entry->d_name, strlen(entry->d_name))));
    }
    closedir(dir);

    /* Sorted, because §11 says a program's output must not depend on the order
     * a filesystem happens to report. Insertion sort: directories are small,
     * and this keeps the comparison identical to the interpreter's. */
    for (int64_t i = 1; i < out.len; i++) {
        vise_slot key = out.items[i];
        const vise_str *ks = key.as_ptr;
        int64_t j = i - 1;
        while (j >= 0) {
            const vise_str *js = out.items[j].as_ptr;
            if (vise_str_cmp(*js, *ks) <= 0) {
                break;
            }
            out.items[j + 1] = out.items[j];
            j--;
        }
        out.items[j + 1] = key;
    }
    return vise_ok_list(out);
}

bool vise_is_dir(vise_str path)
{
    /* lstat, not stat: a symbolic link to a directory is a link, not a
     * directory. A walker that believed otherwise would descend through it,
     * and a link pointing at an ancestor would make the walk unbounded. */
    struct stat info;
    return lstat(as_path(path), &info) == 0 && S_ISDIR(info.st_mode);
}

vise_enum vise_file_size(vise_str path)
{
    struct stat info;
    if (stat(as_path(path), &info) != 0) {
        return errno_error();
    }
    return vise_ok_int((int64_t)info.st_size);
}

/* --- process ------------------------------------------------------------ */

static int stored_argc = 0;
static char **stored_argv = NULL;

void vise_set_args(int argc, char **argv)
{
    stored_argc = argc;
    stored_argv = argv;
}

vise_list vise_args(void)
{
    /* The program's own name is not an argument, so it is skipped. */
    int64_t count = stored_argc > 1 ? stored_argc - 1 : 0;
    vise_list out = vise_list_new(count);
    for (int64_t i = 0; i < count; i++) {
        const char *arg = stored_argv[i + 1];
        out.items[i] = slot_of_str(owned(arg, strlen(arg)));
    }
    return out;
}

int64_t vise_now(void)
{
    return (int64_t)time(NULL);
}

_Noreturn void vise_exit(int64_t code)
{
    fflush(stdout);
    vise_runtime_shutdown();
    exit((int)(code & 0xff));
}
