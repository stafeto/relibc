/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* C11 G.6.2.3: catanh. */
long double complex
catanhl(long double complex z)
{
	long double x = creall(z), y = cimagl(z), ax, ay, h, re, im, d;

	if (isnan(x)) {
		if (isinf(y))
			return CMPLXL(copysignl(0.0L, x), copysignl(LD_PI_2, y));
		return CMPLXL(x, x);
	}
	if (isnan(y)) {
		if (isinf(x) || x == 0)
			return CMPLXL(copysignl(0.0L, x), y);
		return CMPLXL(y, y);
	}
	if (isinf(x) || isinf(y))
		return CMPLXL(copysignl(0.0L, x), copysignl(LD_PI_2, y));
	ax = fabsl(x);
	ay = fabsl(y);
	if (ax > 1e1000L || ay > 1e1000L) {
		h = hypotl(ax, ay);
		return CMPLXL(copysignl((ax / h) / h, x), copysignl(LD_PI_2, y));
	}
	d = (1 - ax) * (1 - ax) + ay * ay;
	re = log1pl(4 * ax / d) / 4;
	im = atan2l(2 * ay, (1 - ax) * (1 + ax) - ay * ay) / 2;
	return CMPLXL(copysignl(re, x), copysignl(im, y));
}
