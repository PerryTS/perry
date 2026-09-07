/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/wide39-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004bcaa8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>:
1004bcaa8:      sub sp, sp, #0x50
1004bcaac:      stp x22, x21, [sp, #0x20]
1004bcab0:      stp x20, x19, [sp, #0x30]
1004bcab4:      stp x29, x30, [sp, #0x40]
1004bcab8:      add x29, sp, #0x40
1004bcabc:      ldr w8, [x0]
1004bcac0:      cbz w8, 0x1004bcb60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0xb8>
1004bcac4:      cmp w8, #0x1
1004bcac8:      b.ne    0x1004bcbe4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x13c>
1004bcacc:      ldr w8, [x0, #0x4]
1004bcad0:      strb    wzr, [sp, #0x4]
1004bcad4:      str wzr, [sp]
1004bcad8:      and x9, x1, #0xffff000000000000
1004bcadc:      mov x10, #0x7fff000000000000 ; =9223090561878065152
1004bcae0:      cmp x9, x10
1004bcae4:      b.eq    0x1004bcc0c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x164>
1004bcae8:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
1004bcaec:      cmp x9, x10
1004bcaf0:      b.ne    0x1004bcd68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x2c0>
1004bcaf4:      ubfx    x9, x1, #40, #8
1004bcaf8:      cbz x9, 0x1004bcb48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1004bcafc:      strb    w1, [sp]
1004bcb00:      cmp x9, #0x1
1004bcb04:      b.eq    0x1004bcb48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1004bcb08:      lsr x10, x1, #8
1004bcb0c:      strb    w10, [sp, #0x1]
1004bcb10:      cmp x9, #0x2
1004bcb14:      b.eq    0x1004bcb48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1004bcb18:      lsr x10, x1, #16
1004bcb1c:      strb    w10, [sp, #0x2]
1004bcb20:      cmp x9, #0x3
1004bcb24:      b.eq    0x1004bcb48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1004bcb28:      lsr x10, x1, #24
1004bcb2c:      strb    w10, [sp, #0x3]
1004bcb30:      cmp x9, #0x4
1004bcb34:      b.eq    0x1004bcb48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1004bcb38:      lsr x10, x1, #32
1004bcb3c:      strb    w10, [sp, #0x4]
1004bcb40:      cmp x9, #0x5
1004bcb44:      b.ne    0x1004bcd98 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x2f0>
1004bcb48:      mov x9, sp
1004bcb4c:      mov w10, #0x22              ; =34
1004bcb50:      strb    w10, [x2]
1004bcb54:      mov w11, #0x1               ; =1
1004bcb58:      cbnz    w8, 0x1004bcc28 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x180>
1004bcb5c:      b   0x1004bcd18 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x270>
1004bcb60:      ldr w19, [x0, #0x4]
1004bcb64:      strb    wzr, [sp, #0x4]
1004bcb68:      str wzr, [sp]
1004bcb6c:      and x8, x1, #0xffff000000000000
1004bcb70:      mov x9, #0x7fff000000000000 ; =9223090561878065152
1004bcb74:      cmp x8, x9
1004bcb78:      b.eq    0x1004bcd24 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x27c>
1004bcb7c:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
1004bcb80:      cmp x8, x9
1004bcb84:      b.ne    0x1004bcd80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x2d8>
1004bcb88:      ubfx    x8, x1, #40, #8
1004bcb8c:      cbz x8, 0x1004bcbdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x134>
1004bcb90:      strb    w1, [sp]
1004bcb94:      cmp x8, #0x1
1004bcb98:      b.eq    0x1004bcbdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x134>
1004bcb9c:      lsr x9, x1, #8
1004bcba0:      strb    w9, [sp, #0x1]
1004bcba4:      cmp x8, #0x2
1004bcba8:      b.eq    0x1004bcbdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x134>
1004bcbac:      lsr x9, x1, #16
1004bcbb0:      strb    w9, [sp, #0x2]
1004bcbb4:      cmp x8, #0x3
1004bcbb8:      b.eq    0x1004bcbdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x134>
1004bcbbc:      lsr x9, x1, #24
1004bcbc0:      strb    w9, [sp, #0x3]
1004bcbc4:      cmp x8, #0x4
1004bcbc8:      b.eq    0x1004bcbdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x134>
1004bcbcc:      lsr x9, x1, #32
1004bcbd0:      strb    w9, [sp, #0x4]
1004bcbd4:      cmp x8, #0x5
1004bcbd8:      b.ne    0x1004bcd98 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x2f0>
1004bcbdc:      mov x1, sp
1004bcbe0:      b   0x1004bcd30 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x288>
1004bcbe4:      ldur    q0, [x0, #0x8]
1004bcbe8:      ldur    q1, [x0, #0x18]
1004bcbec:      stp q0, q1, [sp]
1004bcbf0:      ldr w19, [x0, #0x4]
1004bcbf4:      mov x1, sp
1004bcbf8:      mov x0, x2
1004bcbfc:      mov x2, x19
1004bcc00:      bl  0x100af916c <_writev+0x100af916c>
1004bcc04:      mov x0, x19
1004bcc08:      b   0x1004bcd54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x2ac>
1004bcc0c:      ands    x9, x1, #0xffffffffffff
1004bcc10:      add x9, x9, #0x14
1004bcc14:      csel    x9, xzr, x9, eq
1004bcc18:      mov w10, #0x22              ; =34
1004bcc1c:      strb    w10, [x2]
1004bcc20:      mov w11, #0x1               ; =1
1004bcc24:      cbz w8, 0x1004bcd18 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x270>
1004bcc28:      mov w12, #0x5c              ; =92
1004bcc2c:      mov w13, #0x3075            ; =12405
1004bcc30:      mov w14, #0x30              ; =48
1004bcc34:      adrp    x15, 0x100c2a000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x48a8>
1004bcc38:      add x15, x15, #0xec0
1004bcc3c:      b   0x1004bcc58 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1b0>
1004bcc40:      mov w16, #0x62              ; =98
1004bcc44:      strb    w16, [x17, #0x1]
1004bcc48:      mov w16, #0x2               ; =2
1004bcc4c:      add x11, x16, x11
1004bcc50:      subs    x8, x8, #0x1
1004bcc54:      b.eq    0x1004bcd18 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x270>
1004bcc58:      ldrb    w16, [x9], #0x1
1004bcc5c:      cmp w16, #0x20
1004bcc60:      b.lo    0x1004bcc74 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1cc>
1004bcc64:      cmp w16, #0x5c
1004bcc68:      b.eq    0x1004bcc74 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1cc>
1004bcc6c:      cmp w16, #0x22
1004bcc70:      b.ne    0x1004bccfc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x254>
1004bcc74:      add x17, x2, x11
1004bcc78:      strb    w12, [x17]
1004bcc7c:      cmp w16, #0xb
1004bcc80:      b.le    0x1004bcca4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1fc>
1004bcc84:      cmp w16, #0x21
1004bcc88:      b.gt    0x1004bccc4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x21c>
1004bcc8c:      cmp w16, #0xc
1004bcc90:      b.eq    0x1004bcd08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x260>
1004bcc94:      cmp w16, #0xd
1004bcc98:      b.ne    0x1004bccd4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x22c>
1004bcc9c:      mov w16, #0x72              ; =114
1004bcca0:      b   0x1004bcc44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1004bcca4:      cmp w16, #0x8
1004bcca8:      b.eq    0x1004bcc40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x198>
1004bccac:      cmp w16, #0x9
1004bccb0:      b.eq    0x1004bcd10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x268>
1004bccb4:      cmp w16, #0xa
1004bccb8:      b.ne    0x1004bccd4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x22c>
1004bccbc:      mov w16, #0x6e              ; =110
1004bccc0:      b   0x1004bcc44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1004bccc4:      cmp w16, #0x22
1004bccc8:      b.eq    0x1004bcc44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1004bcccc:      cmp w16, #0x5c
1004bccd0:      b.eq    0x1004bcc44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1004bccd4:      sturh   w13, [x17, #0x1]
1004bccd8:      strb    w14, [x17, #0x3]
1004bccdc:      lsr x0, x16, #4
1004bcce0:      ldrb    w0, [x15, x0]
1004bcce4:      strb    w0, [x17, #0x4]
1004bcce8:      and x16, x16, #0xf
1004bccec:      ldrb    w16, [x15, x16]
1004bccf0:      strb    w16, [x17, #0x5]
1004bccf4:      mov w16, #0x6               ; =6
1004bccf8:      b   0x1004bcc4c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1a4>
1004bccfc:      strb    w16, [x2, x11]
1004bcd00:      mov w16, #0x1               ; =1
1004bcd04:      b   0x1004bcc4c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1a4>
1004bcd08:      mov w16, #0x66              ; =102
1004bcd0c:      b   0x1004bcc44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1004bcd10:      mov w16, #0x74              ; =116
1004bcd14:      b   0x1004bcc44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1004bcd18:      strb    w10, [x2, x11]
1004bcd1c:      add x0, x11, #0x1
1004bcd20:      b   0x1004bcd54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x2ac>
1004bcd24:      ands    x8, x1, #0xffffffffffff
1004bcd28:      add x8, x8, #0x14
1004bcd2c:      csel    x1, xzr, x8, eq
1004bcd30:      mov w20, #0x22              ; =34
1004bcd34:      mov x0, x2
1004bcd38:      strb    w20, [x0], #0x1
1004bcd3c:      mov x21, x2
1004bcd40:      mov x2, x19
1004bcd44:      bl  0x100af916c <_writev+0x100af916c>
1004bcd48:      add x8, x21, x19
1004bcd4c:      strb    w20, [x8, #0x1]
1004bcd50:      add x0, x19, #0x2
1004bcd54:      ldp x29, x30, [sp, #0x40]
1004bcd58:      ldp x20, x19, [sp, #0x30]
1004bcd5c:      ldp x22, x21, [sp, #0x20]
1004bcd60:      add sp, sp, #0x50
1004bcd64:      ret
1004bcd68:      adrp    x0, 0x100c0b000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime10bun_compat12width_tables8EAW_WIDE+0x6ae0>
1004bcd6c:      add x0, x0, #0xec
1004bcd70:      adrp    x2, 0x100e8d000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x1ec0>
1004bcd74:      add x2, x2, #0xf58
1004bcd78:      mov w1, #0x20               ; =32
1004bcd7c:      bl  0x100ab1580 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
1004bcd80:      adrp    x0, 0x100c0b000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime10bun_compat12width_tables8EAW_WIDE+0x6ae0>
1004bcd84:      add x0, x0, #0xd4
1004bcd88:      adrp    x2, 0x100e8d000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x1ec0>
1004bcd8c:      add x2, x2, #0xf40
1004bcd90:      mov w1, #0x18               ; =24
1004bcd94:      bl  0x100ab1580 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
1004bcd98:      adrp    x2, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004bcd9c:      add x2, x2, #0xe58
1004bcda0:      mov w0, #0x5                ; =5
1004bcda4:      mov w1, #0x5                ; =5
1004bcda8:      bl  0x100ab164c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1004bcdac:      nop
1004bcdb0:      nop
1004bcdb4:      nop
1004bcdb8:      nop
1004bcdbc:      nop
