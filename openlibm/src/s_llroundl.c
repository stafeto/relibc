#include "cdefs-compat.h"
//__FBSDID("$FreeBSD: src/lib/msun/src/s_llroundl.c,v 1.1 2005/04/08 01:24:08 das Exp $");

#define type		long double
#define	roundit		roundl
#define dtype		long long
#define	DTYPE_MIN	LLONG_MIN
#define	DTYPE_MAX	LLONG_MAX
#define	fn		llroundl_orig

#undef OLM_DLLEXPORT
#define OLM_DLLEXPORT static
#include "s_lround.c"
#undef OLM_DLLEXPORT
#define OLM_DLLEXPORT __attribute__ ((visibility("default")))

#include "ld_flags.h"

OLM_DLLEXPORT long long
llroundl(long double x)
{
	/* Not a number, infinite, or out of the range of the result. */
	if (LD_ISNAN(x) || LD_ISINF(x) || x >= 0x1p63L || x < -0x1p63L) {
		feraiseexcept(FE_INVALID);
		return LLONG_MIN;
	}
	return llroundl_orig(x);
}
