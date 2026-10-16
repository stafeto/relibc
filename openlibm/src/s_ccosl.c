/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

/* ccos(z) = ccosh(iz). */
long double complex
ccosl(long double complex z)
{
	return ccoshl(CMPLXL(-cimagl(z), creall(z)));
}
