/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/fused35-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004c23bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate>:
1004c23bc:      sub sp, sp, #0xf0
1004c23c0:      stp x28, x27, [sp, #0x90]
1004c23c4:      stp x26, x25, [sp, #0xa0]
1004c23c8:      stp x24, x23, [sp, #0xb0]
1004c23cc:      stp x22, x21, [sp, #0xc0]
1004c23d0:      stp x20, x19, [sp, #0xd0]
1004c23d4:      stp x29, x30, [sp, #0xe0]
1004c23d8:      add x29, sp, #0xe0
1004c23dc:      mov x19, x0
1004c23e0:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004c23e4:      add x0, x0, #0xba8
1004c23e8:      ldr x8, [x0]
1004c23ec:      blr x8
1004c23f0:      mov x21, x0
1004c23f4:      ldrb    w8, [x0, #0x20]
1004c23f8:      cbnz    w8, 0x1004c2e14 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa58>
1004c23fc:      ldr x28, [x21]
1004c2400:      mov x8, #0x7fffffffffffffff ; =9223372036854775807
1004c2404:      cmp x28, x8
1004c2408:      b.hs    0x1004c2e40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa84>
1004c240c:      add x8, x28, #0x1
1004c2410:      str x8, [x21]
1004c2414:      ldr x9, [x21, #0x18]
1004c2418:      cbz x9, 0x1004c2de8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa2c>
1004c241c:      ldr x10, [x21, #0x10]
1004c2420:      ldr x20, [x19, #0x80]
1004c2424:      lsl x8, x9, #5
1004c2428:      cmp x20, #0x9
1004c242c:      b.hs    0x1004c2dd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa14>
1004c2430:      ldr x27, [x19]
1004c2434:      ubfx    x22, x27, #40, #8
1004c2438:      ldr x11, [x19, #0x10]
1004c243c:      str x11, [sp, #0x68]
1004c2440:      ubfx    x23, x11, #40, #8
1004c2444:      ldr x11, [x19, #0x20]
1004c2448:      str x11, [sp, #0x58]
1004c244c:      ubfx    x11, x11, #40, #8
1004c2450:      str x11, [sp, #0x60]
1004c2454:      ldr x11, [x19, #0x30]
1004c2458:      str x11, [sp, #0x48]
1004c245c:      ubfx    x11, x11, #40, #8
1004c2460:      str x11, [sp, #0x50]
1004c2464:      ldr x11, [x19, #0x40]
1004c2468:      str x11, [sp, #0x38]
1004c246c:      ubfx    x11, x11, #40, #8
1004c2470:      str x11, [sp, #0x40]
1004c2474:      ldr x11, [x19, #0x50]
1004c2478:      str x11, [sp, #0x28]
1004c247c:      ubfx    x11, x11, #40, #8
1004c2480:      str x11, [sp, #0x30]
1004c2484:      ldr x11, [x19, #0x60]
1004c2488:      str x11, [sp, #0x18]
1004c248c:      ubfx    x11, x11, #40, #8
1004c2490:      str x11, [sp, #0x20]
1004c2494:      ldr x11, [x19, #0x70]
1004c2498:      str x11, [sp, #0x8]
1004c249c:      ubfx    x11, x11, #40, #8
1004c24a0:      str x11, [sp, #0x10]
1004c24a4:      add x8, x8, x10
1004c24a8:      sub x24, x8, #0x8
1004c24ac:      neg x25, x9, lsl #5
1004c24b0:      b   0x1004c24c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x104>
1004c24b4:      sub x24, x24, #0x20
1004c24b8:      adds    x25, x25, #0x20
1004c24bc:      b.eq    0x1004c2de8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa2c>
1004c24c0:      ldur    x8, [x24, #-0x8]
1004c24c4:      cmp x8, x20
1004c24c8:      b.ne    0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c24cc:      sturb   wzr, [x29, #-0x64]
1004c24d0:      stur    wzr, [x29, #-0x68]
1004c24d4:      cbz x20, 0x1004c29d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c24d8:      ldur    x26, [x24, #-0x10]
1004c24dc:      ldr x8, [x26]
1004c24e0:      cbz x22, 0x1004c2530 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c24e4:      sturb   w27, [x29, #-0x68]
1004c24e8:      cmp x22, #0x1
1004c24ec:      b.eq    0x1004c2530 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c24f0:      lsr x9, x27, #8
1004c24f4:      sturb   w9, [x29, #-0x67]
1004c24f8:      cmp x22, #0x2
1004c24fc:      b.eq    0x1004c2530 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c2500:      lsr x9, x27, #16
1004c2504:      sturb   w9, [x29, #-0x66]
1004c2508:      cmp x22, #0x3
1004c250c:      b.eq    0x1004c2530 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c2510:      lsr x9, x27, #24
1004c2514:      sturb   w9, [x29, #-0x65]
1004c2518:      cmp x22, #0x4
1004c251c:      b.eq    0x1004c2530 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c2520:      lsr x9, x27, #32
1004c2524:      sturb   w9, [x29, #-0x64]
1004c2528:      cmp x22, #0x5
1004c252c:      b.ne    0x1004c2ef4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c2530:      ldr w9, [x8, #0x4]
1004c2534:      cmp x22, x9
1004c2538:      b.ne    0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c253c:      add x0, x8, #0x14
1004c2540:      sub x1, x29, #0x68
1004c2544:      mov x2, x22
1004c2548:      bl  0x100af8f60 <_writev+0x100af8f60>
1004c254c:      cbnz    w0, 0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2550:      cmp x20, #0x1
1004c2554:      b.eq    0x1004c29d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c2558:      ldr x8, [x26, #0x8]
1004c255c:      cbz x23, 0x1004c25c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c2560:      ldr x9, [sp, #0x68]
1004c2564:      sturb   w9, [x29, #-0x68]
1004c2568:      cmp x23, #0x1
1004c256c:      b.eq    0x1004c25c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c2570:      ldr x9, [sp, #0x68]
1004c2574:      lsr x9, x9, #8
1004c2578:      sturb   w9, [x29, #-0x67]
1004c257c:      cmp x23, #0x2
1004c2580:      b.eq    0x1004c25c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c2584:      ldr x9, [sp, #0x68]
1004c2588:      lsr x9, x9, #16
1004c258c:      sturb   w9, [x29, #-0x66]
1004c2590:      cmp x23, #0x3
1004c2594:      b.eq    0x1004c25c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c2598:      ldr x9, [sp, #0x68]
1004c259c:      lsr x9, x9, #24
1004c25a0:      sturb   w9, [x29, #-0x65]
1004c25a4:      cmp x23, #0x4
1004c25a8:      b.eq    0x1004c25c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c25ac:      ldr x9, [sp, #0x68]
1004c25b0:      lsr x9, x9, #32
1004c25b4:      sturb   w9, [x29, #-0x64]
1004c25b8:      cmp x23, #0x5
1004c25bc:      b.ne    0x1004c2ef4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c25c0:      ldr w9, [x8, #0x4]
1004c25c4:      cmp x23, x9
1004c25c8:      b.ne    0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c25cc:      add x0, x8, #0x14
1004c25d0:      sub x1, x29, #0x68
1004c25d4:      mov x2, x23
1004c25d8:      bl  0x100af8f60 <_writev+0x100af8f60>
1004c25dc:      cbnz    w0, 0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c25e0:      cmp x20, #0x2
1004c25e4:      b.eq    0x1004c29d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c25e8:      ldr x8, [x26, #0x10]
1004c25ec:      ldr x9, [sp, #0x60]
1004c25f0:      cbz x9, 0x1004c2664 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c25f4:      ldp x10, x9, [sp, #0x58]
1004c25f8:      sturb   w10, [x29, #-0x68]
1004c25fc:      cmp x9, #0x1
1004c2600:      b.eq    0x1004c2664 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c2604:      ldr x9, [sp, #0x58]
1004c2608:      lsr x9, x9, #8
1004c260c:      sturb   w9, [x29, #-0x67]
1004c2610:      ldr x9, [sp, #0x60]
1004c2614:      cmp x9, #0x2
1004c2618:      b.eq    0x1004c2664 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c261c:      ldr x9, [sp, #0x58]
1004c2620:      lsr x9, x9, #16
1004c2624:      sturb   w9, [x29, #-0x66]
1004c2628:      ldr x9, [sp, #0x60]
1004c262c:      cmp x9, #0x3
1004c2630:      b.eq    0x1004c2664 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c2634:      ldr x9, [sp, #0x58]
1004c2638:      lsr x9, x9, #24
1004c263c:      sturb   w9, [x29, #-0x65]
1004c2640:      ldr x9, [sp, #0x60]
1004c2644:      cmp x9, #0x4
1004c2648:      b.eq    0x1004c2664 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c264c:      ldr x9, [sp, #0x58]
1004c2650:      lsr x9, x9, #32
1004c2654:      sturb   w9, [x29, #-0x64]
1004c2658:      ldr x9, [sp, #0x60]
1004c265c:      cmp x9, #0x5
1004c2660:      b.ne    0x1004c2ef4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c2664:      ldr w9, [x8, #0x4]
1004c2668:      ldr x10, [sp, #0x60]
1004c266c:      cmp x10, x9
1004c2670:      b.ne    0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2674:      add x0, x8, #0x14
1004c2678:      sub x1, x29, #0x68
1004c267c:      ldr x2, [sp, #0x60]
1004c2680:      bl  0x100af8f60 <_writev+0x100af8f60>
1004c2684:      cbnz    w0, 0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2688:      cmp x20, #0x3
1004c268c:      b.eq    0x1004c29d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c2690:      ldr x8, [x26, #0x18]
1004c2694:      ldr x9, [sp, #0x50]
1004c2698:      cbz x9, 0x1004c270c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c269c:      ldp x10, x9, [sp, #0x48]
1004c26a0:      sturb   w10, [x29, #-0x68]
1004c26a4:      cmp x9, #0x1
1004c26a8:      b.eq    0x1004c270c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c26ac:      ldr x9, [sp, #0x48]
1004c26b0:      lsr x9, x9, #8
1004c26b4:      sturb   w9, [x29, #-0x67]
1004c26b8:      ldr x9, [sp, #0x50]
1004c26bc:      cmp x9, #0x2
1004c26c0:      b.eq    0x1004c270c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c26c4:      ldr x9, [sp, #0x48]
1004c26c8:      lsr x9, x9, #16
1004c26cc:      sturb   w9, [x29, #-0x66]
1004c26d0:      ldr x9, [sp, #0x50]
1004c26d4:      cmp x9, #0x3
1004c26d8:      b.eq    0x1004c270c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c26dc:      ldr x9, [sp, #0x48]
1004c26e0:      lsr x9, x9, #24
1004c26e4:      sturb   w9, [x29, #-0x65]
1004c26e8:      ldr x9, [sp, #0x50]
1004c26ec:      cmp x9, #0x4
1004c26f0:      b.eq    0x1004c270c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c26f4:      ldr x9, [sp, #0x48]
1004c26f8:      lsr x9, x9, #32
1004c26fc:      sturb   w9, [x29, #-0x64]
1004c2700:      ldr x9, [sp, #0x50]
1004c2704:      cmp x9, #0x5
1004c2708:      b.ne    0x1004c2ef4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c270c:      ldr w9, [x8, #0x4]
1004c2710:      ldr x10, [sp, #0x50]
1004c2714:      cmp x10, x9
1004c2718:      b.ne    0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c271c:      add x0, x8, #0x14
1004c2720:      sub x1, x29, #0x68
1004c2724:      ldr x2, [sp, #0x50]
1004c2728:      bl  0x100af8f60 <_writev+0x100af8f60>
1004c272c:      cbnz    w0, 0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2730:      cmp x20, #0x4
1004c2734:      b.eq    0x1004c29d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c2738:      ldr x8, [x26, #0x20]
1004c273c:      ldr x9, [sp, #0x40]
1004c2740:      cbz x9, 0x1004c27b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c2744:      ldp x10, x9, [sp, #0x38]
1004c2748:      sturb   w10, [x29, #-0x68]
1004c274c:      cmp x9, #0x1
1004c2750:      b.eq    0x1004c27b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c2754:      ldr x9, [sp, #0x38]
1004c2758:      lsr x9, x9, #8
1004c275c:      sturb   w9, [x29, #-0x67]
1004c2760:      ldr x9, [sp, #0x40]
1004c2764:      cmp x9, #0x2
1004c2768:      b.eq    0x1004c27b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c276c:      ldr x9, [sp, #0x38]
1004c2770:      lsr x9, x9, #16
1004c2774:      sturb   w9, [x29, #-0x66]
1004c2778:      ldr x9, [sp, #0x40]
1004c277c:      cmp x9, #0x3
1004c2780:      b.eq    0x1004c27b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c2784:      ldr x9, [sp, #0x38]
1004c2788:      lsr x9, x9, #24
1004c278c:      sturb   w9, [x29, #-0x65]
1004c2790:      ldr x9, [sp, #0x40]
1004c2794:      cmp x9, #0x4
1004c2798:      b.eq    0x1004c27b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c279c:      ldr x9, [sp, #0x38]
1004c27a0:      lsr x9, x9, #32
1004c27a4:      sturb   w9, [x29, #-0x64]
1004c27a8:      ldr x9, [sp, #0x40]
1004c27ac:      cmp x9, #0x5
1004c27b0:      b.ne    0x1004c2ef4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c27b4:      ldr w9, [x8, #0x4]
1004c27b8:      ldr x10, [sp, #0x40]
1004c27bc:      cmp x10, x9
1004c27c0:      b.ne    0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c27c4:      add x0, x8, #0x14
1004c27c8:      sub x1, x29, #0x68
1004c27cc:      ldr x2, [sp, #0x40]
1004c27d0:      bl  0x100af8f60 <_writev+0x100af8f60>
1004c27d4:      cbnz    w0, 0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c27d8:      cmp x20, #0x5
1004c27dc:      b.eq    0x1004c29d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c27e0:      ldr x8, [x26, #0x28]
1004c27e4:      ldr x9, [sp, #0x30]
1004c27e8:      cbz x9, 0x1004c285c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c27ec:      ldp x10, x9, [sp, #0x28]
1004c27f0:      sturb   w10, [x29, #-0x68]
1004c27f4:      cmp x9, #0x1
1004c27f8:      b.eq    0x1004c285c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c27fc:      ldr x9, [sp, #0x28]
1004c2800:      lsr x9, x9, #8
1004c2804:      sturb   w9, [x29, #-0x67]
1004c2808:      ldr x9, [sp, #0x30]
1004c280c:      cmp x9, #0x2
1004c2810:      b.eq    0x1004c285c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c2814:      ldr x9, [sp, #0x28]
1004c2818:      lsr x9, x9, #16
1004c281c:      sturb   w9, [x29, #-0x66]
1004c2820:      ldr x9, [sp, #0x30]
1004c2824:      cmp x9, #0x3
1004c2828:      b.eq    0x1004c285c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c282c:      ldr x9, [sp, #0x28]
1004c2830:      lsr x9, x9, #24
1004c2834:      sturb   w9, [x29, #-0x65]
1004c2838:      ldr x9, [sp, #0x30]
1004c283c:      cmp x9, #0x4
1004c2840:      b.eq    0x1004c285c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c2844:      ldr x9, [sp, #0x28]
1004c2848:      lsr x9, x9, #32
1004c284c:      sturb   w9, [x29, #-0x64]
1004c2850:      ldr x9, [sp, #0x30]
1004c2854:      cmp x9, #0x5
1004c2858:      b.ne    0x1004c2ef4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c285c:      ldr w9, [x8, #0x4]
1004c2860:      ldr x10, [sp, #0x30]
1004c2864:      cmp x10, x9
1004c2868:      b.ne    0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c286c:      add x0, x8, #0x14
1004c2870:      sub x1, x29, #0x68
1004c2874:      ldr x2, [sp, #0x30]
1004c2878:      bl  0x100af8f60 <_writev+0x100af8f60>
1004c287c:      cbnz    w0, 0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2880:      cmp x20, #0x6
1004c2884:      b.eq    0x1004c29d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c2888:      ldr x8, [x26, #0x30]
1004c288c:      ldr x9, [sp, #0x20]
1004c2890:      cbz x9, 0x1004c2904 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c2894:      ldp x10, x9, [sp, #0x18]
1004c2898:      sturb   w10, [x29, #-0x68]
1004c289c:      cmp x9, #0x1
1004c28a0:      b.eq    0x1004c2904 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c28a4:      ldr x9, [sp, #0x18]
1004c28a8:      lsr x9, x9, #8
1004c28ac:      sturb   w9, [x29, #-0x67]
1004c28b0:      ldr x9, [sp, #0x20]
1004c28b4:      cmp x9, #0x2
1004c28b8:      b.eq    0x1004c2904 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c28bc:      ldr x9, [sp, #0x18]
1004c28c0:      lsr x9, x9, #16
1004c28c4:      sturb   w9, [x29, #-0x66]
1004c28c8:      ldr x9, [sp, #0x20]
1004c28cc:      cmp x9, #0x3
1004c28d0:      b.eq    0x1004c2904 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c28d4:      ldr x9, [sp, #0x18]
1004c28d8:      lsr x9, x9, #24
1004c28dc:      sturb   w9, [x29, #-0x65]
1004c28e0:      ldr x9, [sp, #0x20]
1004c28e4:      cmp x9, #0x4
1004c28e8:      b.eq    0x1004c2904 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c28ec:      ldr x9, [sp, #0x18]
1004c28f0:      lsr x9, x9, #32
1004c28f4:      sturb   w9, [x29, #-0x64]
1004c28f8:      ldr x9, [sp, #0x20]
1004c28fc:      cmp x9, #0x5
1004c2900:      b.ne    0x1004c2ef4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c2904:      ldr w9, [x8, #0x4]
1004c2908:      ldr x10, [sp, #0x20]
1004c290c:      cmp x10, x9
1004c2910:      b.ne    0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2914:      add x0, x8, #0x14
1004c2918:      sub x1, x29, #0x68
1004c291c:      ldr x2, [sp, #0x20]
1004c2920:      bl  0x100af8f60 <_writev+0x100af8f60>
1004c2924:      cbnz    w0, 0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2928:      cmp x20, #0x7
1004c292c:      b.eq    0x1004c29d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c2930:      ldr x8, [x26, #0x38]
1004c2934:      ldr x9, [sp, #0x10]
1004c2938:      cbz x9, 0x1004c29ac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c293c:      ldp x10, x9, [sp, #0x8]
1004c2940:      sturb   w10, [x29, #-0x68]
1004c2944:      cmp x9, #0x1
1004c2948:      b.eq    0x1004c29ac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c294c:      ldr x9, [sp, #0x8]
1004c2950:      lsr x9, x9, #8
1004c2954:      sturb   w9, [x29, #-0x67]
1004c2958:      ldr x9, [sp, #0x10]
1004c295c:      cmp x9, #0x2
1004c2960:      b.eq    0x1004c29ac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c2964:      ldr x9, [sp, #0x8]
1004c2968:      lsr x9, x9, #16
1004c296c:      sturb   w9, [x29, #-0x66]
1004c2970:      ldr x9, [sp, #0x10]
1004c2974:      cmp x9, #0x3
1004c2978:      b.eq    0x1004c29ac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c297c:      ldr x9, [sp, #0x8]
1004c2980:      lsr x9, x9, #24
1004c2984:      sturb   w9, [x29, #-0x65]
1004c2988:      ldr x9, [sp, #0x10]
1004c298c:      cmp x9, #0x4
1004c2990:      b.eq    0x1004c29ac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c2994:      ldr x9, [sp, #0x8]
1004c2998:      lsr x9, x9, #32
1004c299c:      sturb   w9, [x29, #-0x64]
1004c29a0:      ldr x9, [sp, #0x10]
1004c29a4:      cmp x9, #0x5
1004c29a8:      b.ne    0x1004c2ef4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c29ac:      ldr w9, [x8, #0x4]
1004c29b0:      ldr x10, [sp, #0x10]
1004c29b4:      cmp x10, x9
1004c29b8:      b.ne    0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c29bc:      add x0, x8, #0x14
1004c29c0:      sub x1, x29, #0x68
1004c29c4:      ldr x2, [sp, #0x10]
1004c29c8:      bl  0x100af8f60 <_writev+0x100af8f60>
1004c29cc:      cbnz    w0, 0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c29d0:      ldr x22, [x24]
1004c29d4:      str x28, [x21]
1004c29d8:      bl  0x10026a350 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
1004c29dc:      str x0, [sp, #0x70]
1004c29e0:      mov x23, #0x7ffd000000000000 ; =9222527611924643840
1004c29e4:      stp x22, x23, [x29, #-0x60]
1004c29e8:      mov w8, #0x1                ; =1
1004c29ec:      stur    x8, [x29, #-0x68]
1004c29f0:      sub x0, x29, #0x68
1004c29f4:      bl  0x10026a458 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1004c29f8:      mov x22, x0
1004c29fc:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c2a00:      add x0, x0, #0x9d0
1004c2a04:      ldr x8, [x0]
1004c2a08:      blr x8
1004c2a0c:      mov x21, x0
1004c2a10:      ldrb    w8, [x0]
1004c2a14:      strb    wzr, [x0]
1004c2a18:      cbz w8, 0x1004c2a54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x698>
1004c2a1c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c2a20:      add x0, x0, #0x9e8
1004c2a24:      ldr x8, [x0]
1004c2a28:      blr x8
1004c2a2c:      ldrb    w8, [x0]
1004c2a30:      tst w8, #0x3
1004c2a34:      b.ne    0x1004c2a4c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x690>
1004c2a38:      adrp    x8, 0x100fd9000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfeec>
1004c2a3c:      add x8, x8, #0x8fc
1004c2a40:      ldapr   w8, [x8]
1004c2a44:      cmp w8, #0x0
1004c2a48:      b.le    0x1004c2d7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9c0>
1004c2a4c:      mov w8, #0x1                ; =1
1004c2a50:      strb    w8, [x21]
1004c2a54:      mov w0, #0x0                ; =0
1004c2a58:      mov w1, #0x0                ; =0
1004c2a5c:      mov x2, x20
1004c2a60:      bl  0x10081d5c0 <_js_object_alloc_with_parent>
1004c2a64:      stp x0, x23, [x29, #-0x60]
1004c2a68:      mov w8, #0x1                ; =1
1004c2a6c:      stur    x8, [x29, #-0x68]
1004c2a70:      sub x0, x29, #0x68
1004c2a74:      bl  0x10026a458 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1004c2a78:      mov x23, x0
1004c2a7c:      adrp    x25, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2a80:      ldr x8, [x25, #0x48]
1004c2a84:      cmn x8, #0x1
1004c2a88:      b.eq    0x1004c2aec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x730>
1004c2a8c:      mrs x9, TPIDRRO_EL0
1004c2a90:      and x9, x9, #0xfffffffffffffff8
1004c2a94:      ldr x8, [x9, x8, lsl #3]
1004c2a98:      cbz x8, 0x1004c2aec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x730>
1004c2a9c:      ldr x8, [x8, #0x19e8]
1004c2aa0:      cbz x8, 0x1004c2aec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x730>
1004c2aa4:      ldr x9, [x8]
1004c2aa8:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004c2aac:      cmp x9, x10
1004c2ab0:      b.hs    0x1004c2eb4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xaf8>
1004c2ab4:      add x10, x9, #0x1
1004c2ab8:      str x10, [x8]
1004c2abc:      ldr x10, [x8, #0x18]
1004c2ac0:      cmp x22, x10
1004c2ac4:      b.hs    0x1004c2ec0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb04>
1004c2ac8:      ldr x10, [x8, #0x10]
1004c2acc:      mov w11, #0x18              ; =24
1004c2ad0:      madd    x10, x22, x11, x10
1004c2ad4:      ldr x11, [x10]
1004c2ad8:      cmp x11, #0x1
1004c2adc:      b.ne    0x1004c2ec4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb08>
1004c2ae0:      ldr x0, [x10, #0x8]
1004c2ae4:      str x9, [x8]
1004c2ae8:      b   0x1004c2af4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x738>
1004c2aec:      mov x0, x22
1004c2af0:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004c2af4:      mov x1, x20
1004c2af8:      mov x2, x20
1004c2afc:      mov x3, #0x0                ; =0
1004c2b00:      mov w4, #0x0                ; =0
1004c2b04:      mov w5, #0x0                ; =0
1004c2b08:      bl  0x10059ba00 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes34shape_descriptor_ensure_with_holes>
1004c2b0c:      mov x24, x0
1004c2b10:      tbnz    w24, #0x0, 0x1004c2e4c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa90>
1004c2b14:      ldr x8, [x25, #0x48]
1004c2b18:      cmn x8, #0x1
1004c2b1c:      b.eq    0x1004c2b8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x7d0>
1004c2b20:      mrs x9, TPIDRRO_EL0
1004c2b24:      and x9, x9, #0xfffffffffffffff8
1004c2b28:      ldr x8, [x9, x8, lsl #3]
1004c2b2c:      cbz x8, 0x1004c2b8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x7d0>
1004c2b30:      ldr x8, [x8, #0x19e8]
1004c2b34:      cbz x8, 0x1004c2b8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x7d0>
1004c2b38:      ldr x9, [x8]
1004c2b3c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004c2b40:      cmp x9, x10
1004c2b44:      b.hs    0x1004c2eb4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xaf8>
1004c2b48:      add x10, x9, #0x1
1004c2b4c:      str x10, [x8]
1004c2b50:      ldr x10, [x8, #0x18]
1004c2b54:      cmp x23, x10
1004c2b58:      b.hs    0x1004c2ec0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb04>
1004c2b5c:      ldr x10, [x8, #0x10]
1004c2b60:      mov w11, #0x18              ; =24
1004c2b64:      madd    x10, x23, x11, x10
1004c2b68:      ldr x11, [x10]
1004c2b6c:      cmp x11, #0x1
1004c2b70:      b.ne    0x1004c2ec4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb08>
1004c2b74:      ldr x23, [x10, #0x8]
1004c2b78:      str x9, [x8]
1004c2b7c:      ldr x8, [x25, #0x48]
1004c2b80:      cmn x8, #0x1
1004c2b84:      b.ne    0x1004c2ba4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x7e8>
1004c2b88:      b   0x1004c2c04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x848>
1004c2b8c:      mov x0, x23
1004c2b90:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004c2b94:      mov x23, x0
1004c2b98:      ldr x8, [x25, #0x48]
1004c2b9c:      cmn x8, #0x1
1004c2ba0:      b.eq    0x1004c2c04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x848>
1004c2ba4:      mrs x9, TPIDRRO_EL0
1004c2ba8:      and x9, x9, #0xfffffffffffffff8
1004c2bac:      ldr x8, [x9, x8, lsl #3]
1004c2bb0:      cbz x8, 0x1004c2c04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x848>
1004c2bb4:      ldr x8, [x8, #0x19e8]
1004c2bb8:      cbz x8, 0x1004c2c04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x848>
1004c2bbc:      ldr x9, [x8]
1004c2bc0:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004c2bc4:      cmp x9, x10
1004c2bc8:      b.hs    0x1004c2eb4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xaf8>
1004c2bcc:      add x10, x9, #0x1
1004c2bd0:      str x10, [x8]
1004c2bd4:      ldr x10, [x8, #0x18]
1004c2bd8:      cmp x22, x10
1004c2bdc:      b.hs    0x1004c2ec0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb04>
1004c2be0:      ldr x10, [x8, #0x10]
1004c2be4:      mov w11, #0x18              ; =24
1004c2be8:      madd    x10, x22, x11, x10
1004c2bec:      ldr x11, [x10]
1004c2bf0:      cmp x11, #0x1
1004c2bf4:      b.ne    0x1004c2ec4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb08>
1004c2bf8:      ldr x2, [x10, #0x8]
1004c2bfc:      str x9, [x8]
1004c2c00:      b   0x1004c2c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x854>
1004c2c04:      mov x0, x22
1004c2c08:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004c2c0c:      mov x2, x0
1004c2c10:      lsr x1, x24, #32
1004c2c14:      mov x0, x23
1004c2c18:      mov x3, x20
1004c2c1c:      bl  0x10059bf3c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes34try_birth_stamp_preinstalled_shape>
1004c2c20:      tbz w0, #0x0, 0x1004c2e54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa98>
1004c2c24:      cbz x23, 0x1004c2c34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x878>
1004c2c28:      ldurh   w8, [x23, #-0x6]
1004c2c2c:      orr w8, w8, #0x200
1004c2c30:      sturh   w8, [x23, #-0x6]
1004c2c34:      cbz x20, 0x1004c2ce4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x928>
1004c2c38:      lsl x8, x20, #4
1004c2c3c:      sub x9, x8, #0x10
1004c2c40:      cmp x9, #0x80
1004c2c44:      b.hs    0x1004c2c54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x898>
1004c2c48:      mov x10, #0x0               ; =0
1004c2c4c:      mov x9, x19
1004c2c50:      b   0x1004c2cc4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x908>
1004c2c54:      lsr x9, x9, #4
1004c2c58:      add x9, x9, #0x1
1004c2c5c:      ands    x10, x9, #0x7
1004c2c60:      mov w11, #0x8               ; =8
1004c2c64:      csel    x10, x11, x10, eq
1004c2c68:      sub x10, x9, x10
1004c2c6c:      add x9, x19, x10, lsl #4
1004c2c70:      add x11, x19, #0x48
1004c2c74:      add x12, x23, #0x20
1004c2c78:      mov x13, x10
1004c2c7c:      sub x14, x11, #0x30
1004c2c80:      ldur    q0, [x11, #-0x40]
1004c2c84:      ld1.d   { v0 }[1], [x14]
1004c2c88:      sub x14, x11, #0x10
1004c2c8c:      ldur    q1, [x11, #-0x20]
1004c2c90:      ld1.d   { v1 }[1], [x14]
1004c2c94:      add x14, x11, #0x10
1004c2c98:      ldr q2, [x11]
1004c2c9c:      ld1.d   { v2 }[1], [x14]
1004c2ca0:      add x14, x11, #0x30
1004c2ca4:      ldr q3, [x11, #0x20]
1004c2ca8:      ld1.d   { v3 }[1], [x14]
1004c2cac:      stp q0, q1, [x12, #-0x10]
1004c2cb0:      stp q2, q3, [x12, #0x10]
1004c2cb4:      add x11, x11, #0x80
1004c2cb8:      add x12, x12, #0x40
1004c2cbc:      subs    x13, x13, #0x8
1004c2cc0:      b.ne    0x1004c2c7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x8c0>
1004c2cc4:      add x8, x19, x8
1004c2cc8:      add x10, x23, x10, lsl #3
1004c2ccc:      add x10, x10, #0x10
1004c2cd0:      ldr x11, [x9, #0x8]
1004c2cd4:      add x9, x9, #0x10
1004c2cd8:      str x11, [x10], #0x8
1004c2cdc:      cmp x9, x8
1004c2ce0:      b.ne    0x1004c2cd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x914>
1004c2ce4:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004c2ce8:      add x0, x0, #0xb60
1004c2cec:      ldr x8, [x0]
1004c2cf0:      blr x8
1004c2cf4:      ldrb    w8, [x0, #0x38]
1004c2cf8:      cmp w8, #0x1
1004c2cfc:      b.ne    0x1004c2e6c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xab0>
1004c2d00:      ldr x8, [x0]
1004c2d04:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1004c2d08:      cmp x8, x9
1004c2d0c:      b.hs    0x1004c2e80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xac4>
1004c2d10:      ldr x8, [x0, #0x20]
1004c2d14:      cmp x8, #0x1, lsl #12       ; =0x1000
1004c2d18:      b.hi    0x1004c2e8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xad0>
1004c2d1c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2d20:      add x8, x8, #0x610
1004c2d24:      ldapr   x8, [x8]
1004c2d28:      cbnz    x8, 0x1004c2ea0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xae4>
1004c2d2c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2d30:      ldrb    w8, [x8, #0x618]
1004c2d34:      cbz w8, 0x1004c2d64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a8>
1004c2d38:      bl  0x1004e0b7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena4walk18arena_in_use_bytes>
1004c2d3c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2d40:      add x8, x8, #0xdb0
1004c2d44:      ldapr   x8, [x8]
1004c2d48:      cbnz    x8, 0x1004c2ed8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb1c>
1004c2d4c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2d50:      ldr x8, [x8, #0xdb8]
1004c2d54:      cmp x0, x8
1004c2d58:      b.lo    0x1004c2d64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a8>
1004c2d5c:      mov w8, #0x1                ; =1
1004c2d60:      strb    w8, [x21]
1004c2d64:      mov x19, #0x7ffd000000000000 ; =9222527611924643840
1004c2d68:      bfxil   x19, x23, #0, #48
1004c2d6c:      add x0, sp, #0x70
1004c2d70:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004c2d74:      mov w0, #0x1                ; =1
1004c2d78:      b   0x1004c2df0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa34>
1004c2d7c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c2d80:      add x0, x0, #0xda8
1004c2d84:      ldr x8, [x0]
1004c2d88:      blr x8
1004c2d8c:      ldr x8, [x0]
1004c2d90:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c2d94:      add x0, x0, #0x808
1004c2d98:      ldr x9, [x0]
1004c2d9c:      blr x9
1004c2da0:      ldr x9, [x0]
1004c2da4:      cmp x9, x8
1004c2da8:      b.ls    0x1004c2dc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa0c>
1004c2dac:      str x8, [x0]
1004c2db0:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c2db4:      add x0, x0, #0x778
1004c2db8:      ldr x8, [x0]
1004c2dbc:      blr x8
1004c2dc0:      mov w8, #0x1                ; =1
1004c2dc4:      strb    w8, [x0]
1004c2dc8:      bl  0x1004344c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc6policy16gc_check_trigger>
1004c2dcc:      b   0x1004c2a54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x698>
1004c2dd0:      sub x9, x10, #0x10
1004c2dd4:      ldr x10, [x9, x8]
1004c2dd8:      cmp x10, x20
1004c2ddc:      b.eq    0x1004c2f08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb4c>
1004c2de0:      subs    x8, x8, #0x20
1004c2de4:      b.ne    0x1004c2dd4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa18>
1004c2de8:      mov x0, #0x0                ; =0
1004c2dec:      str x28, [x21]
1004c2df0:      mov x1, x19
1004c2df4:      ldp x29, x30, [sp, #0xe0]
1004c2df8:      ldp x20, x19, [sp, #0xd0]
1004c2dfc:      ldp x22, x21, [sp, #0xc0]
1004c2e00:      ldp x24, x23, [sp, #0xb0]
1004c2e04:      ldp x26, x25, [sp, #0xa0]
1004c2e08:      ldp x28, x27, [sp, #0x90]
1004c2e0c:      add sp, sp, #0xf0
1004c2e10:      ret
1004c2e14:      cmp w8, #0x1
1004c2e18:      b.ne    0x1004c2e74 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xab8>
1004c2e1c:      adrp    x1, 0x10018b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs7wa3bwJW6LN_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x34>
1004c2e20:      add x1, x1, #0xa0
1004c2e24:      mov x0, x21
1004c2e28:      bl  0x1009bf05c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1004c2e2c:      strb    wzr, [x21, #0x20]
1004c2e30:      ldr x28, [x21]
1004c2e34:      mov x8, #0x7fffffffffffffff ; =9223372036854775807
1004c2e38:      cmp x28, x8
1004c2e3c:      b.lo    0x1004c240c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x50>
1004c2e40:      adrp    x0, 0x100e7d000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x7fc8>
1004c2e44:      add x0, x0, #0x3f8
1004c2e48:      bl  0x100ab131c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004c2e4c:      tbnz    w24, #0x8, 0x1004c2ed4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb18>
1004c2e50:      bl  0x100ae2074 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes24shape_id_exhausted_abort>
1004c2e54:      adrp    x0, 0x100c0a000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime10bun_compat12width_tables8EAW_WIDE+0x5ce0>
1004c2e58:      add x0, x0, #0xf74
1004c2e5c:      adrp    x2, 0x100e8d000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x1ef0>
1004c2e60:      add x2, x2, #0xfd0
1004c2e64:      mov w1, #0x75               ; =117
1004c2e68:      bl  0x100ab15a8 <__RNvNtCsjgY6bXVaRmE_4core9panicking5panic>
1004c2e6c:      bl  0x100abcbf0 <__RINvMs0_NtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native4lazyINtB6_7StorageINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBe_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs7wa3bwJW6LN_13perry_runtime6string12StringHeaderEEuE16get_or_init_slowNvNvNtB39_4json15PARSE_KEY_CACHE27___rust_std_internal_init_fnEB39_>
1004c2e70:      cbnz    x0, 0x1004c2d00 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x944>
1004c2e74:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
1004c2e78:      add x0, x0, #0x850
1004c2e7c:      bl  0x100aef71c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1004c2e80:      adrp    x0, 0x100e7d000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x7fc8>
1004c2e84:      add x0, x0, #0x3c8
1004c2e88:      bl  0x100ab131c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004c2e8c:      bl  0x100ade708 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json12parse_scalar15clear_key_cache>
1004c2e90:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2e94:      add x8, x8, #0x610
1004c2e98:      ldapr   x8, [x8]
1004c2e9c:      cbz x8, 0x1004c2d2c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x970>
1004c2ea0:      bl  0x100ab7ecc <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtCs7wa3bwJW6LN_13perry_runtime2gc14gen_gc_enabled0E0zEB1y_>
1004c2ea4:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2ea8:      ldrb    w8, [x8, #0x618]
1004c2eac:      cbnz    w8, 0x1004c2d38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x97c>
1004c2eb0:      b   0x1004c2d64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a8>
1004c2eb4:      adrp    x0, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004c2eb8:      add x0, x0, #0x8a8
1004c2ebc:      bl  0x100ab131c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004c2ec0:      bl  0x100ae2bc8 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1004c2ec4:      adrp    x0, 0x100bd9000 <_PERRY_EMPTY_STRING+0xbb0>
1004c2ec8:      add x0, x0, #0xac
1004c2ecc:      mov w1, #0xb                ; =11
1004c2ed0:      bl  0x100ae2b90 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1004c2ed4:      bl  0x100ae2090 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes25invalid_shape_facts_abort>
1004c2ed8:      mov x19, x0
1004c2edc:      bl  0x100ab8984 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockjE10initializeNCINvB2_11get_or_initNCNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc11heap_budget38gc_tiny_parse_in_use_trigger_dyn_bytes0E0zEB1A_>
1004c2ee0:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2ee4:      ldr x8, [x8, #0xdb8]
1004c2ee8:      cmp x19, x8
1004c2eec:      b.hs    0x1004c2d5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a0>
1004c2ef0:      b   0x1004c2d64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a8>
1004c2ef4:      adrp    x2, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004c2ef8:      add x2, x2, #0xe58
1004c2efc:      mov w0, #0x5                ; =5
1004c2f00:      mov w1, #0x5                ; =5
1004c2f04:      bl  0x100ab144c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1004c2f08:      adrp    x3, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004c2f0c:      add x3, x3, #0xe70
1004c2f10:      mov x0, #0x0                ; =0
1004c2f14:      mov x1, x20
1004c2f18:      mov w2, #0x8                ; =8
1004c2f1c:      bl  0x100ab160c <__RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail>
1004c2f20:      nop
1004c2f24:      nop
1004c2f28:      nop
1004c2f2c:      nop
1004c2f30:      nop
1004c2f34:      nop
1004c2f38:      nop
1004c2f3c:      nop
