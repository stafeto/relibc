/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* C11 G.6.2.5: csinh. */
long double complex
csinhl(long double complex z)
{
	long double x = creall(z), y = cimagl(z), c, s, ax;

	if (isnan(x)) {
		if (y == 0)
			return CMPLXL(x, y);
		return CMPLXL(x, x);
	}
	if (isinf(x)) {
		if (y == 0)
			return CMPLXL(x, y);
		if (isinf(y) || isnan(y))
			return CMPLXL(x, y - y);
		return CMPLXL(x * cosl(y), copysignl(INFINITY, sinl(y)));
	}
	if (isinf(y) || isnan(y)) {
		if (x == 0)
			return CMPLXL(x, y - y);
		return CMPLXL(y - y, y - y);
	}
	c = cosl(y);
	s = sinl(y);
	ax = fabsl(x);
	if (ax < 50.0L)
		return CMPLXL(sinhl(x) * c, coshl(x) * s);
	return CMPLXL(cplx_half_exp(ax, copysignl(1.0L, x) * c), cplx_half_exp(ax, s));
}
