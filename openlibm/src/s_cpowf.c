/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 Sergey Subbotin <ssubbotin@gmail.com> */

#include "stafeto_cplx.h"

OLM_DLLEXPORT float complex
cpowf(float complex z, float complex w)
{
	long double complex r = cpowl(CMPLXL((long double)crealf(z), (long double)cimagf(z)),
	    CMPLXL((long double)crealf(w), (long double)cimagf(w)));

	return CMPLXF((float)creall(r), (float)cimagl(r));
}
