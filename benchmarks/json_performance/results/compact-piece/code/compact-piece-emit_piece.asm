
/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/compact-piece-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010046425c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>:
10046425c:      sub sp, sp, #0x50
100464260:      stp x22, x21, [sp, #0x20]
100464264:      stp x20, x19, [sp, #0x30]
100464268:      stp x29, x30, [sp, #0x40]
10046426c:      add x29, sp, #0x40
100464270:      ldr w8, [x0]
100464274:      cbz w8, 0x10046429c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x40>
100464278:      ldur    q0, [x0, #0x8]
10046427c:      ldur    q1, [x0, #0x18]
100464280:      stp q0, q1, [sp]
100464284:      ldr w19, [x0, #0x4]
100464288:      mov x1, sp
10046428c:      mov x0, x2
100464290:      mov x2, x19
100464294:      bl  0x100cd926c <_writev+0x100cd926c>
100464298:      b   0x100464370 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x114>
10046429c:      ldp w8, w19, [x0, #0x4]
1004642a0:      strb    wzr, [sp, #0x4]
1004642a4:      str wzr, [sp]
1004642a8:      and x9, x1, #0xffff000000000000
1004642ac:      mov x10, #0x7fff000000000000 ; =9223090561878065152
1004642b0:      cmp x9, x10
1004642b4:      b.eq    0x100464338 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xdc>
1004642b8:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
1004642bc:      cmp x9, x10
1004642c0:      b.ne    0x100464388 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x12c>
1004642c4:      ubfx    x9, x1, #40, #8
1004642c8:      cbz x9, 0x100464318 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xbc>
1004642cc:      strb    w1, [sp]
1004642d0:      cmp x9, #0x1
1004642d4:      b.eq    0x100464318 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xbc>
1004642d8:      lsr x10, x1, #8
1004642dc:      strb    w10, [sp, #0x1]
1004642e0:      cmp x9, #0x2
1004642e4:      b.eq    0x100464318 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xbc>
1004642e8:      lsr x10, x1, #16
1004642ec:      strb    w10, [sp, #0x2]
1004642f0:      cmp x9, #0x3
1004642f4:      b.eq    0x100464318 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xbc>
1004642f8:      lsr x10, x1, #24
1004642fc:      strb    w10, [sp, #0x3]
100464300:      cmp x9, #0x4
100464304:      b.eq    0x100464318 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xbc>
100464308:      lsr x10, x1, #32
10046430c:      strb    w10, [sp, #0x4]
100464310:      cmp x9, #0x5
100464314:      b.ne    0x1004643a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x144>
100464318:      mov x1, sp
10046431c:      sub w9, w19, #0x2
100464320:      cmp w9, w8
100464324:      b.eq    0x100464350 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xf4>
100464328:      mov x0, x8
10046432c:      bl  0x100435544 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan13write_escaped>
100464330:      mov x19, x0
100464334:      b   0x100464370 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x114>
100464338:      ands    x9, x1, #0xffffffffffff
10046433c:      add x9, x9, #0x14
100464340:      csel    x1, xzr, x9, eq
100464344:      sub w9, w19, #0x2
100464348:      cmp w9, w8
10046434c:      b.ne    0x100464328 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xcc>
100464350:      mov w20, #0x22              ; =34
100464354:      mov x0, x2
100464358:      strb    w20, [x0], #0x1
10046435c:      mov x21, x2
100464360:      mov x2, x8
100464364:      bl  0x100cd926c <_writev+0x100cd926c>
100464368:      add x8, x21, x19
10046436c:      sturb   w20, [x8, #-0x1]
100464370:      mov x0, x19
100464374:      ldp x29, x30, [sp, #0x40]
100464378:      ldp x20, x19, [sp, #0x30]
10046437c:      ldp x22, x21, [sp, #0x20]
100464380:      add sp, sp, #0x50
100464384:      ret
100464388:      adrp    x0, 0x100dd4000 <_anon.152773887b8e060e913a13a302c04959.622+0x1e8>
10046438c:      add x0, x0, #0x364
100464390:      adrp    x2, 0x1010a5000 <_anon.152773887b8e060e913a13a302c04959.151+0x130>
100464394:      add x2, x2, #0x388
100464398:      mov w1, #0x18               ; =24
10046439c:      bl  0x100c89e00 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
1004643a0:      adrp    x2, 0x1010a4000 <_anon.88ed17a1392924f08814ef64693a15d8.653+0x90>
1004643a4:      add x2, x2, #0xe20
1004643a8:      mov w0, #0x5                ; =5
1004643ac:      mov w1, #0x5                ; =5
1004643b0:      bl  0x100c89ecc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
