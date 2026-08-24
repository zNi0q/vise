/* Runtime values for compiled Vise. See value.h for the memory note. */

#define _GNU_SOURCE

#include "value.h"

#include <inttypes.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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

vise_list vise_list_new(int64_t len)
{
    vise_list list;
    list.len = len;
    list.items = len > 0 ? vise_alloc((size_t)len * sizeof(vise_slot)) : NULL;
    return list;
}

vise_slot vise_list_get(vise_list list, int64_t index)
{
    if (index < 0 || index >= list.len) {
        vise_trap("index out of bounds");
    }
    return list.items[index];
}

/* --- trapping arithmetic ------------------------------------------------ */

int64_t vise_add(int64_t a, int64_t b)
{
    int64_t out;
    if (__builtin_add_overflow(a, b, &out)) {
        vise_trap("integer overflow in `+`");
    }
    return out;
}

int64_t vise_sub(int64_t a, int64_t b)
{
    int64_t out;
    if (__builtin_sub_overflow(a, b, &out)) {
        vise_trap("integer overflow in `-`");
    }
    return out;
}

int64_t vise_mul(int64_t a, int64_t b)
{
    int64_t out;
    if (__builtin_mul_overflow(a, b, &out)) {
        vise_trap("integer overflow in `*`");
    }
    return out;
}

int64_t vise_div(int64_t a, int64_t b)
{
    if (b == 0) {
        vise_trap("division by zero");
    }
    /* The one division that overflows: the most negative value by -1. */
    if (a == INT64_MIN && b == -1) {
        vise_trap("integer overflow in `/`");
    }
    return a / b;
}

int64_t vise_rem(int64_t a, int64_t b)
{
    if (b == 0) {
        vise_trap("division by zero");
    }
    if (a == INT64_MIN && b == -1) {
        vise_trap("integer overflow in `%`");
    }
    return a % b;
}

int64_t vise_neg(int64_t a)
{
    if (a == INT64_MIN) {
        vise_trap("integer overflow in `-`");
    }
    return -a;
}

/* --- output ------------------------------------------------------------- */

void vise_print(vise_str text)
{
    fwrite(text.bytes, 1, text.len, stdout);
    fputc('\n', stdout);
}
