/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* C11 G.6.3.1: cexp. */
long double complex
cexpl(long double complex z)
{
	long double x = creall(z), y = cimagl(z), c, s, ax;

	if (y == 0)
		return CMPLXL(expl(x), y);
	if (isnan(y) || isinf(y)) {
		if (isinf(x)) {
			if (x < 0)
				return CMPLXL(0.0L, copysignl(0.0L, y));
			return CMPLXL(x, y - y);
		}
		return CMPLXL(y - y, y - y);
	}
	if (isnan(x))
		return CMPLXL(x, x);
	c = cosl(y);
	s = sinl(y);
	if (isinf(x)) {
		if (x < 0)
			return CMPLXL(copysignl(0.0L, c), copysignl(0.0L, s));
		return CMPLXL(copysignl(x, c), copysignl(x, s));
	}
	if (x == 0)
		return CMPLXL(c, s);
	if (x < 11000.0L) {
		ax = expl(x);
		return CMPLXL(ax * c, ax * s);
	}
	return CMPLXL(cplx_half_exp(x, 2 * c), cplx_half_exp(x, 2 * s));
}
