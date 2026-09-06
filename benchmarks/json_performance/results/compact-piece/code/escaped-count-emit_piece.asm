
/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/escaped-count-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010046415c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>:
10046415c:      sub sp, sp, #0x50
100464160:      stp x22, x21, [sp, #0x20]
100464164:      stp x20, x19, [sp, #0x30]
100464168:      stp x29, x30, [sp, #0x40]
10046416c:      add x29, sp, #0x40
100464170:      ldr w8, [x0]
100464174:      cbz w8, 0x100464214 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xb8>
100464178:      cmp w8, #0x1
10046417c:      b.ne    0x100464298 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x13c>
100464180:      ldr w8, [x0, #0x4]
100464184:      strb    wzr, [sp, #0x4]
100464188:      str wzr, [sp]
10046418c:      and x9, x1, #0xffff000000000000
100464190:      mov x10, #0x7fff000000000000 ; =9223090561878065152
100464194:      cmp x9, x10
100464198:      b.eq    0x1004642c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x164>
10046419c:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
1004641a0:      cmp x9, x10
1004641a4:      b.ne    0x10046441c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2c0>
1004641a8:      ubfx    x9, x1, #40, #8
1004641ac:      cbz x9, 0x1004641fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1004641b0:      strb    w1, [sp]
1004641b4:      cmp x9, #0x1
1004641b8:      b.eq    0x1004641fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1004641bc:      lsr x10, x1, #8
1004641c0:      strb    w10, [sp, #0x1]
1004641c4:      cmp x9, #0x2
1004641c8:      b.eq    0x1004641fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1004641cc:      lsr x10, x1, #16
1004641d0:      strb    w10, [sp, #0x2]
1004641d4:      cmp x9, #0x3
1004641d8:      b.eq    0x1004641fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1004641dc:      lsr x10, x1, #24
1004641e0:      strb    w10, [sp, #0x3]
1004641e4:      cmp x9, #0x4
1004641e8:      b.eq    0x1004641fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1004641ec:      lsr x10, x1, #32
1004641f0:      strb    w10, [sp, #0x4]
1004641f4:      cmp x9, #0x5
1004641f8:      b.ne    0x10046444c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2f0>
1004641fc:      mov x9, sp
100464200:      mov w10, #0x22              ; =34
100464204:      strb    w10, [x2]
100464208:      mov w11, #0x1               ; =1
10046420c:      cbnz    w8, 0x1004642dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x180>
100464210:      b   0x1004643cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
100464214:      ldr w19, [x0, #0x4]
100464218:      strb    wzr, [sp, #0x4]
10046421c:      str wzr, [sp]
100464220:      and x8, x1, #0xffff000000000000
100464224:      mov x9, #0x7fff000000000000 ; =9223090561878065152
100464228:      cmp x8, x9
10046422c:      b.eq    0x1004643d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x27c>
100464230:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
100464234:      cmp x8, x9
100464238:      b.ne    0x100464434 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2d8>
10046423c:      ubfx    x8, x1, #40, #8
100464240:      cbz x8, 0x100464290 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
100464244:      strb    w1, [sp]
100464248:      cmp x8, #0x1
10046424c:      b.eq    0x100464290 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
100464250:      lsr x9, x1, #8
100464254:      strb    w9, [sp, #0x1]
100464258:      cmp x8, #0x2
10046425c:      b.eq    0x100464290 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
100464260:      lsr x9, x1, #16
100464264:      strb    w9, [sp, #0x2]
100464268:      cmp x8, #0x3
10046426c:      b.eq    0x100464290 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
100464270:      lsr x9, x1, #24
100464274:      strb    w9, [sp, #0x3]
100464278:      cmp x8, #0x4
10046427c:      b.eq    0x100464290 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
100464280:      lsr x9, x1, #32
100464284:      strb    w9, [sp, #0x4]
100464288:      cmp x8, #0x5
10046428c:      b.ne    0x10046444c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2f0>
100464290:      mov x1, sp
100464294:      b   0x1004643e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x288>
100464298:      ldur    q0, [x0, #0x8]
10046429c:      ldur    q1, [x0, #0x18]
1004642a0:      stp q0, q1, [sp]
1004642a4:      ldr w19, [x0, #0x4]
1004642a8:      mov x1, sp
1004642ac:      mov x0, x2
1004642b0:      mov x2, x19
1004642b4:      bl  0x100cd956c <_writev+0x100cd956c>
1004642b8:      mov x0, x19
1004642bc:      b   0x100464408 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2ac>
1004642c0:      ands    x9, x1, #0xffffffffffff
1004642c4:      add x9, x9, #0x14
1004642c8:      csel    x9, xzr, x9, eq
1004642cc:      mov w10, #0x22              ; =34
1004642d0:      strb    w10, [x2]
1004642d4:      mov w11, #0x1               ; =1
1004642d8:      cbz w8, 0x1004643cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
1004642dc:      mov w12, #0x5c              ; =92
1004642e0:      mov w13, #0x3075            ; =12405
1004642e4:      mov w14, #0x30              ; =48
1004642e8:      adrp    x15, 0x100dd1000 <_anon.88ed17a1392924f08814ef64693a15d8.1611+0x478>
1004642ec:      add x15, x15, #0x6fa
1004642f0:      b   0x10046430c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1b0>
1004642f4:      mov w16, #0x62              ; =98
1004642f8:      strb    w16, [x17, #0x1]
1004642fc:      mov w16, #0x2               ; =2
100464300:      add x11, x16, x11
100464304:      subs    x8, x8, #0x1
100464308:      b.eq    0x1004643cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
10046430c:      ldrb    w16, [x9], #0x1
100464310:      cmp w16, #0x20
100464314:      b.lo    0x100464328 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1cc>
100464318:      cmp w16, #0x5c
10046431c:      b.eq    0x100464328 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1cc>
100464320:      cmp w16, #0x22
100464324:      b.ne    0x1004643b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x254>
100464328:      add x17, x2, x11
10046432c:      strb    w12, [x17]
100464330:      cmp w16, #0xb
100464334:      b.le    0x100464358 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1fc>
100464338:      cmp w16, #0x21
10046433c:      b.gt    0x100464378 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x21c>
100464340:      cmp w16, #0xc
100464344:      b.eq    0x1004643bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x260>
100464348:      cmp w16, #0xd
10046434c:      b.ne    0x100464388 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x22c>
100464350:      mov w16, #0x72              ; =114
100464354:      b   0x1004642f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
100464358:      cmp w16, #0x8
10046435c:      b.eq    0x1004642f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x198>
100464360:      cmp w16, #0x9
100464364:      b.eq    0x1004643c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x268>
100464368:      cmp w16, #0xa
10046436c:      b.ne    0x100464388 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x22c>
100464370:      mov w16, #0x6e              ; =110
100464374:      b   0x1004642f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
100464378:      cmp w16, #0x22
10046437c:      b.eq    0x1004642f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
100464380:      cmp w16, #0x5c
100464384:      b.eq    0x1004642f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
100464388:      sturh   w13, [x17, #0x1]
10046438c:      strb    w14, [x17, #0x3]
100464390:      lsr x0, x16, #4
100464394:      ldrb    w0, [x15, x0]
100464398:      strb    w0, [x17, #0x4]
10046439c:      and x16, x16, #0xf
1004643a0:      ldrb    w16, [x15, x16]
1004643a4:      strb    w16, [x17, #0x5]
1004643a8:      mov w16, #0x6               ; =6
1004643ac:      b   0x100464300 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1a4>
1004643b0:      strb    w16, [x2, x11]
1004643b4:      mov w16, #0x1               ; =1
1004643b8:      b   0x100464300 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1a4>
1004643bc:      mov w16, #0x66              ; =102
1004643c0:      b   0x1004642f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1004643c4:      mov w16, #0x74              ; =116
1004643c8:      b   0x1004642f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1004643cc:      strb    w10, [x2, x11]
1004643d0:      add x0, x11, #0x1
1004643d4:      b   0x100464408 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2ac>
1004643d8:      ands    x8, x1, #0xffffffffffff
1004643dc:      add x8, x8, #0x14
1004643e0:      csel    x1, xzr, x8, eq
1004643e4:      mov w20, #0x22              ; =34
1004643e8:      mov x0, x2
1004643ec:      strb    w20, [x0], #0x1
1004643f0:      mov x21, x2
1004643f4:      mov x2, x19
1004643f8:      bl  0x100cd956c <_writev+0x100cd956c>
1004643fc:      add x8, x21, x19
100464400:      strb    w20, [x8, #0x1]
100464404:      add x0, x19, #0x2
100464408:      ldp x29, x30, [sp, #0x40]
10046440c:      ldp x20, x19, [sp, #0x30]
100464410:      ldp x22, x21, [sp, #0x20]
100464414:      add sp, sp, #0x50
100464418:      ret
10046441c:      adrp    x0, 0x100dd4000 <_anon.152773887b8e060e913a13a302c04959.589+0x3>
100464420:      add x0, x0, #0x67c
100464424:      adrp    x2, 0x1010a5000 <_anon.152773887b8e060e913a13a302c04959.151+0x130>
100464428:      add x2, x2, #0x3a0
10046442c:      mov w1, #0x20               ; =32
100464430:      bl  0x100c8a100 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
100464434:      adrp    x0, 0x100dd4000 <_anon.152773887b8e060e913a13a302c04959.589+0x3>
100464438:      add x0, x0, #0x664
10046443c:      adrp    x2, 0x1010a5000 <_anon.152773887b8e060e913a13a302c04959.151+0x130>
100464440:      add x2, x2, #0x388
100464444:      mov w1, #0x18               ; =24
100464448:      bl  0x100c8a100 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
10046444c:      adrp    x2, 0x1010a4000 <_anon.88ed17a1392924f08814ef64693a15d8.653+0x90>
100464450:      add x2, x2, #0xe20
100464454:      mov w0, #0x5                ; =5
100464458:      mov w1, #0x5                ; =5
10046445c:      bl  0x100c8a1cc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
