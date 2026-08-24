/* C has no Result, so this is the closest honest equivalent: a tagged union
 * returned by value, with the error carrying a string as Vise's does. */
#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

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
    for (int64_t i = 0; i < n; i++) {
        result r = half(i);
        sum += r.ok ? r.as.value : 0;
    }
    printf("%" PRId64 "\n", sum);
}
