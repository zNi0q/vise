#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv)
{
    int64_t n = argc > 1 ? atoll(argv[1]) : 20000000;
    int64_t cap = 4, len = 0;
    int64_t *xs = malloc((size_t)cap * sizeof *xs);
    for (int64_t i = 0; i < n; i++) {
        if (len == cap) {
            cap *= 2;
            xs = realloc(xs, (size_t)cap * sizeof *xs);
        }
        xs[len++] = i;
    }
    int64_t sum = 0;
    for (int64_t j = 0; j < len; j++) {
        sum += xs[j];
    }
    printf("%" PRId64 "\n", sum);
}
