/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/key33-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010021b34c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write>:
10021b34c:      sub sp, sp, #0x40
10021b350:      stp x22, x21, [sp, #0x10]
10021b354:      stp x20, x19, [sp, #0x20]
10021b358:      stp x29, x30, [sp, #0x30]
10021b35c:      add x29, sp, #0x30
10021b360:      mov x8, x1
10021b364:      strb    wzr, [sp, #0xc]
10021b368:      str wzr, [sp, #0x8]
10021b36c:      and x9, x2, #0xffff000000000000
10021b370:      mov x10, #0x7fff000000000000 ; =9223090561878065152
10021b374:      cmp x9, x10
10021b378:      b.eq    0x10021b410 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0xc4>
10021b37c:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
10021b380:      cmp x9, x10
10021b384:      b.ne    0x10021b540 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x1f4>
10021b388:      ubfx    x9, x2, #40, #8
10021b38c:      cbz x9, 0x10021b3dc <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x90>
10021b390:      strb    w2, [sp, #0x8]
10021b394:      cmp x9, #0x1
10021b398:      b.eq    0x10021b3dc <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x90>
10021b39c:      lsr x10, x2, #8
10021b3a0:      strb    w10, [sp, #0x9]
10021b3a4:      cmp x9, #0x2
10021b3a8:      b.eq    0x10021b3dc <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x90>
10021b3ac:      lsr x10, x2, #16
10021b3b0:      strb    w10, [sp, #0xa]
10021b3b4:      cmp x9, #0x3
10021b3b8:      b.eq    0x10021b3dc <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x90>
10021b3bc:      lsr x10, x2, #24
10021b3c0:      strb    w10, [sp, #0xb]
10021b3c4:      cmp x9, #0x4
10021b3c8:      b.eq    0x10021b3dc <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x90>
10021b3cc:      lsr x10, x2, #32
10021b3d0:      strb    w10, [sp, #0xc]
10021b3d4:      cmp x9, #0x5
10021b3d8:      b.ne    0x10021b558 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x20c>
10021b3dc:      add x1, sp, #0x8
10021b3e0:      mov w19, #0x22              ; =34
10021b3e4:      strb    w19, [x3]
10021b3e8:      cbnz    w8, 0x10021b428 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0xdc>
10021b3ec:      mov x20, x0
10021b3f0:      add x0, x3, #0x1
10021b3f4:      sub w2, w20, #0x2
10021b3f8:      mov x21, x3
10021b3fc:      bl  0x100af8f2c <_writev+0x100af8f2c>
10021b400:      mov w0, w20
10021b404:      add x8, x21, x0
10021b408:      sturb   w19, [x8, #-0x1]
10021b40c:      b   0x10021b52c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x1e0>
10021b410:      ands    x9, x2, #0xffffffffffff
10021b414:      add x9, x9, #0x14
10021b418:      csel    x1, xzr, x9, eq
10021b41c:      mov w19, #0x22              ; =34
10021b420:      strb    w19, [x3]
10021b424:      cbz w8, 0x10021b3ec <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0xa0>
10021b428:      mov w9, w8
10021b42c:      mov w8, #0x1                ; =1
10021b430:      mov w10, #0x5c              ; =92
10021b434:      mov w11, #0x3075            ; =12405
10021b438:      mov w12, #0x30              ; =48
10021b43c:      adrp    x13, 0x100c2a000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x4ad0>
10021b440:      add x13, x13, #0xca0
10021b444:      b   0x10021b460 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x114>
10021b448:      mov w14, #0x62              ; =98
10021b44c:      strb    w14, [x15, #0x1]
10021b450:      mov w14, #0x2               ; =2
10021b454:      add x8, x14, x8
10021b458:      subs    x9, x9, #0x1
10021b45c:      b.eq    0x10021b520 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x1d4>
10021b460:      ldrb    w14, [x1], #0x1
10021b464:      cmp w14, #0x20
10021b468:      b.lo    0x10021b47c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x130>
10021b46c:      cmp w14, #0x5c
10021b470:      b.eq    0x10021b47c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x130>
10021b474:      cmp w14, #0x22
10021b478:      b.ne    0x10021b504 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x1b8>
10021b47c:      add x15, x3, x8
10021b480:      strb    w10, [x15]
10021b484:      cmp w14, #0xb
10021b488:      b.le    0x10021b4ac <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x160>
10021b48c:      cmp w14, #0x21
10021b490:      b.gt    0x10021b4cc <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x180>
10021b494:      cmp w14, #0xc
10021b498:      b.eq    0x10021b510 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x1c4>
10021b49c:      cmp w14, #0xd
10021b4a0:      b.ne    0x10021b4dc <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x190>
10021b4a4:      mov w14, #0x72              ; =114
10021b4a8:      b   0x10021b44c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x100>
10021b4ac:      cmp w14, #0x8
10021b4b0:      b.eq    0x10021b448 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0xfc>
10021b4b4:      cmp w14, #0x9
10021b4b8:      b.eq    0x10021b518 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x1cc>
10021b4bc:      cmp w14, #0xa
10021b4c0:      b.ne    0x10021b4dc <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x190>
10021b4c4:      mov w14, #0x6e              ; =110
10021b4c8:      b   0x10021b44c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x100>
10021b4cc:      cmp w14, #0x22
10021b4d0:      b.eq    0x10021b44c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x100>
10021b4d4:      cmp w14, #0x5c
10021b4d8:      b.eq    0x10021b44c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x100>
10021b4dc:      sturh   w11, [x15, #0x1]
10021b4e0:      strb    w12, [x15, #0x3]
10021b4e4:      lsr x16, x14, #4
10021b4e8:      ldrb    w16, [x13, x16]
10021b4ec:      strb    w16, [x15, #0x4]
10021b4f0:      and x14, x14, #0xf
10021b4f4:      ldrb    w14, [x13, x14]
10021b4f8:      strb    w14, [x15, #0x5]
10021b4fc:      mov w14, #0x6               ; =6
10021b500:      b   0x10021b454 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x108>
10021b504:      strb    w14, [x3, x8]
10021b508:      mov w14, #0x1               ; =1
10021b50c:      b   0x10021b454 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x108>
10021b510:      mov w14, #0x66              ; =102
10021b514:      b   0x10021b44c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x100>
10021b518:      mov w14, #0x74              ; =116
10021b51c:      b   0x10021b44c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write+0x100>
10021b520:      mov w9, #0x22               ; =34
10021b524:      strb    w9, [x3, x8]
10021b528:      add x0, x8, #0x1
10021b52c:      ldp x29, x30, [sp, #0x30]
10021b530:      ldp x20, x19, [sp, #0x20]
10021b534:      ldp x22, x21, [sp, #0x10]
10021b538:      add sp, sp, #0x40
10021b53c:      ret
10021b540:      adrp    x0, 0x100bdc000 <_PERRY_EMPTY_STRING+0x3bf0>
10021b544:      add x0, x0, #0x755
10021b548:      adrp    x2, 0x100e83000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xdfc8>
10021b54c:      add x2, x2, #0xfe0
10021b550:      mov w1, #0x17               ; =23
10021b554:      bl  0x100ab1340 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
10021b558:      adrp    x2, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
10021b55c:      add x2, x2, #0xe58
10021b560:      mov w0, #0x5                ; =5
10021b564:      mov w1, #0x5                ; =5
10021b568:      bl  0x100ab140c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
