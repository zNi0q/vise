/* The Result kernel's arithmetic is a division and a remainder, which C checks
 * for zero the same way Vise does, so the checked arm differs only in the
 * accumulation. */
#include <inttypes.h>
#include <stdbool.h>

#include "checked.h"

typedef struct {
    bool ok;
    union {
        int64_t value;
        const char *error;
    } as;
} result;

static result half(int64_t n)
{
    result r;
    r.ok = n % 2 == 0;
    if (r.ok) {
        r.as.value = n / 2;
    } else {
        r.as.error = "odd";
    }
    return r;
}

int main(int argc, char **argv)
{
    int64_t n = argc > 1 ? atoll(argv[1]) : 50000000;
    int64_t sum = 0;
    for (int64_t i = 0; i < n; i = add(i, 1)) {
        result r = half(i);
        sum = add(sum, r.ok ? r.as.value : 0);
    }
    printf("%" PRId64 "\n", sum);
}
