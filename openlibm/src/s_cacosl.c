/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* C11 G.6.1.1: cacos. Finite arguments follow Kahan:
 * cacos(z) = 2 atan2(Re sqrt(1-z), Re sqrt(1+z))
 *          + i asinh(Im(conj(sqrt(1+z)) sqrt(1-z))). */
long double complex
cacosl(long double complex z)
{
	long double x = creall(z), y = cimagl(z);
	long double complex a, b;

	if (isnan(x)) {
		if (isinf(y))
			return CMPLXL(x, -y);
		return CMPLXL(x, x);
	}
	if (isnan(y)) {
		if (isinf(x))
			return CMPLXL(y, INFINITY);
		if (x == 0)
			return CMPLXL(LD_PI_2, y);
		return CMPLXL(y, y);
	}
	if (isinf(x)) {
		if (isinf(y))
			return CMPLXL(x < 0 ? LD_3PI_4 : LD_PI_4, -y);
		return CMPLXL(x < 0 ? LD_PI : 0.0L, -copysignl(INFINITY, y));
	}
	if (isinf(y))
		return CMPLXL(LD_PI_2, -y);
	a = csqrtl(CMPLXL(1 - x, -y));
	b = csqrtl(CMPLXL(1 + x, y));
	return CMPLXL(2 * atan2l(creall(a), creall(b)),
	    asinhl(creall(b) * cimagl(a) - cimagl(b) * creall(a)));
}
