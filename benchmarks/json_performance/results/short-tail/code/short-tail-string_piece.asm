/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/short-tail-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008d738c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>:
1008d738c:      sub sp, sp, #0x40
1008d7390:      stp x20, x19, [sp, #0x20]
1008d7394:      stp x29, x30, [sp, #0x30]
1008d7398:      add x29, sp, #0x30
1008d739c:      strb    wzr, [sp, #0xc]
1008d73a0:      str wzr, [sp, #0x8]
1008d73a4:      and x9, x1, #0xffff000000000000
1008d73a8:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1008d73ac:      cmp x9, x8
1008d73b0:      b.eq    0x1008d742c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xa0>
1008d73b4:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1008d73b8:      cmp x9, x8
1008d73bc:      b.ne    0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d73c0:      ubfx    x8, x1, #40, #8
1008d73c4:      cbz x8, 0x1008d7414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1008d73c8:      strb    w1, [sp, #0x8]
1008d73cc:      cmp x8, #0x1
1008d73d0:      b.eq    0x1008d7414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1008d73d4:      lsr x10, x1, #8
1008d73d8:      strb    w10, [sp, #0x9]
1008d73dc:      cmp x8, #0x2
1008d73e0:      b.eq    0x1008d7414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1008d73e4:      lsr x10, x1, #16
1008d73e8:      strb    w10, [sp, #0xa]
1008d73ec:      cmp x8, #0x3
1008d73f0:      b.eq    0x1008d7414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1008d73f4:      lsr x10, x1, #24
1008d73f8:      strb    w10, [sp, #0xb]
1008d73fc:      cmp x8, #0x4
1008d7400:      b.eq    0x1008d7414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
1008d7404:      lsr x10, x1, #32
1008d7408:      strb    w10, [sp, #0xc]
1008d740c:      cmp x8, #0x5
1008d7410:      b.ne    0x1008d7994 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x608>
1008d7414:      mov x10, x1
1008d7418:      add x1, sp, #0x8
1008d741c:      mov w2, w8
1008d7420:      cmp w8, #0x10
1008d7424:      b.hs    0x1008d7454 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xc8>
1008d7428:      b   0x1008d74a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x118>
1008d742c:      ands    x11, x1, #0xffffffffffff
1008d7430:      b.eq    0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d7434:      ldr w8, [x11, #0x4]
1008d7438:      cmn w8, #0x3
1008d743c:      b.hi    0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d7440:      mov x10, x1
1008d7444:      add x1, x11, #0x14
1008d7448:      mov w2, w8
1008d744c:      cmp w8, #0x10
1008d7450:      b.lo    0x1008d74a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x118>
1008d7454:      mov w11, #0x10              ; =16
1008d7458:      movi.16b    v0, #0x22
1008d745c:      movi.16b    v1, #0x5c
1008d7460:      movi.16b    v2, #0x20
1008d7464:      mov x12, x1
1008d7468:      movi.16b    v3, #0xed
1008d746c:      ldr q4, [x12], #0x10
1008d7470:      cmeq.16b    v5, v4, v0
1008d7474:      cmeq.16b    v6, v4, v1
1008d7478:      orr.16b v5, v6, v5
1008d747c:      cmhi.16b    v6, v2, v4
1008d7480:      cmeq.16b    v4, v4, v3
1008d7484:      orr.16b v4, v4, v6
1008d7488:      orr.16b v4, v4, v5
1008d748c:      addp.2d d4, v4
1008d7490:      fmov    x13, d4
1008d7494:      cbnz    x13, 0x1008d76e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x35c>
1008d7498:      add x11, x11, #0x10
1008d749c:      cmp x11, x2
1008d74a0:      b.ls    0x1008d746c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xe0>
1008d74a4:      and x13, x2, #0xfffffff0
1008d74a8:      add x12, x1, x13
1008d74ac:      tbnz    w2, #0x3, 0x1008d7558 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x1cc>
1008d74b0:      and x11, x2, #0x8
1008d74b4:      and x14, x2, #0x7
1008d74b8:      add x12, x12, x11
1008d74bc:      cmp x14, #0x3
1008d74c0:      b.ls    0x1008d78c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x53c>
1008d74c4:      ldr w11, [x12]
1008d74c8:      tbz w2, #0x1, 0x1008d74d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x148>
1008d74cc:      ldrh    w13, [x12, #0x4]
1008d74d0:      orr x11, x11, x13, lsl #32
1008d74d4:      tbz w2, #0x0, 0x1008d74ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x160>
1008d74d8:      sub x13, x14, #0x1
1008d74dc:      ldrb    w12, [x12, x13]
1008d74e0:      lsl x13, x13, #3
1008d74e4:      lsl x12, x12, x13
1008d74e8:      orr x11, x12, x11
1008d74ec:      ubfiz   x12, x2, #3, #3
1008d74f0:      mov x13, #0x2020202020202020 ; =2314885530818453536
1008d74f4:      lsl x12, x13, x12
1008d74f8:      orr x12, x11, x12
1008d74fc:      eor x13, x12, #0x2222222222222222
1008d7500:      mov x14, #-0x101010101010102 ; =-72340172838076674
1008d7504:      movk    x14, #0xfeff
1008d7508:      mov x15, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
1008d750c:      orr x15, x15, #0x4444444444444444
1008d7510:      eor x15, x12, x15
1008d7514:      add x15, x15, x14
1008d7518:      mov x16, #-0x2020202020202021 ; =-2314885530818453537
1008d751c:      movk    x16, #0xdfe0
1008d7520:      add x16, x12, x16
1008d7524:      orr x15, x15, x16
1008d7528:      add x13, x13, x14
1008d752c:      orr x13, x15, x13
1008d7530:      bic x13, x13, x11
1008d7534:      mov x15, #-0x3333333333333334 ; =-3689348814741910324
1008d7538:      orr x15, x15, #0xe1e1e1e1e1e1e1e1
1008d753c:      eor x12, x12, x15
1008d7540:      add x12, x12, x14
1008d7544:      and x11, x12, x11
1008d7548:      orr x11, x13, x11
1008d754c:      tst x11, #0x8080808080808080
1008d7550:      cset    w11, ne
1008d7554:      b   0x1008d76f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x368>
1008d7558:      mov x14, #0x101010101010101 ; =72340172838076673
1008d755c:      movk    x14, #0x100
1008d7560:      ldr x11, [x12]
1008d7564:      eor x15, x11, #0x2222222222222222
1008d7568:      sub x15, x14, x15
1008d756c:      orr x16, x15, x11
1008d7570:      mov x15, #-0x7f7f7f7f7f7f7f80 ; =-9187201950435737472
1008d7574:      bics    xzr, x15, x16
1008d7578:      b.ne    0x1008d75d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x24c>
1008d757c:      mov x16, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
1008d7580:      orr x16, x16, #0x4444444444444444
1008d7584:      eor x16, x11, x16
1008d7588:      sub x14, x14, x16
1008d758c:      orr x14, x14, x11
1008d7590:      bics    xzr, x15, x14
1008d7594:      b.ne    0x1008d75d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x24c>
1008d7598:      mov x14, #0x2020202020202020 ; =2314885530818453536
1008d759c:      movk    x14, #0x201f
1008d75a0:      sub x14, x14, x11
1008d75a4:      orr x14, x14, x11
1008d75a8:      bic x14, x15, x14
1008d75ac:      cmp x14, #0x0
1008d75b0:      mov x14, #-0x3333333333333334 ; =-3689348814741910324
1008d75b4:      orr x14, x14, #0xe1e1e1e1e1e1e1e1
1008d75b8:      eor x14, x11, x14
1008d75bc:      mov x15, #-0x101010101010102 ; =-72340172838076674
1008d75c0:      movk    x15, #0xfeff
1008d75c4:      add x14, x14, x15
1008d75c8:      and x14, x11, x14
1008d75cc:      and x14, x14, #0x8080808080808080
1008d75d0:      ccmp    x14, #0x0, #0x0, eq
1008d75d4:      b.eq    0x1008d74b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x124>
1008d75d8:      and w12, w11, #0xff
1008d75dc:      cmp w12, #0x22
1008d75e0:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d75e4:      cmp w12, #0x5c
1008d75e8:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d75ec:      cmp w12, #0x20
1008d75f0:      b.lo    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d75f4:      cmp w12, #0xed
1008d75f8:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d75fc:      ubfx    w12, w11, #8, #8
1008d7600:      cmp w12, #0x22
1008d7604:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d7608:      cmp w12, #0x5c
1008d760c:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d7610:      cmp w12, #0xed
1008d7614:      mov w13, #0x20              ; =32
1008d7618:      ccmp    w12, w13, #0x0, ne
1008d761c:      b.lo    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d7620:      ubfx    w12, w11, #16, #8
1008d7624:      cmp w12, #0x22
1008d7628:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d762c:      cmp w12, #0x5c
1008d7630:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d7634:      cmp w12, #0xed
1008d7638:      ccmp    w12, w13, #0x0, ne
1008d763c:      b.lo    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d7640:      lsr w12, w11, #24
1008d7644:      cmp w12, #0x22
1008d7648:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d764c:      cmp w12, #0x5c
1008d7650:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d7654:      cmp w12, #0xed
1008d7658:      ccmp    w12, w13, #0x0, ne
1008d765c:      b.lo    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d7660:      ubfx    x12, x11, #32, #8
1008d7664:      cmp w12, #0x22
1008d7668:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d766c:      cmp w12, #0x5c
1008d7670:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d7674:      cmp w12, #0xed
1008d7678:      ccmp    w12, w13, #0x0, ne
1008d767c:      b.lo    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d7680:      ubfx    x12, x11, #40, #8
1008d7684:      cmp w12, #0x22
1008d7688:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d768c:      cmp w12, #0x5c
1008d7690:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d7694:      cmp w12, #0xed
1008d7698:      ccmp    w12, w13, #0x0, ne
1008d769c:      b.lo    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d76a0:      ubfx    x12, x11, #48, #8
1008d76a4:      cmp w12, #0x22
1008d76a8:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d76ac:      cmp w12, #0x5c
1008d76b0:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d76b4:      cmp w12, #0xed
1008d76b8:      ccmp    w12, w13, #0x0, ne
1008d76bc:      b.lo    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d76c0:      lsr x12, x11, #56
1008d76c4:      cmp w12, #0x22
1008d76c8:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d76cc:      cmp w12, #0x5c
1008d76d0:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d76d4:      lsr x11, x11, #61
1008d76d8:      cmp x12, #0xed
1008d76dc:      ccmp    x11, #0x0, #0x4, ne
1008d76e0:      b.eq    0x1008d76f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x364>
1008d76e4:      b   0x1008d7910 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x584>
1008d76e8:      fmov    x11, d4
1008d76ec:      cbz x11, 0x1008d76f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x368>
1008d76f0:      mov w11, #0x1               ; =1
1008d76f4:      mov x12, #0x7fff000000000000 ; =9223090561878065152
1008d76f8:      cmp x9, x12
1008d76fc:      b.ne    0x1008d773c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3b0>
1008d7700:      and x9, x10, #0xffffffffffff
1008d7704:      ldr w3, [x9]
1008d7708:      tbz w11, #0x0, 0x1008d7804 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x478>
1008d770c:      mov x19, x0
1008d7710:      add x0, sp, #0x10
1008d7714:      bl  0x100886a74 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new>
1008d7718:      ldr w8, [sp, #0x10]
1008d771c:      tbz w8, #0x0, 0x1008d7918 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x58c>
1008d7720:      ldur    x8, [sp, #0x14]
1008d7724:      stur    x8, [x19, #0x4]
1008d7728:      ldr w8, [sp, #0x1c]
1008d772c:      str w8, [x19, #0xc]
1008d7730:      mov w8, #0x1                ; =1
1008d7734:      str w8, [x19]
1008d7738:      b   0x1008d782c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4a0>
1008d773c:      cmp w8, #0x40
1008d7740:      b.hs    0x1008d783c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4b0>
1008d7744:      ands    x9, x2, #0x38
1008d7748:      b.eq    0x1008d776c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3e0>
1008d774c:      and x10, x2, #0x38
1008d7750:      neg x10, x10
1008d7754:      mov x12, x1
1008d7758:      ldr x13, [x12], #0x8
1008d775c:      tst x13, #0x8080808080808080
1008d7760:      b.ne    0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d7764:      adds    x10, x10, #0x8
1008d7768:      b.ne    0x1008d7758 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3cc>
1008d776c:      mov x3, x8
1008d7770:      and x10, x2, #0x7
1008d7774:      cbz x10, 0x1008d7708 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x37c>
1008d7778:      add x9, x1, x9
1008d777c:      ldrsb   w12, [x9]
1008d7780:      tbnz    w12, #0x1f, 0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d7784:      mov x3, x8
1008d7788:      cmp x10, #0x1
1008d778c:      b.eq    0x1008d7708 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x37c>
1008d7790:      ldrsb   w12, [x9, #0x1]
1008d7794:      tbnz    w12, #0x1f, 0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d7798:      mov x3, x8
1008d779c:      cmp x10, #0x2
1008d77a0:      b.eq    0x1008d7708 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x37c>
1008d77a4:      ldrsb   w12, [x9, #0x2]
1008d77a8:      tbnz    w12, #0x1f, 0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d77ac:      mov x3, x8
1008d77b0:      cmp x10, #0x3
1008d77b4:      b.eq    0x1008d7708 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x37c>
1008d77b8:      ldrsb   w12, [x9, #0x3]
1008d77bc:      tbnz    w12, #0x1f, 0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d77c0:      mov x3, x8
1008d77c4:      cmp x10, #0x4
1008d77c8:      b.eq    0x1008d7708 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x37c>
1008d77cc:      ldrsb   w12, [x9, #0x4]
1008d77d0:      tbnz    w12, #0x1f, 0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d77d4:      mov x3, x8
1008d77d8:      cmp x10, #0x5
1008d77dc:      b.eq    0x1008d7708 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x37c>
1008d77e0:      ldrsb   w12, [x9, #0x5]
1008d77e4:      tbnz    w12, #0x1f, 0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d77e8:      mov x3, x8
1008d77ec:      cmp x10, #0x6
1008d77f0:      b.eq    0x1008d7708 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x37c>
1008d77f4:      ldrsb   w9, [x9, #0x6]
1008d77f8:      mov x3, x8
1008d77fc:      tbz w9, #0x1f, 0x1008d7708 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x37c>
1008d7800:      b   0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d7804:      mov w9, #0x3                ; =3
1008d7808:      cmp x2, #0x3
1008d780c:      csel    x9, x2, x9, lo
1008d7810:      cbz w8, 0x1008d7980 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5f4>
1008d7814:      add x10, x1, x2
1008d7818:      ldurb   w10, [x10, #-0x1]
1008d781c:      cmp w10, #0xbf
1008d7820:      b.ls    0x1008d7924 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x598>
1008d7824:      mov w8, #-0x1               ; =-1
1008d7828:      str w8, [x0]
1008d782c:      ldp x29, x30, [sp, #0x30]
1008d7830:      ldp x20, x19, [sp, #0x20]
1008d7834:      add sp, sp, #0x40
1008d7838:      ret
1008d783c:      and x9, x2, #0xffffffc0
1008d7840:      add x9, x1, x9
1008d7844:      mov x10, x1
1008d7848:      ldp q0, q1, [x10]
1008d784c:      ldp q2, q3, [x10, #0x20]
1008d7850:      orr.16b v0, v1, v0
1008d7854:      orr.16b v1, v2, v3
1008d7858:      orr.16b v0, v0, v1
1008d785c:      umaxv.16b   b0, v0
1008d7860:      fmov    w12, s0
1008d7864:      tbnz    w12, #0x7, 0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d7868:      add x10, x10, #0x40
1008d786c:      cmp x10, x9
1008d7870:      b.ne    0x1008d7848 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1008d7874:      ands    x10, x2, #0x30
1008d7878:      b.eq    0x1008d789c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
1008d787c:      mov x12, x10
1008d7880:      mov x13, x9
1008d7884:      ldr q0, [x13], #0x10
1008d7888:      umaxv.16b   b0, v0
1008d788c:      fmov    w14, s0
1008d7890:      tbnz    w14, #0x7, 0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d7894:      subs    x12, x12, #0x10
1008d7898:      b.ne    0x1008d7884 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4f8>
1008d789c:      mov x3, x8
1008d78a0:      and x12, x2, #0xf
1008d78a4:      cbz x12, 0x1008d7708 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x37c>
1008d78a8:      add x9, x9, x10
1008d78ac:      ldrsb   w10, [x9]
1008d78b0:      tbnz    w10, #0x1f, 0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d78b4:      add x9, x9, #0x1
1008d78b8:      subs    x12, x12, #0x1
1008d78bc:      b.ne    0x1008d78ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x520>
1008d78c0:      mov x3, x8
1008d78c4:      b   0x1008d7708 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x37c>
1008d78c8:      tst x2, #0x7
1008d78cc:      b.eq    0x1008d7910 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x584>
1008d78d0:      mov x14, #0x0               ; =0
1008d78d4:      sub x11, x2, x11
1008d78d8:      sub x13, x11, x13
1008d78dc:      ldrb    w15, [x12, x14]
1008d78e0:      mov w11, #0x1               ; =1
1008d78e4:      cmp w15, #0x22
1008d78e8:      b.eq    0x1008d76f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x368>
1008d78ec:      cmp w15, #0x5c
1008d78f0:      b.eq    0x1008d76f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x368>
1008d78f4:      cmp w15, #0x20
1008d78f8:      b.lo    0x1008d76f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x368>
1008d78fc:      cmp w15, #0xed
1008d7900:      b.eq    0x1008d76f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x368>
1008d7904:      add x14, x14, #0x1
1008d7908:      cmp x13, x14
1008d790c:      b.ne    0x1008d78dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x550>
1008d7910:      mov w11, #0x0               ; =0
1008d7914:      b   0x1008d76f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x368>
1008d7918:      mov w8, #-0x1               ; =-1
1008d791c:      str w8, [x19]
1008d7920:      b   0x1008d782c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4a0>
1008d7924:      cmp w8, #0x1
1008d7928:      b.eq    0x1008d7980 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5f4>
1008d792c:      add x10, x1, x2
1008d7930:      ldurb   w10, [x10, #-0x2]
1008d7934:      cmp w10, #0xdf
1008d7938:      b.ls    0x1008d7944 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5b8>
1008d793c:      mov w10, #0x2               ; =2
1008d7940:      b   0x1008d7978 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5ec>
1008d7944:      cmp w8, #0x2
1008d7948:      b.eq    0x1008d7980 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5f4>
1008d794c:      add x11, x1, x2
1008d7950:      mov x10, #-0x3              ; =-3
1008d7954:      ldrb    w12, [x11, x10]
1008d7958:      cmp w12, #0xef
1008d795c:      b.hi    0x1008d7974 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5e8>
1008d7960:      sub x10, x10, #0x1
1008d7964:      add x12, x9, x10
1008d7968:      cmn x12, #0x1
1008d796c:      b.ne    0x1008d7954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5c8>
1008d7970:      b   0x1008d7980 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5f4>
1008d7974:      neg x10, x10
1008d7978:      cmp x10, x9
1008d797c:      b.ls    0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d7980:      cmn w3, #0x3
1008d7984:      b.hi    0x1008d7824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1008d7988:      stp wzr, w8, [x0]
1008d798c:      str w3, [x0, #0x8]
1008d7990:      b   0x1008d782c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4a0>
1008d7994:      adrp    x2, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008d7998:      add x2, x2, #0x968
1008d799c:      mov w0, #0x5                ; =5
1008d79a0:      mov w1, #0x5                ; =5
1008d79a4:      bl  0x100c83dcc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
