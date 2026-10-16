/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* catan(z) = -i catanh(iz). */
long double complex
catanl(long double complex z)
{
	long double complex w = catanhl(CMPLXL(-cimagl(z), creall(z)));

	return CMPLXL(cimagl(w), -creall(w));
}
