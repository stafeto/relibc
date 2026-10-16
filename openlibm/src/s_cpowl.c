/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* cpow(z, w) = cexp(w clog(z)); a zero base gives zero for a positive real
 * part of the exponent. */
long double complex
cpowl(long double complex z, long double complex w)
{
	long double complex l;
	long double a = creall(w), b = cimagl(w), c, d;

	if (creall(z) == 0 && cimagl(z) == 0 && a > 0 && !isnan(b) && !isinf(a))
		return CMPLXL(0.0L, 0.0L);
	l = clogl(z);
	c = creall(l);
	d = cimagl(l);
	return cexpl(CMPLXL(a * c - b * d, a * d + b * c));
}
