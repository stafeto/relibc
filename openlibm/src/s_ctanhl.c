/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* C11 G.6.2.6: ctanh. */
long double complex
ctanhl(long double complex z)
{
	long double x = creall(z), y = cimagl(z), t, beta, sh, ch, d, e;

	if (isnan(x)) {
		if (y == 0)
			return CMPLXL(x, y);
		return CMPLXL(x, x);
	}
	if (isinf(x)) {
		if (isinf(y) || isnan(y))
			return CMPLXL(copysignl(1.0L, x), 0.0L);
		return CMPLXL(copysignl(1.0L, x), copysignl(0.0L, sinl(2 * y)));
	}
	if (x == 0 && (isnan(y) || isinf(y)))
		return CMPLXL(x, y - y);
	if (isinf(y) || isnan(y))
		return CMPLXL(y - y, y - y);
	if (fabsl(x) >= 50.0L) {
		/* tanh x is 1 within 2^-140; the imaginary part is 4 sin y cos y e^-2|x|. */
		e = expl(-2 * fabsl(x));
		return CMPLXL(copysignl(1.0L, x), 4 * sinl(y) * cosl(y) * e);
	}
	t = tanl(y);
	sh = sinhl(x);
	ch = coshl(x);
	beta = 1 + t * t;
	d = 1 + beta * sh * sh;
	return CMPLXL(beta * ch * sh / d, t / d);
}
