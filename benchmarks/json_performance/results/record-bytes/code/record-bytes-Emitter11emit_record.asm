/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008b4b80 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record>:
1008b4b80:      sub sp, sp, #0xb0
1008b4b84:      stp x28, x27, [sp, #0x50]
1008b4b88:      stp x26, x25, [sp, #0x60]
1008b4b8c:      stp x24, x23, [sp, #0x70]
1008b4b90:      stp x22, x21, [sp, #0x80]
1008b4b94:      stp x20, x19, [sp, #0x90]
1008b4b98:      stp x29, x30, [sp, #0xa0]
1008b4b9c:      add x29, sp, #0xa0
1008b4ba0:      mov w8, #0x7ffd             ; =32765
1008b4ba4:      cmp x8, x1, lsr #48
1008b4ba8:      b.ne    0x1008b4ccc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008b4bac:      mov x20, x0
1008b4bb0:      mov x23, x2
1008b4bb4:      mov x19, x3
1008b4bb8:      and x21, x1, #0xffffffffffff
1008b4bbc:      mov x0, x21
1008b4bc0:      bl  0x100902d58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008b4bc4:      cbz x0, 0x1008b4cd0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x150>
1008b4bc8:      ldrb    w8, [x0]
1008b4bcc:      cmp w8, #0x2
1008b4bd0:      b.ne    0x1008b4ccc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008b4bd4:      ldrsb   w8, [x0, #0x1]
1008b4bd8:      tbnz    w8, #0x1f, 0x1008b4ccc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008b4bdc:      ldrh    w8, [x0, #0x2]
1008b4be0:      mov w9, #0xa00              ; =2560
1008b4be4:      and w8, w8, w9
1008b4be8:      cmp w8, #0x200
1008b4bec:      b.ne    0x1008b4ccc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008b4bf0:      ldr w8, [x0, #0x4]
1008b4bf4:      cmp w8, #0x18
1008b4bf8:      b.lo    0x1008b4ccc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008b4bfc:      ldr w8, [x21]
1008b4c00:      cbnz    w8, 0x1008b4ccc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008b4c04:      mov x22, x0
1008b4c08:      mov x0, x21
1008b4c0c:      bl  0x1009ed870 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object15prototype_chain23object_static_prototype>
1008b4c10:      cmp x0, #0x1
1008b4c14:      b.eq    0x1008b4ccc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008b4c18:      ldr w8, [x21, #0x4]
1008b4c1c:      mov w9, #-0x40000001        ; =-1073741825
1008b4c20:      cmp w8, w9
1008b4c24:      csel    w2, wzr, w8, gt
1008b4c28:      b.gt    0x1008b4ccc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008b4c2c:      ldr w22, [x22, #0x4]
1008b4c30:      mov w9, #0x79b9             ; =31161
1008b4c34:      movk    w9, #0x9e37, lsl #16
1008b4c38:      mul w8, w8, w9
1008b4c3c:      lsr x3, x8, #25
1008b4c40:      mov x9, x20
1008b4c44:      add x10, x20, #0x18
1008b4c48:      ldrb    w13, [x10, x3]
1008b4c4c:      cbz w13, 0x1008b4c90 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x110>
1008b4c50:      add x11, x9, #0x98
1008b4c54:      mov w12, #0x8c              ; =140
1008b4c58:      mov x8, x19
1008b4c5c:      and w14, w13, #0xff
1008b4c60:      sub w13, w13, #0x1
1008b4c64:      and x1, x13, #0xff
1008b4c68:      cmp w14, #0x41
1008b4c6c:      b.hs    0x1008b5450 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x8d0>
1008b4c70:      umull   x13, w1, w12
1008b4c74:      ldr w13, [x11, x13]
1008b4c78:      cmp w13, w2
1008b4c7c:      b.eq    0x1008b4ca8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x128>
1008b4c80:      add w13, w3, #0x1
1008b4c84:      and x3, x13, #0x7f
1008b4c88:      ldrb    w13, [x10, x3]
1008b4c8c:      cbnz    w13, 0x1008b4c5c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0xdc>
1008b4c90:      mov x0, x9
1008b4c94:      mov x1, x21
1008b4c98:      bl  0x1008b54a0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan>
1008b4c9c:      tbz w0, #0x0, 0x1008b4ccc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008b4ca0:      mov x8, x19
1008b4ca4:      mov x9, x20
1008b4ca8:      cmp x1, #0x40
1008b4cac:      b.hs    0x1008b5450 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x8d0>
1008b4cb0:      mov w10, #0x8c              ; =140
1008b4cb4:      madd    x10, x1, x10, x9
1008b4cb8:      ldr w24, [x10, #0x9c]
1008b4cbc:      lsl x11, x24, #3
1008b4cc0:      add x11, x11, #0x18
1008b4cc4:      cmp x11, x22
1008b4cc8:      b.ls    0x1008b4cf0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x170>
1008b4ccc:      mov w0, #0x0                ; =0
1008b4cd0:      ldp x29, x30, [sp, #0xa0]
1008b4cd4:      ldp x20, x19, [sp, #0x90]
1008b4cd8:      ldp x22, x21, [sp, #0x80]
1008b4cdc:      ldp x24, x23, [sp, #0x70]
1008b4ce0:      ldp x26, x25, [sp, #0x60]
1008b4ce4:      ldp x28, x27, [sp, #0x50]
1008b4ce8:      add sp, sp, #0xb0
1008b4cec:      ret
1008b4cf0:      cbz w24, 0x1008b53b8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x838>
1008b4cf4:      mov x28, #0x0               ; =0
1008b4cf8:      add x10, x10, #0x98
1008b4cfc:      add x11, x21, #0x10
1008b4d00:      add x27, x10, #0x8
1008b4d04:      add x10, sp, #0x20
1008b4d08:      orr x10, x10, #0x1
1008b4d0c:      stp x10, x11, [sp, #0x8]
1008b4d10:      ldr x11, [sp, #0x10]
1008b4d14:      ldr x21, [x11, x28, lsl #3]
1008b4d18:      mov x11, #0x1               ; =1
1008b4d1c:      movk    x11, #0x7ffc, lsl #48
1008b4d20:      cmp x21, x11
1008b4d24:      b.eq    0x1008b4ccc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008b4d28:      mov x10, x28
1008b4d2c:      cmp x28, #0x20
1008b4d30:      b.eq    0x1008b5464 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x8e4>
1008b4d34:      add x28, x10, #0x1
1008b4d38:      ldr w25, [x27, x10, lsl #2]
1008b4d3c:      ldr w10, [x27, x28, lsl #2]
1008b4d40:      ldr x26, [x9, #0x8]
1008b4d44:      sub x22, x10, x25
1008b4d48:      ldr x1, [x8, #0x10]
1008b4d4c:      ldr x9, [x8]
1008b4d50:      sub x9, x9, x1
1008b4d54:      cmp x22, x9
1008b4d58:      b.hi    0x1008b4d8c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x20c>
1008b4d5c:      add x9, x26, x25
1008b4d60:      ldr x10, [x8, #0x8]
1008b4d64:      add x0, x10, x1
1008b4d68:      cmp x22, #0x20
1008b4d6c:      b.ls    0x1008b4dbc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x23c>
1008b4d70:      mov x1, x9
1008b4d74:      mov x2, x22
1008b4d78:      bl  0x100ce9f6c <_writev+0x100ce9f6c>
1008b4d7c:      mov x8, x19
1008b4d80:      ldr x1, [x19, #0x10]
1008b4d84:      mov x10, x23
1008b4d88:      b   0x1008b4e54 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x2d4>
1008b4d8c:      mov x0, x8
1008b4d90:      mov x2, x22
1008b4d94:      mov w3, #0x1                ; =1
1008b4d98:      mov w4, #0x1                ; =1
1008b4d9c:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008b4da0:      mov x8, x19
1008b4da4:      ldr x1, [x19, #0x10]
1008b4da8:      add x9, x26, x25
1008b4dac:      ldr x10, [x19, #0x8]
1008b4db0:      add x0, x10, x1
1008b4db4:      cmp x22, #0x20
1008b4db8:      b.hi    0x1008b4d70 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x1f0>
1008b4dbc:      cmp x22, #0xf
1008b4dc0:      mov x10, x23
1008b4dc4:      b.ls    0x1008b4de8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x268>
1008b4dc8:      ldp x11, x12, [x9]
1008b4dcc:      stp x11, x12, [x0]
1008b4dd0:      sub x11, x22, #0x10
1008b4dd4:      add x12, x0, x11
1008b4dd8:      add x9, x9, x11
1008b4ddc:      ldp x9, x11, [x9]
1008b4de0:      stp x9, x11, [x12]
1008b4de4:      b   0x1008b4e54 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x2d4>
1008b4de8:      cmp x22, #0x7
1008b4dec:      b.ls    0x1008b4e08 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x288>
1008b4df0:      ldr x11, [x9]
1008b4df4:      str x11, [x0]
1008b4df8:      sub x11, x22, #0x8
1008b4dfc:      ldr x9, [x9, x11]
1008b4e00:      str x9, [x0, x11]
1008b4e04:      b   0x1008b4e54 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x2d4>
1008b4e08:      cmp x22, #0x3
1008b4e0c:      b.ls    0x1008b4e28 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x2a8>
1008b4e10:      ldr w11, [x9]
1008b4e14:      str w11, [x0]
1008b4e18:      sub x11, x22, #0x4
1008b4e1c:      ldr w9, [x9, x11]
1008b4e20:      str w9, [x0, x11]
1008b4e24:      b   0x1008b4e54 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x2d4>
1008b4e28:      cmp x22, #0x1
1008b4e2c:      b.ls    0x1008b4e48 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x2c8>
1008b4e30:      ldrh    w11, [x9]
1008b4e34:      strh    w11, [x0]
1008b4e38:      sub x11, x22, #0x2
1008b4e3c:      ldrh    w9, [x9, x11]
1008b4e40:      strh    w9, [x0, x11]
1008b4e44:      b   0x1008b4e54 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x2d4>
1008b4e48:      b.ne    0x1008b4e54 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x2d4>
1008b4e4c:      ldrb    w9, [x9]
1008b4e50:      strb    w9, [x0]
1008b4e54:      add x1, x1, x22
1008b4e58:      str x1, [x8, #0x10]
1008b4e5c:      mov x9, #0x1                ; =1
1008b4e60:      movk    x9, #0x7ffc, lsl #48
1008b4e64:      add x9, x9, #0xf
1008b4e68:      cmp x21, x9
1008b4e6c:      b.eq    0x1008b4ef8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x378>
1008b4e70:      and x9, x21, #0xffff000000000000
1008b4e74:      mov x11, #0x7ffa000000000000 ; =9221683186994511872
1008b4e78:      cmp x9, x11
1008b4e7c:      b.eq    0x1008b4ef8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x378>
1008b4e80:      mov x11, #0x7ffd000000000000 ; =9222527611924643840
1008b4e84:      cmp x9, x11
1008b4e88:      b.eq    0x1008b4ef8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x378>
1008b4e8c:      mov x11, #-0x10000000000000 ; =-4503599627370496
1008b4e90:      add x11, x21, x11
1008b4e94:      tst x21, #0x7
1008b4e98:      mov x12, #-0xfffffffffffff  ; =-4503599627370495
1008b4e9c:      ccmp    x11, x12, #0x0, eq
1008b4ea0:      b.hs    0x1008b4ef8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x378>
1008b4ea4:      mov x10, #0x2               ; =2
1008b4ea8:      movk    x10, #0x7ffc, lsl #48
1008b4eac:      cmp x21, x10
1008b4eb0:      b.eq    0x1008b4f64 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3e4>
1008b4eb4:      mov x10, #0x3               ; =3
1008b4eb8:      movk    x10, #0x7ffc, lsl #48
1008b4ebc:      cmp x21, x10
1008b4ec0:      b.eq    0x1008b4f28 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3a8>
1008b4ec4:      mov x10, #0x4               ; =4
1008b4ec8:      movk    x10, #0x7ffc, lsl #48
1008b4ecc:      cmp x21, x10
1008b4ed0:      b.ne    0x1008b4f9c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x41c>
1008b4ed4:      ldr x8, [x8]
1008b4ed8:      sub x8, x8, x1
1008b4edc:      cmp x8, #0x3
1008b4ee0:      b.ls    0x1008b5310 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x790>
1008b4ee4:      mov x8, x19
1008b4ee8:      ldr x9, [x19, #0x8]
1008b4eec:      mov w10, #0x7274            ; =29300
1008b4ef0:      movk    w10, #0x6575, lsl #16
1008b4ef4:      b   0x1008b4f84 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x404>
1008b4ef8:      tbz w10, #0x0, 0x1008b4ccc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x14c>
1008b4efc:      mov x0, x20
1008b4f00:      mov x1, x21
1008b4f04:      mov w2, #0x0                ; =0
1008b4f08:      mov x3, x19
1008b4f0c:      bl  0x1008b4b80 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record>
1008b4f10:      mov x8, x19
1008b4f14:      mov x9, x20
1008b4f18:      cbz w0, 0x1008b4cd0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x150>
1008b4f1c:      cmp x28, x24
1008b4f20:      b.ne    0x1008b4d10 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x190>
1008b4f24:      b   0x1008b53e4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x864>
1008b4f28:      ldr x8, [x8]
1008b4f2c:      sub x8, x8, x1
1008b4f30:      cmp x8, #0x4
1008b4f34:      b.ls    0x1008b52f4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x774>
1008b4f38:      mov x8, x19
1008b4f3c:      ldr x9, [x19, #0x8]
1008b4f40:      add x9, x9, x1
1008b4f44:      mov w10, #0x65              ; =101
1008b4f48:      strb    w10, [x9, #0x4]
1008b4f4c:      mov w10, #0x6166            ; =24934
1008b4f50:      movk    w10, #0x736c, lsl #16
1008b4f54:      str w10, [x9]
1008b4f58:      ldr x9, [x19, #0x10]
1008b4f5c:      add x9, x9, #0x5
1008b4f60:      b   0x1008b4f90 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x410>
1008b4f64:      ldr x8, [x8]
1008b4f68:      sub x8, x8, x1
1008b4f6c:      cmp x8, #0x3
1008b4f70:      b.ls    0x1008b5128 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x5a8>
1008b4f74:      mov x8, x19
1008b4f78:      ldr x9, [x19, #0x8]
1008b4f7c:      mov w10, #0x756e            ; =30062
1008b4f80:      movk    w10, #0x6c6c, lsl #16
1008b4f84:      str w10, [x9, x1]
1008b4f88:      ldr x9, [x19, #0x10]
1008b4f8c:      add x9, x9, #0x4
1008b4f90:      str x9, [x19, #0x10]
1008b4f94:      mov x9, x20
1008b4f98:      b   0x1008b4f1c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x39c>
1008b4f9c:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1008b4fa0:      cmp x9, x8
1008b4fa4:      b.eq    0x1008b4fcc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x44c>
1008b4fa8:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1008b4fac:      cmp x9, x8
1008b4fb0:      b.ne    0x1008b5030 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x4b0>
1008b4fb4:      and x8, x21, #0xffffffffffff
1008b4fb8:      cmp x8, #0x1, lsl #12       ; =0x1000
1008b4fbc:      b.lo    0x1008b5118 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x598>
1008b4fc0:      ldr w2, [x8, #0x4]
1008b4fc4:      add x1, x8, #0x14
1008b4fc8:      b   0x1008b518c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x60c>
1008b4fcc:      strb    wzr, [sp, #0x1c]
1008b4fd0:      str wzr, [sp, #0x18]
1008b4fd4:      ubfx    x8, x21, #40, #8
1008b4fd8:      mov x22, x8
1008b4fdc:      cbz x8, 0x1008b5068 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x4e8>
1008b4fe0:      strb    w21, [sp, #0x18]
1008b4fe4:      cmp x22, #0x1
1008b4fe8:      b.eq    0x1008b506c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x4ec>
1008b4fec:      lsr x8, x21, #8
1008b4ff0:      strb    w8, [sp, #0x19]
1008b4ff4:      cmp x22, #0x2
1008b4ff8:      b.eq    0x1008b506c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x4ec>
1008b4ffc:      lsr x8, x21, #16
1008b5000:      strb    w8, [sp, #0x1a]
1008b5004:      cmp x22, #0x3
1008b5008:      b.eq    0x1008b506c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x4ec>
1008b500c:      lsr x8, x21, #24
1008b5010:      strb    w8, [sp, #0x1b]
1008b5014:      cmp x22, #0x4
1008b5018:      b.eq    0x1008b506c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x4ec>
1008b501c:      lsr x8, x21, #32
1008b5020:      strb    w8, [sp, #0x1c]
1008b5024:      cmp x22, #0x5
1008b5028:      b.eq    0x1008b506c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x4ec>
1008b502c:      b   0x1008b548c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x90c>
1008b5030:      fmov    d0, x21
1008b5034:      fcvtzs  w22, d0
1008b5038:      scvtf   d1, w22
1008b503c:      fcmp    d0, d1
1008b5040:      b.ne    0x1008b5144 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x5c4>
1008b5044:      cmp w22, #0x0
1008b5048:      cneg    w0, w22, mi
1008b504c:      add x8, sp, #0x20
1008b5050:      add x1, x8, #0x1
1008b5054:      bl  0x1000991b0 <__RNvXst_Cs8xF4iOrs9m2_4itoamNtB5_8Unsigned3fmt>
1008b5058:      mov x21, x0
1008b505c:      tbnz    w22, #0x1f, 0x1008b51a0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x620>
1008b5060:      add x21, x21, #0x1
1008b5064:      b   0x1008b51b4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x634>
1008b5068:      mov w21, #0x0               ; =0
1008b506c:      mov x9, x22
1008b5070:      add x11, sp, #0x18
1008b5074:      cbz x22, 0x1008b50a8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x528>
1008b5078:      mov x8, #0x0                ; =0
1008b507c:      ldrsb   w10, [x11, x8]
1008b5080:      cmp w10, #0x20
1008b5084:      b.lt    0x1008b50fc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x57c>
1008b5088:      and w10, w10, #0xff
1008b508c:      cmp w10, #0x22
1008b5090:      b.eq    0x1008b50fc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x57c>
1008b5094:      cmp w10, #0x5c
1008b5098:      b.eq    0x1008b50fc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x57c>
1008b509c:      add x8, x8, #0x1
1008b50a0:      cmp x9, x8
1008b50a4:      b.ne    0x1008b507c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x4fc>
1008b50a8:      add x10, sp, #0x18
1008b50ac:      mov w8, #0x22               ; =34
1008b50b0:      strb    w8, [sp, #0x26]
1008b50b4:      mov w8, #0x2222             ; =8738
1008b50b8:      strh    w8, [sp, #0x24]
1008b50bc:      mov w8, #0x22222222         ; =572662306
1008b50c0:      str w8, [sp, #0x20]
1008b50c4:      cmp x9, #0x3
1008b50c8:      b.ls    0x1008b5150 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x5d0>
1008b50cc:      ldr w8, [sp, #0x18]
1008b50d0:      stur    w8, [sp, #0x21]
1008b50d4:      sub x8, x22, #0x4
1008b50d8:      ldr w9, [x10, x8]
1008b50dc:      ldr x10, [sp, #0x8]
1008b50e0:      str w9, [x10, x8]
1008b50e4:      add x21, x22, #0x2
1008b50e8:      ldr x8, [x19]
1008b50ec:      sub x8, x8, x1
1008b50f0:      cmp x21, x8
1008b50f4:      b.ls    0x1008b5214 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x694>
1008b50f8:      b   0x1008b5264 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x6e4>
1008b50fc:      add x8, sp, #0x20
1008b5100:      add x0, sp, #0x18
1008b5104:      mov x1, x9
1008b5108:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1008b510c:      ldr w8, [sp, #0x20]
1008b5110:      tbz w8, #0x0, 0x1008b5188 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x608>
1008b5114:      ldr x1, [x19, #0x10]
1008b5118:      ldr x8, [x19]
1008b511c:      sub x8, x8, x1
1008b5120:      cmp x8, #0x3
1008b5124:      b.hi    0x1008b4f74 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3f4>
1008b5128:      mov x0, x19
1008b512c:      mov w2, #0x4                ; =4
1008b5130:      mov w3, #0x1                ; =1
1008b5134:      mov w4, #0x1                ; =1
1008b5138:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008b513c:      ldr x1, [x19, #0x10]
1008b5140:      b   0x1008b4f74 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3f4>
1008b5144:      mov x0, x19
1008b5148:      bl  0x1008ec808 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
1008b514c:      b   0x1008b5194 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x614>
1008b5150:      cmp x22, #0x1
1008b5154:      b.ls    0x1008b51fc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x67c>
1008b5158:      ldrh    w8, [sp, #0x18]
1008b515c:      sturh   w8, [sp, #0x21]
1008b5160:      sub x8, x22, #0x2
1008b5164:      ldrh    w9, [x10, x8]
1008b5168:      ldr x10, [sp, #0x8]
1008b516c:      strh    w9, [x10, x8]
1008b5170:      add x21, x22, #0x2
1008b5174:      ldr x8, [x19]
1008b5178:      sub x8, x8, x1
1008b517c:      cmp x21, x8
1008b5180:      b.ls    0x1008b5214 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x694>
1008b5184:      b   0x1008b5264 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x6e4>
1008b5188:      ldp x1, x2, [sp, #0x28]
1008b518c:      mov x0, x19
1008b5190:      bl  0x1008edb14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1008b5194:      mov x8, x19
1008b5198:      mov x9, x20
1008b519c:      b   0x1008b4f1c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x39c>
1008b51a0:      cmp x21, #0xa
1008b51a4:      b.hi    0x1008b5478 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x8f8>
1008b51a8:      add x8, sp, #0x20
1008b51ac:      mov w9, #0x2d               ; =45
1008b51b0:      strb    w9, [x8, x21]
1008b51b4:      mov w8, #0xb                ; =11
1008b51b8:      sub x22, x8, x21
1008b51bc:      ldr x1, [x19, #0x10]
1008b51c0:      ldr x8, [x19]
1008b51c4:      sub x8, x8, x1
1008b51c8:      cmp x22, x8
1008b51cc:      b.hi    0x1008b532c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x7ac>
1008b51d0:      ldr x8, [x19, #0x8]
1008b51d4:      add x8, x8, x1
1008b51d8:      cmp x21, #0x4
1008b51dc:      b.hs    0x1008b5354 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x7d4>
1008b51e0:      add x9, sp, #0x20
1008b51e4:      ldr x9, [x9, x21]
1008b51e8:      str x9, [x8]
1008b51ec:      sub x8, x8, x21
1008b51f0:      ldur    x9, [sp, #0x23]
1008b51f4:      stur    x9, [x8, #0x3]
1008b51f8:      b   0x1008b53ac <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x82c>
1008b51fc:      b.eq    0x1008b524c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x6cc>
1008b5200:      add x21, x22, #0x2
1008b5204:      ldr x8, [x19]
1008b5208:      sub x8, x8, x1
1008b520c:      cmp x21, x8
1008b5210:      b.hi    0x1008b5264 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x6e4>
1008b5214:      ldr x8, [x19, #0x8]
1008b5218:      add x8, x8, x1
1008b521c:      cmp x22, #0xd
1008b5220:      b.ls    0x1008b5288 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x708>
1008b5224:      ldp x9, x10, [sp, #0x20]
1008b5228:      stp x9, x10, [x8]
1008b522c:      sub x9, x22, #0xe
1008b5230:      add x8, x8, x9
1008b5234:      add x10, sp, #0x20
1008b5238:      add x9, x10, x9
1008b523c:      ldp x9, x10, [x9]
1008b5240:      stp x9, x10, [x8]
1008b5244:      add x9, x1, x21
1008b5248:      b   0x1008b53b0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x830>
1008b524c:      strb    w21, [sp, #0x21]
1008b5250:      add x21, x22, #0x2
1008b5254:      ldr x8, [x19]
1008b5258:      sub x8, x8, x1
1008b525c:      cmp x21, x8
1008b5260:      b.ls    0x1008b5214 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x694>
1008b5264:      add x2, x22, #0x2
1008b5268:      mov x0, x19
1008b526c:      mov w3, #0x1                ; =1
1008b5270:      mov w4, #0x1                ; =1
1008b5274:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008b5278:      ldp x8, x1, [x19, #0x8]
1008b527c:      add x8, x8, x1
1008b5280:      cmp x22, #0xd
1008b5284:      b.hi    0x1008b5224 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x6a4>
1008b5288:      cmp x22, #0x5
1008b528c:      b.ls    0x1008b52b0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x730>
1008b5290:      ldr x9, [sp, #0x20]
1008b5294:      str x9, [x8]
1008b5298:      sub x9, x22, #0x6
1008b529c:      add x10, sp, #0x20
1008b52a0:      ldr x10, [x10, x9]
1008b52a4:      str x10, [x8, x9]
1008b52a8:      add x9, x1, x21
1008b52ac:      b   0x1008b53b0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x830>
1008b52b0:      cmp x22, #0x1
1008b52b4:      b.ls    0x1008b52d8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x758>
1008b52b8:      ldr w9, [sp, #0x20]
1008b52bc:      str w9, [x8]
1008b52c0:      sub x9, x22, #0x2
1008b52c4:      add x10, sp, #0x20
1008b52c8:      ldr w10, [x10, x9]
1008b52cc:      str w10, [x8, x9]
1008b52d0:      add x9, x1, x21
1008b52d4:      b   0x1008b53b0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x830>
1008b52d8:      ldrh    w9, [sp, #0x20]
1008b52dc:      strh    w9, [x8]
1008b52e0:      add x9, sp, #0x20
1008b52e4:      ldrh    w9, [x9, x22]
1008b52e8:      strh    w9, [x8, x22]
1008b52ec:      add x9, x1, x21
1008b52f0:      b   0x1008b53b0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x830>
1008b52f4:      mov x0, x19
1008b52f8:      mov w2, #0x5                ; =5
1008b52fc:      mov w3, #0x1                ; =1
1008b5300:      mov w4, #0x1                ; =1
1008b5304:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008b5308:      ldr x1, [x19, #0x10]
1008b530c:      b   0x1008b4f38 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x3b8>
1008b5310:      mov x0, x19
1008b5314:      mov w2, #0x4                ; =4
1008b5318:      mov w3, #0x1                ; =1
1008b531c:      mov w4, #0x1                ; =1
1008b5320:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008b5324:      ldr x1, [x19, #0x10]
1008b5328:      b   0x1008b4ee4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x364>
1008b532c:      mov w8, #0xb                ; =11
1008b5330:      sub x2, x8, x21
1008b5334:      mov x0, x19
1008b5338:      mov w3, #0x1                ; =1
1008b533c:      mov w4, #0x1                ; =1
1008b5340:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008b5344:      ldp x8, x1, [x19, #0x8]
1008b5348:      add x8, x8, x1
1008b534c:      cmp x21, #0x4
1008b5350:      b.lo    0x1008b51e0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x660>
1008b5354:      cmp x21, #0x8
1008b5358:      b.hs    0x1008b5378 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x7f8>
1008b535c:      add x9, sp, #0x20
1008b5360:      ldr w9, [x9, x21]
1008b5364:      str w9, [x8]
1008b5368:      sub x8, x8, x21
1008b536c:      ldur    w9, [sp, #0x27]
1008b5370:      stur    w9, [x8, #0x7]
1008b5374:      b   0x1008b53ac <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x82c>
1008b5378:      cmp x21, #0xa
1008b537c:      b.hs    0x1008b539c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x81c>
1008b5380:      add x9, sp, #0x20
1008b5384:      ldrh    w9, [x9, x21]
1008b5388:      strh    w9, [x8]
1008b538c:      sub x8, x8, x21
1008b5390:      ldurh   w9, [sp, #0x29]
1008b5394:      sturh   w9, [x8, #0x9]
1008b5398:      b   0x1008b53ac <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x82c>
1008b539c:      b.ne    0x1008b53ac <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x82c>
1008b53a0:      add x9, sp, #0x20
1008b53a4:      ldrb    w9, [x9, x21]
1008b53a8:      strb    w9, [x8]
1008b53ac:      add x9, x1, x22
1008b53b0:      mov x8, x19
1008b53b4:      b   0x1008b4f90 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x410>
1008b53b8:      ldr x1, [x8, #0x10]
1008b53bc:      ldr x9, [x8]
1008b53c0:      sub x9, x9, x1
1008b53c4:      cmp x9, #0x1
1008b53c8:      b.ls    0x1008b5410 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x890>
1008b53cc:      ldr x9, [x8, #0x8]
1008b53d0:      mov w10, #0x7d7b            ; =32123
1008b53d4:      strh    w10, [x9, x1]
1008b53d8:      ldr x9, [x8, #0x10]
1008b53dc:      add x9, x9, #0x2
1008b53e0:      b   0x1008b5404 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x884>
1008b53e4:      ldr x20, [x8, #0x10]
1008b53e8:      ldr x9, [x8]
1008b53ec:      cmp x9, x20
1008b53f0:      b.eq    0x1008b5430 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x8b0>
1008b53f4:      ldr x9, [x8, #0x8]
1008b53f8:      mov w10, #0x7d              ; =125
1008b53fc:      strb    w10, [x9, x20]
1008b5400:      add x9, x20, #0x1
1008b5404:      str x9, [x8, #0x10]
1008b5408:      mov w0, #0x1                ; =1
1008b540c:      b   0x1008b4cd0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x150>
1008b5410:      mov x0, x8
1008b5414:      mov w2, #0x2                ; =2
1008b5418:      mov w3, #0x1                ; =1
1008b541c:      mov w4, #0x1                ; =1
1008b5420:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008b5424:      mov x8, x19
1008b5428:      ldr x1, [x19, #0x10]
1008b542c:      b   0x1008b53cc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x84c>
1008b5430:      mov x0, x8
1008b5434:      mov x1, x20
1008b5438:      mov w2, #0x1                ; =1
1008b543c:      mov w3, #0x1                ; =1
1008b5440:      mov w4, #0x1                ; =1
1008b5444:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008b5448:      mov x8, x19
1008b544c:      b   0x1008b53f4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record+0x874>
1008b5450:      adrp    x2, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008b5454:      add x2, x2, #0xcb0
1008b5458:      mov x0, x1
1008b545c:      mov w1, #0x40               ; =64
1008b5460:      bl  0x100c9dfcc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1008b5464:      adrp    x2, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008b5468:      add x2, x2, #0xc98
1008b546c:      mov w0, #0x21               ; =33
1008b5470:      mov w1, #0x21               ; =33
1008b5474:      bl  0x100c9dfcc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1008b5478:      adrp    x2, 0x1010e1000 <_anon.3c709ec65efe22d27798c2815252f2a2.1960+0x7d0>
1008b547c:      add x2, x2, #0x1c0
1008b5480:      mov x0, x21
1008b5484:      mov w1, #0xb                ; =11
1008b5488:      bl  0x100c9dfcc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1008b548c:      adrp    x2, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008b5490:      add x2, x2, #0xcf8
1008b5494:      mov w0, #0x5                ; =5
1008b5498:      mov w1, #0x5                ; =5
1008b549c:      bl  0x100c9dfcc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
