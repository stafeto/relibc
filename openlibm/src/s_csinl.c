/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* csin(z) = -i csinh(iz). */
long double complex
csinl(long double complex z)
{
	long double complex w = csinhl(CMPLXL(-cimagl(z), creall(z)));

	return CMPLXL(cimagl(w), -creall(w));
}
