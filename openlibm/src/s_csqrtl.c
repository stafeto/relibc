/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* C11 G.6.4.2: csqrt. */
long double complex
csqrtl(long double complex z)
{
	long double x = creall(z), y = cimagl(z), h, t;

	if (isinf(y))
		return CMPLXL(INFINITY, y);
	if (isinf(x)) {
		if (x > 0)
			return CMPLXL(x, isnan(y) ? y : copysignl(0.0L, y));
		return CMPLXL(isnan(y) ? y : 0.0L, copysignl(INFINITY, y));
	}
	if (isnan(x) || isnan(y))
		return CMPLXL(x + y, x + y);
	if (y == 0) {
		if (x < 0)
			return CMPLXL(0.0L, copysignl(sqrtl(-x), y));
		return CMPLXL(sqrtl(x), y);
	}
	if (x == 0) {
		t = sqrtl(fabsl(y) / 2);
		return CMPLXL(t, copysignl(t, y));
	}
	h = hypotl(x, y);
	if (x > 0) {
		t = sqrtl(h / 2 + x / 2);
		return CMPLXL(t, y / (2 * t));
	}
	t = sqrtl(h / 2 - x / 2);
	return CMPLXL(fabsl(y) / (2 * t), copysignl(t, y));
}
