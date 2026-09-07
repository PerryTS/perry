	.build_version macos, 11, 0
	.section	__TEXT,__text,regular,pure_instructions
	.globl	_pair
	.p2align	2
_pair:
	.cfi_startproc
	mov	w8, w0
	orr	x0, x8, x1, lsl #32
	mov	x1, x2
	ret
	.cfi_endproc

	.globl	_triple
	.p2align	2
_triple:
	.cfi_startproc
	cbz	w0, LBB1_2
	stp	w1, w2, [x8, #4]
LBB1_2:
	str	w0, [x8]
	ret
	.cfi_endproc

	.globl	_use_pair
	.p2align	2
_use_pair:
	.cfi_startproc
	stp	x29, x30, [sp, #-16]!
	.cfi_def_cfa_offset 16
	mov	x29, sp
	.cfi_def_cfa w29, 16
	.cfi_offset w30, -8
	.cfi_offset w29, -16
	bl	_pair
	lsr	x8, x0, #32
	add	x8, x8, w0, uxtw
	add	x8, x8, w1, uxtw
	cmp	x0, #0
	csel	x0, xzr, x8, eq
	.cfi_def_cfa wsp, 16
	ldp	x29, x30, [sp], #16
	.cfi_def_cfa_offset 0
	.cfi_restore w30
	.cfi_restore w29
	ret
	.cfi_endproc

	.globl	_use_triple
	.p2align	2
_use_triple:
	.cfi_startproc
	sub	sp, sp, #32
	.cfi_def_cfa_offset 32
	stp	x29, x30, [sp, #16]
	add	x29, sp, #16
	.cfi_def_cfa w29, 16
	.cfi_offset w30, -8
	.cfi_offset w29, -16
	add	x8, sp, #4
	bl	_triple
	ldp	w8, w9, [sp, #4]
	ldr	w10, [sp, #12]
	add	x9, x9, x8
	add	x9, x9, x10
	cmp	w8, #0
	csel	x0, xzr, x9, eq
	.cfi_def_cfa wsp, 32
	ldp	x29, x30, [sp, #16]
	add	sp, sp, #32
	.cfi_def_cfa_offset 0
	.cfi_restore w30
	.cfi_restore w29
	ret
	.cfi_endproc

.subsections_via_symbols
