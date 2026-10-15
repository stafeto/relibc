#include "cdefs-compat.h"
//__FBSDID("$FreeBSD: src/lib/msun/src/s_lrintl.c,v 1.1 2008/01/14 02:12:06 das Exp $");

#define type		long double
#define	roundit		rintl
#define dtype		long
#define	fn		lrintl_orig

#undef OLM_DLLEXPORT
#define OLM_DLLEXPORT static
#include "s_lrint.c"
#undef OLM_DLLEXPORT
#define OLM_DLLEXPORT __attribute__ ((visibility("default")))

#include "ld_flags.h"

OLM_DLLEXPORT long
lrintl(long double x)
{
	/* Not a number, infinite, or out of the range of the result. */
	if (LD_ISNAN(x) || LD_ISINF(x) || x >= 0x1p63L || x < -0x1p63L) {
		feraiseexcept(FE_INVALID);
		return LONG_MIN;
	}
	return lrintl_orig(x);
}
