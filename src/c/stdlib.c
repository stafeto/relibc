double strtod(const char *nptr, char **endptr);
int strncasecmp(const char *a, const char *b, unsigned long n);

static int ld_digit(char c, int radix) {
    if (c >= '0' && c <= '9') return c - '0';
    if (radix == 16 && c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (radix == 16 && c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

static long double ld_pow(int base, long e) {
    long double r = 1.0L, b = (long double)base;
    while (e > 0) {
        if (e & 1) r *= b;
        b *= b;
        e >>= 1;
    }
    return r;
}

/* The digits go into a 128-bit integer; up to 10^48 the powers of ten are
 * exact in binary128, so a number of 38 digits or fewer with an exponent
 * within that range comes out correctly rounded. Beyond it the result is
 * within a few units of the last place. */
long double strtold(const char *nptr, char **endptr) {
    const char *s = nptr;
    while (*s == ' ' || (*s >= '\t' && *s <= '\r')) s++;
    int neg = 0;
    if (*s == '-') { neg = 1; s++; } else if (*s == '+') s++;

    long double result;
    if (strncasecmp(s, "inf", 3) == 0) {
        s += 3;
        if (strncasecmp(s, "inity", 5) == 0) s += 5;
        result = __builtin_infl();
    } else if (strncasecmp(s, "nan", 3) == 0) {
        s += 3;
        if (*s == '(') {
            const char *t = s + 1;
            while ((*t >= '0' && *t <= '9') || (*t >= 'a' && *t <= 'z') || (*t >= 'A' && *t <= 'Z') || *t == '_') t++;
            if (*t == ')') s = t + 1;
        }
        result = __builtin_nanl("");
    } else {
        int radix = 10;
        const char *start = s;
        if (s[0] == '0' && (s[1] == 'x' || s[1] == 'X') &&
            (ld_digit(s[2], 16) >= 0 || (s[2] == '.' && ld_digit(s[3], 16) >= 0))) {
            radix = 16;
            s += 2;
        }
        unsigned __int128 mant = 0;
        int used = 0;
        long exp = 0;
        int any = 0;
        int d;
        while ((d = ld_digit(*s, radix)) >= 0) {
            any = 1;
            if (used < (radix == 16 ? 30 : 38)) {
                if (mant != 0 || d != 0) used++;
                mant = mant * radix + d;
            } else {
                exp += 1;
            }
            s++;
        }
        if (*s == '.') {
            const char *t = s + 1;
            if (ld_digit(*t, radix) >= 0 || any) {
                s = t;
                while ((d = ld_digit(*s, radix)) >= 0) {
                    any = 1;
                    if (used < (radix == 16 ? 30 : 38)) {
                        if (mant != 0 || d != 0) used++;
                        mant = mant * radix + d;
                        exp -= 1;
                    }
                    s++;
                }
            }
        }
        if (!any) {
            if (endptr) *endptr = (char *)nptr;
            return 0.0L;
        }
        (void)start;
        char marker = radix == 16 ? 'p' : 'e';
        long e = 0;
        if ((*s | 32) == marker) {
            const char *t = s + 1;
            int eneg = 0;
            if (*t == '-') { eneg = 1; t++; } else if (*t == '+') t++;
            if (*t >= '0' && *t <= '9') {
                while (*t >= '0' && *t <= '9') {
                    if (e < 100000) e = e * 10 + (*t - '0');
                    t++;
                }
                e = eneg ? -e : e;
                s = t;
            }
        }
        if (mant == 0) {
            result = 0.0L;
        } else if (radix == 16) {
            /* exp counts hexadecimal digits: four bits each. */
            result = __builtin_ldexpl((long double)mant, (int)(e + 4 * exp));
        } else {
            long double v = (long double)mant;
            long total = e + exp;
            if (total > 0) {
                if (total > 5000) result = __builtin_infl();
                else result = v * ld_pow(10, total);
            } else if (total < 0) {
                if (total < -5000) result = 0.0L;
                else if (total < -4900) result = v / ld_pow(10, 4900) / ld_pow(10, -total - 4900);
                else result = v / ld_pow(10, -total);
            } else {
                result = v;
            }
        }
    }
    if (endptr) *endptr = (char *)s;
    return neg ? -result : result;
}

/* The wide characters of the number are ASCII, and the narrow function
 * finds its end. */
long double wcstold(const int *ws, int **endptr) {
    char buf[1024];
    unsigned long i = 0;
    const int *p = ws;
    while (i < sizeof(buf) - 1 && *p > 0 && *p < 128) {
        buf[i++] = (char)*p++;
    }
    buf[i] = 0;
    char *end;
    long double r = strtold(buf, &end);
    if (endptr) *endptr = (int *)(ws + (end - buf));
    return r;
}

double relibc_ldtod(const long double* val) {
    return (double)(*val);
}

void relibc_dtold(double val, long double* out) {
    *out = (long double)val;
}
