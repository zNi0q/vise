/* Reproducible transcendentals.
 *
 * Two things are checked. Accuracy: each function agrees with the platform libm
 * to within a few units in the last place, which is what the header promises.
 * Reproducibility: repeated calls give bit-identical answers, and the value is
 * a function of the input alone.
 *
 * Exits 0 if every case behaved.
 */

#include "softfloat.h"

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static int failures = 0;

static void fail(const char *what, double input, double got, double want)
{
    fprintf(stderr, "FAIL: %s(%.17g) = %.17g, expected about %.17g\n",
            what, input, got, want);
    failures++;
}

/* Distance in units in the last place, which is the only scale on which
 * floating-point agreement means anything. */
static double ulps_apart(double a, double b)
{
    if (a == b) {
        return 0.0;
    }
    if (a != a || b != b) {
        return 1.0e300;
    }
    int64_t ia, ib;
    memcpy(&ia, &a, sizeof ia);
    memcpy(&ib, &b, sizeof ib);
    if ((ia < 0) != (ib < 0)) {
        return 1.0e300;
    }
    int64_t diff = ia > ib ? ia - ib : ib - ia;
    return (double)diff;
}

/* A few ulps is the stated tolerance: these are reproducible, not correctly
 * rounded. */
#define TOLERANCE 8.0

static void check(const char *what, double input, double got, double want)
{
    if (ulps_apart(got, want) > TOLERANCE) {
        fail(what, input, got, want);
    }
}

int main(void)
{
    static const double SAMPLES[] = {
        0.0, 1.0, -1.0, 0.5, -0.5, 2.0, -2.0, 0.1, 3.14159265358979,
        -3.14159265358979, 10.0, -10.0, 100.0, -100.0, 700.0, -700.0,
        1e-8, -1e-8, 1.7320508075688772, 6.283185307179586,
    };
    const unsigned n = sizeof SAMPLES / sizeof SAMPLES[0];

    for (unsigned i = 0; i < n; i++) {
        double x = SAMPLES[i];
        check("exp", x, vise_exp(x), exp(x));
        check("sin", x, vise_sin(x), sin(x));
        check("cos", x, vise_cos(x), cos(x));
        if (x > 0.0) {
            check("log", x, vise_log(x), log(x));
        }
    }

    /* pow over a grid, since its error is the sum of exp's and log's. */
    for (unsigned i = 0; i < n; i++) {
        for (unsigned j = 0; j < n; j++) {
            double x = SAMPLES[i], y = SAMPLES[j];
            if (x <= 0.0 || y > 20.0 || y < -20.0) {
                continue;
            }
            double got = vise_pow(x, y);
            double want = pow(x, y);
            if (want == 0.0 || want != want || want > 1e300) {
                continue;
            }
            /* pow compounds two approximations, so it gets a wider band. */
            if (ulps_apart(got, want) > 64.0) {
                fail("pow", x, got, want);
            }
        }
    }

    /* Special cases, which is where an implementation usually goes wrong. */
    if (vise_exp(0.0) != 1.0) fail("exp", 0.0, vise_exp(0.0), 1.0);
    if (vise_log(1.0) != 0.0) fail("log", 1.0, vise_log(1.0), 0.0);
    if (vise_sin(0.0) != 0.0) fail("sin", 0.0, vise_sin(0.0), 0.0);
    if (vise_cos(0.0) != 1.0) fail("cos", 0.0, vise_cos(0.0), 1.0);
    if (!isinf(vise_exp(1e6))) fail("exp", 1e6, vise_exp(1e6), INFINITY);
    if (vise_exp(-1e6) != 0.0) fail("exp", -1e6, vise_exp(-1e6), 0.0);
    if (!isinf(vise_log(0.0)) || vise_log(0.0) > 0.0) {
        fail("log", 0.0, vise_log(0.0), -INFINITY);
    }
    if (!isnan(vise_log(-1.0))) fail("log", -1.0, vise_log(-1.0), NAN);
    if (!isnan(vise_sin(INFINITY))) fail("sin", INFINITY, vise_sin(INFINITY), NAN);

    /* pow's IEEE-754 corners. */
    if (vise_pow(2.0, 0.0) != 1.0) fail("pow", 2.0, vise_pow(2.0, 0.0), 1.0);
    if (vise_pow(NAN, 0.0) != 1.0) fail("pow", NAN, vise_pow(NAN, 0.0), 1.0);
    if (vise_pow(-2.0, 3.0) != -8.0) fail("pow", -2.0, vise_pow(-2.0, 3.0), -8.0);
    if (vise_pow(-2.0, 2.0) != 4.0) fail("pow", -2.0, vise_pow(-2.0, 2.0), 4.0);
    if (!isnan(vise_pow(-2.0, 0.5))) fail("pow", -2.0, vise_pow(-2.0, 0.5), NAN);

    /* scale2 is exact, and is what the others use in place of ldexp. */
    if (vise_scale2(1.0, 10) != 1024.0) fail("scale2", 1.0, vise_scale2(1.0, 10), 1024.0);
    if (vise_scale2(3.0, -1) != 1.5) fail("scale2", 3.0, vise_scale2(3.0, -1), 1.5);
    if (vise_scale2(0.0, 100) != 0.0) fail("scale2", 0.0, vise_scale2(0.0, 100), 0.0);
    if (!isinf(vise_scale2(1.0, 5000))) fail("scale2", 1.0, vise_scale2(1.0, 5000), INFINITY);

    /* The point of the exercise: the same input always gives the same bits. */
    for (unsigned i = 0; i < n; i++) {
        double x = SAMPLES[i];
        double first = vise_exp(x);
        for (int repeat = 0; repeat < 50; repeat++) {
            if (memcmp(&first, &(double){vise_exp(x)}, sizeof first) != 0) {
                fail("exp is not reproducible", x, vise_exp(x), first);
                break;
            }
        }
    }

    if (failures == 0) {
        printf("softfloat: all cases behaved\n");
    }
    return failures == 0 ? 0 : 1;
}
