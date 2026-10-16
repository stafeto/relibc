/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* ctan(z) = -i ctanh(iz). */
long double complex
ctanl(long double complex z)
{
	long double complex w = ctanhl(CMPLXL(-cimagl(z), creall(z)));

	return CMPLXL(cimagl(w), -creall(w));
}
