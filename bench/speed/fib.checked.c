#include <inttypes.h>

#include "checked.h"

static int64_t fib(int64_t n)
{
    return n < 2 ? n : add(fib(sub(n, 1)), fib(sub(n, 2)));
}

int main(int argc, char **argv)
{
    int64_t n = argc > 1 ? atoll(argv[1]) : 32;
    printf("%" PRId64 "\n", fib(n));
}
