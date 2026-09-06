
/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/escaped-count-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100465274 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>:
100465274:      sub sp, sp, #0x40
100465278:      stp x20, x19, [sp, #0x20]
10046527c:      stp x29, x30, [sp, #0x30]
100465280:      add x29, sp, #0x30
100465284:      strb    wzr, [sp, #0xc]
100465288:      str wzr, [sp, #0x8]
10046528c:      and x9, x1, #0xffff000000000000
100465290:      mov x8, #0x7fff000000000000 ; =9223090561878065152
100465294:      cmp x9, x8
100465298:      b.eq    0x100465314 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xa0>
10046529c:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1004652a0:      cmp x9, x8
1004652a4:      b.ne    0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1004652a8:      ubfx    x8, x1, #40, #8
1004652ac:      cbz x8, 0x1004652fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1004652b0:      strb    w1, [sp, #0x8]
1004652b4:      cmp x8, #0x1
1004652b8:      b.eq    0x1004652fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1004652bc:      lsr x10, x1, #8
1004652c0:      strb    w10, [sp, #0x9]
1004652c4:      cmp x8, #0x2
1004652c8:      b.eq    0x1004652fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1004652cc:      lsr x10, x1, #16
1004652d0:      strb    w10, [sp, #0xa]
1004652d4:      cmp x8, #0x3
1004652d8:      b.eq    0x1004652fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1004652dc:      lsr x10, x1, #24
1004652e0:      strb    w10, [sp, #0xb]
1004652e4:      cmp x8, #0x4
1004652e8:      b.eq    0x1004652fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1004652ec:      lsr x10, x1, #32
1004652f0:      strb    w10, [sp, #0xc]
1004652f4:      cmp x8, #0x5
1004652f8:      b.ne    0x1004657f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x580>
1004652fc:      mov x10, x1
100465300:      add x1, sp, #0x8
100465304:      mov w2, w8
100465308:      cmp w8, #0x10
10046530c:      b.hs    0x10046533c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xc8>
100465310:      b   0x10046538c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x118>
100465314:      ands    x11, x1, #0xffffffffffff
100465318:      b.eq    0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
10046531c:      ldr w8, [x11, #0x4]
100465320:      cmn w8, #0x3
100465324:      b.hi    0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100465328:      mov x10, x1
10046532c:      add x1, x11, #0x14
100465330:      mov w2, w8
100465334:      cmp w8, #0x10
100465338:      b.lo    0x10046538c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x118>
10046533c:      mov w11, #0x10              ; =16
100465340:      movi.16b    v0, #0x22
100465344:      movi.16b    v1, #0x5c
100465348:      movi.16b    v2, #0x20
10046534c:      mov x12, x1
100465350:      movi.16b    v3, #0xed
100465354:      ldr q4, [x12], #0x10
100465358:      cmeq.16b    v5, v4, v0
10046535c:      cmeq.16b    v6, v4, v1
100465360:      orr.16b v5, v6, v5
100465364:      cmhi.16b    v6, v2, v4
100465368:      cmeq.16b    v4, v4, v3
10046536c:      orr.16b v4, v4, v6
100465370:      orr.16b v4, v4, v5
100465374:      addp.2d d4, v4
100465378:      fmov    x13, d4
10046537c:      cbnz    x13, 0x100465588 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x314>
100465380:      add x11, x11, #0x10
100465384:      cmp x11, x2
100465388:      b.ls    0x100465354 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xe0>
10046538c:      and x12, x2, #0xfffffff0
100465390:      add x13, x1, x12
100465394:      tbnz    w2, #0x3, 0x1004653e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x174>
100465398:      tst x2, #0x7
10046539c:      b.eq    0x1004656f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x480>
1004653a0:      and x11, x2, #0x8
1004653a4:      add x13, x13, x11
1004653a8:      sub x11, x2, x11
1004653ac:      sub x12, x11, x12
1004653b0:      mov w11, #0x0               ; =0
1004653b4:      ldrb    w14, [x13], #0x1
1004653b8:      cmp w14, #0x22
1004653bc:      b.eq    0x1004656f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x484>
1004653c0:      cmp w14, #0x5c
1004653c4:      b.eq    0x1004656f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x484>
1004653c8:      mov w11, #0x0               ; =0
1004653cc:      cmp w14, #0x20
1004653d0:      b.lo    0x1004656f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x484>
1004653d4:      cmp w14, #0xed
1004653d8:      b.eq    0x1004656f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x484>
1004653dc:      subs    x12, x12, #0x1
1004653e0:      b.ne    0x1004653b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x13c>
1004653e4:      b   0x100465574 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x300>
1004653e8:      mov x14, #0x101010101010101 ; =72340172838076673
1004653ec:      movk    x14, #0x100
1004653f0:      ldr x11, [x13]
1004653f4:      eor x15, x11, #0x2222222222222222
1004653f8:      sub x15, x14, x15
1004653fc:      orr x16, x15, x11
100465400:      mov x15, #-0x7f7f7f7f7f7f7f80 ; =-9187201950435737472
100465404:      bics    xzr, x15, x16
100465408:      b.ne    0x100465468 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x1f4>
10046540c:      mov x16, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
100465410:      orr x16, x16, #0x4444444444444444
100465414:      eor x16, x11, x16
100465418:      sub x14, x14, x16
10046541c:      orr x14, x14, x11
100465420:      bics    xzr, x15, x14
100465424:      b.ne    0x100465468 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x1f4>
100465428:      mov x14, #0x2020202020202020 ; =2314885530818453536
10046542c:      movk    x14, #0x201f
100465430:      sub x14, x14, x11
100465434:      orr x14, x14, x11
100465438:      bic x14, x15, x14
10046543c:      cmp x14, #0x0
100465440:      mov x14, #-0x3333333333333334 ; =-3689348814741910324
100465444:      orr x14, x14, #0xe1e1e1e1e1e1e1e1
100465448:      eor x14, x11, x14
10046544c:      mov x15, #-0x101010101010102 ; =-72340172838076674
100465450:      movk    x15, #0xfeff
100465454:      add x14, x14, x15
100465458:      and x14, x11, x14
10046545c:      and x14, x14, #0x8080808080808080
100465460:      ccmp    x14, #0x0, #0x0, eq
100465464:      b.eq    0x100465398 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x124>
100465468:      and w12, w11, #0xff
10046546c:      cmp w12, #0x22
100465470:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465474:      cmp w12, #0x5c
100465478:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
10046547c:      cmp w12, #0x20
100465480:      b.lo    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465484:      cmp w12, #0xed
100465488:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
10046548c:      ubfx    w12, w11, #8, #8
100465490:      cmp w12, #0x22
100465494:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465498:      cmp w12, #0x5c
10046549c:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004654a0:      cmp w12, #0xed
1004654a4:      mov w13, #0x20              ; =32
1004654a8:      ccmp    w12, w13, #0x0, ne
1004654ac:      b.lo    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004654b0:      ubfx    w12, w11, #16, #8
1004654b4:      cmp w12, #0x22
1004654b8:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004654bc:      cmp w12, #0x5c
1004654c0:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004654c4:      cmp w12, #0xed
1004654c8:      ccmp    w12, w13, #0x0, ne
1004654cc:      b.lo    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004654d0:      lsr w12, w11, #24
1004654d4:      cmp w12, #0x22
1004654d8:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004654dc:      cmp w12, #0x5c
1004654e0:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004654e4:      cmp w12, #0xed
1004654e8:      ccmp    w12, w13, #0x0, ne
1004654ec:      b.lo    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004654f0:      ubfx    x12, x11, #32, #8
1004654f4:      cmp w12, #0x22
1004654f8:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004654fc:      cmp w12, #0x5c
100465500:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465504:      cmp w12, #0xed
100465508:      ccmp    w12, w13, #0x0, ne
10046550c:      b.lo    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465510:      ubfx    x12, x11, #40, #8
100465514:      cmp w12, #0x22
100465518:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
10046551c:      cmp w12, #0x5c
100465520:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465524:      cmp w12, #0xed
100465528:      ccmp    w12, w13, #0x0, ne
10046552c:      b.lo    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465530:      ubfx    x12, x11, #48, #8
100465534:      cmp w12, #0x22
100465538:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
10046553c:      cmp w12, #0x5c
100465540:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465544:      cmp w12, #0xed
100465548:      ccmp    w12, w13, #0x0, ne
10046554c:      b.lo    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465550:      lsr x12, x11, #56
100465554:      cmp w12, #0x22
100465558:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
10046555c:      cmp w12, #0x5c
100465560:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465564:      lsr x11, x11, #61
100465568:      cmp x12, #0xed
10046556c:      ccmp    x11, #0x0, #0x4, ne
100465570:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465574:      mov w11, #0x1               ; =1
100465578:      mov x12, #0x7fff000000000000 ; =9223090561878065152
10046557c:      cmp x9, x12
100465580:      b.ne    0x1004655a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x32c>
100465584:      b   0x100465704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x490>
100465588:      fmov    x11, d4
10046558c:      cbz x11, 0x100465574 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x300>
100465590:      mov w11, #0x0               ; =0
100465594:      mov x12, #0x7fff000000000000 ; =9223090561878065152
100465598:      cmp x9, x12
10046559c:      b.eq    0x100465704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x490>
1004655a0:      cmp w8, #0x40
1004655a4:      b.hs    0x100465668 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3f4>
1004655a8:      ands    x9, x2, #0x38
1004655ac:      b.eq    0x1004655d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x35c>
1004655b0:      and x10, x2, #0x38
1004655b4:      neg x10, x10
1004655b8:      mov x12, x1
1004655bc:      ldr x13, [x12], #0x8
1004655c0:      tst x13, #0x8080808080808080
1004655c4:      b.ne    0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1004655c8:      adds    x10, x10, #0x8
1004655cc:      b.ne    0x1004655bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x348>
1004655d0:      mov x3, x8
1004655d4:      and x10, x2, #0x7
1004655d8:      cbz x10, 0x10046570c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1004655dc:      add x9, x1, x9
1004655e0:      ldrsb   w12, [x9]
1004655e4:      tbnz    w12, #0x1f, 0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1004655e8:      mov x3, x8
1004655ec:      cmp x10, #0x1
1004655f0:      b.eq    0x10046570c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1004655f4:      ldrsb   w12, [x9, #0x1]
1004655f8:      tbnz    w12, #0x1f, 0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1004655fc:      mov x3, x8
100465600:      cmp x10, #0x2
100465604:      b.eq    0x10046570c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100465608:      ldrsb   w12, [x9, #0x2]
10046560c:      tbnz    w12, #0x1f, 0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100465610:      mov x3, x8
100465614:      cmp x10, #0x3
100465618:      b.eq    0x10046570c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
10046561c:      ldrsb   w12, [x9, #0x3]
100465620:      tbnz    w12, #0x1f, 0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100465624:      mov x3, x8
100465628:      cmp x10, #0x4
10046562c:      b.eq    0x10046570c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100465630:      ldrsb   w12, [x9, #0x4]
100465634:      tbnz    w12, #0x1f, 0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100465638:      mov x3, x8
10046563c:      cmp x10, #0x5
100465640:      b.eq    0x10046570c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100465644:      ldrsb   w12, [x9, #0x5]
100465648:      tbnz    w12, #0x1f, 0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
10046564c:      mov x3, x8
100465650:      cmp x10, #0x6
100465654:      b.eq    0x10046570c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100465658:      ldrsb   w9, [x9, #0x6]
10046565c:      mov x3, x8
100465660:      tbz w9, #0x1f, 0x10046570c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100465664:      b   0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100465668:      and x9, x2, #0xffffffc0
10046566c:      add x9, x1, x9
100465670:      mov x10, x1
100465674:      ldp q0, q1, [x10]
100465678:      ldp q2, q3, [x10, #0x20]
10046567c:      orr.16b v0, v1, v0
100465680:      orr.16b v1, v2, v3
100465684:      orr.16b v0, v0, v1
100465688:      umaxv.16b   b0, v0
10046568c:      fmov    w12, s0
100465690:      tbnz    w12, #0x7, 0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100465694:      add x10, x10, #0x40
100465698:      cmp x10, x9
10046569c:      b.ne    0x100465674 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x400>
1004656a0:      ands    x10, x2, #0x30
1004656a4:      b.eq    0x1004656c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x454>
1004656a8:      mov x12, x10
1004656ac:      mov x13, x9
1004656b0:      ldr q0, [x13], #0x10
1004656b4:      umaxv.16b   b0, v0
1004656b8:      fmov    w14, s0
1004656bc:      tbnz    w14, #0x7, 0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1004656c0:      subs    x12, x12, #0x10
1004656c4:      b.ne    0x1004656b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x43c>
1004656c8:      mov x3, x8
1004656cc:      and x12, x2, #0xf
1004656d0:      cbz x12, 0x10046570c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1004656d4:      add x9, x9, x10
1004656d8:      ldrsb   w10, [x9]
1004656dc:      tbnz    w10, #0x1f, 0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1004656e0:      add x9, x9, #0x1
1004656e4:      subs    x12, x12, #0x1
1004656e8:      b.ne    0x1004656d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x464>
1004656ec:      mov x3, x8
1004656f0:      b   0x10046570c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1004656f4:      mov w11, #0x1               ; =1
1004656f8:      mov x12, #0x7fff000000000000 ; =9223090561878065152
1004656fc:      cmp x9, x12
100465700:      b.ne    0x1004655a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x32c>
100465704:      and x9, x10, #0xffffffffffff
100465708:      ldr w3, [x9]
10046570c:      tbz w11, #0x0, 0x100465748 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4d4>
100465710:      mov w9, #0x3                ; =3
100465714:      cmp x2, #0x3
100465718:      csel    x9, x2, x9, lo
10046571c:      cbz w8, 0x1004657e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x56c>
100465720:      add x10, x1, x2
100465724:      ldurb   w10, [x10, #-0x1]
100465728:      cmp w10, #0xbf
10046572c:      b.ls    0x100465784 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
100465730:      mov w8, #-0x1               ; =-1
100465734:      str w8, [x0]
100465738:      ldp x29, x30, [sp, #0x30]
10046573c:      ldp x20, x19, [sp, #0x20]
100465740:      add sp, sp, #0x40
100465744:      ret
100465748:      mov x19, x0
10046574c:      add x0, sp, #0x10
100465750:      bl  0x100435544 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new>
100465754:      ldr w8, [sp, #0x10]
100465758:      tbz w8, #0x0, 0x100465778 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x504>
10046575c:      ldur    x8, [sp, #0x14]
100465760:      stur    x8, [x19, #0x4]
100465764:      ldr w8, [sp, #0x1c]
100465768:      str w8, [x19, #0xc]
10046576c:      mov w8, #0x1                ; =1
100465770:      str w8, [x19]
100465774:      b   0x100465738 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4c4>
100465778:      mov w8, #-0x1               ; =-1
10046577c:      str w8, [x19]
100465780:      b   0x100465738 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4c4>
100465784:      cmp w8, #0x1
100465788:      b.eq    0x1004657e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x56c>
10046578c:      add x10, x1, x2
100465790:      ldurb   w10, [x10, #-0x2]
100465794:      cmp w10, #0xdf
100465798:      b.ls    0x1004657a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x530>
10046579c:      mov w10, #0x2               ; =2
1004657a0:      b   0x1004657d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x564>
1004657a4:      cmp w8, #0x2
1004657a8:      b.eq    0x1004657e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x56c>
1004657ac:      add x11, x1, x2
1004657b0:      mov x10, #-0x3              ; =-3
1004657b4:      ldrb    w12, [x11, x10]
1004657b8:      cmp w12, #0xef
1004657bc:      b.hi    0x1004657d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x560>
1004657c0:      sub x10, x10, #0x1
1004657c4:      add x12, x9, x10
1004657c8:      cmn x12, #0x1
1004657cc:      b.ne    0x1004657b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x540>
1004657d0:      b   0x1004657e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x56c>
1004657d4:      neg x10, x10
1004657d8:      cmp x10, x9
1004657dc:      b.ls    0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1004657e0:      cmn w3, #0x3
1004657e4:      b.hi    0x100465730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1004657e8:      stp wzr, w8, [x0]
1004657ec:      str w3, [x0, #0x8]
1004657f0:      b   0x100465738 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4c4>
1004657f4:      adrp    x2, 0x1010a4000 <_anon.88ed17a1392924f08814ef64693a15d8.653+0x90>
1004657f8:      add x2, x2, #0xe20
1004657fc:      mov w0, #0x5                ; =5
100465800:      mov w1, #0x5                ; =5
100465804:      bl  0x100c8a1cc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
