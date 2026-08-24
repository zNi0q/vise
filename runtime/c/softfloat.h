/* Reproducible floating-point transcendentals.
 *
 * Spec §11: "Floating-point transcendentals come from a bundled softfloat
 * implementation rather than the platform libm, because platform libm is not
 * reproducible across machines."
 *
 * WHAT IS AND IS NOT PROMISED
 *
 * These are *reproducible*, not correctly rounded. Every one is built from the
 * five IEEE-754 basic operations -- add, subtract, multiply, divide, and square
 * root -- which the standard requires to be correctly rounded, so they give
 * bit-identical answers on every conforming machine. The polynomial
 * coefficients and the order of operations are fixed here, so the result is a
 * property of this source rather than of the C library that happens to be
 * installed.
 *
 * Accuracy is a few units in the last place, not half a unit. Correct rounding
 * for transcendentals is a much harder problem, and buying it would cost the
 * thing this exists for: an answer that does not depend on where it ran.
 *
 * `sqrt` is not implemented here. IEEE-754 already requires it to be correctly
 * rounded, so it is reproducible as it stands, and reimplementing it would only
 * make it worse.
 */
#ifndef VISE_SOFTFLOAT_H
#define VISE_SOFTFLOAT_H

/* e raised to x. */
double vise_exp(double x);

/* Natural logarithm. NaN for x < 0, -infinity for x == 0. */
double vise_log(double x);

double vise_sin(double x);
double vise_cos(double x);

/* x raised to y, as exp(y * log(x)) with the usual special cases. */
double vise_pow(double x, double y);

/* Multiply by a power of two. Exact, and the building block the others use in
 * place of ldexp. */
double vise_scale2(double x, int exponent);

#endif /* VISE_SOFTFLOAT_H */
