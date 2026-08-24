#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static int64_t fib(int64_t n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }

int main(int argc, char **argv)
{
    int64_t n = argc > 1 ? atoll(argv[1]) : 32;
    printf("%" PRId64 "\n", fib(n));
}
