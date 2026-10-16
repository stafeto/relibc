/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* cacosh(z) = i cacos(z) for Im z >= 0 and -i cacos(z) below, so that the
 * real part is not negative. */
long double complex
cacoshl(long double complex z)
{
	long double complex w = cacosl(z);

	if (signbit(cimagl(z)))
		return CMPLXL(fabsl(cimagl(w)), -creall(w));
	return CMPLXL(fabsl(cimagl(w)), creall(w));
}
