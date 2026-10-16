/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* C11 G.6.2.2: casinh. Finite arguments follow Kahan:
 * casin(u + iv) = atan2(u, Re(sqrt(1-z) sqrt(1+z)))
 *               + i asinh(Im(conj(sqrt(1-z)) sqrt(1+z))),
 * and casinh(z) = -i casin(iz). */
long double complex
casinhl(long double complex z)
{
	long double x = creall(z), y = cimagl(z), u, v;
	long double complex a, b, p, q;

	if (isinf(x)) {
		if (isinf(y))
			return CMPLXL(x, copysignl(LD_PI_4, y));
		if (isnan(y))
			return CMPLXL(x, y);
		return CMPLXL(x, copysignl(0.0L, y));
	}
	if (isnan(x)) {
		if (y == 0)
			return CMPLXL(x, y);
		if (isinf(y))
			return CMPLXL(INFINITY, x);
		return CMPLXL(x, x);
	}
	if (isinf(y))
		return CMPLXL(copysignl(INFINITY, x), copysignl(LD_PI_2, y));
	if (isnan(y))
		return CMPLXL(y, y);
	/* iz = u + iv */
	u = -y;
	v = x;
	a = csqrtl(CMPLXL(1 - u, -v));
	b = csqrtl(CMPLXL(1 + u, v));
	p = CMPLXL(creall(a) * creall(b) - cimagl(a) * cimagl(b),
	    creall(a) * cimagl(b) + cimagl(a) * creall(b));
	q = CMPLXL(creall(a) * creall(b) + cimagl(a) * cimagl(b),
	    creall(a) * cimagl(b) - cimagl(a) * creall(b));
	/* casin(iz) = atan2(u, Re p) + i asinh(Im q); times -i. */
	return CMPLXL(asinhl(cimagl(q)), -atan2l(u, creall(p)));
}
