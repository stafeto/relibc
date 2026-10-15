.global _longjmp
.global longjmp
.type _longjmp,%function
.type longjmp,%function
_longjmp:
longjmp:
	// One assembly-owned deferral lasts through the actual SP restoration.
	// The callback may unlock library state, but must not deliver user code
	// on the frame this jump is about to abandon. The syscall preserves
	// x12/x13; an ordinary BL does not, so its arguments get a real frame.
	mov x12, x0
	mov x13, x1
	mov x0, #3
	svc #32
	cbnz x0, 3f
	sub sp, sp, #32
	stp x12, x13, [sp]
	str x30, [sp,#16]
	ldr x0, [x12,#104]
	bl stafeto_longjmp_mark_v1
	ldp x12, x13, [sp]
	ldr x30, [sp,#16]
	add sp, sp, #32
	mov x0, x12
	mov x1, x13
	// stafeto: the layer's mark (stafeto_longjmp_mark_v1) already settled
	// the entry record of the thread: a jump that abandons the live
	// resident call of the entry distributor cleared its word and gave
	// back the request it owed. The frames of the two stacks (the main
	// one and the alternate signal stack) are compared there.
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

	// Resume only after the target stack and all preserved registers exist.
	// A pending handler now observes the target, never the abandoned frame.
	mov x13, x1
	mov x0, #4
	svc #32
	cbnz x0, 3f
	// val is an int: the upper half of x13 is undefined (AAPCS64), so the
	// test and the move use w13; setjmp returns 1 for a val of 0.
	mov w0, w13
	cbnz w13, 2f
	mov w0, #1
2:	br x30
3:	// A violated internal deferral balance never continues a partial jump.
	mov x0, #127
	svc #14
	b 3b
