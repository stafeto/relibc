/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* C11 G.6.3.2: clog. */
long double complex
clogl(long double complex z)
{
	long double x = creall(z), y = cimagl(z), ax, ay, t, re;

	if (isinf(x) || isinf(y)) {
		if (isinf(y) && isinf(x)) {
			if (x < 0)
				return CMPLXL(INFINITY, copysignl(LD_3PI_4, y));
			return CMPLXL(INFINITY, copysignl(LD_PI_4, y));
		}
		if (isinf(y))
			return CMPLXL(INFINITY, isnan(x) ? x : copysignl(LD_PI_2, y));
		if (isnan(y))
			return CMPLXL(INFINITY, y);
		return CMPLXL(INFINITY, x < 0 ? copysignl(LD_PI, y) : copysignl(0.0L, y));
	}
	if (isnan(x) || isnan(y))
		return CMPLXL(x + y, x + y);
	ax = fabsl(x);
	ay = fabsl(y);
	if (ax < ay) {
		t = ax;
		ax = ay;
		ay = t;
	}
	if (ax == 0)
		re = -INFINITY;
	else {
		if (ax >= 0.5L && ax <= 1.5L && ay <= 1.5L)
			/* Near the unit circle x*x + y*y - 1 is formed without
			 * cancelling: (ax - 1) and (ax + 1) multiply exactly for
			 * a double, and log1pl keeps the small result. */
			re = log1pl((ax - 1) * (ax + 1) + ay * ay) / 2;
		else {
			t = ay / ax;
			re = logl(ax) + log1pl(t * t) / 2;
		}
	}
	return CMPLXL(re, atan2l(y, x));
}
