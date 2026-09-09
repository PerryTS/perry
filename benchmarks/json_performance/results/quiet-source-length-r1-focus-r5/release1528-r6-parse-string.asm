
benchmarks/json_performance/.work/release1528-r6/worker:	file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100240e88 <<perry_runtime::json::parser::DirectParser>::parse_string_value>:
100240e88: d10183ff    	sub	sp, sp, #0x60
100240e8c: a9025ff8    	stp	x24, x23, [sp, #0x20]
100240e90: a90357f6    	stp	x22, x21, [sp, #0x30]
100240e94: a9044ff4    	stp	x20, x19, [sp, #0x40]
100240e98: a9057bfd    	stp	x29, x30, [sp, #0x50]
100240e9c: 910143fd    	add	x29, sp, #0x50
100240ea0: aa0003f5    	mov	x21, x0
100240ea4: f9403817    	ldr	x23, [x0, #0x70]
100240ea8: a9402408    	ldp	x8, x9, [x0]
100240eac: f100011f    	cmp	x8, #0x0
100240eb0: fa571120    	ccmp	x9, x23, #0x0, ne
100240eb4: 54000160    	b.eq	0x100240ee0 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x58>
100240eb8: 910023e0    	add	x0, sp, #0x8
100240ebc: aa1503e1    	mov	x1, x21
100240ec0: 97fffede    	bl	0x100240a38 <<perry_runtime::json::parser::DirectParser>::parse_string_bytes>
100240ec4: f94007f6    	ldr	x22, [sp, #0x8]
100240ec8: b1000adf    	cmn	x22, #0x2
100240ecc: 540001e1    	b.ne	0x100240f08 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x80>
100240ed0: 390352bf    	strb	wzr, [x21, #0xd4]
100240ed4: d2800040    	mov	x0, #0x2                ; =2
100240ed8: f2efff80    	movk	x0, #0x7ffc, lsl #48
100240edc: 14000005    	b	0x100240ef0 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x68>
100240ee0: a94126a8    	ldp	x8, x9, [x21, #0x10]
100240ee4: f9003aa8    	str	x8, [x21, #0x70]
100240ee8: d2efffe0    	mov	x0, #0x7fff000000000000 ; =9223090561878065152
100240eec: b340bd20    	bfxil	x0, x9, #0, #48
100240ef0: a9457bfd    	ldp	x29, x30, [sp, #0x50]
100240ef4: a9444ff4    	ldp	x20, x19, [sp, #0x40]
100240ef8: a94357f6    	ldp	x22, x21, [sp, #0x30]
100240efc: a9425ff8    	ldp	x24, x23, [sp, #0x20]
100240f00: 910183ff    	add	sp, sp, #0x60
100240f04: d65f03c0    	ret
100240f08: a9414ff4    	ldp	x20, x19, [sp, #0x10]
100240f0c: f1001a7f    	cmp	x19, #0x6
100240f10: 54000482    	b.hs	0x100240fa0 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x118>
100240f14: f1000e68    	subs	x8, x19, #0x3
100240f18: 540005a3    	b.lo	0x100240fcc <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x144>
100240f1c: 39400289    	ldrb	w9, [x20]
100240f20: 7103b53f    	cmp	w9, #0xed
100240f24: 54000101    	b.ne	0x100240f44 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0xbc>
100240f28: 39400689    	ldrb	w9, [x20, #0x1]
100240f2c: 121b0929    	and	w9, w9, #0xe0
100240f30: 7102813f    	cmp	w9, #0xa0
100240f34: 54000081    	b.ne	0x100240f44 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0xbc>
100240f38: 39c00a89    	ldrsb	w9, [x20, #0x2]
100240f3c: 3101013f    	cmn	w9, #0x40
100240f40: 5400030b    	b.lt	0x100240fa0 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x118>
100240f44: b4000448    	cbz	x8, 0x100240fcc <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x144>
100240f48: 39400689    	ldrb	w9, [x20, #0x1]
100240f4c: 7103b53f    	cmp	w9, #0xed
100240f50: 54000101    	b.ne	0x100240f70 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0xe8>
100240f54: 39400a89    	ldrb	w9, [x20, #0x2]
100240f58: 121b0929    	and	w9, w9, #0xe0
100240f5c: 7102813f    	cmp	w9, #0xa0
100240f60: 54000081    	b.ne	0x100240f70 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0xe8>
100240f64: 39c00e89    	ldrsb	w9, [x20, #0x3]
100240f68: 3101013f    	cmn	w9, #0x40
100240f6c: 540001ab    	b.lt	0x100240fa0 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x118>
100240f70: f100051f    	cmp	x8, #0x1
100240f74: 540002c0    	b.eq	0x100240fcc <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x144>
100240f78: 39400a88    	ldrb	w8, [x20, #0x2]
100240f7c: 7103b51f    	cmp	w8, #0xed
100240f80: 54000261    	b.ne	0x100240fcc <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x144>
100240f84: 39400e88    	ldrb	w8, [x20, #0x3]
100240f88: 121b0908    	and	w8, w8, #0xe0
100240f8c: 7102811f    	cmp	w8, #0xa0
100240f90: 540001e1    	b.ne	0x100240fcc <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x144>
100240f94: 39c01288    	ldrsb	w8, [x20, #0x4]
100240f98: 3101011f    	cmn	w8, #0x40
100240f9c: 5400018a    	b.ge	0x100240fcc <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x144>
100240fa0: b10006df    	cmn	x22, #0x1
100240fa4: 54000220    	b.eq	0x100240fe8 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x160>
100240fa8: aa1403e0    	mov	x0, x20
100240fac: aa1303e1    	mov	x1, x19
100240fb0: 94041640    	bl	0x1003468b0 <perry_runtime::string::js_string_from_builder_bytes>
100240fb4: aa0003e8    	mov	x8, x0
100240fb8: d2efffe0    	mov	x0, #0x7fff000000000000 ; =9223090561878065152
100240fbc: b340bd00    	bfxil	x0, x8, #0, #48
100240fc0: f10006df    	cmp	x22, #0x1
100240fc4: 54000daa    	b.ge	0x100241178 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x2f0>
100240fc8: 17ffffca    	b	0x100240ef0 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x68>
100240fcc: b4000713    	cbz	x19, 0x1002410ac <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x224>
100240fd0: f100127f    	cmp	x19, #0x4
100240fd4: 54000702    	b.hs	0x1002410b4 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x22c>
100240fd8: d280000a    	mov	x10, #0x0               ; =0
100240fdc: d2800009    	mov	x9, #0x0                ; =0
100240fe0: aa1403e8    	mov	x8, x20
100240fe4: 14000058    	b	0x100241144 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x2bc>
100240fe8: 910082a0    	add	x0, x21, #0x20
100240fec: aa1403e1    	mov	x1, x20
100240ff0: aa1303e2    	mov	x2, x19
100240ff4: 940ea2b4    	bl	0x1005e9ac4 <perry_runtime::string::json_construction::string_from_json_bytes>
100240ff8: f94036b6    	ldr	x22, [x21, #0x68]
100240ffc: f14802df    	cmp	x22, #0x200, lsl #12    ; =0x200000
100241000: 54000f68    	b.hi	0x1002411ec <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x364>
100241004: f104027f    	cmp	x19, #0x100
100241008: 54000f23    	b.lo	0x1002411ec <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x364>
10024100c: b4000f00    	cbz	x0, 0x1002411ec <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x364>
100241010: f94066b4    	ldr	x20, [x21, #0xc8]
100241014: b4000ed4    	cbz	x20, 0x1002411ec <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x364>
100241018: f9403ab5    	ldr	x21, [x21, #0x70]
10024101c: cb5606c8    	sub	x8, x22, x22, lsr #1
100241020: eb08027f    	cmp	x19, x8
100241024: aa1702a8    	orr	x8, x21, x23
100241028: 92607d08    	and	x8, x8, #0xffffffff00000000
10024102c: fa402900    	ccmp	x8, #0x0, #0x0, hs
100241030: 54000de1    	b.ne	0x1002411ec <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x364>
100241034: b00069e8    	adrp	x8, 0x100f7e000 <__MergedGlobals.1917+0xc8>
100241038: b9456113    	ldr	w19, [x8, #0x560]
10024103c: 710c027f    	cmp	w19, #0x300
100241040: 54000ec2    	b.hs	0x100241218 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x390>
100241044: f00069c8    	adrp	x8, 0x100f7c000 <_perry_global_worker_ts__1>
100241048: f9402508    	ldr	x8, [x8, #0x48]
10024104c: b100051f    	cmn	x8, #0x1
100241050: 54000d60    	b.eq	0x1002411fc <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x374>
100241054: d53bd069    	mrs	x9, TPIDRRO_EL0
100241058: 927df129    	and	x9, x9, #0xfffffffffffffff8
10024105c: f8687928    	ldr	x8, [x9, x8, lsl #3]
100241060: b4000ce8    	cbz	x8, 0x1002411fc <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x374>
100241064: 8b130d08    	add	x8, x8, x19, lsl #3
100241068: f940f513    	ldr	x19, [x8, #0x1e8]
10024106c: b4000d73    	cbz	x19, 0x100241218 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x390>
100241070: f9400268    	ldr	x8, [x19]
100241074: b5000e28    	cbnz	x8, 0x100241238 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x3b0>
100241078: 92800008    	mov	x8, #-0x1               ; =-1
10024107c: f9000268    	str	x8, [x19]
100241080: 39409268    	ldrb	w8, [x19, #0x24]
100241084: 7100091f    	cmp	w8, #0x2
100241088: 54000820    	b.eq	0x10024118c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x304>
10024108c: f9400668    	ldr	x8, [x19, #0x8]
100241090: eb14011f    	cmp	x8, x20
100241094: 540007c1    	b.ne	0x10024118c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x304>
100241098: b9401a68    	ldr	w8, [x19, #0x18]
10024109c: 6b16011f    	cmp	w8, w22
1002410a0: 54000761    	b.ne	0x10024118c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x304>
1002410a4: d2800008    	mov	x8, #0x0                ; =0
1002410a8: 14000050    	b	0x1002411e8 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x360>
1002410ac: d280000a    	mov	x10, #0x0               ; =0
1002410b0: 1400002d    	b	0x100241164 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x2dc>
1002410b4: 927e0269    	and	x9, x19, #0x4
1002410b8: 8b090288    	add	x8, x20, x9
1002410bc: f00052aa    	adrp	x10, 0x100c98000 <perry_runtime::string::format::fmt_fixed_int::POW10+0x5010>
1002410c0: 3dc21940    	ldr	q0, [x10, #0x860]
1002410c4: f0004faa    	adrp	x10, 0x100c38000 <itoa::DECIMAL_PAIRS+0x3383a>
1002410c8: 3dc1d142    	ldr	q2, [x10, #0x740]
1002410cc: 6f00e401    	movi.2d	v1, #0000000000000000
1002410d0: 6f00e423    	movi.2d	v3, #0x000000000000ff
1002410d4: 5280008a    	mov	w10, #0x4               ; =4
1002410d8: 4e080d44    	dup.2d	v4, x10
1002410dc: aa1403ea    	mov	x10, x20
1002410e0: 927e026b    	and	x11, x19, #0x4
1002410e4: 6f00e405    	movi.2d	v5, #0000000000000000
1002410e8: bc404546    	ldr	s6, [x10], #0x4
1002410ec: 2f08a4c6    	ushll.8h	v6, v6, #0x0
1002410f0: 2f10a4c6    	ushll.4s	v6, v6, #0x0
1002410f4: 6f20a4c7    	ushll2.2d	v7, v6, #0x0
1002410f8: 4e231ce7    	and.16b	v7, v7, v3
1002410fc: 2f20a4c6    	ushll.2d	v6, v6, #0x0
100241100: 4e231cc6    	and.16b	v6, v6, v3
100241104: 4f435410    	shl.2d	v16, v0, #0x3
100241108: 4f435451    	shl.2d	v17, v2, #0x3
10024110c: 6ef144c6    	ushl.2d	v6, v6, v17
100241110: 6ef044e7    	ushl.2d	v7, v7, v16
100241114: 4ea11ce1    	orr.16b	v1, v7, v1
100241118: 4ea51cc5    	orr.16b	v5, v6, v5
10024111c: 4ee48400    	add.2d	v0, v0, v4
100241120: 4ee48442    	add.2d	v2, v2, v4
100241124: f100116b    	subs	x11, x11, #0x4
100241128: 54fffe01    	b.ne	0x1002410e8 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x260>
10024112c: 4ea11ca0    	orr.16b	v0, v5, v1
100241130: 5e180401    	mov	d1, v0[1]
100241134: 0ea11c00    	orr.8b	v0, v0, v1
100241138: 9e66000a    	fmov	x10, d0
10024113c: eb09027f    	cmp	x19, x9
100241140: 54000120    	b.eq	0x100241164 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x2dc>
100241144: 8b13028b    	add	x11, x20, x19
100241148: d37df129    	lsl	x9, x9, #3
10024114c: 3840150c    	ldrb	w12, [x8], #0x1
100241150: 9ac9218c    	lsl	x12, x12, x9
100241154: aa0a018a    	orr	x10, x12, x10
100241158: 91002129    	add	x9, x9, #0x8
10024115c: eb0b011f    	cmp	x8, x11
100241160: 54ffff61    	b.ne	0x10024114c <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x2c4>
100241164: aa13a148    	orr	x8, x10, x19, lsl #40
100241168: d2efff29    	mov	x9, #0x7ff9000000000000 ; =9221401712017801216
10024116c: aa090100    	orr	x0, x8, x9
100241170: f10006df    	cmp	x22, #0x1
100241174: 54ffebeb    	b.lt	0x100240ef0 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x68>
100241178: aa0003f3    	mov	x19, x0
10024117c: aa1403e0    	mov	x0, x20
100241180: 94247310    	bl	0x100b5ddc0 <_mi_free>
100241184: aa1303e0    	mov	x0, x19
100241188: 17ffff5a    	b	0x100240ef0 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x68>
10024118c: a9008274    	stp	x20, x0, [x19, #0x8]
100241190: 29035e76    	stp	w22, w23, [x19, #0x18]
100241194: b9002275    	str	w21, [x19, #0x20]
100241198: 3900927f    	strb	wzr, [x19, #0x24]
10024119c: 900070f5    	adrp	x21, 0x10105d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xf79c>
1002411a0: b94866a8    	ldr	w8, [x21, #0x864]
1002411a4: 340000e8    	cbz	w8, 0x1002411c0 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x338>
1002411a8: d2efffe8    	mov	x8, #0x7fff000000000000 ; =9223090561878065152
1002411ac: b340be88    	bfxil	x8, x20, #0, #48
1002411b0: aa0003f4    	mov	x20, x0
1002411b4: aa0803e0    	mov	x0, x8
1002411b8: 9408cd0e    	bl	0x1004745f0 <perry_runtime::gc::barrier::incremental_mark_barrier_value_active>
1002411bc: aa1403e0    	mov	x0, x20
1002411c0: b94866a8    	ldr	w8, [x21, #0x864]
1002411c4: 340000e8    	cbz	w8, 0x1002411e0 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x358>
1002411c8: d2efffe8    	mov	x8, #0x7fff000000000000 ; =9223090561878065152
1002411cc: b340bc08    	bfxil	x8, x0, #0, #48
1002411d0: aa0003f4    	mov	x20, x0
1002411d4: aa0803e0    	mov	x0, x8
1002411d8: 9408cd06    	bl	0x1004745f0 <perry_runtime::gc::barrier::incremental_mark_barrier_value_active>
1002411dc: aa1403e0    	mov	x0, x20
1002411e0: f9400268    	ldr	x8, [x19]
1002411e4: 91000508    	add	x8, x8, #0x1
1002411e8: f9000268    	str	x8, [x19]
1002411ec: d2efffe8    	mov	x8, #0x7fff000000000000 ; =9223090561878065152
1002411f0: b340bc08    	bfxil	x8, x0, #0, #48
1002411f4: aa0803e0    	mov	x0, x8
1002411f8: 17ffff3e    	b	0x100240ef0 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x68>
1002411fc: aa0003f8    	mov	x24, x0
100241200: 9423f40f    	bl	0x100b3e23c <perry_runtime::tls_hot::hot_uncached>
100241204: aa0003e8    	mov	x8, x0
100241208: aa1803e0    	mov	x0, x24
10024120c: 8b130d08    	add	x8, x8, x19, lsl #3
100241210: f940f513    	ldr	x19, [x8, #0x1e8]
100241214: b5fff2f3    	cbnz	x19, 0x100241070 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x1e8>
100241218: f90003e0    	str	x0, [sp]
10024121c: d0006640    	adrp	x0, 0x100f0b000 <perry_runtime::os::os_process_emitter::PROCESS_EXIT_EVENT_EMITTED+0x1eb8>
100241220: 913e8000    	add	x0, x0, #0xfa0
100241224: 9423ea0e    	bl	0x100b3ba5c <<perry_runtime::tls_hot::HotKey<perry_runtime::closure::registry::DispatchRecent>>::get_slow>
100241228: aa0003f3    	mov	x19, x0
10024122c: f94003e0    	ldr	x0, [sp]
100241230: f9400268    	ldr	x8, [x19]
100241234: b4fff228    	cbz	x8, 0x100241078 <<perry_runtime::json::parser::DirectParser>::parse_string_value+0x1f0>
100241238: 900065c0    	adrp	x0, 0x100ef9000 <perry_runtime::closure::dispatch::bound::KEEP_JS_FUNCTION_BIND+0x7ed0>
10024123c: 91326000    	add	x0, x0, #0xc98
100241240: 94233f9b    	bl	0x100b110ac <core::cell::panic_already_borrowed>
		...
