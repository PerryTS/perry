
/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/escaped-output-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100266184 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>:
100266184:      sub sp, sp, #0x40
100266188:      stp x20, x19, [sp, #0x20]
10026618c:      stp x29, x30, [sp, #0x30]
100266190:      add x29, sp, #0x30
100266194:      strb    wzr, [sp, #0xc]
100266198:      str wzr, [sp, #0x8]
10026619c:      and x9, x1, #0xffff000000000000
1002661a0:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1002661a4:      cmp x9, x8
1002661a8:      b.eq    0x100266224 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xa0>
1002661ac:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1002661b0:      cmp x9, x8
1002661b4:      b.ne    0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1002661b8:      ubfx    x8, x1, #40, #8
1002661bc:      cbz x8, 0x10026620c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1002661c0:      strb    w1, [sp, #0x8]
1002661c4:      cmp x8, #0x1
1002661c8:      b.eq    0x10026620c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1002661cc:      lsr x10, x1, #8
1002661d0:      strb    w10, [sp, #0x9]
1002661d4:      cmp x8, #0x2
1002661d8:      b.eq    0x10026620c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1002661dc:      lsr x10, x1, #16
1002661e0:      strb    w10, [sp, #0xa]
1002661e4:      cmp x8, #0x3
1002661e8:      b.eq    0x10026620c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1002661ec:      lsr x10, x1, #24
1002661f0:      strb    w10, [sp, #0xb]
1002661f4:      cmp x8, #0x4
1002661f8:      b.eq    0x10026620c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1002661fc:      lsr x10, x1, #32
100266200:      strb    w10, [sp, #0xc]
100266204:      cmp x8, #0x5
100266208:      b.ne    0x100266704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x580>
10026620c:      mov x10, x1
100266210:      add x1, sp, #0x8
100266214:      mov w2, w8
100266218:      cmp w8, #0x10
10026621c:      b.hs    0x10026624c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xc8>
100266220:      b   0x10026629c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x118>
100266224:      ands    x11, x1, #0xffffffffffff
100266228:      b.eq    0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
10026622c:      ldr w8, [x11, #0x4]
100266230:      cmn w8, #0x3
100266234:      b.hi    0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100266238:      mov x10, x1
10026623c:      add x1, x11, #0x14
100266240:      mov w2, w8
100266244:      cmp w8, #0x10
100266248:      b.lo    0x10026629c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x118>
10026624c:      mov w11, #0x10              ; =16
100266250:      movi.16b    v0, #0x22
100266254:      movi.16b    v1, #0x5c
100266258:      movi.16b    v2, #0x20
10026625c:      mov x12, x1
100266260:      movi.16b    v3, #0xed
100266264:      ldr q4, [x12], #0x10
100266268:      cmeq.16b    v5, v4, v0
10026626c:      cmeq.16b    v6, v4, v1
100266270:      orr.16b v5, v6, v5
100266274:      cmhi.16b    v6, v2, v4
100266278:      cmeq.16b    v4, v4, v3
10026627c:      orr.16b v4, v4, v6
100266280:      orr.16b v4, v4, v5
100266284:      addp.2d d4, v4
100266288:      fmov    x13, d4
10026628c:      cbnz    x13, 0x100266498 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x314>
100266290:      add x11, x11, #0x10
100266294:      cmp x11, x2
100266298:      b.ls    0x100266264 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xe0>
10026629c:      and x12, x2, #0xfffffff0
1002662a0:      add x13, x1, x12
1002662a4:      tbnz    w2, #0x3, 0x1002662f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x174>
1002662a8:      tst x2, #0x7
1002662ac:      b.eq    0x100266604 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x480>
1002662b0:      and x11, x2, #0x8
1002662b4:      add x13, x13, x11
1002662b8:      sub x11, x2, x11
1002662bc:      sub x12, x11, x12
1002662c0:      mov w11, #0x0               ; =0
1002662c4:      ldrb    w14, [x13], #0x1
1002662c8:      cmp w14, #0x22
1002662cc:      b.eq    0x100266608 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x484>
1002662d0:      cmp w14, #0x5c
1002662d4:      b.eq    0x100266608 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x484>
1002662d8:      mov w11, #0x0               ; =0
1002662dc:      cmp w14, #0x20
1002662e0:      b.lo    0x100266608 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x484>
1002662e4:      cmp w14, #0xed
1002662e8:      b.eq    0x100266608 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x484>
1002662ec:      subs    x12, x12, #0x1
1002662f0:      b.ne    0x1002662c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x13c>
1002662f4:      b   0x100266484 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x300>
1002662f8:      mov x14, #0x101010101010101 ; =72340172838076673
1002662fc:      movk    x14, #0x100
100266300:      ldr x11, [x13]
100266304:      eor x15, x11, #0x2222222222222222
100266308:      sub x15, x14, x15
10026630c:      orr x16, x15, x11
100266310:      mov x15, #-0x7f7f7f7f7f7f7f80 ; =-9187201950435737472
100266314:      bics    xzr, x15, x16
100266318:      b.ne    0x100266378 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x1f4>
10026631c:      mov x16, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
100266320:      orr x16, x16, #0x4444444444444444
100266324:      eor x16, x11, x16
100266328:      sub x14, x14, x16
10026632c:      orr x14, x14, x11
100266330:      bics    xzr, x15, x14
100266334:      b.ne    0x100266378 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x1f4>
100266338:      mov x14, #0x2020202020202020 ; =2314885530818453536
10026633c:      movk    x14, #0x201f
100266340:      sub x14, x14, x11
100266344:      orr x14, x14, x11
100266348:      bic x14, x15, x14
10026634c:      cmp x14, #0x0
100266350:      mov x14, #-0x3333333333333334 ; =-3689348814741910324
100266354:      orr x14, x14, #0xe1e1e1e1e1e1e1e1
100266358:      eor x14, x11, x14
10026635c:      mov x15, #-0x101010101010102 ; =-72340172838076674
100266360:      movk    x15, #0xfeff
100266364:      add x14, x14, x15
100266368:      and x14, x11, x14
10026636c:      and x14, x14, #0x8080808080808080
100266370:      ccmp    x14, #0x0, #0x0, eq
100266374:      b.eq    0x1002662a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x124>
100266378:      and w12, w11, #0xff
10026637c:      cmp w12, #0x22
100266380:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100266384:      cmp w12, #0x5c
100266388:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
10026638c:      cmp w12, #0x20
100266390:      b.lo    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100266394:      cmp w12, #0xed
100266398:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
10026639c:      ubfx    w12, w11, #8, #8
1002663a0:      cmp w12, #0x22
1002663a4:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1002663a8:      cmp w12, #0x5c
1002663ac:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1002663b0:      cmp w12, #0xed
1002663b4:      mov w13, #0x20              ; =32
1002663b8:      ccmp    w12, w13, #0x0, ne
1002663bc:      b.lo    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1002663c0:      ubfx    w12, w11, #16, #8
1002663c4:      cmp w12, #0x22
1002663c8:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1002663cc:      cmp w12, #0x5c
1002663d0:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1002663d4:      cmp w12, #0xed
1002663d8:      ccmp    w12, w13, #0x0, ne
1002663dc:      b.lo    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1002663e0:      lsr w12, w11, #24
1002663e4:      cmp w12, #0x22
1002663e8:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1002663ec:      cmp w12, #0x5c
1002663f0:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1002663f4:      cmp w12, #0xed
1002663f8:      ccmp    w12, w13, #0x0, ne
1002663fc:      b.lo    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100266400:      ubfx    x12, x11, #32, #8
100266404:      cmp w12, #0x22
100266408:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
10026640c:      cmp w12, #0x5c
100266410:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100266414:      cmp w12, #0xed
100266418:      ccmp    w12, w13, #0x0, ne
10026641c:      b.lo    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100266420:      ubfx    x12, x11, #40, #8
100266424:      cmp w12, #0x22
100266428:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
10026642c:      cmp w12, #0x5c
100266430:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100266434:      cmp w12, #0xed
100266438:      ccmp    w12, w13, #0x0, ne
10026643c:      b.lo    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100266440:      ubfx    x12, x11, #48, #8
100266444:      cmp w12, #0x22
100266448:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
10026644c:      cmp w12, #0x5c
100266450:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100266454:      cmp w12, #0xed
100266458:      ccmp    w12, w13, #0x0, ne
10026645c:      b.lo    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100266460:      lsr x12, x11, #56
100266464:      cmp w12, #0x22
100266468:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
10026646c:      cmp w12, #0x5c
100266470:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100266474:      lsr x11, x11, #61
100266478:      cmp x12, #0xed
10026647c:      ccmp    x11, #0x0, #0x4, ne
100266480:      b.eq    0x1002664a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100266484:      mov w11, #0x1               ; =1
100266488:      mov x12, #0x7fff000000000000 ; =9223090561878065152
10026648c:      cmp x9, x12
100266490:      b.ne    0x1002664b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x32c>
100266494:      b   0x100266614 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x490>
100266498:      fmov    x11, d4
10026649c:      cbz x11, 0x100266484 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x300>
1002664a0:      mov w11, #0x0               ; =0
1002664a4:      mov x12, #0x7fff000000000000 ; =9223090561878065152
1002664a8:      cmp x9, x12
1002664ac:      b.eq    0x100266614 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x490>
1002664b0:      cmp w8, #0x40
1002664b4:      b.hs    0x100266578 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3f4>
1002664b8:      ands    x9, x2, #0x38
1002664bc:      b.eq    0x1002664e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x35c>
1002664c0:      and x10, x2, #0x38
1002664c4:      neg x10, x10
1002664c8:      mov x12, x1
1002664cc:      ldr x13, [x12], #0x8
1002664d0:      tst x13, #0x8080808080808080
1002664d4:      b.ne    0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1002664d8:      adds    x10, x10, #0x8
1002664dc:      b.ne    0x1002664cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x348>
1002664e0:      mov x3, x8
1002664e4:      and x10, x2, #0x7
1002664e8:      cbz x10, 0x10026661c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1002664ec:      add x9, x1, x9
1002664f0:      ldrsb   w12, [x9]
1002664f4:      tbnz    w12, #0x1f, 0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1002664f8:      mov x3, x8
1002664fc:      cmp x10, #0x1
100266500:      b.eq    0x10026661c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100266504:      ldrsb   w12, [x9, #0x1]
100266508:      tbnz    w12, #0x1f, 0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
10026650c:      mov x3, x8
100266510:      cmp x10, #0x2
100266514:      b.eq    0x10026661c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100266518:      ldrsb   w12, [x9, #0x2]
10026651c:      tbnz    w12, #0x1f, 0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100266520:      mov x3, x8
100266524:      cmp x10, #0x3
100266528:      b.eq    0x10026661c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
10026652c:      ldrsb   w12, [x9, #0x3]
100266530:      tbnz    w12, #0x1f, 0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100266534:      mov x3, x8
100266538:      cmp x10, #0x4
10026653c:      b.eq    0x10026661c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100266540:      ldrsb   w12, [x9, #0x4]
100266544:      tbnz    w12, #0x1f, 0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100266548:      mov x3, x8
10026654c:      cmp x10, #0x5
100266550:      b.eq    0x10026661c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100266554:      ldrsb   w12, [x9, #0x5]
100266558:      tbnz    w12, #0x1f, 0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
10026655c:      mov x3, x8
100266560:      cmp x10, #0x6
100266564:      b.eq    0x10026661c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100266568:      ldrsb   w9, [x9, #0x6]
10026656c:      mov x3, x8
100266570:      tbz w9, #0x1f, 0x10026661c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100266574:      b   0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100266578:      and x9, x2, #0xffffffc0
10026657c:      add x9, x1, x9
100266580:      mov x10, x1
100266584:      ldp q0, q1, [x10]
100266588:      ldp q2, q3, [x10, #0x20]
10026658c:      orr.16b v0, v1, v0
100266590:      orr.16b v1, v2, v3
100266594:      orr.16b v0, v0, v1
100266598:      umaxv.16b   b0, v0
10026659c:      fmov    w12, s0
1002665a0:      tbnz    w12, #0x7, 0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1002665a4:      add x10, x10, #0x40
1002665a8:      cmp x10, x9
1002665ac:      b.ne    0x100266584 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x400>
1002665b0:      ands    x10, x2, #0x30
1002665b4:      b.eq    0x1002665d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x454>
1002665b8:      mov x12, x10
1002665bc:      mov x13, x9
1002665c0:      ldr q0, [x13], #0x10
1002665c4:      umaxv.16b   b0, v0
1002665c8:      fmov    w14, s0
1002665cc:      tbnz    w14, #0x7, 0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1002665d0:      subs    x12, x12, #0x10
1002665d4:      b.ne    0x1002665c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x43c>
1002665d8:      mov x3, x8
1002665dc:      and x12, x2, #0xf
1002665e0:      cbz x12, 0x10026661c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1002665e4:      add x9, x9, x10
1002665e8:      ldrsb   w10, [x9]
1002665ec:      tbnz    w10, #0x1f, 0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1002665f0:      add x9, x9, #0x1
1002665f4:      subs    x12, x12, #0x1
1002665f8:      b.ne    0x1002665e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x464>
1002665fc:      mov x3, x8
100266600:      b   0x10026661c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100266604:      mov w11, #0x1               ; =1
100266608:      mov x12, #0x7fff000000000000 ; =9223090561878065152
10026660c:      cmp x9, x12
100266610:      b.ne    0x1002664b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x32c>
100266614:      and x9, x10, #0xffffffffffff
100266618:      ldr w3, [x9]
10026661c:      tbz w11, #0x0, 0x100266658 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4d4>
100266620:      mov w9, #0x3                ; =3
100266624:      cmp x2, #0x3
100266628:      csel    x9, x2, x9, lo
10026662c:      cbz w8, 0x1002666f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x56c>
100266630:      add x10, x1, x2
100266634:      ldurb   w10, [x10, #-0x1]
100266638:      cmp w10, #0xbf
10026663c:      b.ls    0x100266694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
100266640:      mov w8, #-0x1               ; =-1
100266644:      str w8, [x0]
100266648:      ldp x29, x30, [sp, #0x30]
10026664c:      ldp x20, x19, [sp, #0x20]
100266650:      add sp, sp, #0x40
100266654:      ret
100266658:      mov x19, x0
10026665c:      add x0, sp, #0x10
100266660:      bl  0x10021420c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new>
100266664:      ldr w8, [sp, #0x10]
100266668:      tbz w8, #0x0, 0x100266688 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x504>
10026666c:      ldur    x8, [sp, #0x14]
100266670:      stur    x8, [x19, #0x4]
100266674:      ldr w8, [sp, #0x1c]
100266678:      str w8, [x19, #0xc]
10026667c:      mov w8, #0x1                ; =1
100266680:      str w8, [x19]
100266684:      b   0x100266648 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4c4>
100266688:      mov w8, #-0x1               ; =-1
10026668c:      str w8, [x19]
100266690:      b   0x100266648 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4c4>
100266694:      cmp w8, #0x1
100266698:      b.eq    0x1002666f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x56c>
10026669c:      add x10, x1, x2
1002666a0:      ldurb   w10, [x10, #-0x2]
1002666a4:      cmp w10, #0xdf
1002666a8:      b.ls    0x1002666b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x530>
1002666ac:      mov w10, #0x2               ; =2
1002666b0:      b   0x1002666e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x564>
1002666b4:      cmp w8, #0x2
1002666b8:      b.eq    0x1002666f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x56c>
1002666bc:      add x11, x1, x2
1002666c0:      mov x10, #-0x3              ; =-3
1002666c4:      ldrb    w12, [x11, x10]
1002666c8:      cmp w12, #0xef
1002666cc:      b.hi    0x1002666e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x560>
1002666d0:      sub x10, x10, #0x1
1002666d4:      add x12, x9, x10
1002666d8:      cmn x12, #0x1
1002666dc:      b.ne    0x1002666c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x540>
1002666e0:      b   0x1002666f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x56c>
1002666e4:      neg x10, x10
1002666e8:      cmp x10, x9
1002666ec:      b.ls    0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1002666f0:      cmn w3, #0x3
1002666f4:      b.hi    0x100266640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1002666f8:      stp wzr, w8, [x0]
1002666fc:      str w3, [x0, #0x8]
100266700:      b   0x100266648 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4c4>
100266704:      adrp    x2, 0x101099000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object11global_this13fetch_globals22THREAD_MODULE_TOP_THIS+0x1d40>
100266708:      add x2, x2, #0xac8
10026670c:      mov w0, #0x5                ; =5
100266710:      mov w1, #0x5                ; =5
100266714:      bl  0x100c84b8c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
