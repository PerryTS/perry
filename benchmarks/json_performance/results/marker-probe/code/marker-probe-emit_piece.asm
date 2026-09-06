/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/marker-probe-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010074efe4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>:
10074efe4:      sub sp, sp, #0x50
10074efe8:      stp x22, x21, [sp, #0x20]
10074efec:      stp x20, x19, [sp, #0x30]
10074eff0:      stp x29, x30, [sp, #0x40]
10074eff4:      add x29, sp, #0x40
10074eff8:      ldr w8, [x0]
10074effc:      cbz w8, 0x10074f09c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xb8>
10074f000:      cmp w8, #0x1
10074f004:      b.ne    0x10074f120 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x13c>
10074f008:      ldr w8, [x0, #0x4]
10074f00c:      strb    wzr, [sp, #0x4]
10074f010:      str wzr, [sp]
10074f014:      and x9, x1, #0xffff000000000000
10074f018:      mov x10, #0x7fff000000000000 ; =9223090561878065152
10074f01c:      cmp x9, x10
10074f020:      b.eq    0x10074f148 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x164>
10074f024:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
10074f028:      cmp x9, x10
10074f02c:      b.ne    0x10074f2a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2c0>
10074f030:      ubfx    x9, x1, #40, #8
10074f034:      cbz x9, 0x10074f084 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
10074f038:      strb    w1, [sp]
10074f03c:      cmp x9, #0x1
10074f040:      b.eq    0x10074f084 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
10074f044:      lsr x10, x1, #8
10074f048:      strb    w10, [sp, #0x1]
10074f04c:      cmp x9, #0x2
10074f050:      b.eq    0x10074f084 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
10074f054:      lsr x10, x1, #16
10074f058:      strb    w10, [sp, #0x2]
10074f05c:      cmp x9, #0x3
10074f060:      b.eq    0x10074f084 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
10074f064:      lsr x10, x1, #24
10074f068:      strb    w10, [sp, #0x3]
10074f06c:      cmp x9, #0x4
10074f070:      b.eq    0x10074f084 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
10074f074:      lsr x10, x1, #32
10074f078:      strb    w10, [sp, #0x4]
10074f07c:      cmp x9, #0x5
10074f080:      b.ne    0x10074f2d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2f0>
10074f084:      mov x9, sp
10074f088:      mov w10, #0x22              ; =34
10074f08c:      strb    w10, [x2]
10074f090:      mov w11, #0x1               ; =1
10074f094:      cbnz    w8, 0x10074f164 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x180>
10074f098:      b   0x10074f254 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
10074f09c:      ldr w19, [x0, #0x4]
10074f0a0:      strb    wzr, [sp, #0x4]
10074f0a4:      str wzr, [sp]
10074f0a8:      and x8, x1, #0xffff000000000000
10074f0ac:      mov x9, #0x7fff000000000000 ; =9223090561878065152
10074f0b0:      cmp x8, x9
10074f0b4:      b.eq    0x10074f260 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x27c>
10074f0b8:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
10074f0bc:      cmp x8, x9
10074f0c0:      b.ne    0x10074f2bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2d8>
10074f0c4:      ubfx    x8, x1, #40, #8
10074f0c8:      cbz x8, 0x10074f118 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
10074f0cc:      strb    w1, [sp]
10074f0d0:      cmp x8, #0x1
10074f0d4:      b.eq    0x10074f118 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
10074f0d8:      lsr x9, x1, #8
10074f0dc:      strb    w9, [sp, #0x1]
10074f0e0:      cmp x8, #0x2
10074f0e4:      b.eq    0x10074f118 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
10074f0e8:      lsr x9, x1, #16
10074f0ec:      strb    w9, [sp, #0x2]
10074f0f0:      cmp x8, #0x3
10074f0f4:      b.eq    0x10074f118 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
10074f0f8:      lsr x9, x1, #24
10074f0fc:      strb    w9, [sp, #0x3]
10074f100:      cmp x8, #0x4
10074f104:      b.eq    0x10074f118 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
10074f108:      lsr x9, x1, #32
10074f10c:      strb    w9, [sp, #0x4]
10074f110:      cmp x8, #0x5
10074f114:      b.ne    0x10074f2d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2f0>
10074f118:      mov x1, sp
10074f11c:      b   0x10074f26c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x288>
10074f120:      ldur    q0, [x0, #0x8]
10074f124:      ldur    q1, [x0, #0x18]
10074f128:      stp q0, q1, [sp]
10074f12c:      ldr w19, [x0, #0x4]
10074f130:      mov x1, sp
10074f134:      mov x0, x2
10074f138:      mov x2, x19
10074f13c:      bl  0x100cdd7ec <_writev+0x100cdd7ec>
10074f140:      mov x0, x19
10074f144:      b   0x10074f290 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2ac>
10074f148:      ands    x9, x1, #0xffffffffffff
10074f14c:      add x9, x9, #0x14
10074f150:      csel    x9, xzr, x9, eq
10074f154:      mov w10, #0x22              ; =34
10074f158:      strb    w10, [x2]
10074f15c:      mov w11, #0x1               ; =1
10074f160:      cbz w8, 0x10074f254 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
10074f164:      mov w12, #0x5c              ; =92
10074f168:      mov w13, #0x3075            ; =12405
10074f16c:      mov w14, #0x30              ; =48
10074f170:      adrp    x15, 0x100dfd000 <_anon.4ff118d01ccdc9bd41517af7abf33093.1077+0xe2>
10074f174:      add x15, x15, #0xba1
10074f178:      b   0x10074f194 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1b0>
10074f17c:      mov w16, #0x62              ; =98
10074f180:      strb    w16, [x17, #0x1]
10074f184:      mov w16, #0x2               ; =2
10074f188:      add x11, x16, x11
10074f18c:      subs    x8, x8, #0x1
10074f190:      b.eq    0x10074f254 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
10074f194:      ldrb    w16, [x9], #0x1
10074f198:      cmp w16, #0x20
10074f19c:      b.lo    0x10074f1b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1cc>
10074f1a0:      cmp w16, #0x5c
10074f1a4:      b.eq    0x10074f1b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1cc>
10074f1a8:      cmp w16, #0x22
10074f1ac:      b.ne    0x10074f238 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x254>
10074f1b0:      add x17, x2, x11
10074f1b4:      strb    w12, [x17]
10074f1b8:      cmp w16, #0xb
10074f1bc:      b.le    0x10074f1e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1fc>
10074f1c0:      cmp w16, #0x21
10074f1c4:      b.gt    0x10074f200 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x21c>
10074f1c8:      cmp w16, #0xc
10074f1cc:      b.eq    0x10074f244 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x260>
10074f1d0:      cmp w16, #0xd
10074f1d4:      b.ne    0x10074f210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x22c>
10074f1d8:      mov w16, #0x72              ; =114
10074f1dc:      b   0x10074f180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
10074f1e0:      cmp w16, #0x8
10074f1e4:      b.eq    0x10074f17c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x198>
10074f1e8:      cmp w16, #0x9
10074f1ec:      b.eq    0x10074f24c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x268>
10074f1f0:      cmp w16, #0xa
10074f1f4:      b.ne    0x10074f210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x22c>
10074f1f8:      mov w16, #0x6e              ; =110
10074f1fc:      b   0x10074f180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
10074f200:      cmp w16, #0x22
10074f204:      b.eq    0x10074f180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
10074f208:      cmp w16, #0x5c
10074f20c:      b.eq    0x10074f180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
10074f210:      sturh   w13, [x17, #0x1]
10074f214:      strb    w14, [x17, #0x3]
10074f218:      lsr x0, x16, #4
10074f21c:      ldrb    w0, [x15, x0]
10074f220:      strb    w0, [x17, #0x4]
10074f224:      and x16, x16, #0xf
10074f228:      ldrb    w16, [x15, x16]
10074f22c:      strb    w16, [x17, #0x5]
10074f230:      mov w16, #0x6               ; =6
10074f234:      b   0x10074f188 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1a4>
10074f238:      strb    w16, [x2, x11]
10074f23c:      mov w16, #0x1               ; =1
10074f240:      b   0x10074f188 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1a4>
10074f244:      mov w16, #0x66              ; =102
10074f248:      b   0x10074f180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
10074f24c:      mov w16, #0x74              ; =116
10074f250:      b   0x10074f180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
10074f254:      strb    w10, [x2, x11]
10074f258:      add x0, x11, #0x1
10074f25c:      b   0x10074f290 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2ac>
10074f260:      ands    x8, x1, #0xffffffffffff
10074f264:      add x8, x8, #0x14
10074f268:      csel    x1, xzr, x8, eq
10074f26c:      mov w20, #0x22              ; =34
10074f270:      mov x0, x2
10074f274:      strb    w20, [x0], #0x1
10074f278:      mov x21, x2
10074f27c:      mov x2, x19
10074f280:      bl  0x100cdd7ec <_writev+0x100cdd7ec>
10074f284:      add x8, x21, x19
10074f288:      strb    w20, [x8, #0x1]
10074f28c:      add x0, x19, #0x2
10074f290:      ldp x29, x30, [sp, #0x40]
10074f294:      ldp x20, x19, [sp, #0x30]
10074f298:      ldp x22, x21, [sp, #0x20]
10074f29c:      add sp, sp, #0x50
10074f2a0:      ret
10074f2a4:      adrp    x0, 0x100e04000 <_anon.fd7e678389f6d6013308189123b84ec8.899+0x2>
10074f2a8:      add x0, x0, #0x38e
10074f2ac:      adrp    x2, 0x1010bf000 <_anon.fd7e678389f6d6013308189123b84ec8.144+0x50>
10074f2b0:      add x2, x2, #0x610
10074f2b4:      mov w1, #0x20               ; =32
10074f2b8:      bl  0x100c8d2c0 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
10074f2bc:      adrp    x0, 0x100e04000 <_anon.fd7e678389f6d6013308189123b84ec8.899+0x2>
10074f2c0:      add x0, x0, #0x376
10074f2c4:      adrp    x2, 0x1010bf000 <_anon.fd7e678389f6d6013308189123b84ec8.144+0x50>
10074f2c8:      add x2, x2, #0x5f8
10074f2cc:      mov w1, #0x18               ; =24
10074f2d0:      bl  0x100c8d2c0 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
10074f2d4:      adrp    x2, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
10074f2d8:      add x2, x2, #0xf10
10074f2dc:      mov w0, #0x5                ; =5
10074f2e0:      mov w1, #0x5                ; =5
10074f2e4:      bl  0x100c8d38c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
