/* Binary128 arithmetic is done in software, which raises none of the floating
 * point exceptions; the functions of long double that must raise them do so
 * here, by the class of their arguments and of their results. */
#ifndef LD_FLAGS_H
#define LD_FLAGS_H

#include <float.h>
#include <openlibm_fenv.h>
#include <openlibm_math.h>

#define LD_ISNAN(x) __builtin_isnan(x)
#define LD_ISINF(x) __builtin_isinf(x)

static inline long double ld_invalid(void)
{
	feraiseexcept(FE_INVALID);
	return __builtin_nanl("");
}

static inline long double ld_divzero(long double sign)
{
	feraiseexcept(FE_DIVBYZERO);
	return __builtin_copysignl(__builtin_infl(), sign);
}

/* An integer valued number that is odd. */
static inline int ld_odd_integer(long double y)
{
	long double h;

	if (LD_ISNAN(y) || LD_ISINF(y) || __builtin_fabsl(y) >= 0x1p113L)
		return 0;
	h = __builtin_floorl(y);
	return h == y && __builtin_floorl(y / 2) * 2 != y;
}

#endif
