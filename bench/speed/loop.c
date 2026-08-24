/* See README.md. The size comes from argv so nothing folds. */
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv)
{
    int64_t n = argc > 1 ? atoll(argv[1]) : 300000000;
    int64_t total = 0;
    for (int64_t i = 0; i < n; i++) {
        total += i;
    }
    printf("%" PRId64 "\n", total);
}
