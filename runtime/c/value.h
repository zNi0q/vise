/* Runtime values for compiled Vise.
 *
 * Vise is statically typed, so most values need no runtime representation at
 * all: an Int is an int64_t and a Bool is a bool. Only the heap kinds -- strings
 * and lists -- need anything here, plus the arithmetic that has to trap and the
 * `core` functions that cannot be written in Vise itself.
 *
 * MEMORY
 *
 * Heap values come from a bump arena released when the program exits. Nothing
 * is freed individually. A large list buffer is the one exception: it is mapped
 * separately so that it can grow in place, and value.c explains why.
 *
 * This is a deliberate first step rather than the intended end state. Freeing at
 * the right moment means lowering the ownership information the borrow checker
 * already computes, which is its own piece of work; until that exists an arena
 * is correct where individual frees would be guesswork. It suits programs that
 * run and exit, and not a server.
 *
 * What the arena does not excuse is allocating where nothing needs to be
 * allocated, since an arena never takes it back. That is why an enum payload
 * wider than one slot occupies several of them instead of being boxed, and why
 * a list appended to at its own end extends rather than being copied. Both were
 * costing more than the arena ever did.
 */
#ifndef VISE_VALUE_H
#define VISE_VALUE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
/* Generated code boxes values with memcpy, so it needs this too. */
#include <string.h>

/* One value's worth of storage, for enum payloads and list elements. Every Vise
 * value is either an eight-byte scalar or a pointer, so one slot holds any of
 * them without instantiating a type per element. */
typedef union {
    int64_t as_int;
    double as_float;
    bool as_bool;
    uint32_t as_char;
    void *as_ptr;
} vise_slot;

typedef struct {
    const char *bytes;
    size_t len;
} vise_str;

typedef struct {
    vise_slot *items;
    int64_t len;
} vise_list;

/* --- enums ---------------------------------------------------------------
 *
 * Every Vise enum shares one shape, so `Result<T, E>` needs no instantiation
 * per pair of types. The width is fixed here rather than computed per module,
 * because the functions below return `Result` and `Option` and cannot depend on
 * a type whose size varies with the program. A variant wider than this is
 * refused by the backend rather than silently truncated.
 */
#define VISE_MAX_FIELDS 8

typedef struct {
    uint32_t tag;
    vise_slot f[VISE_MAX_FIELDS];
} vise_enum;

/* The built-in constructors always take the first four tags. The backend emits
 * matching defines, and a test asserts the two agree. */
#define VISE_TAG_OK 0u
#define VISE_TAG_ERR 1u
#define VISE_TAG_SOME 2u
#define VISE_TAG_NONE 3u

vise_enum vise_ok_unit(void);
vise_enum vise_ok_int(int64_t value);
vise_enum vise_ok_str(vise_str value);
vise_enum vise_ok_list(vise_list value);
vise_enum vise_err_str(vise_str message);
vise_enum vise_some_int(int64_t value);
vise_enum vise_none(void);

/* --- arena ------------------------------------------------------------- */

void *vise_alloc(size_t bytes);
/* Release everything. Called once, as the program exits. */
void vise_runtime_shutdown(void);
/* Return the mappings large list buffers grew into. Shutdown calls this; it is
 * declared because it is defined below its only caller. */
void vise_list_unmap_all(void);

/* Stop the program. Used by the trapping helpers and by a failed contract. */
_Noreturn void vise_trap(const char *message);

/* --- strings ----------------------------------------------------------- */

vise_str vise_str_literal(const char *bytes, size_t len);
vise_str vise_str_concat(vise_str a, vise_str b);
vise_str vise_str_from_int(int64_t value);
vise_str vise_str_from_float(double value);
vise_str vise_str_from_bool(bool value);
vise_str vise_str_from_char(uint32_t value);
bool vise_str_eq(vise_str a, vise_str b);
/* Negative, zero, or positive, like strcmp. */
int vise_str_cmp(vise_str a, vise_str b);

/* --- lists ------------------------------------------------------------- */

vise_list vise_list_new(int64_t len);

/* Bounds-checked; traps rather than reading past the end. Inline for the same
 * reason as the arithmetic: it is one compare, and the optimiser can often
 * prove the index is in range and drop it. */
static inline vise_slot vise_list_get(vise_list list, int64_t index)
{
    if (index < 0 || index >= list.len) {
        vise_trap("index out of bounds");
    }
    return list.items[index];
}
/* A new list one element longer. The original is untouched, as §9 requires. */
vise_list vise_list_append(vise_list list, vise_slot item);

/* --- trapping arithmetic ----------------------------------------------- */

/* Spec §4: integer arithmetic traps on overflow, it never wraps. Each of these
 * ends the program with a message rather than returning a wrong answer.
 *
 * They are defined here, and not in value.c, because a call would cost more
 * than the arithmetic. Inlined, the C optimiser sees the whole operation: it
 * hoists the checks out of loops, folds them away where it can prove the range,
 * and keeps the values in registers. Measured on a 300-million-iteration
 * accumulation, that is 0.58s as calls against 0.14s inlined -- and 0.14s is
 * what the same loop costs in C with no overflow check at all. Trapping
 * arithmetic is free; calling out to it is not. */

static inline int64_t vise_add(int64_t a, int64_t b)
{
    int64_t out;
    if (__builtin_add_overflow(a, b, &out)) {
        vise_trap("integer overflow in `+`");
    }
    return out;
}

static inline int64_t vise_sub(int64_t a, int64_t b)
{
    int64_t out;
    if (__builtin_sub_overflow(a, b, &out)) {
        vise_trap("integer overflow in `-`");
    }
    return out;
}

static inline int64_t vise_mul(int64_t a, int64_t b)
{
    int64_t out;
    if (__builtin_mul_overflow(a, b, &out)) {
        vise_trap("integer overflow in `*`");
    }
    return out;
}

static inline int64_t vise_div(int64_t a, int64_t b)
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

static inline int64_t vise_rem(int64_t a, int64_t b)
{
    if (b == 0) {
        vise_trap("division by zero");
    }
    if (a == INT64_MIN && b == -1) {
        vise_trap("integer overflow in `%`");
    }
    return a % b;
}

static inline int64_t vise_neg(int64_t a)
{
    if (a == INT64_MIN) {
        vise_trap("integer overflow in `-`");
    }
    return -a;
}

/* --- core ----------------------------------------------------------------
 *
 * The `core` functions that cannot be written in Vise. The set, the signatures,
 * and the effects come from `crates/vise-check/src/builtins.rs`, and a test
 * runs each one compiled and interpreted to confirm they agree.
 */

void vise_print(vise_str text);

int64_t vise_str_length(vise_str text);
vise_list vise_lines(vise_str text);
vise_list vise_split(vise_str text, vise_str separator);
vise_str vise_join(vise_list parts, vise_str separator);
bool vise_starts_with(vise_str text, vise_str prefix);
bool vise_contains(vise_str text, vise_str needle);
vise_enum vise_parse_int(vise_str text);

vise_enum vise_read_file(vise_str path);
vise_enum vise_write_file(vise_str path, vise_str contents);
vise_enum vise_list_dir(vise_str path);
bool vise_is_dir(vise_str path);
vise_enum vise_file_size(vise_str path);

/* Recorded by the generated `main` so `args()` can report them. */
void vise_set_args(int argc, char **argv);
vise_list vise_args(void);
int64_t vise_now(void);
_Noreturn void vise_exit(int64_t code);

#endif /* VISE_VALUE_H */
