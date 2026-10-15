/* e_hypotf.c -- float version of e_hypot.c.
 * Conversion to float by Ian Lance Taylor, Cygnus Support, ian@cygnus.com.
 */

/*
 * ====================================================
 * Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
 *
 * Developed at SunPro, a Sun Microsystems, Inc. business.
 * Permission to use, copy, modify, and distribute this
 * software is freely granted, provided that this notice
 * is preserved.
 * ====================================================
 */

#include "cdefs-compat.h"
//__FBSDID("$FreeBSD: src/lib/msun/src/e_hypotf.c,v 1.14 2011/10/15 07:00:28 das Exp $");

#include <openlibm_math.h>

#include "math_private.h"

/* Squares and sum are exact in double for the range of float but the very
 * edge; one rounding to float follows the root. */
OLM_DLLEXPORT float
__ieee754_hypotf(float x, float y)
{
	if (isinf(x) || isinf(y))
		return INFINITY;
	if (isnan(x) || isnan(y))
		return x + y;
	return (float)sqrt((double)x * x + (double)y * y);
}
