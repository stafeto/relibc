/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* casin(z) = -i casinh(iz). */
long double complex
casinl(long double complex z)
{
	long double complex w = casinhl(CMPLXL(-cimagl(z), creall(z)));

	return CMPLXL(cimagl(w), -creall(w));
}
