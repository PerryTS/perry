
/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/empty-parse-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010025f9a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>:
10025f9a8:      sub sp, sp, #0x50
10025f9ac:      stp x22, x21, [sp, #0x20]
10025f9b0:      stp x20, x19, [sp, #0x30]
10025f9b4:      stp x29, x30, [sp, #0x40]
10025f9b8:      add x29, sp, #0x40
10025f9bc:      ldr w8, [x0]
10025f9c0:      cbz w8, 0x10025f9e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x40>
10025f9c4:      ldur    q0, [x0, #0x8]
10025f9c8:      ldur    q1, [x0, #0x18]
10025f9cc:      stp q0, q1, [sp]
10025f9d0:      ldr w19, [x0, #0x4]
10025f9d4:      mov x1, sp
10025f9d8:      mov x0, x2
10025f9dc:      mov x2, x19
10025f9e0:      bl  0x100cdb3ac <_writev+0x100cdb3ac>
10025f9e4:      b   0x10025fa9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xf4>
10025f9e8:      ldr w19, [x0, #0x4]
10025f9ec:      strb    wzr, [sp, #0x4]
10025f9f0:      str wzr, [sp]
10025f9f4:      and x8, x1, #0xffff000000000000
10025f9f8:      mov x9, #0x7fff000000000000 ; =9223090561878065152
10025f9fc:      cmp x8, x9
10025fa00:      b.eq    0x10025fa6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xc4>
10025fa04:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
10025fa08:      cmp x8, x9
10025fa0c:      b.ne    0x10025fab4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x10c>
10025fa10:      ubfx    x8, x1, #40, #8
10025fa14:      cbz x8, 0x10025fa64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xbc>
10025fa18:      strb    w1, [sp]
10025fa1c:      cmp x8, #0x1
10025fa20:      b.eq    0x10025fa64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xbc>
10025fa24:      lsr x9, x1, #8
10025fa28:      strb    w9, [sp, #0x1]
10025fa2c:      cmp x8, #0x2
10025fa30:      b.eq    0x10025fa64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xbc>
10025fa34:      lsr x9, x1, #16
10025fa38:      strb    w9, [sp, #0x2]
10025fa3c:      cmp x8, #0x3
10025fa40:      b.eq    0x10025fa64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xbc>
10025fa44:      lsr x9, x1, #24
10025fa48:      strb    w9, [sp, #0x3]
10025fa4c:      cmp x8, #0x4
10025fa50:      b.eq    0x10025fa64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xbc>
10025fa54:      lsr x9, x1, #32
10025fa58:      strb    w9, [sp, #0x4]
10025fa5c:      cmp x8, #0x5
10025fa60:      b.ne    0x10025facc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x124>
10025fa64:      mov x1, sp
10025fa68:      b   0x10025fa78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xd0>
10025fa6c:      ands    x8, x1, #0xffffffffffff
10025fa70:      add x8, x8, #0x14
10025fa74:      csel    x1, xzr, x8, eq
10025fa78:      mov w20, #0x22              ; =34
10025fa7c:      mov x0, x2
10025fa80:      strb    w20, [x0], #0x1
10025fa84:      mov x21, x2
10025fa88:      mov x2, x19
10025fa8c:      bl  0x100cdb3ac <_writev+0x100cdb3ac>
10025fa90:      add x8, x21, x19
10025fa94:      strb    w20, [x8, #0x1]
10025fa98:      add x19, x19, #0x2
10025fa9c:      mov x0, x19
10025faa0:      ldp x29, x30, [sp, #0x40]
10025faa4:      ldp x20, x19, [sp, #0x30]
10025faa8:      ldp x22, x21, [sp, #0x20]
10025faac:      add sp, sp, #0x50
10025fab0:      ret
10025fab4:      adrp    x0, 0x100dbe000 <_anon.c5eda94442f690a6096fdfc2a543495d.715+0x1b0>
10025fab8:      add x0, x0, #0x2e9
10025fabc:      adrp    x2, 0x10109c000 <_anon.c5eda94442f690a6096fdfc2a543495d.175+0x2d8>
10025fac0:      add x2, x2, #0x2f0
10025fac4:      mov w1, #0x18               ; =24
10025fac8:      bl  0x100c8c240 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
10025facc:      adrp    x2, 0x10109b000 <_anon.7e5d8b44b4d44cb11aa03af0ef44b42e.937+0x48>
10025fad0:      add x2, x2, #0xd10
10025fad4:      mov w0, #0x5                ; =5
10025fad8:      mov w1, #0x5                ; =5
10025fadc:      bl  0x100c8c30c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
