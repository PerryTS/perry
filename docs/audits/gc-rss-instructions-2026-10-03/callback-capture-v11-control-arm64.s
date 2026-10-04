__RNCNCNvNtNtCsa0ttIZdYGZx_13perry_runtime2gc5trace24trace_heap_rewrite_slots00B9_:
	.cfi_startproc
	sub	sp, sp, #64
	stp	x22, x21, [sp, #16]
	stp	x20, x19, [sp, #32]
	stp	x29, x30, [sp, #48]
	add	x29, sp, #48
	.cfi_def_cfa w29, 16
	.cfi_offset w30, -8
	.cfi_offset w29, -16
	.cfi_offset w19, -24
	.cfi_offset w20, -32
	.cfi_offset w21, -40
	.cfi_offset w22, -48
	ldr	x8, [x0]
	ldrb	w8, [x8]
	tbz	w8, #0, LBB99_4
	ldr	x8, [x0, #8]
	ldr	x9, [x8]
	cbz	x9, LBB99_4
	ldrb	w8, [x9]
	cmp	w8, #2
	b.ne	LBB99_4
	ldr	w8, [x9, #8]
	mov	w10, #65495
	add	w10, w8, w10
	cmp	w10, #3
	ccmp	w10, #1, #4, ls
	b.ne	LBB99_25
LBB99_4:
	mov	w8, #255
	bics	wzr, w8, w2
	b.eq	LBB99_6
Lloh774:
	adrp	x8, __RNvNtNtCsa0ttIZdYGZx_13perry_runtime2gc9telemetry27LAYOUT_SCAN_TRACE_ARMED_ANY@PAGE
Lloh775:
	add	x8, x8, __RNvNtNtCsa0ttIZdYGZx_13perry_runtime2gc9telemetry27LAYOUT_SCAN_TRACE_ARMED_ANY@PAGEOFF
	ldaprb	w8, [x8]
	cbnz	w8, LBB99_24
LBB99_6:
	ldr	x20, [x1]
	ldp	x21, x19, [x0, #16]
	ldr	x8, [x0, #32]
	ldrb	w22, [x8]
	tbz	w22, #0, LBB99_8
LBB99_7:
	mov	x0, x20
	mov	x1, x21
	bl	__RNvNtNtCsa0ttIZdYGZx_13perry_runtime2gc10full_trace14observe_handle
	tbnz	w0, #0, LBB99_10
LBB99_8:
	and	x8, x20, #0xffff000000000000
	mov	x9, #9221683186994511872
	cmp	x8, x9
	mov	x9, #9222527611924643840
	ccmp	x8, x9, #4, ne
	mov	x9, #9223090561878065152
	ccmp	x8, x9, #4, ne
	b.ne	LBB99_11
	ands	x20, x20, #0xffffffffffff
	b.ne	LBB99_12
LBB99_10:
	ldp	x29, x30, [sp, #48]
	ldp	x20, x19, [sp, #32]
	ldp	x22, x21, [sp, #16]
	add	sp, sp, #64
	ret
LBB99_11:
	sub	x8, x20, #1, lsl #12
	mov	x9, #-4097
	movk	x9, #0, lsl #48
	cmp	x8, x9
	b.hi	LBB99_10
LBB99_12:
	str	x20, [sp, #8]
	add	x1, sp, #8
	mov	x0, x21
	bl	__RNvMs_NtNtCsa0ttIZdYGZx_13perry_runtime2gc5traceNtB4_15ValidPointerSet8contains
	cbz	w0, LBB99_10
	ldurb	w8, [x20, #-7]
	tbnz	w8, #0, LBB99_10
	sub	x21, x20, #8
	orr	w9, w8, #0x1
	sturb	w9, [x20, #-7]
	tbnz	w8, #7, LBB99_19
	ldrb	w8, [x21]
	cmp	w8, #26
	b.hs	LBB99_10
Lloh776:
	adrp	x9, __RNvNtNtCsa0ttIZdYGZx_13perry_runtime2gc5types18GC_TYPE_INFO_BY_ID@GOTPAGE
Lloh777:
	ldr	x9, [x9, __RNvNtNtCsa0ttIZdYGZx_13perry_runtime2gc5types18GC_TYPE_INFO_BY_ID@GOTPAGEOFF]
	add	x9, x9, x8, lsl #5
	ldrb	w10, [x9, #27]
	cmp	w10, #2
	b.eq	LBB99_10
	ldrb	w9, [x9, #17]
	cmp	w9, #2
	b.eq	LBB99_10
	tbz	w22, #0, LBB99_22
LBB99_19:
	ldr	x20, [x19, #16]
	ldr	x8, [x19]
	cmp	x20, x8
	b.ne	LBB99_21
	mov	x0, x19
	bl	__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecONtNtNtCsa0ttIZdYGZx_13perry_runtime2gc5types8GcHeaderE8grow_oneBT_
LBB99_21:
	ldr	x8, [x19, #8]
	str	x21, [x8, x20, lsl #3]
	add	x8, x20, #1
	str	x8, [x19, #16]
	ldp	x29, x30, [sp, #48]
	ldp	x20, x19, [sp, #32]
	ldp	x22, x21, [sp, #16]
	add	sp, sp, #64
	ret
LBB99_22:
	cmp	w8, #1
	b.ne	LBB99_19
	ldurh	w8, [x20, #-6]
	mov	w9, #49472
	and	w8, w8, w9
	cmp	w8, #4, lsl #12
	b.eq	LBB99_10
	b	LBB99_19
LBB99_24:
	mov	x19, x0
	mov	x0, x2
	mov	x20, x1
	bl	__RNvNtNtCsa0ttIZdYGZx_13perry_runtime2gc9telemetry35record_layout_child_slot_read_armed
	mov	x0, x19
	ldr	x20, [x20]
	ldp	x21, x19, [x19, #16]
	ldr	x8, [x0, #32]
	ldrb	w22, [x8]
	tbnz	w22, #0, LBB99_7
	b	LBB99_8
LBB99_25:
	mov	x20, x0
	add	x0, x9, #8
	mov	x19, x1
	mov	x1, x8
	mov	x21, x2
	mov	x2, x19
	bl	__RNvNtCsa0ttIZdYGZx_13perry_runtime7weakref33is_weak_branded_target_trace_slot
	mov	x2, x21
	mov	x1, x19
	mov	x8, x0
	mov	x0, x20
	tbz	w8, #0, LBB99_4
	b	LBB99_10
	.loh AdrpAdd	Lloh774, Lloh775
	.loh AdrpLdrGot	Lloh776, Lloh777
	.cfi_endproc
