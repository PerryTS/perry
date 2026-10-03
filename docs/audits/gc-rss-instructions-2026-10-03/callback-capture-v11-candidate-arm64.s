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
	ldrb	w8, [x0, #24]
	tbz	w8, #0, LBB169_4
	ldr	x9, [x0]
	cbz	x9, LBB169_4
	ldrb	w8, [x9]
	cmp	w8, #2
	b.ne	LBB169_4
	ldr	w8, [x9, #8]
	mov	w10, #65495
	add	w10, w8, w10
	cmp	w10, #3
	ccmp	w10, #1, #4, ls
	b.ne	LBB169_25
LBB169_4:
	mov	w8, #255
	bics	wzr, w8, w2
	b.eq	LBB169_6
Lloh1127:
	adrp	x8, __RNvNtNtCsa0ttIZdYGZx_13perry_runtime2gc9telemetry27LAYOUT_SCAN_TRACE_ARMED_ANY@PAGE
Lloh1128:
	add	x8, x8, __RNvNtNtCsa0ttIZdYGZx_13perry_runtime2gc9telemetry27LAYOUT_SCAN_TRACE_ARMED_ANY@PAGEOFF
	ldaprb	w8, [x8]
	cbnz	w8, LBB169_24
LBB169_6:
	ldr	x20, [x1]
	ldp	x21, x19, [x0, #8]
	ldrb	w22, [x0, #25]
	tbz	w22, #0, LBB169_8
LBB169_7:
	mov	x0, x20
	mov	x1, x21
	bl	__RNvNtNtCsa0ttIZdYGZx_13perry_runtime2gc10full_trace14observe_handle
	tbnz	w0, #0, LBB169_10
LBB169_8:
	and	x8, x20, #0xffff000000000000
	mov	x9, #9221683186994511872
	cmp	x8, x9
	mov	x9, #9222527611924643840
	ccmp	x8, x9, #4, ne
	mov	x9, #9223090561878065152
	ccmp	x8, x9, #4, ne
	b.ne	LBB169_11
	ands	x20, x20, #0xffffffffffff
	b.ne	LBB169_12
LBB169_10:
	ldp	x29, x30, [sp, #48]
	ldp	x20, x19, [sp, #32]
	ldp	x22, x21, [sp, #16]
	add	sp, sp, #64
	ret
LBB169_11:
	sub	x8, x20, #1, lsl #12
	mov	x9, #-4097
	movk	x9, #0, lsl #48
	cmp	x8, x9
	b.hi	LBB169_10
LBB169_12:
	str	x20, [sp, #8]
	add	x1, sp, #8
	mov	x0, x21
	bl	__RNvMs_NtNtCsa0ttIZdYGZx_13perry_runtime2gc5traceNtB4_15ValidPointerSet8contains
	cbz	w0, LBB169_10
	ldurb	w8, [x20, #-7]
	tbnz	w8, #0, LBB169_10
	sub	x21, x20, #8
	orr	w9, w8, #0x1
	sturb	w9, [x20, #-7]
	tbnz	w8, #7, LBB169_19
	ldrb	w8, [x21]
	cmp	w8, #26
	b.hs	LBB169_10
Lloh1129:
	adrp	x9, __RNvNtNtCsa0ttIZdYGZx_13perry_runtime2gc5types18GC_TYPE_INFO_BY_ID@GOTPAGE
Lloh1130:
	ldr	x9, [x9, __RNvNtNtCsa0ttIZdYGZx_13perry_runtime2gc5types18GC_TYPE_INFO_BY_ID@GOTPAGEOFF]
	add	x9, x9, x8, lsl #5
	ldrb	w10, [x9, #27]
	cmp	w10, #2
	b.eq	LBB169_10
	ldrb	w9, [x9, #17]
	cmp	w9, #2
	b.eq	LBB169_10
	tbz	w22, #0, LBB169_22
LBB169_19:
	ldr	x20, [x19, #16]
	ldr	x8, [x19]
	cmp	x20, x8
	b.ne	LBB169_21
	mov	x0, x19
	bl	__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecONtNtNtCsa0ttIZdYGZx_13perry_runtime2gc5types8GcHeaderE8grow_oneBT_
LBB169_21:
	ldr	x8, [x19, #8]
	str	x21, [x8, x20, lsl #3]
	add	x8, x20, #1
	str	x8, [x19, #16]
	ldp	x29, x30, [sp, #48]
	ldp	x20, x19, [sp, #32]
	ldp	x22, x21, [sp, #16]
	add	sp, sp, #64
	ret
LBB169_22:
	cmp	w8, #1
	b.ne	LBB169_19
	ldurh	w8, [x20, #-6]
	mov	w9, #49472
	and	w8, w8, w9
	cmp	w8, #4, lsl #12
	b.eq	LBB169_10
	b	LBB169_19
LBB169_24:
	mov	x19, x0
	mov	x0, x2
	mov	x20, x1
	bl	__RNvNtNtCsa0ttIZdYGZx_13perry_runtime2gc9telemetry35record_layout_child_slot_read_armed
	mov	x0, x19
	ldr	x20, [x20]
	ldp	x21, x19, [x19, #8]
	ldrb	w22, [x0, #25]
	tbnz	w22, #0, LBB169_7
	b	LBB169_8
LBB169_25:
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
	tbz	w8, #0, LBB169_4
	b	LBB169_10
	.loh AdrpAdd	Lloh1127, Lloh1128
	.loh AdrpLdrGot	Lloh1129, Lloh1130
	.cfi_endproc
