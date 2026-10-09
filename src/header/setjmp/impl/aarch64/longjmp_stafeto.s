.global _longjmp
.global longjmp
.type _longjmp,%function
.type longjmp,%function
_longjmp:
longjmp:
	// stafeto: the entry record of the thread (word `outer`) names the
	// frame of a live resident call of the entry distributor. A jump to a
	// stack pointer above that frame abandons the call: clear the word.
	// A target at the frame or below it keeps the word. x9-x11 are free.
	// When a nested entry left the handler of the program to the abandoned
	// entry (word `owed`), the request that entry spent goes back to the
	// kernel: thread_upcall_request on the handle in word `thread`. The
	// call changes x0-x11 only, so the arguments wait in x12 and x13.
	mrs x9, tpidrro_el0
	ldr x10, [x9, #{outer}]
	cbz x10, 1f
	ldr x11, [x0,#104]
	cmp x11, x10
	b.ls 1f
	str xzr, [x9, #{outer}]
	ldr x10, [x9, #{owed}]
	cbz x10, 1f
	str xzr, [x9, #{owed}]
	ldr x10, [x9, #{thread}]
	mov x12, x0
	mov x13, x1
	mov x0, x10
	svc #33
	mov x0, x12
	mov x1, x13
1:
	// IHI0055B_aapcs64.pdf 5.1.1, 5.1.2 callee saved registers
	ldp x19, x20, [x0,#0]
	ldp x21, x22, [x0,#16]
	ldp x23, x24, [x0,#32]
	ldp x25, x26, [x0,#48]
	ldp x27, x28, [x0,#64]
	ldp x29, x30, [x0,#80]
	ldr x2, [x0,#104]
	mov sp, x2
	ldp d8 , d9, [x0,#112]
	ldp d10, d11, [x0,#128]
	ldp d12, d13, [x0,#144]
	ldp d14, d15, [x0,#160]

	// val is an int: the upper half of x1 is undefined (AAPCS64), so the
	// test and the move use w1; setjmp returns 1 for a val of 0.
	mov w0, w1
	cbnz w1, 2f
	mov w0, #1
2:	br x30
