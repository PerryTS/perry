
benchmarks/json_performance/.work/dispatch-r5/worker:	file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100845800 <_js_json_stringify_full>:
100845800:     	sub	sp, sp, #0x90
100845804:     	stp	d11, d10, [sp, #0x30]
100845808:     	stp	d9, d8, [sp, #0x40]
10084580c:     	stp	x24, x23, [sp, #0x50]
100845810:     	stp	x22, x21, [sp, #0x60]
100845814:     	stp	x20, x19, [sp, #0x70]
100845818:     	stp	x29, x30, [sp, #0x80]
10084581c:     	add	x29, sp, #0x80
100845820:     	mov	x8, #0x1                ; =1
100845824:     	movk	x8, #0x7ffc, lsl #48
100845828:     	fmov	x9, d1
10084582c:     	fmov	x10, d2
100845830:     	add	x11, x8, #0x1
100845834:     	cmp	x9, x11
100845838:     	b.ne	0x100845850 <_js_json_stringify_full+0x50>
10084583c:     	mov	x8, #-0x7ffc000000000001 ; =-9222246136947933185
100845840:     	add	x8, x10, x8
100845844:     	cmp	x8, #0x3
100845848:     	b.lo	0x10084587c <_js_json_stringify_full+0x7c>
10084584c:     	b	0x100845e34 <_js_json_stringify_full+0x634>
100845850:     	mov	x11, #-0x7ffc000000000001 ; =-9222246136947933185
100845854:     	add	x11, x10, x11
100845858:     	cmp	x11, #0x1
10084585c:     	b.hi	0x10084586c <_js_json_stringify_full+0x6c>
100845860:     	cmp	x9, x8
100845864:     	b.eq	0x10084587c <_js_json_stringify_full+0x7c>
100845868:     	b	0x100845e34 <_js_json_stringify_full+0x634>
10084586c:     	add	x11, x8, #0x2
100845870:     	cmp	x10, x11
100845874:     	ccmp	x9, x8, #0x0, eq
100845878:     	b.ne	0x100845e34 <_js_json_stringify_full+0x634>
10084587c:     	fmov	x8, d0
100845880:     	mov	x9, #-0x2               ; =-2
100845884:     	movk	x9, #0x8003, lsl #48
100845888:     	add	x9, x8, x9
10084588c:     	cmp	x9, #0x3
100845890:     	b.hs	0x1008458c0 <_js_json_stringify_full+0xc0>
100845894:     	adrp	x8, 0x100c98000 <__RNvNvNtNtCseICAOKjEbXS_13perry_runtime6string6format13fmt_fixed_int5POW10+0x4f10>
100845898:     	add	x8, x8, #0x678
10084589c:     	ldr	x0, [x8, x9, lsl #3]
1008458a0:     	ldp	x29, x30, [sp, #0x80]
1008458a4:     	ldp	x20, x19, [sp, #0x70]
1008458a8:     	ldp	x22, x21, [sp, #0x60]
1008458ac:     	ldp	x24, x23, [sp, #0x50]
1008458b0:     	ldp	d9, d8, [sp, #0x40]
1008458b4:     	ldp	d11, d10, [sp, #0x30]
1008458b8:     	add	sp, sp, #0x90
1008458bc:     	ret
1008458c0:     	strb	wzr, [sp, #0x4]
1008458c4:     	str	wzr, [sp]
1008458c8:     	and	x9, x8, #0xffff000000000000
1008458cc:     	mov	x10, #0x7fff000000000000 ; =9223090561878065152
1008458d0:     	cmp	x9, x10
1008458d4:     	b.eq	0x100845948 <_js_json_stringify_full+0x148>
1008458d8:     	mov	x10, #0x7ff9000000000000 ; =9221401712017801216
1008458dc:     	cmp	x9, x10
1008458e0:     	b.ne	0x100845d3c <_js_json_stringify_full+0x53c>
1008458e4:     	ubfx	x11, x8, #40, #8
1008458e8:     	cbz	x11, 0x100845938 <_js_json_stringify_full+0x138>
1008458ec:     	strb	w8, [sp]
1008458f0:     	cmp	x11, #0x1
1008458f4:     	b.eq	0x100845938 <_js_json_stringify_full+0x138>
1008458f8:     	lsr	x10, x8, #8
1008458fc:     	strb	w10, [sp, #0x1]
100845900:     	cmp	x11, #0x2
100845904:     	b.eq	0x100845938 <_js_json_stringify_full+0x138>
100845908:     	lsr	x10, x8, #16
10084590c:     	strb	w10, [sp, #0x2]
100845910:     	cmp	x11, #0x3
100845914:     	b.eq	0x100845938 <_js_json_stringify_full+0x138>
100845918:     	lsr	x10, x8, #24
10084591c:     	strb	w10, [sp, #0x3]
100845920:     	cmp	x11, #0x4
100845924:     	b.eq	0x100845938 <_js_json_stringify_full+0x138>
100845928:     	lsr	x10, x8, #32
10084592c:     	strb	w10, [sp, #0x4]
100845930:     	cmp	x11, #0x5
100845934:     	b.ne	0x100846168 <_js_json_stringify_full+0x968>
100845938:     	mov	x13, sp
10084593c:     	cmp	w11, #0x3
100845940:     	b.ls	0x100845960 <_js_json_stringify_full+0x160>
100845944:     	b	0x100845d3c <_js_json_stringify_full+0x53c>
100845948:     	ands	x10, x8, #0xffffffffffff
10084594c:     	b.eq	0x100845e34 <_js_json_stringify_full+0x634>
100845950:     	add	x13, x10, #0x14
100845954:     	ldr	w11, [x10, #0x4]
100845958:     	cmp	w11, #0x3
10084595c:     	b.hi	0x100845d3c <_js_json_stringify_full+0x53c>
100845960:     	stur	wzr, [sp, #0x9]
100845964:     	mov	w10, #0x22              ; =34
100845968:     	strb	w10, [sp, #0x8]
10084596c:     	cbz	w11, 0x1008459a4 <_js_json_stringify_full+0x1a4>
100845970:     	add	x12, sp, #0x8
100845974:     	ldrb	w14, [x13]
100845978:     	sxtb	w16, w14
10084597c:     	cmp	w14, #0xb
100845980:     	b.le	0x1008459ac <_js_json_stringify_full+0x1ac>
100845984:     	cmp	w14, #0x21
100845988:     	b.gt	0x1008459cc <_js_json_stringify_full+0x1cc>
10084598c:     	cmp	w14, #0xc
100845990:     	b.eq	0x100845a00 <_js_json_stringify_full+0x200>
100845994:     	cmp	w14, #0xd
100845998:     	b.ne	0x1008459dc <_js_json_stringify_full+0x1dc>
10084599c:     	mov	w16, #0x72              ; =114
1008459a0:     	b	0x100845a0c <_js_json_stringify_full+0x20c>
1008459a4:     	mov	w12, #0x1               ; =1
1008459a8:     	b	0x100845a34 <_js_json_stringify_full+0x234>
1008459ac:     	cmp	w14, #0x8
1008459b0:     	b.eq	0x1008459f8 <_js_json_stringify_full+0x1f8>
1008459b4:     	cmp	w14, #0x9
1008459b8:     	b.eq	0x100845a08 <_js_json_stringify_full+0x208>
1008459bc:     	cmp	w14, #0xa
1008459c0:     	b.ne	0x1008459dc <_js_json_stringify_full+0x1dc>
1008459c4:     	mov	w16, #0x6e              ; =110
1008459c8:     	b	0x100845a0c <_js_json_stringify_full+0x20c>
1008459cc:     	cmp	w14, #0x22
1008459d0:     	b.eq	0x100845a0c <_js_json_stringify_full+0x20c>
1008459d4:     	cmp	w14, #0x5c
1008459d8:     	b.eq	0x100845a0c <_js_json_stringify_full+0x20c>
1008459dc:     	cmp	w16, #0x20
1008459e0:     	b.lt	0x100845d3c <_js_json_stringify_full+0x53c>
1008459e4:     	mov	w15, #0x0               ; =0
1008459e8:     	add	x14, x12, #0x2
1008459ec:     	mov	w12, #0x2               ; =2
1008459f0:     	mov	w17, #0x1               ; =1
1008459f4:     	b	0x100845a24 <_js_json_stringify_full+0x224>
1008459f8:     	mov	w16, #0x62              ; =98
1008459fc:     	b	0x100845a0c <_js_json_stringify_full+0x20c>
100845a00:     	mov	w16, #0x66              ; =102
100845a04:     	b	0x100845a0c <_js_json_stringify_full+0x20c>
100845a08:     	mov	w16, #0x74              ; =116
100845a0c:     	add	x14, x12, #0x3
100845a10:     	mov	w12, #0x5c              ; =92
100845a14:     	strb	w12, [sp, #0x9]
100845a18:     	mov	w15, #0x1               ; =1
100845a1c:     	mov	w12, #0x3               ; =3
100845a20:     	mov	w17, #0x2               ; =2
100845a24:     	add	x0, sp, #0x8
100845a28:     	strb	w16, [x0, x17]
100845a2c:     	cmp	w11, #0x1
100845a30:     	b.ne	0x100845a74 <_js_json_stringify_full+0x274>
100845a34:     	add	x9, sp, #0x8
100845a38:     	strb	w10, [x9, x12]
100845a3c:     	add	x8, x12, #0x1
100845a40:     	cmp	x12, #0x3
100845a44:     	b.hs	0x100845a58 <_js_json_stringify_full+0x258>
100845a48:     	mov	x15, #0x0               ; =0
100845a4c:     	mov	x11, #0x0               ; =0
100845a50:     	add	x10, sp, #0x8
100845a54:     	b	0x100845cac <_js_json_stringify_full+0x4ac>
100845a58:     	adrp	x14, 0x100c98000 <__RNvNvNtNtCseICAOKjEbXS_13perry_runtime6string6format13fmt_fixed_int5POW10+0x4f10>
100845a5c:     	adrp	x13, 0x100c38000 <__RNvCs8xF4iOrs9m2_4itoa13DECIMAL_PAIRS+0x3373a>
100845a60:     	cmp	x12, #0xf
100845a64:     	b.hs	0x100845aa4 <_js_json_stringify_full+0x2a4>
100845a68:     	mov	x11, #0x0               ; =0
100845a6c:     	mov	x15, #0x0               ; =0
100845a70:     	b	0x100845c10 <_js_json_stringify_full+0x410>
100845a74:     	ldrb	w17, [x13, #0x1]
100845a78:     	sxtb	w16, w17
100845a7c:     	cmp	w17, #0xb
100845a80:     	b.le	0x100845cdc <_js_json_stringify_full+0x4dc>
100845a84:     	cmp	w17, #0x21
100845a88:     	b.gt	0x100845cfc <_js_json_stringify_full+0x4fc>
100845a8c:     	cmp	w17, #0xc
100845a90:     	b.eq	0x100845d2c <_js_json_stringify_full+0x52c>
100845a94:     	cmp	w17, #0xd
100845a98:     	b.ne	0x100845d0c <_js_json_stringify_full+0x50c>
100845a9c:     	mov	w16, #0x72              ; =114
100845aa0:     	b	0x100845d38 <_js_json_stringify_full+0x538>
100845aa4:     	and	x12, x8, #0xc
100845aa8:     	and	x11, x8, #0x10
100845aac:     	add	x15, sp, #0x8
100845ab0:     	add	x10, x15, x11
100845ab4:     	adrp	x16, 0x100c99000 <__RNvNvNtNtCseICAOKjEbXS_13perry_runtime6string6format13fmt_fixed_int5POW10+0x5f10>
100845ab8:     	ldr	q0, [x16, #0x7f0]
100845abc:     	adrp	x16, 0x100c99000 <__RNvNvNtNtCseICAOKjEbXS_13perry_runtime6string6format13fmt_fixed_int5POW10+0x5f10>
100845ac0:     	ldr	q1, [x16, #0x800]
100845ac4:     	adrp	x16, 0x100c99000 <__RNvNvNtNtCseICAOKjEbXS_13perry_runtime6string6format13fmt_fixed_int5POW10+0x5f10>
100845ac8:     	ldr	q2, [x16, #0x810]
100845acc:     	adrp	x16, 0x100c99000 <__RNvNvNtNtCseICAOKjEbXS_13perry_runtime6string6format13fmt_fixed_int5POW10+0x5f10>
100845ad0:     	ldr	q3, [x16, #0x820]
100845ad4:     	adrp	x16, 0x100c99000 <__RNvNvNtNtCseICAOKjEbXS_13perry_runtime6string6format13fmt_fixed_int5POW10+0x5f10>
100845ad8:     	ldr	q4, [x16, #0x830]
100845adc:     	movi.2d	v5, #0000000000000000
100845ae0:     	adrp	x16, 0x100c99000 <__RNvNvNtNtCseICAOKjEbXS_13perry_runtime6string6format13fmt_fixed_int5POW10+0x5f10>
100845ae4:     	ldr	q6, [x16, #0x840]
100845ae8:     	mov	w16, #0x10              ; =16
100845aec:     	dup.2d	v7, x16
100845af0:     	and	x16, x8, #0x10
100845af4:     	movi.2d	v16, #0000000000000000
100845af8:     	ldr	q19, [x14, #0x960]
100845afc:     	movi.2d	v18, #0000000000000000
100845b00:     	movi.2d	v17, #0000000000000000
100845b04:     	ldr	q23, [x13, #0x840]
100845b08:     	movi.2d	v22, #0000000000000000
100845b0c:     	movi.2d	v20, #0000000000000000
100845b10:     	movi.2d	v24, #0000000000000000
100845b14:     	movi.2d	v21, #0000000000000000
100845b18:     	ldr	q25, [x15], #0x10
100845b1c:     	ushll.8h	v26, v25, #0x0
100845b20:     	ushll2.4s	v27, v26, #0x0
100845b24:     	ushll2.2d	v28, v27, #0x0
100845b28:     	ushll2.8h	v25, v25, #0x0
100845b2c:     	ushll.4s	v29, v25, #0x0
100845b30:     	ushll2.2d	v30, v29, #0x0
100845b34:     	ushll.2d	v29, v29, #0x0
100845b38:     	ushll.2d	v27, v27, #0x0
100845b3c:     	ushll.4s	v26, v26, #0x0
100845b40:     	ushll2.2d	v31, v26, #0x0
100845b44:     	ushll2.4s	v25, v25, #0x0
100845b48:     	ushll.2d	v8, v25, #0x0
100845b4c:     	ushll.2d	v26, v26, #0x0
100845b50:     	ushll2.2d	v25, v25, #0x0
100845b54:     	shl.2d	v9, v0, #0x3
100845b58:     	ushl.2d	v25, v25, v9
100845b5c:     	shl.2d	v9, v23, #0x3
100845b60:     	ushl.2d	v26, v26, v9
100845b64:     	shl.2d	v9, v1, #0x3
100845b68:     	ushl.2d	v8, v8, v9
100845b6c:     	shl.2d	v9, v19, #0x3
100845b70:     	ushl.2d	v31, v31, v9
100845b74:     	shl.2d	v9, v6, #0x3
100845b78:     	ushl.2d	v27, v27, v9
100845b7c:     	shl.2d	v9, v3, #0x3
100845b80:     	ushl.2d	v29, v29, v9
100845b84:     	shl.2d	v9, v2, #0x3
100845b88:     	ushl.2d	v30, v30, v9
100845b8c:     	shl.2d	v9, v4, #0x3
100845b90:     	ushl.2d	v28, v28, v9
100845b94:     	orr.16b	v17, v28, v17
100845b98:     	orr.16b	v20, v30, v20
100845b9c:     	orr.16b	v22, v29, v22
100845ba0:     	orr.16b	v18, v27, v18
100845ba4:     	orr.16b	v5, v31, v5
100845ba8:     	orr.16b	v24, v8, v24
100845bac:     	orr.16b	v16, v26, v16
100845bb0:     	orr.16b	v21, v25, v21
100845bb4:     	add.2d	v6, v6, v7
100845bb8:     	add.2d	v19, v19, v7
100845bbc:     	add.2d	v23, v23, v7
100845bc0:     	add.2d	v4, v4, v7
100845bc4:     	add.2d	v3, v3, v7
100845bc8:     	add.2d	v2, v2, v7
100845bcc:     	add.2d	v1, v1, v7
100845bd0:     	add.2d	v0, v0, v7
100845bd4:     	subs	x16, x16, #0x10
100845bd8:     	b.ne	0x100845b18 <_js_json_stringify_full+0x318>
100845bdc:     	orr.16b	v0, v16, v22
100845be0:     	orr.16b	v1, v18, v24
100845be4:     	orr.16b	v0, v0, v1
100845be8:     	orr.16b	v1, v5, v20
100845bec:     	orr.16b	v2, v17, v21
100845bf0:     	orr.16b	v1, v1, v2
100845bf4:     	orr.16b	v0, v0, v1
100845bf8:     	mov	d1, v0[1]
100845bfc:     	orr.8b	v0, v0, v1
100845c00:     	fmov	x15, d0
100845c04:     	cmp	x8, x11
100845c08:     	b.eq	0x100845ccc <_js_json_stringify_full+0x4cc>
100845c0c:     	cbz	x12, 0x100845cac <_js_json_stringify_full+0x4ac>
100845c10:     	mov	x16, x11
100845c14:     	and	x11, x8, #0x1c
100845c18:     	add	x17, sp, #0x8
100845c1c:     	add	x10, x17, x11
100845c20:     	fmov	d0, x15
100845c24:     	movi.2d	v1, #0000000000000000
100845c28:     	dup.2d	v3, x16
100845c2c:     	ldr	q2, [x14, #0x960]
100845c30:     	orr.16b	v2, v3, v2
100845c34:     	ldr	q4, [x13, #0x840]
100845c38:     	orr.16b	v3, v3, v4
100845c3c:     	sub	x12, x16, x11
100845c40:     	add	x13, x17, x16
100845c44:     	movi.2d	v4, #0x000000000000ff
100845c48:     	mov	w14, #0x4               ; =4
100845c4c:     	dup.2d	v5, x14
100845c50:     	ldr	s6, [x13], #0x4
100845c54:     	ushll.8h	v6, v6, #0x0
100845c58:     	ushll.4s	v6, v6, #0x0
100845c5c:     	ushll2.2d	v7, v6, #0x0
100845c60:     	and.16b	v7, v7, v4
100845c64:     	ushll.2d	v6, v6, #0x0
100845c68:     	and.16b	v6, v6, v4
100845c6c:     	shl.2d	v16, v2, #0x3
100845c70:     	shl.2d	v17, v3, #0x3
100845c74:     	ushl.2d	v6, v6, v17
100845c78:     	ushl.2d	v7, v7, v16
100845c7c:     	orr.16b	v1, v7, v1
100845c80:     	orr.16b	v0, v6, v0
100845c84:     	add.2d	v2, v2, v5
100845c88:     	add.2d	v3, v3, v5
100845c8c:     	adds	x12, x12, #0x4
100845c90:     	b.ne	0x100845c50 <_js_json_stringify_full+0x450>
100845c94:     	orr.16b	v0, v0, v1
100845c98:     	mov	d1, v0[1]
100845c9c:     	orr.8b	v0, v0, v1
100845ca0:     	fmov	x15, d0
100845ca4:     	cmp	x8, x11
100845ca8:     	b.eq	0x100845ccc <_js_json_stringify_full+0x4cc>
100845cac:     	add	x9, x9, x8
100845cb0:     	lsl	x11, x11, #3
100845cb4:     	ldrb	w12, [x10], #0x1
100845cb8:     	lsl	x12, x12, x11
100845cbc:     	orr	x15, x12, x15
100845cc0:     	add	x11, x11, #0x8
100845cc4:     	cmp	x10, x9
100845cc8:     	b.ne	0x100845cb4 <_js_json_stringify_full+0x4b4>
100845ccc:     	orr	x8, x15, x8, lsl #40
100845cd0:     	mov	x9, #0x7ff9000000000000 ; =9221401712017801216
100845cd4:     	orr	x0, x8, x9
100845cd8:     	b	0x1008458a0 <_js_json_stringify_full+0xa0>
100845cdc:     	cmp	w17, #0x8
100845ce0:     	b.eq	0x100845d24 <_js_json_stringify_full+0x524>
100845ce4:     	cmp	w17, #0x9
100845ce8:     	b.eq	0x100845d34 <_js_json_stringify_full+0x534>
100845cec:     	cmp	w17, #0xa
100845cf0:     	b.ne	0x100845d0c <_js_json_stringify_full+0x50c>
100845cf4:     	mov	w16, #0x6e              ; =110
100845cf8:     	b	0x100845d38 <_js_json_stringify_full+0x538>
100845cfc:     	cmp	w17, #0x22
100845d00:     	b.eq	0x100845d38 <_js_json_stringify_full+0x538>
100845d04:     	cmp	w17, #0x5c
100845d08:     	b.eq	0x100845d38 <_js_json_stringify_full+0x538>
100845d0c:     	cmp	w16, #0x20
100845d10:     	b.lt	0x100845d3c <_js_json_stringify_full+0x53c>
100845d14:     	add	x14, sp, #0x8
100845d18:     	strb	w16, [x14, x12]
100845d1c:     	add	x12, x12, #0x1
100845d20:     	b	0x100845e68 <_js_json_stringify_full+0x668>
100845d24:     	mov	w16, #0x62              ; =98
100845d28:     	b	0x100845d38 <_js_json_stringify_full+0x538>
100845d2c:     	mov	w16, #0x66              ; =102
100845d30:     	b	0x100845d38 <_js_json_stringify_full+0x538>
100845d34:     	mov	w16, #0x74              ; =116
100845d38:     	tbz	w15, #0x0, 0x100845e54 <_js_json_stringify_full+0x654>
100845d3c:     	mov	x10, #0x7ffd000000000000 ; =9222527611924643840
100845d40:     	cmp	x9, x10
100845d44:     	b.eq	0x100845da8 <_js_json_stringify_full+0x5a8>
100845d48:     	mov	x10, #0x7fff000000000000 ; =9223090561878065152
100845d4c:     	cmp	x9, x10
100845d50:     	b.ne	0x100845e34 <_js_json_stringify_full+0x634>
100845d54:     	and	x0, x8, #0xffffffffffff
100845d58:     	sub	x8, x0, #0x100, lsl #12 ; =0x100000
100845d5c:     	mov	x9, #0x7fffffffffff     ; =140737488355327
100845d60:     	movk	x9, #0xffef, lsl #16
100845d64:     	cmp	x8, x9
100845d68:     	b.hi	0x100845e34 <_js_json_stringify_full+0x634>
100845d6c:     	ldr	w8, [x0, #0x4]
100845d70:     	cmp	w8, #0x40
100845d74:     	b.lo	0x100845e34 <_js_json_stringify_full+0x634>
100845d78:     	mov.16b	v8, v2
100845d7c:     	mov.16b	v10, v1
100845d80:     	mov.16b	v9, v0
100845d84:     	bl	0x1004f01e8 <__RNvNtNtCseICAOKjEbXS_13perry_runtime4json16stringify_string17quote_heap_string>
100845d88:     	mov.16b	v0, v9
100845d8c:     	mov.16b	v1, v10
100845d90:     	mov.16b	v2, v8
100845d94:     	cmp	x0, #0x1
100845d98:     	b.ne	0x100845e34 <_js_json_stringify_full+0x634>
100845d9c:     	mov	x0, #0x7fff000000000000 ; =9223090561878065152
100845da0:     	bfxil	x0, x1, #0, #48
100845da4:     	b	0x1008458a0 <_js_json_stringify_full+0xa0>
100845da8:     	and	x19, x8, #0xffffffffffff
100845dac:     	mov	x0, x19
100845db0:     	mov.16b	v8, v2
100845db4:     	mov.16b	v9, v1
100845db8:     	mov.16b	v10, v0
100845dbc:     	bl	0x10057a9c0 <__RNvNtNtCseICAOKjEbXS_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100845dc0:     	cbz	x0, 0x100845df4 <_js_json_stringify_full+0x5f4>
100845dc4:     	ldrb	w8, [x0]
100845dc8:     	cmp	w8, #0x2
100845dcc:     	b.ne	0x100845df4 <_js_json_stringify_full+0x5f4>
100845dd0:     	ldrsb	w8, [x0, #0x1]
100845dd4:     	tbnz	w8, #0x1f, 0x100845df4 <_js_json_stringify_full+0x5f4>
100845dd8:     	ldrh	w8, [x0, #0x2]
100845ddc:     	tbnz	w8, #0xb, 0x100845df4 <_js_json_stringify_full+0x5f4>
100845de0:     	ldr	w8, [x0, #0x4]
100845de4:     	cmp	w8, #0x18
100845de8:     	b.lo	0x100845df4 <_js_json_stringify_full+0x5f4>
100845dec:     	ldr	w8, [x19]
100845df0:     	cbz	w8, 0x100845f28 <_js_json_stringify_full+0x728>
100845df4:     	mov	x0, x19
100845df8:     	bl	0x10057a9c0 <__RNvNtNtCseICAOKjEbXS_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100845dfc:     	mov.16b	v0, v10
100845e00:     	mov.16b	v1, v9
100845e04:     	mov.16b	v2, v8
100845e08:     	cbz	x0, 0x100845e34 <_js_json_stringify_full+0x634>
100845e0c:     	ldrb	w8, [x0]
100845e10:     	cmp	w8, #0x2
100845e14:     	b.ne	0x100845e34 <_js_json_stringify_full+0x634>
100845e18:     	ldrh	w8, [x0, #0x2]
100845e1c:     	tbnz	w8, #0xb, 0x100845e34 <_js_json_stringify_full+0x634>
100845e20:     	ldr	w8, [x0, #0x4]
100845e24:     	cmp	w8, #0x18
100845e28:     	b.lo	0x100845e34 <_js_json_stringify_full+0x634>
100845e2c:     	ldr	w8, [x19]
100845e30:     	cbz	w8, 0x100845ec0 <_js_json_stringify_full+0x6c0>
100845e34:     	ldp	x29, x30, [sp, #0x80]
100845e38:     	ldp	x20, x19, [sp, #0x70]
100845e3c:     	ldp	x22, x21, [sp, #0x60]
100845e40:     	ldp	x24, x23, [sp, #0x50]
100845e44:     	ldp	d9, d8, [sp, #0x40]
100845e48:     	ldp	d11, d10, [sp, #0x30]
100845e4c:     	add	sp, sp, #0x90
100845e50:     	b	0x1004fcc78 <__RNvNtNtCseICAOKjEbXS_13perry_runtime4json8replacer19stringify_full_slow>
100845e54:     	add	x15, sp, #0x8
100845e58:     	mov	w17, #0x5c              ; =92
100845e5c:     	strb	w17, [x15, x12]
100845e60:     	add	x12, x12, #0x2
100845e64:     	strb	w16, [x14, #0x1]
100845e68:     	cmp	w11, #0x2
100845e6c:     	b.eq	0x100845a34 <_js_json_stringify_full+0x234>
100845e70:     	ldrb	w13, [x13, #0x2]
100845e74:     	sxtb	w11, w13
100845e78:     	cmp	w13, #0xb
100845e7c:     	b.le	0x100845ea0 <_js_json_stringify_full+0x6a0>
100845e80:     	cmp	w13, #0x21
100845e84:     	b.gt	0x100845ef8 <_js_json_stringify_full+0x6f8>
100845e88:     	cmp	w13, #0xc
100845e8c:     	b.eq	0x100845f50 <_js_json_stringify_full+0x750>
100845e90:     	cmp	w13, #0xd
100845e94:     	b.ne	0x100845f08 <_js_json_stringify_full+0x708>
100845e98:     	mov	w11, #0x72              ; =114
100845e9c:     	b	0x100845f5c <_js_json_stringify_full+0x75c>
100845ea0:     	cmp	w13, #0x8
100845ea4:     	b.eq	0x100845f48 <_js_json_stringify_full+0x748>
100845ea8:     	cmp	w13, #0x9
100845eac:     	b.eq	0x100845f58 <_js_json_stringify_full+0x758>
100845eb0:     	cmp	w13, #0xa
100845eb4:     	b.ne	0x100845f08 <_js_json_stringify_full+0x708>
100845eb8:     	mov	w11, #0x6e              ; =110
100845ebc:     	b	0x100845f5c <_js_json_stringify_full+0x75c>
100845ec0:     	mov	x21, x0
100845ec4:     	mov	x0, x19
100845ec8:     	bl	0x1003430c0 <__RNvNtCseICAOKjEbXS_13perry_runtime6object17object_keys_array>
100845ecc:     	cbz	x0, 0x100845f80 <_js_json_stringify_full+0x780>
100845ed0:     	ldr	w20, [x0]
100845ed4:     	cmp	w20, #0x4
100845ed8:     	mov.16b	v2, v8
100845edc:     	mov.16b	v1, v9
100845ee0:     	mov.16b	v0, v10
100845ee4:     	b.hi	0x100845e34 <_js_json_stringify_full+0x634>
100845ee8:     	ldr	w8, [x0, #0x4]
100845eec:     	cmp	w20, w8
100845ef0:     	b.hi	0x100845e34 <_js_json_stringify_full+0x634>
100845ef4:     	b	0x100845f90 <_js_json_stringify_full+0x790>
100845ef8:     	cmp	w13, #0x22
100845efc:     	b.eq	0x100845f5c <_js_json_stringify_full+0x75c>
100845f00:     	cmp	w13, #0x5c
100845f04:     	b.eq	0x100845f5c <_js_json_stringify_full+0x75c>
100845f08:     	cmp	x12, #0x3
100845f0c:     	mov	w13, #0x20              ; =32
100845f10:     	ccmp	w11, w13, #0x8, ls
100845f14:     	b.lt	0x100845d3c <_js_json_stringify_full+0x53c>
100845f18:     	add	x8, sp, #0x8
100845f1c:     	strb	w11, [x8, x12]
100845f20:     	add	x12, x12, #0x1
100845f24:     	b	0x100845a34 <_js_json_stringify_full+0x234>
100845f28:     	mov	x1, x0
100845f2c:     	mov	x0, x19
100845f30:     	mov	x21, x1
100845f34:     	bl	0x1004f5e00 <__RNvNtNtCseICAOKjEbXS_13perry_runtime4json23stringify_record_output20emit_repeated_output>
100845f38:     	cmp	x0, #0x1
100845f3c:     	b.ne	0x100846050 <_js_json_stringify_full+0x850>
100845f40:     	mov	x0, x1
100845f44:     	b	0x1008458a0 <_js_json_stringify_full+0xa0>
100845f48:     	mov	w11, #0x62              ; =98
100845f4c:     	b	0x100845f5c <_js_json_stringify_full+0x75c>
100845f50:     	mov	w11, #0x66              ; =102
100845f54:     	b	0x100845f5c <_js_json_stringify_full+0x75c>
100845f58:     	mov	w11, #0x74              ; =116
100845f5c:     	cmp	x12, #0x2
100845f60:     	b.hi	0x100845d3c <_js_json_stringify_full+0x53c>
100845f64:     	add	x8, sp, #0x8
100845f68:     	add	x8, x8, x12
100845f6c:     	add	x12, x12, #0x2
100845f70:     	mov	w9, #0x5c               ; =92
100845f74:     	strb	w9, [x8]
100845f78:     	strb	w11, [x8, #0x1]
100845f7c:     	b	0x100845a34 <_js_json_stringify_full+0x234>
100845f80:     	mov	x20, #0x0               ; =0
100845f84:     	mov.16b	v2, v8
100845f88:     	mov.16b	v1, v9
100845f8c:     	mov.16b	v0, v10
100845f90:     	ldr	w8, [x21, #0x4]
100845f94:     	lsl	x9, x20, #3
100845f98:     	add	x9, x9, #0x18
100845f9c:     	cmp	x9, x8
100845fa0:     	b.hi	0x100845e34 <_js_json_stringify_full+0x634>
100845fa4:     	ldr	w8, [x19, #0x4]
100845fa8:     	mov	w9, #-0x40000000        ; =-1073741824
100845fac:     	cmp	w8, w9
100845fb0:     	csel	w8, w8, wzr, lt
100845fb4:     	mov	x21, x0
100845fb8:     	mov	x0, x8
100845fbc:     	bl	0x1005e56e4 <__RNvNtNtCseICAOKjEbXS_13perry_runtime6object6shapes34shape_live_inline_slot_count_by_id>
100845fc0:     	mov.16b	v0, v10
100845fc4:     	mov.16b	v1, v9
100845fc8:     	mov.16b	v2, v8
100845fcc:     	mov	w9, #0x2                ; =2
100845fd0:     	cmp	w1, #0x2
100845fd4:     	csel	w10, w1, w9, hi
100845fd8:     	tst	w0, #0x1
100845fdc:     	csel	x8, x10, x9, ne
100845fe0:     	cmp	x20, x8
100845fe4:     	b.hi	0x100845e34 <_js_json_stringify_full+0x634>
100845fe8:     	cbz	x20, 0x1008460fc <_js_json_stringify_full+0x8fc>
100845fec:     	mov	x0, x21
100845ff0:     	mov	x1, x20
100845ff4:     	bl	0x1004ed400 <__RNvNtNtCseICAOKjEbXS_13perry_runtime4json14stringify_flat22bounded_keys_are_dense>
100845ff8:     	mov.16b	v2, v8
100845ffc:     	mov.16b	v1, v9
100846000:     	mov.16b	v0, v10
100846004:     	tbz	w0, #0x0, 0x100845e34 <_js_json_stringify_full+0x634>
100846008:     	cmp	x20, #0x1
10084600c:     	b.eq	0x100846118 <_js_json_stringify_full+0x918>
100846010:     	cmp	x20, #0x2
100846014:     	b.ne	0x100846140 <_js_json_stringify_full+0x940>
100846018:     	mov	x21, #0x0               ; =0
10084601c:     	add	x22, x19, #0x10
100846020:     	mov	w8, #0x1                ; =1
100846024:     	mov	x23, x8
100846028:     	ldr	x1, [x22, x21, lsl #3]
10084602c:     	add	x0, sp, #0x8
100846030:     	bl	0x1004ed454 <__RNvNtNtCseICAOKjEbXS_13perry_runtime4json14stringify_flat25parsed_plain_string_piece>
100846034:     	ldr	w8, [sp, #0x8]
100846038:     	cmn	w8, #0x1
10084603c:     	b.ne	0x100846124 <_js_json_stringify_full+0x924>
100846040:     	mov	w8, #0x0                ; =0
100846044:     	mov	w21, #0x1               ; =1
100846048:     	tbnz	w23, #0x0, 0x100846024 <_js_json_stringify_full+0x824>
10084604c:     	b	0x100846140 <_js_json_stringify_full+0x940>
100846050:     	mov	x0, x19
100846054:     	bl	0x1003430c0 <__RNvNtCseICAOKjEbXS_13perry_runtime6object17object_keys_array>
100846058:     	cbz	x0, 0x1008460e4 <_js_json_stringify_full+0x8e4>
10084605c:     	ldr	w20, [x0]
100846060:     	cbz	w20, 0x1008460e4 <_js_json_stringify_full+0x8e4>
100846064:     	cmp	w20, #0x8
100846068:     	b.hi	0x100845df4 <_js_json_stringify_full+0x5f4>
10084606c:     	ldr	w8, [x0, #0x4]
100846070:     	cmp	w20, w8
100846074:     	b.hi	0x100845df4 <_js_json_stringify_full+0x5f4>
100846078:     	ldr	w8, [x21, #0x4]
10084607c:     	lsl	x9, x20, #3
100846080:     	add	x9, x9, #0x18
100846084:     	cmp	x9, x8
100846088:     	b.hi	0x100845df4 <_js_json_stringify_full+0x5f4>
10084608c:     	ldr	w8, [x19, #0x4]
100846090:     	mov	w9, #-0x40000000        ; =-1073741824
100846094:     	cmp	w8, w9
100846098:     	csel	w8, w8, wzr, lt
10084609c:     	mov	x21, x0
1008460a0:     	mov	x0, x8
1008460a4:     	bl	0x1005e56e4 <__RNvNtNtCseICAOKjEbXS_13perry_runtime6object6shapes34shape_live_inline_slot_count_by_id>
1008460a8:     	mov	w9, #0x2                ; =2
1008460ac:     	cmp	w1, #0x2
1008460b0:     	csel	w10, w1, w9, hi
1008460b4:     	tst	w0, #0x1
1008460b8:     	csel	w8, w10, w9, ne
1008460bc:     	cmp	w20, w8
1008460c0:     	b.hi	0x100845df4 <_js_json_stringify_full+0x5f4>
1008460c4:     	mov	x0, x21
1008460c8:     	mov	x1, x20
1008460cc:     	bl	0x1004ed400 <__RNvNtNtCseICAOKjEbXS_13perry_runtime4json14stringify_flat22bounded_keys_are_dense>
1008460d0:     	cbz	w0, 0x100845df4 <_js_json_stringify_full+0x5f4>
1008460d4:     	mov	x0, x19
1008460d8:     	mov	x1, x20
1008460dc:     	bl	0x1004f4f40 <__RNvNtNtCseICAOKjEbXS_13perry_runtime4json23stringify_record_output11emit_record>
1008460e0:     	b	0x1008460ec <_js_json_stringify_full+0x8ec>
1008460e4:     	mov	x0, x19
1008460e8:     	bl	0x1004f5cc0 <__RNvNtNtCseICAOKjEbXS_13perry_runtime4json23stringify_record_output17emit_empty_object>
1008460ec:     	mov	x8, x0
1008460f0:     	mov	x0, x1
1008460f4:     	tbnz	w8, #0x0, 0x1008458a0 <_js_json_stringify_full+0xa0>
1008460f8:     	b	0x100845df4 <_js_json_stringify_full+0x5f4>
1008460fc:     	mov	x0, x19
100846100:     	bl	0x1004f4d70 <__RNvNtNtCseICAOKjEbXS_13perry_runtime4json22stringify_tojson_probe36to_json_definitely_absent_without_gc>
100846104:     	mov	w8, w0
100846108:     	mov	x0, #0x7d7b             ; =32123
10084610c:     	movk	x0, #0x200, lsl #32
100846110:     	movk	x0, #0x7ff9, lsl #48
100846114:     	b	0x100846154 <_js_json_stringify_full+0x954>
100846118:     	mov	x0, x19
10084611c:     	bl	0x1004ed000 <__RNvNtNtCseICAOKjEbXS_13perry_runtime4json14stringify_flat21emit_one_field_object>
100846120:     	b	0x10084614c <_js_json_stringify_full+0x94c>
100846124:     	add	x2, sp, #0x8
100846128:     	mov	x0, x19
10084612c:     	mov	x1, x21
100846130:     	bl	0x1004ed700 <__RNvNtNtCseICAOKjEbXS_13perry_runtime4json14stringify_flat35emit_two_field_parsed_string_object>
100846134:     	mov	x8, x0
100846138:     	tbnz	w8, #0x0, 0x100845f40 <_js_json_stringify_full+0x740>
10084613c:     	mov	w20, #0x2               ; =2
100846140:     	mov	x0, x19
100846144:     	mov	x1, x20
100846148:     	bl	0x1004ebe80 <__RNvNtNtCseICAOKjEbXS_13perry_runtime4json14stringify_flat11emit_object>
10084614c:     	mov	x8, x0
100846150:     	mov	x0, x1
100846154:     	mov.16b	v2, v8
100846158:     	mov.16b	v1, v9
10084615c:     	mov.16b	v0, v10
100846160:     	tbnz	w8, #0x0, 0x1008458a0 <_js_json_stringify_full+0xa0>
100846164:     	b	0x100845e34 <_js_json_stringify_full+0x634>
100846168:     	adrp	x2, 0x100ef4000 <__RNvNtNtNtCseICAOKjEbXS_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x2ed0>
10084616c:     	add	x2, x2, #0x380
100846170:     	mov	w0, #0x5                ; =5
100846174:     	mov	w1, #0x5                ; =5
100846178:     	bl	0x100b1124c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
		...
