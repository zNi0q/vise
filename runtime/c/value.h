/* Runtime values for compiled Vise.
 *
 * Vise is statically typed, so most values need no runtime representation at
 * all: an Int is an int64_t and a Bool is a bool. Only the two heap kinds --
 * strings and lists -- need anything here, plus the arithmetic that has to
 * trap.
 *
 * MEMORY
 *
 * Heap values come from a bump arena that is released when the program exits.
 * Nothing is freed individually.
 *
 * This is a deliberate first step rather than the intended end state. Freeing
 * at the right moment means lowering the ownership information the borrow
 * checker already computes, which is a separate piece of work; until that
 * exists, an arena is correct where individual frees would be guesswork. It
 * suits programs that run and exit, and not a server.
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
 * them without the compiler needing to instantiate a type per element. */
typedef union {
    int64_t as_int;
    double as_float;
    bool as_bool;
    uint32_t as_char;
    void *as_ptr;
} vise_slot;

/* --- arena ------------------------------------------------------------- */

void *vise_alloc(size_t bytes);
/* Release everything. Called once, as the program exits. */
void vise_runtime_shutdown(void);

/* --- strings ----------------------------------------------------------- */

typedef struct {
    const char *bytes;
    size_t len;
} vise_str;

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

typedef struct {
    vise_slot *items;
    int64_t len;
} vise_list;

vise_list vise_list_new(int64_t len);
/* Bounds-checked; traps rather than reading past the end. */
vise_slot vise_list_get(vise_list list, int64_t index);

/* --- trapping arithmetic ----------------------------------------------- */

/* Spec §4: integer arithmetic traps on overflow, it never wraps. Each of these
 * ends the program with a message rather than returning a wrong answer. */
int64_t vise_add(int64_t a, int64_t b);
int64_t vise_sub(int64_t a, int64_t b);
int64_t vise_mul(int64_t a, int64_t b);
int64_t vise_div(int64_t a, int64_t b);
int64_t vise_rem(int64_t a, int64_t b);
int64_t vise_neg(int64_t a);

/* Stop the program. Used by the trapping helpers and by a failed contract. */
_Noreturn void vise_trap(const char *message);

/* --- output ------------------------------------------------------------ */

void vise_print(vise_str text);

#endif /* VISE_VALUE_H */
