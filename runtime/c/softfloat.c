/* Reproducible transcendentals. See softfloat.h for what is promised. */

#include "softfloat.h"

#include <stdint.h>
#include <string.h>

/* Splitting a constant into a high part with trailing zero bits and a small
 * remainder lets the argument reduction below subtract it without losing the
 * low bits of the result. This is Cody and Waite's technique. */
#define LN2_HI 6.93147180369123816490e-01 /* 0x3FE62E42, 0xFEE00000 */
#define LN2_LO 1.90821492927058770002e-10 /* the rest of ln 2 */
#define INV_LN2 1.44269504088896338700e+00

/* pi/2 in three parts, each with trailing zero bits so that multiplying by a
 * moderate integer stays exact.
 *
 * A third part matters more than it looks. Near a zero of sine the result is
 * tiny while the reduction error is not, so relative accuracy there is decided
 * entirely by how many bits of pi/2 were subtracted.
 *
 * The three must partition pi/2 without overlapping. The obvious pairing --
 * a head and "all the rest" -- cannot be extended by a third term, because the
 * rest already contains it, and subtracting it again makes the answer worse.
 * The middle term below is therefore truncated, not the full remainder. */
#define PIO2_HI 1.57079632673412561417e+00  /* 0x3FF921FB, 0x54400000 */
#define PIO2_MID 6.07710050630396597660e-11 /* 0x3DD0B461, 0x1A600000 */
#define PIO2_LO 2.02226624879595063154e-21  /* 0x3BA3198A, 0x2E037073 */
#define TWO_OVER_PI 6.36619772367581382433e-01

static double positive_infinity(void)
{
    uint64_t bits = 0x7ff0000000000000ull;
    double d;
    memcpy(&d, &bits, sizeof d);
    return d;
}

static double quiet_nan(void)
{
    uint64_t bits = 0x7ff8000000000000ull;
    double d;
    memcpy(&d, &bits, sizeof d);
    return d;
}

static int is_nan(double x)
{
    return x != x;
}

double vise_scale2(double x, int exponent)
{
    uint64_t bits;
    memcpy(&bits, &x, sizeof bits);
    int biased = (int)((bits >> 52) & 0x7ff);

    if (biased == 0x7ff || x == 0.0) {
        return x; /* inf, NaN, and zero are unchanged by scaling */
    }
    if (biased == 0) {
        /* Subnormal: normalise by multiplying by 2^54 first, then account for
         * it. Doing this in one step would lose the low bits. */
        x *= 18014398509481984.0; /* 2^54 */
        memcpy(&bits, &x, sizeof bits);
        biased = (int)((bits >> 52) & 0x7ff) - 54;
    }

    biased += exponent;
    if (biased >= 0x7ff) {
        return x < 0.0 ? -positive_infinity() : positive_infinity();
    }
    if (biased <= 0) {
        /* Underflow to zero rather than producing a subnormal: a subnormal
         * result would be rounded differently on machines that flush them. */
        return x < 0.0 ? -0.0 : 0.0;
    }
    bits = (bits & 0x800fffffffffffffull) | ((uint64_t)biased << 52);
    memcpy(&x, &bits, sizeof x);
    return x;
}

/* exp(r) for r in roughly [-ln2/2, ln2/2], by Taylor series.
 *
 * Fourteen terms take the remainder below 2^-60 over that interval, and Horner
 * fixes the order of operations so the result does not depend on how a compiler
 * chooses to associate them. */
static double exp_small(double r)
{
    static const double INV_FACTORIAL[] = {
        1.0 / 14.0, 1.0 / 13.0, 1.0 / 12.0, 1.0 / 11.0, 1.0 / 10.0,
        1.0 / 9.0,  1.0 / 8.0,  1.0 / 7.0,  1.0 / 6.0,  1.0 / 5.0,
        1.0 / 4.0,  1.0 / 3.0,  1.0 / 2.0,
    };
    double sum = 1.0;
    for (unsigned i = 0; i < sizeof INV_FACTORIAL / sizeof INV_FACTORIAL[0]; i++) {
        sum = 1.0 + r * INV_FACTORIAL[i] * sum;
    }
    return 1.0 + r * sum;
}

double vise_exp(double x)
{
    if (is_nan(x)) {
        return x;
    }
    if (x > 709.782712893384) {
        return positive_infinity();
    }
    if (x < -745.1332191019411) {
        return 0.0;
    }

    /* x = k*ln2 + r, with |r| <= ln2/2. Subtracting ln2 in two parts keeps the
     * low bits of r, which is where the accuracy of the result lives. */
    double kd = x * INV_LN2;
    int k = (int)(kd < 0.0 ? kd - 0.5 : kd + 0.5);
    double r = x - (double)k * LN2_HI;
    r -= (double)k * LN2_LO;

    return vise_scale2(exp_small(r), k);
}

