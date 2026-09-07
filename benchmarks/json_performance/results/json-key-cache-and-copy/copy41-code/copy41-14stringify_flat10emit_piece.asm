/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/copy41-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004bcaa8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>:
1004bcaa8:      sub sp, sp, #0x40
1004bcaac:      stp x20, x19, [sp, #0x20]
1004bcab0:      stp x29, x30, [sp, #0x30]
1004bcab4:      add x29, sp, #0x30
1004bcab8:      ldr w8, [x0]
1004bcabc:      cbz w8, 0x1004bcb5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0xb4>
1004bcac0:      cmp w8, #0x1
1004bcac4:      b.ne    0x1004bcc04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x15c>
1004bcac8:      ldr w8, [x0, #0x4]
1004bcacc:      strb    wzr, [sp, #0x4]
1004bcad0:      str wzr, [sp]
1004bcad4:      and x9, x1, #0xffff000000000000
1004bcad8:      mov x10, #0x7fff000000000000 ; =9223090561878065152
1004bcadc:      cmp x9, x10
1004bcae0:      b.eq    0x1004bcc64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1bc>
1004bcae4:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
1004bcae8:      cmp x9, x10
1004bcaec:      b.ne    0x1004bceb0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x408>
1004bcaf0:      ubfx    x9, x1, #40, #8
1004bcaf4:      cbz x9, 0x1004bcb44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x9c>
1004bcaf8:      strb    w1, [sp]
1004bcafc:      cmp x9, #0x1
1004bcb00:      b.eq    0x1004bcb44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x9c>
1004bcb04:      lsr x10, x1, #8
1004bcb08:      strb    w10, [sp, #0x1]
1004bcb0c:      cmp x9, #0x2
1004bcb10:      b.eq    0x1004bcb44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x9c>
1004bcb14:      lsr x10, x1, #16
1004bcb18:      strb    w10, [sp, #0x2]
1004bcb1c:      cmp x9, #0x3
1004bcb20:      b.eq    0x1004bcb44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x9c>
1004bcb24:      lsr x10, x1, #24
1004bcb28:      strb    w10, [sp, #0x3]
1004bcb2c:      cmp x9, #0x4
1004bcb30:      b.eq    0x1004bcb44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x9c>
1004bcb34:      lsr x10, x1, #32
1004bcb38:      strb    w10, [sp, #0x4]
1004bcb3c:      cmp x9, #0x5
1004bcb40:      b.ne    0x1004bcee0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x438>
1004bcb44:      mov x9, sp
1004bcb48:      mov w10, #0x22              ; =34
1004bcb4c:      strb    w10, [x2]
1004bcb50:      mov w11, #0x1               ; =1
1004bcb54:      cbnz    w8, 0x1004bcc80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1d8>
1004bcb58:      b   0x1004bcd70 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x2c8>
1004bcb5c:      ldr w19, [x0, #0x4]
1004bcb60:      strb    wzr, [sp, #0x4]
1004bcb64:      str wzr, [sp]
1004bcb68:      and x8, x1, #0xffff000000000000
1004bcb6c:      mov x9, #0x7fff000000000000 ; =9223090561878065152
1004bcb70:      cmp x8, x9
1004bcb74:      b.eq    0x1004bcd7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x2d4>
1004bcb78:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
1004bcb7c:      cmp x8, x9
1004bcb80:      b.ne    0x1004bcec8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x420>
1004bcb84:      ubfx    x8, x1, #40, #8
1004bcb88:      cbz x8, 0x1004bcbd8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x130>
1004bcb8c:      strb    w1, [sp]
1004bcb90:      cmp x8, #0x1
1004bcb94:      b.eq    0x1004bcbd8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x130>
1004bcb98:      lsr x9, x1, #8
1004bcb9c:      strb    w9, [sp, #0x1]
1004bcba0:      cmp x8, #0x2
1004bcba4:      b.eq    0x1004bcbd8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x130>
1004bcba8:      lsr x9, x1, #16
1004bcbac:      strb    w9, [sp, #0x2]
1004bcbb0:      cmp x8, #0x3
1004bcbb4:      b.eq    0x1004bcbd8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x130>
1004bcbb8:      lsr x9, x1, #24
1004bcbbc:      strb    w9, [sp, #0x3]
1004bcbc0:      cmp x8, #0x4
1004bcbc4:      b.eq    0x1004bcbd8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x130>
1004bcbc8:      lsr x9, x1, #32
1004bcbcc:      strb    w9, [sp, #0x4]
1004bcbd0:      cmp x8, #0x5
1004bcbd4:      b.ne    0x1004bcee0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x438>
1004bcbd8:      mov x1, sp
1004bcbdc:      mov w8, #0x22               ; =34
1004bcbe0:      mov x0, x2
1004bcbe4:      strb    w8, [x0], #0x1
1004bcbe8:      cmp w19, #0x21
1004bcbec:      b.lo    0x1004bcd9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x2f4>
1004bcbf0:      mov x20, x2
1004bcbf4:      mov x2, x19
1004bcbf8:      bl  0x100af92ac <_writev+0x100af92ac>
1004bcbfc:      mov x2, x20
1004bcc00:      b   0x1004bce90 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3e8>
1004bcc04:      ldur    q0, [x0, #0x8]
1004bcc08:      ldur    q1, [x0, #0x18]
1004bcc0c:      stp q0, q1, [sp]
1004bcc10:      ldr w0, [x0, #0x4]
1004bcc14:      cmp w0, #0xf
1004bcc18:      b.ls    0x1004bcc40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x198>
1004bcc1c:      ldp x8, x9, [sp]
1004bcc20:      sub x10, x0, #0x10
1004bcc24:      mov x11, sp
1004bcc28:      add x11, x11, x10
1004bcc2c:      ldp x12, x11, [x11]
1004bcc30:      stp x8, x9, [x2]
1004bcc34:      add x8, x2, x10
1004bcc38:      stp x12, x11, [x8]
1004bcc3c:      b   0x1004bcea0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3f8>
1004bcc40:      cmp w0, #0x7
1004bcc44:      b.ls    0x1004bcdc4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x31c>
1004bcc48:      ldr x8, [sp]
1004bcc4c:      sub x9, x0, #0x8
1004bcc50:      mov x10, sp
1004bcc54:      ldr x10, [x10, x9]
1004bcc58:      str x8, [x2]
1004bcc5c:      str x10, [x2, x9]
1004bcc60:      b   0x1004bcea0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3f8>
1004bcc64:      ands    x9, x1, #0xffffffffffff
1004bcc68:      add x9, x9, #0x14
1004bcc6c:      csel    x9, xzr, x9, eq
1004bcc70:      mov w10, #0x22              ; =34
1004bcc74:      strb    w10, [x2]
1004bcc78:      mov w11, #0x1               ; =1
1004bcc7c:      cbz w8, 0x1004bcd70 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x2c8>
1004bcc80:      mov w12, #0x5c              ; =92
1004bcc84:      mov w13, #0x3075            ; =12405
1004bcc88:      mov w14, #0x30              ; =48
1004bcc8c:      adrp    x15, 0x100c2b000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x5768>
1004bcc90:      add x15, x15, #0x0
1004bcc94:      b   0x1004bccb0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x208>
1004bcc98:      mov w16, #0x62              ; =98
1004bcc9c:      strb    w16, [x17, #0x1]
1004bcca0:      mov w16, #0x2               ; =2
1004bcca4:      add x11, x16, x11
1004bcca8:      subs    x8, x8, #0x1
1004bccac:      b.eq    0x1004bcd70 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x2c8>
1004bccb0:      ldrb    w16, [x9], #0x1
1004bccb4:      cmp w16, #0x20
1004bccb8:      b.lo    0x1004bcccc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x224>
1004bccbc:      cmp w16, #0x5c
1004bccc0:      b.eq    0x1004bcccc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x224>
1004bccc4:      cmp w16, #0x22
1004bccc8:      b.ne    0x1004bcd54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x2ac>
1004bcccc:      add x17, x2, x11
1004bccd0:      strb    w12, [x17]
1004bccd4:      cmp w16, #0xb
1004bccd8:      b.le    0x1004bccfc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x254>
1004bccdc:      cmp w16, #0x21
1004bcce0:      b.gt    0x1004bcd1c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x274>
1004bcce4:      cmp w16, #0xc
1004bcce8:      b.eq    0x1004bcd60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x2b8>
1004bccec:      cmp w16, #0xd
1004bccf0:      b.ne    0x1004bcd2c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x284>
1004bccf4:      mov w16, #0x72              ; =114
1004bccf8:      b   0x1004bcc9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1f4>
1004bccfc:      cmp w16, #0x8
1004bcd00:      b.eq    0x1004bcc98 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1f0>
1004bcd04:      cmp w16, #0x9
1004bcd08:      b.eq    0x1004bcd68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x2c0>
1004bcd0c:      cmp w16, #0xa
1004bcd10:      b.ne    0x1004bcd2c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x284>
1004bcd14:      mov w16, #0x6e              ; =110
1004bcd18:      b   0x1004bcc9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1f4>
1004bcd1c:      cmp w16, #0x22
1004bcd20:      b.eq    0x1004bcc9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1f4>
1004bcd24:      cmp w16, #0x5c
1004bcd28:      b.eq    0x1004bcc9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1f4>
1004bcd2c:      sturh   w13, [x17, #0x1]
1004bcd30:      strb    w14, [x17, #0x3]
1004bcd34:      lsr x0, x16, #4
1004bcd38:      ldrb    w0, [x15, x0]
1004bcd3c:      strb    w0, [x17, #0x4]
1004bcd40:      and x16, x16, #0xf
1004bcd44:      ldrb    w16, [x15, x16]
1004bcd48:      strb    w16, [x17, #0x5]
1004bcd4c:      mov w16, #0x6               ; =6
1004bcd50:      b   0x1004bcca4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1fc>
1004bcd54:      strb    w16, [x2, x11]
1004bcd58:      mov w16, #0x1               ; =1
1004bcd5c:      b   0x1004bcca4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1fc>
1004bcd60:      mov w16, #0x66              ; =102
1004bcd64:      b   0x1004bcc9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1f4>
1004bcd68:      mov w16, #0x74              ; =116
1004bcd6c:      b   0x1004bcc9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x1f4>
1004bcd70:      strb    w10, [x2, x11]
1004bcd74:      add x0, x11, #0x1
1004bcd78:      b   0x1004bcea0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3f8>
1004bcd7c:      ands    x8, x1, #0xffffffffffff
1004bcd80:      add x8, x8, #0x14
1004bcd84:      csel    x1, xzr, x8, eq
1004bcd88:      mov w8, #0x22               ; =34
1004bcd8c:      mov x0, x2
1004bcd90:      strb    w8, [x0], #0x1
1004bcd94:      cmp w19, #0x21
1004bcd98:      b.hs    0x1004bcbf0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x148>
1004bcd9c:      cmp w19, #0xf
1004bcda0:      b.ls    0x1004bcde8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x340>
1004bcda4:      ldp x8, x9, [x1]
1004bcda8:      sub x10, x19, #0x10
1004bcdac:      add x11, x1, x10
1004bcdb0:      ldp x12, x11, [x11]
1004bcdb4:      stp x8, x9, [x0]
1004bcdb8:      add x8, x0, x10
1004bcdbc:      stp x12, x11, [x8]
1004bcdc0:      b   0x1004bce90 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3e8>
1004bcdc4:      cmp w0, #0x3
1004bcdc8:      b.ls    0x1004bce08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x360>
1004bcdcc:      ldr w8, [sp]
1004bcdd0:      sub x9, x0, #0x4
1004bcdd4:      mov x10, sp
1004bcdd8:      ldr w10, [x10, x9]
1004bcddc:      str w8, [x2]
1004bcde0:      str w10, [x2, x9]
1004bcde4:      b   0x1004bcea0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3f8>
1004bcde8:      cmp w19, #0x7
1004bcdec:      b.ls    0x1004bce2c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x384>
1004bcdf0:      ldr x8, [x1]
1004bcdf4:      sub x9, x19, #0x8
1004bcdf8:      ldr x10, [x1, x9]
1004bcdfc:      str x8, [x0]
1004bce00:      str x10, [x0, x9]
1004bce04:      b   0x1004bce90 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3e8>
1004bce08:      cmp w0, #0x1
1004bce0c:      b.ls    0x1004bce4c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3a4>
1004bce10:      ldrh    w8, [sp]
1004bce14:      sub x9, x0, #0x2
1004bce18:      mov x10, sp
1004bce1c:      ldrh    w10, [x10, x9]
1004bce20:      strh    w8, [x2]
1004bce24:      strh    w10, [x2, x9]
1004bce28:      b   0x1004bcea0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3f8>
1004bce2c:      cmp w19, #0x3
1004bce30:      b.ls    0x1004bce60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3b8>
1004bce34:      ldr w8, [x1]
1004bce38:      sub x9, x19, #0x4
1004bce3c:      ldr w10, [x1, x9]
1004bce40:      str w8, [x0]
1004bce44:      str w10, [x0, x9]
1004bce48:      b   0x1004bce90 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3e8>
1004bce4c:      cmp w0, #0x1
1004bce50:      b.ne    0x1004bcea0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3f8>
1004bce54:      ldrb    w8, [sp]
1004bce58:      strb    w8, [x2]
1004bce5c:      b   0x1004bcea0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3f8>
1004bce60:      cmp w19, #0x1
1004bce64:      b.ls    0x1004bce80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3d8>
1004bce68:      ldrh    w8, [x1]
1004bce6c:      sub x9, x19, #0x2
1004bce70:      ldrh    w10, [x1, x9]
1004bce74:      strh    w8, [x0]
1004bce78:      strh    w10, [x0, x9]
1004bce7c:      b   0x1004bce90 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3e8>
1004bce80:      cmp w19, #0x1
1004bce84:      b.ne    0x1004bce90 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece+0x3e8>
1004bce88:      ldrb    w8, [x1]
1004bce8c:      strb    w8, [x0]
1004bce90:      add x8, x2, x19
1004bce94:      mov w9, #0x22               ; =34
1004bce98:      strb    w9, [x8, #0x1]
1004bce9c:      add x0, x19, #0x2
1004bcea0:      ldp x29, x30, [sp, #0x30]
1004bcea4:      ldp x20, x19, [sp, #0x20]
1004bcea8:      add sp, sp, #0x40
1004bceac:      ret
1004bceb0:      adrp    x0, 0x100c0b000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime10bun_compat12width_tables8EAW_WIDE+0x69a0>
1004bceb4:      add x0, x0, #0x22c
1004bceb8:      adrp    x2, 0x100e8d000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x1ec0>
1004bcebc:      add x2, x2, #0xf58
1004bcec0:      mov w1, #0x20               ; =32
1004bcec4:      bl  0x100ab16c0 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
1004bcec8:      adrp    x0, 0x100c0b000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime10bun_compat12width_tables8EAW_WIDE+0x69a0>
1004bcecc:      add x0, x0, #0x214
1004bced0:      adrp    x2, 0x100e8d000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x1ec0>
1004bced4:      add x2, x2, #0xf40
1004bced8:      mov w1, #0x18               ; =24
1004bcedc:      bl  0x100ab16c0 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
1004bcee0:      adrp    x2, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004bcee4:      add x2, x2, #0xe58
1004bcee8:      mov w0, #0x5                ; =5
1004bceec:      mov w1, #0x5                ; =5
1004bcef0:      bl  0x100ab178c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1004bcef4:      nop
1004bcef8:      nop
1004bcefc:      nop
