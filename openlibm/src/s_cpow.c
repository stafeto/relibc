/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

OLM_DLLEXPORT double complex
cpow(double complex z, double complex w)
{
	long double complex r = cpowl(CMPLXL((long double)creal(z), (long double)cimag(z)),
	    CMPLXL((long double)creal(w), (long double)cimag(w)));

	return CMPLX((double)creall(r), (double)cimagl(r));
}
