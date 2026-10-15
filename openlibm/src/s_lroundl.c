#include "cdefs-compat.h"
//__FBSDID("$FreeBSD: src/lib/msun/src/s_lroundl.c,v 1.1 2005/04/08 01:24:08 das Exp $");

#define type		long double
#define	roundit		roundl
#define dtype		long
#define	DTYPE_MIN	LONG_MIN
#define	DTYPE_MAX	LONG_MAX
#define	fn		lroundl_orig

#undef OLM_DLLEXPORT
#define OLM_DLLEXPORT static
#include "s_lround.c"
#undef OLM_DLLEXPORT
#define OLM_DLLEXPORT __attribute__ ((visibility("default")))

#include "ld_flags.h"

OLM_DLLEXPORT long
lroundl(long double x)
{
	/* Not a number, infinite, or out of the range of the result. */
	if (LD_ISNAN(x) || LD_ISINF(x) || x >= 0x1p63L || x < -0x1p63L) {
		feraiseexcept(FE_INVALID);
		return LONG_MIN;
	}
	return lroundl_orig(x);
}