double vise_log(double x)
{
    if (is_nan(x)) {
        return x;
    }
    if (x < 0.0) {
        return quiet_nan();
    }
    if (x == 0.0) {
        return -positive_infinity();
    }
    if (x == positive_infinity()) {
        return x;
    }

    /* x = m * 2^e with m in [sqrt(1/2), sqrt(2)), so the series below converges
     * quickly and symmetrically about 1. */
    uint64_t bits;
    memcpy(&bits, &x, sizeof bits);
    int e = (int)((bits >> 52) & 0x7ff) - 1023;
    if (e == -1023) {
        x *= 18014398509481984.0; /* 2^54: normalise a subnormal */
        memcpy(&bits, &x, sizeof bits);
        e = (int)((bits >> 52) & 0x7ff) - 1023 - 54;
    }
    bits = (bits & 0x800fffffffffffffull) | ((uint64_t)1023 << 52);
    double m;
    memcpy(&m, &bits, sizeof m);
    if (m > 1.4142135623730951) {
        m *= 0.5;
        e += 1;
    }

    /* log(m) = 2 * atanh(s) with s = (m-1)/(m+1). |s| < 0.1716 here, so the
     * odd-power series converges fast. */
    double s = (m - 1.0) / (m + 1.0);
    double s2 = s * s;
    double sum = 0.0;
    for (int n = 25; n >= 1; n -= 2) {
        sum = 1.0 / (double)n + s2 * sum;
    }
    return 2.0 * s * sum + (double)e * LN2_HI + (double)e * LN2_LO;
}

/* sin(r) and cos(r) for |r| <= pi/4, by Taylor series with a fixed term count. */
static double sin_small(double r)
{
    double r2 = r * r;
    double sum = 0.0;
    /* 1/17! down to 1/3!, alternating sign, by Horner. */
    /* -1/15!, 1/13!, -1/11!, 1/9!, -1/7!, 1/5!, -1/3! */
    static const double C[] = {
        -1.0 / 1307674368000.0, 1.0 / 6227020800.0, -1.0 / 39916800.0,
        1.0 / 362880.0,         -1.0 / 5040.0,      1.0 / 120.0,
        -1.0 / 6.0,
    };
    for (unsigned i = 0; i < sizeof C / sizeof C[0]; i++) {
        sum = C[i] + r2 * sum;
    }
    return r + r * r2 * sum;
}

static double cos_small(double r)
{
    double r2 = r * r;
    double sum = 0.0;
    /* -1/14!, 1/12!, -1/10!, 1/8!, -1/6!, 1/4!, -1/2! */
    static const double C[] = {
        -1.0 / 87178291200.0, 1.0 / 479001600.0, -1.0 / 3628800.0,
        1.0 / 40320.0,        -1.0 / 720.0,      1.0 / 24.0,
        -1.0 / 2.0,
    };
    for (unsigned i = 0; i < sizeof C / sizeof C[0]; i++) {
        sum = C[i] + r2 * sum;
    }
    return 1.0 + r2 * sum;
}

/* Reduce x to a quadrant and a remainder in [-pi/4, pi/4].
 *
 * Two-part pi/2 keeps this accurate to roughly |x| < 2^20. Beyond that the
 * reduction loses bits, which is a known limit rather than a hidden one: a
 * program that needs sin of a huge argument should reduce it itself, where the
 * loss is visible. */
static int reduce_quadrant(double x, double *remainder)
{
    double qd = x * TWO_OVER_PI;
    int q = (int)(qd < 0.0 ? qd - 0.5 : qd + 0.5);
    double r = x - (double)q * PIO2_HI;
    r -= (double)q * PIO2_MID;
    r -= (double)q * PIO2_LO;
    *remainder = r;
    return q & 3;
}

double vise_sin(double x)
{
    if (is_nan(x) || x == positive_infinity() || x == -positive_infinity()) {
        return quiet_nan();
    }
    double r;
    int quadrant = reduce_quadrant(x, &r);
    switch (quadrant) {
    case 0:  return sin_small(r);
    case 1:  return cos_small(r);
    case 2:  return -sin_small(r);
    default: return -cos_small(r);
    }
}

double vise_cos(double x)
{
    if (is_nan(x) || x == positive_infinity() || x == -positive_infinity()) {
        return quiet_nan();
    }
    double r;
    int quadrant = reduce_quadrant(x, &r);
    switch (quadrant) {
    case 0:  return cos_small(r);
    case 1:  return -sin_small(r);
    case 2:  return -cos_small(r);
    default: return sin_small(r);
    }
}

/* x to an integral power, by binary exponentiation.
 *
 * Worth the special case: `exp(y * log(x))` compounds two approximations, so it
 * returns 7.999999999999998 for pow(2, 3). Squaring is exact until it
 * overflows, and integral exponents are most of the uses. */
static double pow_integral(double x, long long n)
{
    double result = 1.0;
    double base = x;
    unsigned long long k = n < 0 ? (unsigned long long)(-(n + 1)) + 1ull
                                 : (unsigned long long)n;
    while (k > 0) {
        if (k & 1ull) {
            result *= base;
        }
        k >>= 1;
        if (k > 0) {
            base *= base;
        }
    }
    return n < 0 ? 1.0 / result : result;
}

double vise_pow(double x, double y)
{
    if (y == 0.0) {
        return 1.0; /* including pow(NaN, 0), as IEEE-754 specifies */
    }
    if (is_nan(x) || is_nan(y)) {
        return quiet_nan();
    }
    if (x == 1.0) {
        return 1.0;
    }
    if (x == 0.0) {
        return y > 0.0 ? 0.0 : positive_infinity();
    }
    /* An integral exponent small enough that repeated squaring cannot lose
     * more than the multiplications themselves do. */
    if (y >= -1024.0 && y <= 1024.0) {
        double truncated = (double)(long long)y;
        if (truncated == y) {
            return pow_integral(x, (long long)y);
        }
    }
    if (x < 0.0) {
        /* Only an integral exponent has a real answer. */
        double truncated = (double)(long long)y;
        if (truncated != y) {
            return quiet_nan();
        }
        double magnitude = vise_exp(y * vise_log(-x));
        long long n = (long long)y;
        return (n & 1) ? -magnitude : magnitude;
    }
    return vise_exp(y * vise_log(x));
}
