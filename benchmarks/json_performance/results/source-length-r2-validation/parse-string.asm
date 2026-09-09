
benchmarks/json_performance/.work/source-length-r2/worker:	file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100240f48 <<perry_runtime::json::parser::DirectParser>::parse_string_value>:
100240f48: d10243ff    	sub	sp, sp, #0x90
100240f4c: a90467fa    	stp	x26, x25, [sp, #0x40]
100240f50: a9055ff8    	stp	x24, x23, [sp, #0x50]
100240f54: a90657f6    	stp	x22, x21, [sp, #0x60]
100240f58: a9074ff4    	stp	x20, x19, [sp, #0x70]
100240f5c: a9087bfd    	stp	x29, x30, [sp, #0x80]
100240f60: 910203fd    	add	x29, sp, #0x80
100240f64: aa0003f5    	mov	x21, x0
100240f68: f9403817    	ldr	x23, [x0, #0x70]
100240f6c: a9402408    	ldp	x8, x9, [x0]
100240f70: f100011f    	cmp	x8, #0x0
100240f74: fa571120    	ccmp	x9, x23, #0x0, ne
100240f78: 54000160    	b.eq	0x100240fa4 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x5c>
100240f7c: 910043e0    	add	x0, sp, #0x10
100240f80: aa1503e1    	mov	x1, x21
100240f84: 97fffedd    	bl	0x100240af8 <<perry_runtime::json::parser::DirectParser>::parse_string_bytes>
100240f88: f9400bf6    	ldr	x22, [sp, #0x10]
100240f8c: b1000adf    	cmn	x22, #0x2
100240f90: 54000141    	b.ne	0x100240fb8 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x70>
100240f94: 390352bf    	strb	wzr, [x21, #0xd4]
100240f98: d2800040    	mov	x0, #0x2                ; =2
100240f9c: f2efff80    	movk	x0, #0x7ffc, lsl #48
100240fa0: 1400003a    	b	0x100241088 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x140>
100240fa4: a94126a8    	ldp	x8, x9, [x21, #0x10]
100240fa8: f9003aa8    	str	x8, [x21, #0x70]
100240fac: d2efffe0    	mov	x0, #0x7fff000000000000 ; =9223090561878065152
100240fb0: b340bd20    	bfxil	x0, x9, #0, #48
100240fb4: 14000035    	b	0x100241088 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x140>
100240fb8: a941d3f3    	ldp	x19, x20, [sp, #0x18]
100240fbc: f1001a9f    	cmp	x20, #0x6
100240fc0: 54000482    	b.hs	0x100241050 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x108>
100240fc4: f1000e88    	subs	x8, x20, #0x3
100240fc8: 540006e3    	b.lo	0x1002410a4 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x15c>
100240fcc: 39400269    	ldrb	w9, [x19]
100240fd0: 7103b53f    	cmp	w9, #0xed
100240fd4: 54000101    	b.ne	0x100240ff4 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0xac>
100240fd8: 39400669    	ldrb	w9, [x19, #0x1]
100240fdc: 121b0929    	and	w9, w9, #0xe0
100240fe0: 7102813f    	cmp	w9, #0xa0
100240fe4: 54000081    	b.ne	0x100240ff4 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0xac>
100240fe8: 39c00a69    	ldrsb	w9, [x19, #0x2]
100240fec: 3101013f    	cmn	w9, #0x40
100240ff0: 5400030b    	b.lt	0x100241050 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x108>
100240ff4: b4000588    	cbz	x8, 0x1002410a4 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x15c>
100240ff8: 39400669    	ldrb	w9, [x19, #0x1]
100240ffc: 7103b53f    	cmp	w9, #0xed
100241000: 54000101    	b.ne	0x100241020 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0xd8>
100241004: 39400a69    	ldrb	w9, [x19, #0x2]
100241008: 121b0929    	and	w9, w9, #0xe0
10024100c: 7102813f    	cmp	w9, #0xa0
100241010: 54000081    	b.ne	0x100241020 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0xd8>
100241014: 39c00e69    	ldrsb	w9, [x19, #0x3]
100241018: 3101013f    	cmn	w9, #0x40
10024101c: 540001ab    	b.lt	0x100241050 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x108>
100241020: f100051f    	cmp	x8, #0x1
100241024: 54000400    	b.eq	0x1002410a4 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x15c>
100241028: 39400a68    	ldrb	w8, [x19, #0x2]
10024102c: 7103b51f    	cmp	w8, #0xed
100241030: 540003a1    	b.ne	0x1002410a4 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x15c>
100241034: 39400e68    	ldrb	w8, [x19, #0x3]
100241038: 121b0908    	and	w8, w8, #0xe0
10024103c: 7102811f    	cmp	w8, #0xa0
100241040: 54000321    	b.ne	0x1002410a4 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x15c>
100241044: 39c01268    	ldrsb	w8, [x19, #0x4]
100241048: 3101011f    	cmn	w8, #0x40
10024104c: 540002ca    	b.ge	0x1002410a4 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x15c>
100241050: b10006df    	cmn	x22, #0x1
100241054: 54000360    	b.eq	0x1002410c0 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x178>
100241058: aa1303e0    	mov	x0, x19
10024105c: aa1403e1    	mov	x1, x20
100241060: 940417a4    	bl	0x100346ef0 <perry_runtime::string::js_string_from_builder_bytes>
100241064: d2efffe8    	mov	x8, #0x7fff000000000000 ; =9223090561878065152
100241068: b340bc08    	bfxil	x8, x0, #0, #48
10024106c: aa0803e0    	mov	x0, x8
100241070: f10006df    	cmp	x22, #0x1
100241074: 540000ab    	b.lt	0x100241088 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x140>
100241078: aa0003f4    	mov	x20, x0
10024107c: aa1303e0    	mov	x0, x19
100241080: 942475e0    	bl	0x100b5e800 <_mi_free>
100241084: aa1403e0    	mov	x0, x20
100241088: a9487bfd    	ldp	x29, x30, [sp, #0x80]
10024108c: a9474ff4    	ldp	x20, x19, [sp, #0x70]
100241090: a94657f6    	ldp	x22, x21, [sp, #0x60]
100241094: a9455ff8    	ldp	x24, x23, [sp, #0x50]
100241098: a94467fa    	ldp	x26, x25, [sp, #0x40]
10024109c: 910243ff    	add	sp, sp, #0x90
1002410a0: d65f03c0    	ret
1002410a4: b40001d4    	cbz	x20, 0x1002410dc <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x194>
1002410a8: f100129f    	cmp	x20, #0x4
1002410ac: 540001c2    	b.hs	0x1002410e4 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x19c>
1002410b0: d280000a    	mov	x10, #0x0               ; =0
1002410b4: d2800009    	mov	x9, #0x0                ; =0
1002410b8: aa1303e8    	mov	x8, x19
1002410bc: 1400002e    	b	0x100241174 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x22c>
1002410c0: d353fe88    	lsr	x8, x20, #19
1002410c4: b4000748    	cbz	x8, 0x1002411ac <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x264>
1002410c8: aa1503e0    	mov	x0, x21
1002410cc: aa1303e1    	mov	x1, x19
1002410d0: aa1403e2    	mov	x2, x20
1002410d4: 97ffdbbc    	bl	0x100237fc4 <<perry_runtime::json::parser::DirectParser>::alloc_large_borrowed_string>
1002410d8: 140000ce    	b	0x100241410 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x4c8>
1002410dc: d280000a    	mov	x10, #0x0               ; =0
1002410e0: 1400002d    	b	0x100241194 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x24c>
1002410e4: 927e0289    	and	x9, x20, #0x4
1002410e8: 8b090268    	add	x8, x19, x9
1002410ec: 900052ca    	adrp	x10, 0x100c99000 <perry_runtime::string::format::fmt_fixed_int::POW10+0x55d0>
1002410f0: 3dc0a940    	ldr	q0, [x10, #0x2a0]
1002410f4: 90004fca    	adrp	x10, 0x100c39000 <itoa::DECIMAL_PAIRS+0x33dfa>
1002410f8: 3dc06142    	ldr	q2, [x10, #0x180]
1002410fc: 6f00e401    	movi.2d	v1, #0000000000000000
100241100: 6f00e423    	movi.2d	v3, #0x000000000000ff
100241104: 5280008a    	mov	w10, #0x4               ; =4
100241108: 4e080d44    	dup.2d	v4, x10
10024110c: aa1303ea    	mov	x10, x19
100241110: 927e028b    	and	x11, x20, #0x4
100241114: 6f00e405    	movi.2d	v5, #0000000000000000
100241118: bc404546    	ldr	s6, [x10], #0x4
10024111c: 2f08a4c6    	ushll.8h	v6, v6, #0x0
100241120: 2f10a4c6    	ushll.4s	v6, v6, #0x0
100241124: 6f20a4c7    	ushll2.2d	v7, v6, #0x0
100241128: 4e231ce7    	and.16b	v7, v7, v3
10024112c: 2f20a4c6    	ushll.2d	v6, v6, #0x0
100241130: 4e231cc6    	and.16b	v6, v6, v3
100241134: 4f435410    	shl.2d	v16, v0, #0x3
100241138: 4f435451    	shl.2d	v17, v2, #0x3
10024113c: 6ef144c6    	ushl.2d	v6, v6, v17
100241140: 6ef044e7    	ushl.2d	v7, v7, v16
100241144: 4ea11ce1    	orr.16b	v1, v7, v1
100241148: 4ea51cc5    	orr.16b	v5, v6, v5
10024114c: 4ee48400    	add.2d	v0, v0, v4
100241150: 4ee48442    	add.2d	v2, v2, v4
100241154: f100116b    	subs	x11, x11, #0x4
100241158: 54fffe01    	b.ne	0x100241118 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x1d0>
10024115c: 4ea11ca0    	orr.16b	v0, v5, v1
100241160: 5e180401    	mov	d1, v0[1]
100241164: 0ea11c00    	orr.8b	v0, v0, v1
100241168: 9e66000a    	fmov	x10, d0
10024116c: eb09029f    	cmp	x20, x9
100241170: 54000120    	b.eq	0x100241194 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x24c>
100241174: 8b14026b    	add	x11, x19, x20
100241178: d37df129    	lsl	x9, x9, #3
10024117c: 3840150c    	ldrb	w12, [x8], #0x1
100241180: 9ac9218c    	lsl	x12, x12, x9
100241184: aa0a018a    	orr	x10, x12, x10
100241188: 91002129    	add	x9, x9, #0x8
10024118c: eb0b011f    	cmp	x8, x11
100241190: 54ffff61    	b.ne	0x10024117c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x234>
100241194: aa14a148    	orr	x8, x10, x20, lsl #40
100241198: d2efff29    	mov	x9, #0x7ff9000000000000 ; =9221401712017801216
10024119c: aa090100    	orr	x0, x8, x9
1002411a0: f10006df    	cmp	x22, #0x1
1002411a4: 54fff6aa    	b.ge	0x100241078 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x130>
1002411a8: 17ffffb8    	b	0x100241088 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x140>
1002411ac: f101029f    	cmp	x20, #0x40
1002411b0: 54000822    	b.hs	0x1002412b4 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x36c>
1002411b4: f27d0a89    	ands	x9, x20, #0x38
1002411b8: 54000120    	b.eq	0x1002411dc <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x294>
1002411bc: 927d0a88    	and	x8, x20, #0x38
1002411c0: cb0803e8    	neg	x8, x8
1002411c4: aa1303ea    	mov	x10, x19
1002411c8: f840854b    	ldr	x11, [x10], #0x8
1002411cc: f201c17f    	tst	x11, #0x8080808080808080
1002411d0: 54000401    	b.ne	0x100241250 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x308>
1002411d4: b1002108    	adds	x8, x8, #0x8
1002411d8: 54ffff81    	b.ne	0x1002411c8 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x280>
1002411dc: 92400a88    	and	x8, x20, #0x7
1002411e0: b4000aa8    	cbz	x8, 0x100241334 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3ec>
1002411e4: 8b090269    	add	x9, x19, x9
1002411e8: 39c0012a    	ldrsb	w10, [x9]
1002411ec: 37f8032a    	tbnz	w10, #0x1f, 0x100241250 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x308>
1002411f0: f100051f    	cmp	x8, #0x1
1002411f4: 54000a00    	b.eq	0x100241334 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3ec>
1002411f8: 39c0052a    	ldrsb	w10, [x9, #0x1]
1002411fc: 37f802aa    	tbnz	w10, #0x1f, 0x100241250 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x308>
100241200: f100091f    	cmp	x8, #0x2
100241204: 54000980    	b.eq	0x100241334 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3ec>
100241208: 39c0092a    	ldrsb	w10, [x9, #0x2]
10024120c: 37f8022a    	tbnz	w10, #0x1f, 0x100241250 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x308>
100241210: f1000d1f    	cmp	x8, #0x3
100241214: 54000900    	b.eq	0x100241334 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3ec>
100241218: 39c00d2a    	ldrsb	w10, [x9, #0x3]
10024121c: 37f801aa    	tbnz	w10, #0x1f, 0x100241250 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x308>
100241220: f100111f    	cmp	x8, #0x4
100241224: 54000880    	b.eq	0x100241334 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3ec>
100241228: 39c0112a    	ldrsb	w10, [x9, #0x4]
10024122c: 37f8012a    	tbnz	w10, #0x1f, 0x100241250 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x308>
100241230: f100151f    	cmp	x8, #0x5
100241234: 54000800    	b.eq	0x100241334 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3ec>
100241238: 39c0152a    	ldrsb	w10, [x9, #0x5]
10024123c: 37f800aa    	tbnz	w10, #0x1f, 0x100241250 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x308>
100241240: f100191f    	cmp	x8, #0x6
100241244: 54000780    	b.eq	0x100241334 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3ec>
100241248: 39c01928    	ldrsb	w8, [x9, #0x6]
10024124c: 36f80748    	tbz	w8, #0x1f, 0x100241334 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3ec>
100241250: b4000d74    	cbz	x20, 0x1002413fc <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x4b4>
100241254: aa1303e8    	mov	x8, x19
100241258: f2400689    	ands	x9, x20, #0x3
10024125c: 540000e0    	b.eq	0x100241278 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x330>
100241260: aa1303e8    	mov	x8, x19
100241264: 39c0010a    	ldrsb	w10, [x8]
100241268: 37f806aa    	tbnz	w10, #0x1f, 0x10024133c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3f4>
10024126c: 91000508    	add	x8, x8, #0x1
100241270: f1000529    	subs	x9, x9, #0x1
100241274: 54ffff81    	b.ne	0x100241264 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x31c>
100241278: f100129f    	cmp	x20, #0x4
10024127c: 540005c3    	b.lo	0x100241334 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3ec>
100241280: 8b140269    	add	x9, x19, x20
100241284: 39c0010a    	ldrsb	w10, [x8]
100241288: 37f805aa    	tbnz	w10, #0x1f, 0x10024133c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3f4>
10024128c: 39c0050a    	ldrsb	w10, [x8, #0x1]
100241290: 37f8056a    	tbnz	w10, #0x1f, 0x10024133c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3f4>
100241294: 39c0090a    	ldrsb	w10, [x8, #0x2]
100241298: 37f8052a    	tbnz	w10, #0x1f, 0x10024133c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3f4>
10024129c: 39c00d0a    	ldrsb	w10, [x8, #0x3]
1002412a0: 37f804ea    	tbnz	w10, #0x1f, 0x10024133c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3f4>
1002412a4: 91001108    	add	x8, x8, #0x4
1002412a8: eb09011f    	cmp	x8, x9
1002412ac: 54fffec1    	b.ne	0x100241284 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x33c>
1002412b0: 14000021    	b	0x100241334 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3ec>
1002412b4: 927a3288    	and	x8, x20, #0x7ffc0
1002412b8: 8b080268    	add	x8, x19, x8
1002412bc: aa1303e9    	mov	x9, x19
1002412c0: ad400520    	ldp	q0, q1, [x9]
1002412c4: ad410d22    	ldp	q2, q3, [x9, #0x20]
1002412c8: 4ea01c20    	orr.16b	v0, v1, v0
1002412cc: 4ea31c41    	orr.16b	v1, v2, v3
1002412d0: 4ea11c00    	orr.16b	v0, v0, v1
1002412d4: 6e30a800    	umaxv.16b	b0, v0
1002412d8: 1e26000a    	fmov	w10, s0
1002412dc: 373ffbaa    	tbnz	w10, #0x7, 0x100241250 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x308>
1002412e0: 91010129    	add	x9, x9, #0x40
1002412e4: eb08013f    	cmp	x9, x8
1002412e8: 54fffec1    	b.ne	0x1002412c0 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x378>
1002412ec: f27c0689    	ands	x9, x20, #0x30
1002412f0: 54000120    	b.eq	0x100241314 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3cc>
1002412f4: aa0903ea    	mov	x10, x9
1002412f8: aa0803eb    	mov	x11, x8
1002412fc: 3cc10560    	ldr	q0, [x11], #0x10
100241300: 6e30a800    	umaxv.16b	b0, v0
100241304: 1e26000c    	fmov	w12, s0
100241308: 373ffa4c    	tbnz	w12, #0x7, 0x100241250 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x308>
10024130c: f100414a    	subs	x10, x10, #0x10
100241310: 54ffff61    	b.ne	0x1002412fc <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3b4>
100241314: 92400e8a    	and	x10, x20, #0xf
100241318: b40000ea    	cbz	x10, 0x100241334 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3ec>
10024131c: 8b090108    	add	x8, x8, x9
100241320: 39c00109    	ldrsb	w9, [x8]
100241324: 37fff969    	tbnz	w9, #0x1f, 0x100241250 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x308>
100241328: 91000508    	add	x8, x8, #0x1
10024132c: f100054a    	subs	x10, x10, #0x1
100241330: 54ffff81    	b.ne	0x100241320 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3d8>
100241334: aa1403e3    	mov	x3, x20
100241338: 14000032    	b	0x100241400 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x4b8>
10024133c: f100fe9f    	cmp	x20, #0x3f
100241340: 540000c9    	b.ls	0x100241358 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x410>
100241344: aa1303e0    	mov	x0, x19
100241348: aa1403e1    	mov	x1, x20
10024134c: 940ea215    	bl	0x1005e9ba0 <perry_runtime::string::utf16_count::count_bytes>
100241350: aa0003e3    	mov	x3, x0
100241354: 1400002b    	b	0x100241400 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x4b8>
100241358: 9100a3e8    	add	x8, sp, #0x28
10024135c: aa1303e0    	mov	x0, x19
100241360: aa1403e1    	mov	x1, x20
100241364: 97f7b26d    	bl	0x10002dd18 <core::str::converts::from_utf8>
100241368: f94017e8    	ldr	x8, [sp, #0x28]
10024136c: b4000388    	cbz	x8, 0x1002413dc <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x494>
100241370: 52800003    	mov	w3, #0x0                ; =0
100241374: d2800008    	mov	x8, #0x0                ; =0
100241378: 52800049    	mov	w9, #0x2                ; =2
10024137c: 5280006a    	mov	w10, #0x3               ; =3
100241380: 5280008b    	mov	w11, #0x4               ; =4
100241384: 14000006    	b	0x10024139c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x454>
100241388: 11000463    	add	w3, w3, #0x1
10024138c: 5280002c    	mov	w12, #0x1               ; =1
100241390: 8b080188    	add	x8, x12, x8
100241394: eb14011f    	cmp	x8, x20
100241398: 54000342    	b.hs	0x100241400 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x4b8>
10024139c: 38e86a6c    	ldrsb	w12, [x19, x8]
1002413a0: 36ffff4c    	tbz	w12, #0x1f, 0x100241388 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x440>
1002413a4: 12001d8c    	and	w12, w12, #0xff
1002413a8: 7103019f    	cmp	w12, #0xc0
1002413ac: 54ffff03    	b.lo	0x10024138c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x444>
1002413b0: 7103c19f    	cmp	w12, #0xf0
1002413b4: 1100086d    	add	w13, w3, #0x2
1002413b8: 9a8b314e    	csel	x14, x10, x11, lo
1002413bc: 1a8325ad    	csinc	w13, w13, w3, hs
1002413c0: 7103819f    	cmp	w12, #0xe0
1002413c4: 9a8e312c    	csel	x12, x9, x14, lo
1002413c8: 1a8325a3    	csinc	w3, w13, w3, hs
1002413cc: 8b080188    	add	x8, x12, x8
1002413d0: eb14011f    	cmp	x8, x20
1002413d4: 54fffe43    	b.lo	0x10024139c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x454>
1002413d8: 1400000a    	b	0x100241400 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x4b8>
1002413dc: f9401fe8    	ldr	x8, [sp, #0x38]
1002413e0: b40000e8    	cbz	x8, 0x1002413fc <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x4b4>
1002413e4: f9401be9    	ldr	x9, [sp, #0x30]
1002413e8: f100211f    	cmp	x8, #0x8
1002413ec: 54000d62    	b.hs	0x100241598 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x650>
1002413f0: d280000a    	mov	x10, #0x0               ; =0
1002413f4: 52800003    	mov	w3, #0x0                ; =0
1002413f8: 1400011a    	b	0x100241860 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x918>
1002413fc: 52800003    	mov	w3, #0x0                ; =0
100241400: 910082a0    	add	x0, x21, #0x20
100241404: aa1303e1    	mov	x1, x19
100241408: aa1403e2    	mov	x2, x20
10024140c: 940ea4cd    	bl	0x1005ea740 <perry_runtime::string::json_construction::string_from_json_bytes_counted>
100241410: f94036b9    	ldr	x25, [x21, #0x68]
100241414: f148033f    	cmp	x25, #0x200, lsl #12    ; =0x200000
100241418: 54ffe268    	b.hi	0x100241064 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x11c>
10024141c: f104029f    	cmp	x20, #0x100
100241420: 54ffe223    	b.lo	0x100241064 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x11c>
100241424: b4ffe200    	cbz	x0, 0x100241064 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x11c>
100241428: f94066b8    	ldr	x24, [x21, #0xc8]
10024142c: b4ffe1d8    	cbz	x24, 0x100241064 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x11c>
100241430: f9403ab5    	ldr	x21, [x21, #0x70]
100241434: cb590728    	sub	x8, x25, x25, lsr #1
100241438: eb08029f    	cmp	x20, x8
10024143c: aa1702a8    	orr	x8, x21, x23
100241440: 92607d08    	and	x8, x8, #0xffffffff00000000
100241444: fa402900    	ccmp	x8, #0x0, #0x0, hs
100241448: 54ffe0e1    	b.ne	0x100241064 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x11c>
10024144c: b00069e8    	adrp	x8, 0x100f7e000 <__MergedGlobals.1917+0xc8>
100241450: b9456114    	ldr	w20, [x8, #0x560]
100241454: 710c029f    	cmp	w20, #0x300
100241458: 540008a2    	b.hs	0x10024156c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x624>
10024145c: f00069c8    	adrp	x8, 0x100f7c000 <_perry_global_worker_ts__1>
100241460: f9402508    	ldr	x8, [x8, #0x48]
100241464: b100051f    	cmn	x8, #0x1
100241468: 54000740    	b.eq	0x100241550 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x608>
10024146c: d53bd069    	mrs	x9, TPIDRRO_EL0
100241470: 927df129    	and	x9, x9, #0xfffffffffffffff8
100241474: f8687928    	ldr	x8, [x9, x8, lsl #3]
100241478: b40006c8    	cbz	x8, 0x100241550 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x608>
10024147c: 8b140d08    	add	x8, x8, x20, lsl #3
100241480: f940f514    	ldr	x20, [x8, #0x1e8]
100241484: b4000754    	cbz	x20, 0x10024156c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x624>
100241488: f9400288    	ldr	x8, [x20]
10024148c: b5000808    	cbnz	x8, 0x10024158c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x644>
100241490: 92800008    	mov	x8, #-0x1               ; =-1
100241494: f9000288    	str	x8, [x20]
100241498: 39409288    	ldrb	w8, [x20, #0x24]
10024149c: 7100091f    	cmp	w8, #0x2
1002414a0: 540001c0    	b.eq	0x1002414d8 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x590>
1002414a4: f9400688    	ldr	x8, [x20, #0x8]
1002414a8: eb18011f    	cmp	x8, x24
1002414ac: 54000161    	b.ne	0x1002414d8 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x590>
1002414b0: b9401a88    	ldr	w8, [x20, #0x18]
1002414b4: 6b19011f    	cmp	w8, w25
1002414b8: 54000101    	b.ne	0x1002414d8 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x590>
1002414bc: f900029f    	str	xzr, [x20]
1002414c0: d2efffe8    	mov	x8, #0x7fff000000000000 ; =9223090561878065152
1002414c4: b340bc08    	bfxil	x8, x0, #0, #48
1002414c8: aa0803e0    	mov	x0, x8
1002414cc: f10006df    	cmp	x22, #0x1
1002414d0: 54ffdd4a    	b.ge	0x100241078 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x130>
1002414d4: 17fffeed    	b	0x100241088 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x140>
1002414d8: a9008298    	stp	x24, x0, [x20, #0x8]
1002414dc: 29035e99    	stp	w25, w23, [x20, #0x18]
1002414e0: b9002295    	str	w21, [x20, #0x20]
1002414e4: 3900929f    	strb	wzr, [x20, #0x24]
1002414e8: 900070f5    	adrp	x21, 0x10105d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xf79c>
1002414ec: b94866a8    	ldr	w8, [x21, #0x864]
1002414f0: 340000e8    	cbz	w8, 0x10024150c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x5c4>
1002414f4: d2efffe8    	mov	x8, #0x7fff000000000000 ; =9223090561878065152
1002414f8: b340bf08    	bfxil	x8, x24, #0, #48
1002414fc: aa0003f7    	mov	x23, x0
100241500: aa0803e0    	mov	x0, x8
100241504: 9408cdcb    	bl	0x100474c30 <perry_runtime::gc::barrier::incremental_mark_barrier_value_active>
100241508: aa1703e0    	mov	x0, x23
10024150c: b94866a8    	ldr	w8, [x21, #0x864]
100241510: 340000e8    	cbz	w8, 0x10024152c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x5e4>
100241514: d2efffe8    	mov	x8, #0x7fff000000000000 ; =9223090561878065152
100241518: b340bc08    	bfxil	x8, x0, #0, #48
10024151c: aa0003f5    	mov	x21, x0
100241520: aa0803e0    	mov	x0, x8
100241524: 9408cdc3    	bl	0x100474c30 <perry_runtime::gc::barrier::incremental_mark_barrier_value_active>
100241528: aa1503e0    	mov	x0, x21
10024152c: f9400288    	ldr	x8, [x20]
100241530: 91000508    	add	x8, x8, #0x1
100241534: f9000288    	str	x8, [x20]
100241538: d2efffe8    	mov	x8, #0x7fff000000000000 ; =9223090561878065152
10024153c: b340bc08    	bfxil	x8, x0, #0, #48
100241540: aa0803e0    	mov	x0, x8
100241544: f10006df    	cmp	x22, #0x1
100241548: 54ffd98a    	b.ge	0x100241078 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x130>
10024154c: 17fffecf    	b	0x100241088 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x140>
100241550: aa0003fa    	mov	x26, x0
100241554: 9423f5ba    	bl	0x100b3ec3c <perry_runtime::tls_hot::hot_uncached>
100241558: aa0003e8    	mov	x8, x0
10024155c: aa1a03e0    	mov	x0, x26
100241560: 8b140d08    	add	x8, x8, x20, lsl #3
100241564: f940f514    	ldr	x20, [x8, #0x1e8]
100241568: b5fff914    	cbnz	x20, 0x100241488 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x540>
10024156c: f90007e0    	str	x0, [sp, #0x8]
100241570: d0006640    	adrp	x0, 0x100f0b000 <perry_runtime::os::os_process_emitter::PROCESS_EXIT_EVENT_EMITTED+0x1eb8>
100241574: 913e8000    	add	x0, x0, #0xfa0
100241578: 9423ebb9    	bl	0x100b3c45c <<perry_runtime::tls_hot::HotKey<perry_runtime::closure::registry::DispatchRecent>>::get_slow>
10024157c: aa0003f4    	mov	x20, x0
100241580: f94007e0    	ldr	x0, [sp, #0x8]
100241584: f9400288    	ldr	x8, [x20]
100241588: b4fff848    	cbz	x8, 0x100241490 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x548>
10024158c: 900065c0    	adrp	x0, 0x100ef9000 <perry_runtime::closure::dispatch::bound::KEEP_JS_FUNCTION_BIND+0x7ed0>
100241590: 91326000    	add	x0, x0, #0xc98
100241594: 94234136    	bl	0x100b11a6c <core::cell::panic_already_borrowed>
100241598: f100811f    	cmp	x8, #0x20
10024159c: 54000082    	b.hs	0x1002415ac <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x664>
1002415a0: d280000a    	mov	x10, #0x0               ; =0
1002415a4: 52800003    	mov	w3, #0x0                ; =0
1002415a8: 14000082    	b	0x1002417b0 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x868>
1002415ac: 6f00e400    	movi.2d	v0, #0000000000000000
1002415b0: 927d050b    	and	x11, x8, #0x18
1002415b4: 4f07e601    	movi.16b	v1, #0xf0
1002415b8: 927be90a    	and	x10, x8, #0xffffffffffffffe0
1002415bc: 4f000442    	movi.4s	v2, #0x2
1002415c0: 9100412c    	add	x12, x9, #0x10
1002415c4: 4f06e404    	movi.16b	v4, #0xc0
1002415c8: 927be90d    	and	x13, x8, #0xffffffffffffffe0
1002415cc: 6f00e403    	movi.2d	v3, #0000000000000000
1002415d0: 6f00e406    	movi.2d	v6, #0000000000000000
1002415d4: 6f00e405    	movi.2d	v5, #0000000000000000
1002415d8: 6f00e407    	movi.2d	v7, #0000000000000000
1002415dc: 6f00e410    	movi.2d	v16, #0000000000000000
1002415e0: 6f00e411    	movi.2d	v17, #0000000000000000
1002415e4: 6f00e412    	movi.2d	v18, #0000000000000000
1002415e8: ad7fcd94    	ldp	q20, q19, [x12, #-0x10]
1002415ec: 6e343435    	cmhi.16b	v21, v1, v20
1002415f0: 4f08a6b6    	sshll2.8h	v22, v21, #0x0
1002415f4: 4f10a6d7    	sshll2.4s	v23, v22, #0x0
1002415f8: 0f10a6d8    	sshll.4s	v24, v22, #0x0
1002415fc: 0f08a6b5    	sshll.8h	v21, v21, #0x0
100241600: 4f10a6b9    	sshll2.4s	v25, v21, #0x0
100241604: 0f10a6ba    	sshll.4s	v26, v21, #0x0
100241608: 6e33343b    	cmhi.16b	v27, v1, v19
10024160c: 4f08a77c    	sshll2.8h	v28, v27, #0x0
100241610: 4f10a79d    	sshll2.4s	v29, v28, #0x0
100241614: 0f10a79e    	sshll.4s	v30, v28, #0x0
100241618: 0f08a77b    	sshll.8h	v27, v27, #0x0
10024161c: 4f10a77f    	sshll2.4s	v31, v27, #0x0
100241620: 4e7a1c5a    	bic.16b	v26, v2, v26
100241624: 0e75335a    	ssubw.4s	v26, v26, v21
100241628: 4e791c59    	bic.16b	v25, v2, v25
10024162c: 4e753339    	ssubw2.4s	v25, v25, v21
100241630: 0f10a775    	sshll.4s	v21, v27, #0x0
100241634: 4e781c58    	bic.16b	v24, v2, v24
100241638: 0e763318    	ssubw.4s	v24, v24, v22
10024163c: 4e771c57    	bic.16b	v23, v2, v23
100241640: 4e7632f6    	ssubw2.4s	v22, v23, v22
100241644: 4e751c55    	bic.16b	v21, v2, v21
100241648: 0e7b32b5    	ssubw.4s	v21, v21, v27
10024164c: 4e7f1c57    	bic.16b	v23, v2, v31
100241650: 4e7b32f7    	ssubw2.4s	v23, v23, v27
100241654: 4e7e1c5b    	bic.16b	v27, v2, v30
100241658: 0e7c337b    	ssubw.4s	v27, v27, v28
10024165c: 4e7d1c5d    	bic.16b	v29, v2, v29
100241660: 4e7c33bc    	ssubw2.4s	v28, v29, v28
100241664: 4e34349d    	cmgt.16b	v29, v4, v20
100241668: 4f08a7be    	sshll2.8h	v30, v29, #0x0
10024166c: 6f10a7df    	ushll2.4s	v31, v30, #0x0
100241670: 4e7f1ed6    	bic.16b	v22, v22, v31
100241674: 4e20aa94    	cmlt.16b	v20, v20, #0
100241678: 0f08a7bd    	sshll.8h	v29, v29, #0x0
10024167c: 2f10a7de    	ushll.4s	v30, v30, #0x0
100241680: 4e7e1f18    	bic.16b	v24, v24, v30
100241684: 6f10a7be    	ushll2.4s	v30, v29, #0x0
100241688: 4e7e1f39    	bic.16b	v25, v25, v30
10024168c: 4f08a69e    	sshll2.8h	v30, v20, #0x0
100241690: 0f08a694    	sshll.8h	v20, v20, #0x0
100241694: 2f10a7bd    	ushll.4s	v29, v29, #0x0
100241698: 4e7d1f5a    	bic.16b	v26, v26, v29
10024169c: 0f10a69d    	sshll.4s	v29, v20, #0x0
1002416a0: 4e3d1f5a    	and.16b	v26, v26, v29
1002416a4: 6e205bbd    	mvn.16b	v29, v29
1002416a8: 6ebd875a    	sub.4s	v26, v26, v29
1002416ac: 4f10a7dd    	sshll2.4s	v29, v30, #0x0
1002416b0: 0f10a7de    	sshll.4s	v30, v30, #0x0
1002416b4: 4f10a694    	sshll2.4s	v20, v20, #0x0
1002416b8: 4e341f39    	and.16b	v25, v25, v20
1002416bc: 6e205a94    	mvn.16b	v20, v20
1002416c0: 6eb48734    	sub.4s	v20, v25, v20
1002416c4: 4e333499    	cmgt.16b	v25, v4, v19
1002416c8: 4e3e1f18    	and.16b	v24, v24, v30
1002416cc: 6e205bde    	mvn.16b	v30, v30
1002416d0: 6ebe8718    	sub.4s	v24, v24, v30
1002416d4: 4f08a73e    	sshll2.8h	v30, v25, #0x0
1002416d8: 4e3d1ed6    	and.16b	v22, v22, v29
1002416dc: 6e205bbd    	mvn.16b	v29, v29
1002416e0: 6ebd86d6    	sub.4s	v22, v22, v29
1002416e4: 6f10a7dd    	ushll2.4s	v29, v30, #0x0
1002416e8: 4e7d1f9c    	bic.16b	v28, v28, v29
1002416ec: 4e20aa73    	cmlt.16b	v19, v19, #0
1002416f0: 0f08a739    	sshll.8h	v25, v25, #0x0
1002416f4: 2f10a7dd    	ushll.4s	v29, v30, #0x0
1002416f8: 4e7d1f7b    	bic.16b	v27, v27, v29
1002416fc: 6f10a73d    	ushll2.4s	v29, v25, #0x0
100241700: 4e7d1ef7    	bic.16b	v23, v23, v29
100241704: 0f08a67d    	sshll.8h	v29, v19, #0x0
100241708: 2f10a739    	ushll.4s	v25, v25, #0x0
10024170c: 4e791eb5    	bic.16b	v21, v21, v25
100241710: 0f10a7b9    	sshll.4s	v25, v29, #0x0
100241714: 4e391eb5    	and.16b	v21, v21, v25
100241718: 6e205b39    	mvn.16b	v25, v25
10024171c: 6eb986b5    	sub.4s	v21, v21, v25
100241720: 4f08a673    	sshll2.8h	v19, v19, #0x0
100241724: 4f10a7b9    	sshll2.4s	v25, v29, #0x0
100241728: 4e391ef7    	and.16b	v23, v23, v25
10024172c: 6e205b39    	mvn.16b	v25, v25
100241730: 6eb986f7    	sub.4s	v23, v23, v25
100241734: 0f10a679    	sshll.4s	v25, v19, #0x0
100241738: 4e391f7b    	and.16b	v27, v27, v25
10024173c: 6e205b39    	mvn.16b	v25, v25
100241740: 6eb98779    	sub.4s	v25, v27, v25
100241744: 4f10a673    	sshll2.4s	v19, v19, #0x0
100241748: 4e331f9b    	and.16b	v27, v28, v19
10024174c: 6e205a73    	mvn.16b	v19, v19
100241750: 6eb38773    	sub.4s	v19, v27, v19
100241754: 4ea786c7    	add.4s	v7, v22, v7
100241758: 4ea58705    	add.4s	v5, v24, v5
10024175c: 4ea68686    	add.4s	v6, v20, v6
100241760: 4ea38743    	add.4s	v3, v26, v3
100241764: 4eb28672    	add.4s	v18, v19, v18
100241768: 4eb18731    	add.4s	v17, v25, v17
10024176c: 4ea086e0    	add.4s	v0, v23, v0
100241770: 4eb086b0    	add.4s	v16, v21, v16
100241774: 9100818c    	add	x12, x12, #0x20
100241778: f10081ad    	subs	x13, x13, #0x20
10024177c: 54fff361    	b.ne	0x1002415e8 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x6a0>
100241780: 4ea68400    	add.4s	v0, v0, v6
100241784: 4ea78641    	add.4s	v1, v18, v7
100241788: 4ea38602    	add.4s	v2, v16, v3
10024178c: 4ea58623    	add.4s	v3, v17, v5
100241790: 4ea38442    	add.4s	v2, v2, v3
100241794: 4ea18400    	add.4s	v0, v0, v1
100241798: 4ea08440    	add.4s	v0, v2, v0
10024179c: 4eb1b800    	addv.4s	s0, v0
1002417a0: 1e260003    	fmov	w3, s0
1002417a4: eb0a011f    	cmp	x8, x10
1002417a8: 54ffe2c0    	b.eq	0x100241400 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x4b8>
1002417ac: b40005ab    	cbz	x11, 0x100241860 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x918>
1002417b0: aa0a03ec    	mov	x12, x10
1002417b4: 927df10a    	and	x10, x8, #0xfffffffffffffff8
1002417b8: 6f00e400    	movi.2d	v0, #0000000000000000
1002417bc: 4e041c60    	mov.s	v0[0], w3
1002417c0: 6f00e401    	movi.2d	v1, #0000000000000000
1002417c4: cb0a018b    	sub	x11, x12, x10
1002417c8: 8b0c012c    	add	x12, x9, x12
1002417cc: 0f07e602    	movi.8b	v2, #0xf0
1002417d0: 4f000443    	movi.4s	v3, #0x2
1002417d4: 0f06e404    	movi.8b	v4, #0xc0
1002417d8: fc408585    	ldr	d5, [x12], #0x8
1002417dc: 0f08a4a6    	sshll.8h	v6, v5, #0x0
1002417e0: 4e60a8c6    	cmlt.8h	v6, v6, #0
1002417e4: 4f10a4c7    	sshll2.4s	v7, v6, #0x0
1002417e8: 0f10a4c6    	sshll.4s	v6, v6, #0x0
1002417ec: 2e253450    	cmhi.8b	v16, v2, v5
1002417f0: 0f08a610    	sshll.8h	v16, v16, #0x0
1002417f4: 4f10a611    	sshll2.4s	v17, v16, #0x0
1002417f8: 0f10a612    	sshll.4s	v18, v16, #0x0
1002417fc: 4e721c72    	bic.16b	v18, v3, v18
100241800: 0e703252    	ssubw.4s	v18, v18, v16
100241804: 4e711c71    	bic.16b	v17, v3, v17
100241808: 4e703230    	ssubw2.4s	v16, v17, v16
10024180c: 0e253485    	cmgt.8b	v5, v4, v5
100241810: 0f08a4a5    	sshll.8h	v5, v5, #0x0
100241814: 2f10a4b1    	ushll.4s	v17, v5, #0x0
100241818: 6f10a4a5    	ushll2.4s	v5, v5, #0x0
10024181c: 4e651e05    	bic.16b	v5, v16, v5
100241820: 4e711e50    	bic.16b	v16, v18, v17
100241824: 4e261e10    	and.16b	v16, v16, v6
100241828: 6e2058c6    	mvn.16b	v6, v6
10024182c: 6ea68606    	sub.4s	v6, v16, v6
100241830: 4e271ca5    	and.16b	v5, v5, v7
100241834: 6e2058e7    	mvn.16b	v7, v7
100241838: 6ea784a5    	sub.4s	v5, v5, v7
10024183c: 4ea184a1    	add.4s	v1, v5, v1
100241840: 4ea084c0    	add.4s	v0, v6, v0
100241844: b100216b    	adds	x11, x11, #0x8
100241848: 54fffc81    	b.ne	0x1002417d8 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x890>
10024184c: 4ea18400    	add.4s	v0, v0, v1
100241850: 4eb1b800    	addv.4s	s0, v0
100241854: 1e260003    	fmov	w3, s0
100241858: eb0a011f    	cmp	x8, x10
10024185c: 54ffdd20    	b.eq	0x100241400 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x4b8>
100241860: cb0a0108    	sub	x8, x8, x10
100241864: 8b0a0129    	add	x9, x9, x10
100241868: 5280002a    	mov	w10, #0x1               ; =1
10024186c: 38c0152b    	ldrsb	w11, [x9], #0x1
100241870: 12001d6c    	and	w12, w11, #0xff
100241874: 7103c19f    	cmp	w12, #0xf0
100241878: 1a8a354d    	cinc	w13, w10, hs
10024187c: 7103019f    	cmp	w12, #0xc0
100241880: 1a8d33ec    	csel	w12, wzr, w13, lo
100241884: 7201017f    	tst	w11, #0x80000000
100241888: 1a9f158b    	csinc	w11, w12, wzr, ne
10024188c: 0b030163    	add	w3, w11, w3
100241890: f1000508    	subs	x8, x8, #0x1
100241894: 54fffec1    	b.ne	0x10024186c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x924>
100241898: 17fffeda    	b	0x100241400 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x4b8>
		...
