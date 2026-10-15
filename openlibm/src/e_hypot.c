
/* @(#)e_hypot.c 1.3 95/01/18 */
/*
 * ====================================================
 * Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
 *
 * Developed at SunSoft, a Sun Microsystems, Inc. business.
 * Permission to use, copy, modify, and distribute this
 * software is freely granted, provided that this notice 
 * is preserved.
 * ====================================================
 */

#include "cdefs-compat.h"
//__FBSDID("$FreeBSD: src/lib/msun/src/e_hypot.c,v 1.14 2011/10/15 07:00:28 das Exp $");

/* __ieee754_hypot(x,y)
 *
 * Method :                  
 *	If (assume round-to-nearest) z=x*x+y*y 
 *	has error less than sqrt(2)/2 ulp, than 
 *	sqrt(z) has error less than 1 ulp (exercise).
 *
 *	So, compute sqrt(x*x+y*y) with some care as 
 *	follows to get the error below 1 ulp:
 *
 *	Assume x>y>0;
 *	(if possible, set rounding to round-to-nearest)
 *	1. if x > 2y  use
 *		x1*x1+(y*y+(x2*(x+x1))) for x*x+y*y
 *	where x1 = x with lower 32 bits cleared, x2 = x-x1; else
 *	2. if x <= 2y use
 *		t1*y1+((x-y)*(x-y)+(t1*y2+t2*y))
 *	where t1 = 2x with lower 32 bits cleared, t2 = 2x-t1, 
 *	y1= y with lower 32 bits chopped, y2 = y-y1.
 *		
 *	NOTE: scaling may be necessary if some argument is too 
 *	      large or too tiny
 *
 * Special cases:
 *	hypot(x,y) is INF if x or y is +INF or -INF; else
 *	hypot(x,y) is NAN if x or y is NAN.
 *
 * Accuracy:
 * 	hypot(x,y) returns sqrt(x^2+y^2) with error less 
 * 	than 1 ulps (units in the last place) 
 */

#include <float.h>
#include <openlibm_math.h>

#include "math_private.h"
#include <openlibm_fenv.h>

/* The sum of the squares is made in binary128, where it is exact to within
 * a unit of 2**-113, and rounded once to double: the result is the correctly
 * rounded one but for the rarest of cases. The soft float of binary128
 * raises no flags; those of the result are raised here. */
OLM_DLLEXPORT double
__ieee754_hypot(double x, double y)
{
	long double r;
	double d;

	if (isinf(x) || isinf(y))
		return INFINITY;
	if (isnan(x) || isnan(y))
		return x + y;
	r = sqrtl((long double)x * x + (long double)y * y);
	d = (double)r;
	if (isinf(d))
		feraiseexcept(FE_OVERFLOW | FE_INEXACT);
	else if (d != 0 && fabs(d) < DBL_MIN && (long double)d != r)
		feraiseexcept(FE_UNDERFLOW | FE_INEXACT);
	return d;
}
