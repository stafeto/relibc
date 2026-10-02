#ifndef _SETJMP_H
#define _SETJMP_H

#ifdef __aarch64__
/* 22 words of registers, then what sigsetjmp keeps (as musl's
 * __jmp_buf_tag): the return address at 176, the signal mask at 184, its
 * saved x19 at 192, 128 bytes from 184 in all. */
typedef unsigned long long jmp_buf[39];
#endif

#ifdef __i386__
typedef unsigned long long jmp_buf[6];
#endif

#ifdef __x86_64__
typedef unsigned long long jmp_buf[16];
#endif

#ifdef __riscv
typedef unsigned long long jmp_buf[26];
#endif

typedef jmp_buf sigjmp_buf;

#ifdef __cplusplus
extern "C" {
#endif

int setjmp(jmp_buf buf);
void longjmp(jmp_buf buf, int value);
int sigsetjmp(jmp_buf buf, int savemask);
int siglongjmp(jmp_buf buf, int savemask);

#ifdef __cplusplus
} // extern "C"
#endif

#endif /* _SETJMP_H */
