#include <inttypes.h>

#include "checked.h"

int main(int argc, char **argv)
{
    int64_t n = argc > 1 ? atoll(argv[1]) : 300000000;
    int64_t total = 0;
    for (int64_t i = 0; i < n; i = add(i, 1)) {
        total = add(total, i);
    }
    printf("%" PRId64 "\n", total);
}
