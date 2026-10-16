/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* C11 G.6.2.4: ccosh. */
long double complex
ccoshl(long double complex z)
{
	long double x = creall(z), y = cimagl(z), c, s, ax;

	if (isnan(x)) {
		if (y == 0)
			return CMPLXL(x, y);
		return CMPLXL(x, x);
	}
	if (isinf(x)) {
		if (y == 0)
			return CMPLXL(INFINITY, copysignl(0.0L, y) * copysignl(1.0L, x));
		if (isinf(y) || isnan(y))
			return CMPLXL(INFINITY, y - y);
		return CMPLXL(copysignl(INFINITY, cosl(y)), copysignl(INFINITY, sinl(y) * copysignl(1.0L, x)));
	}
	if (isinf(y) || isnan(y)) {
		if (x == 0)
			return CMPLXL(y - y, x * 0.0L);
		return CMPLXL(y - y, y - y);
	}
	c = cosl(y);
	s = sinl(y);
	ax = fabsl(x);
	if (ax < 50.0L)
		return CMPLXL(coshl(x) * c, sinhl(x) * s);
	return CMPLXL(cplx_half_exp(ax, c), cplx_half_exp(ax, copysignl(1.0L, x) * s));
}
