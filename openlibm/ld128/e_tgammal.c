/*	$OpenBSD: e_tgammal.c,v 1.1 2011/07/06 00:02:42 martynas Exp $	*/

/*
 * Copyright (c) 2011 Martynas Venckus <martynas@openbsd.org>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

#include <openlibm_math.h>
#include "ld_flags.h"

#include "math_private.h"

static long double
tgammal_orig(long double x)
{
	int64_t i0,i1;

	GET_LDOUBLE_WORDS64(i0,i1,x);
	if (((i0&0x7fffffffffffffffLL)|i1) == 0)
		return (1.0/x);

	if (i0<0 && (u_int64_t)i0<0xffff000000000000ULL && rintl(x)==x)
		return (x-x)/(x-x);

	if (i0==0xffff000000000000ULL && i1==0)
		return (x-x);

	{
		int sign;
		long double r = expl(lgammal_r(x, &sign));

		return sign < 0 ? -r : r;
	}
}

OLM_DLLEXPORT long double
tgammal(long double x)
{
	if (x == 0.0L)
		return ld_divzero(x);
	if (x < 0.0L && (LD_ISINF(x) || __builtin_floorl(x) == x))
		return ld_invalid();
	return tgammal_orig(x);
}
