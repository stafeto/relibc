/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

/*
 * The complex functions of C99 and C11 Annex G are computed in binary128
 * (long double), and the double and float functions round that result once.
 * binary128 leaves sixty guard bits for a double result, so the rounding is
 * correct except in cases of about 2^-60 probability, and the sums and
 * differences that cancel in the textbook formulas lose nothing that shows.
 * The identities of Annex G (casin, catan, ccos, csin and ctan from the
 * hyperbolic functions) give the functions they define.
 */

#ifndef STAFETO_CPLX_H
#define STAFETO_CPLX_H

#include <float.h>
#include <openlibm_complex.h>
#include <openlibm_math.h>

#define LD_PI    3.141592653589793238462643383279502884L
#define LD_PI_2  1.570796326794896619231321691639751442L
#define LD_PI_4  0.785398163397448309615660845819875721L
#define LD_3PI_4 2.356194490192344928846982537459627163L

/* The double and float function of the name `name` from `name`l. */
#define CPLX_NARROW_DOUBLE(name) \
	OLM_DLLEXPORT double complex name(double complex z) \
	{ \
		long double complex w = name##l(CMPLXL((long double)creal(z), (long double)cimag(z))); \
		return CMPLX((double)creall(w), (double)cimagl(w)); \
	}

#define CPLX_NARROW_FLOAT(name) \
	OLM_DLLEXPORT float complex name##f(float complex z) \
	{ \
		long double complex w = name##l(CMPLXL((long double)crealf(z), (long double)cimagf(z))); \
		return CMPLXF((float)creall(w), (float)cimagl(w)); \
	}

/* c * e^ax / 2 for ax >= 0, with no overflow before the product. */
static inline long double
cplx_half_exp(long double ax, long double c)
{
	long double t;

	if (ax < 11000.0L)
		return c * expl(ax) / 2;
	t = expl(ax / 2);
	return (c * t) * (t / 2);
}

#endif
