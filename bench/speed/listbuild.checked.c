/* Appending and indexing dominate here; the checked arm adds the accumulation
 * check and the bounds check Vise's `at` performs. */
#include <inttypes.h>

#include "checked.h"

static int64_t at(const int64_t *xs, int64_t len, int64_t index)
{
    if (index < 0 || index >= len) {
        fputs("trap: index out of bounds\n", stderr);
        exit(1);
    }
    return xs[index];
}

int main(int argc, char **argv)
{
    int64_t n = argc > 1 ? atoll(argv[1]) : 20000000;
    int64_t cap = 4, len = 0;
    int64_t *xs = malloc((size_t)cap * sizeof *xs);
    for (int64_t i = 0; i < n; i = add(i, 1)) {
        if (len == cap) {
            cap *= 2;
            xs = realloc(xs, (size_t)cap * sizeof *xs);
        }
        xs[len++] = i;
    }
    int64_t sum = 0;
    for (int64_t j = 0; j < len; j = add(j, 1)) {
        sum = add(sum, at(xs, len, j));
    }
    printf("%" PRId64 "\n", sum);
}
