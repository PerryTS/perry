/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/wide39-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004c25bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate>:
1004c25bc:      sub sp, sp, #0xf0
1004c25c0:      stp x28, x27, [sp, #0x90]
1004c25c4:      stp x26, x25, [sp, #0xa0]
1004c25c8:      stp x24, x23, [sp, #0xb0]
1004c25cc:      stp x22, x21, [sp, #0xc0]
1004c25d0:      stp x20, x19, [sp, #0xd0]
1004c25d4:      stp x29, x30, [sp, #0xe0]
1004c25d8:      add x29, sp, #0xe0
1004c25dc:      mov x19, x0
1004c25e0:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004c25e4:      add x0, x0, #0xba8
1004c25e8:      ldr x8, [x0]
1004c25ec:      blr x8
1004c25f0:      mov x21, x0
1004c25f4:      ldrb    w8, [x0, #0x20]
1004c25f8:      cbnz    w8, 0x1004c3014 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa58>
1004c25fc:      ldr x28, [x21]
1004c2600:      mov x8, #0x7fffffffffffffff ; =9223372036854775807
1004c2604:      cmp x28, x8
1004c2608:      b.hs    0x1004c3040 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa84>
1004c260c:      add x8, x28, #0x1
1004c2610:      str x8, [x21]
1004c2614:      ldr x9, [x21, #0x18]
1004c2618:      cbz x9, 0x1004c2fe8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa2c>
1004c261c:      ldr x10, [x21, #0x10]
1004c2620:      ldr x20, [x19, #0x80]
1004c2624:      lsl x8, x9, #5
1004c2628:      cmp x20, #0x9
1004c262c:      b.hs    0x1004c2fd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa14>
1004c2630:      ldr x27, [x19]
1004c2634:      ubfx    x22, x27, #40, #8
1004c2638:      ldr x11, [x19, #0x10]
1004c263c:      str x11, [sp, #0x68]
1004c2640:      ubfx    x23, x11, #40, #8
1004c2644:      ldr x11, [x19, #0x20]
1004c2648:      str x11, [sp, #0x58]
1004c264c:      ubfx    x11, x11, #40, #8
1004c2650:      str x11, [sp, #0x60]
1004c2654:      ldr x11, [x19, #0x30]
1004c2658:      str x11, [sp, #0x48]
1004c265c:      ubfx    x11, x11, #40, #8
1004c2660:      str x11, [sp, #0x50]
1004c2664:      ldr x11, [x19, #0x40]
1004c2668:      str x11, [sp, #0x38]
1004c266c:      ubfx    x11, x11, #40, #8
1004c2670:      str x11, [sp, #0x40]
1004c2674:      ldr x11, [x19, #0x50]
1004c2678:      str x11, [sp, #0x28]
1004c267c:      ubfx    x11, x11, #40, #8
1004c2680:      str x11, [sp, #0x30]
1004c2684:      ldr x11, [x19, #0x60]
1004c2688:      str x11, [sp, #0x18]
1004c268c:      ubfx    x11, x11, #40, #8
1004c2690:      str x11, [sp, #0x20]
1004c2694:      ldr x11, [x19, #0x70]
1004c2698:      str x11, [sp, #0x8]
1004c269c:      ubfx    x11, x11, #40, #8
1004c26a0:      str x11, [sp, #0x10]
1004c26a4:      add x8, x8, x10
1004c26a8:      sub x24, x8, #0x8
1004c26ac:      neg x25, x9, lsl #5
1004c26b0:      b   0x1004c26c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x104>
1004c26b4:      sub x24, x24, #0x20
1004c26b8:      adds    x25, x25, #0x20
1004c26bc:      b.eq    0x1004c2fe8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa2c>
1004c26c0:      ldur    x8, [x24, #-0x8]
1004c26c4:      cmp x8, x20
1004c26c8:      b.ne    0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c26cc:      sturb   wzr, [x29, #-0x64]
1004c26d0:      stur    wzr, [x29, #-0x68]
1004c26d4:      cbz x20, 0x1004c2bd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c26d8:      ldur    x26, [x24, #-0x10]
1004c26dc:      ldr x8, [x26]
1004c26e0:      cbz x22, 0x1004c2730 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c26e4:      sturb   w27, [x29, #-0x68]
1004c26e8:      cmp x22, #0x1
1004c26ec:      b.eq    0x1004c2730 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c26f0:      lsr x9, x27, #8
1004c26f4:      sturb   w9, [x29, #-0x67]
1004c26f8:      cmp x22, #0x2
1004c26fc:      b.eq    0x1004c2730 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c2700:      lsr x9, x27, #16
1004c2704:      sturb   w9, [x29, #-0x66]
1004c2708:      cmp x22, #0x3
1004c270c:      b.eq    0x1004c2730 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c2710:      lsr x9, x27, #24
1004c2714:      sturb   w9, [x29, #-0x65]
1004c2718:      cmp x22, #0x4
1004c271c:      b.eq    0x1004c2730 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c2720:      lsr x9, x27, #32
1004c2724:      sturb   w9, [x29, #-0x64]
1004c2728:      cmp x22, #0x5
1004c272c:      b.ne    0x1004c30f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c2730:      ldr w9, [x8, #0x4]
1004c2734:      cmp x22, x9
1004c2738:      b.ne    0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c273c:      add x0, x8, #0x14
1004c2740:      sub x1, x29, #0x68
1004c2744:      mov x2, x22
1004c2748:      bl  0x100af9160 <_writev+0x100af9160>
1004c274c:      cbnz    w0, 0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2750:      cmp x20, #0x1
1004c2754:      b.eq    0x1004c2bd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c2758:      ldr x8, [x26, #0x8]
1004c275c:      cbz x23, 0x1004c27c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c2760:      ldr x9, [sp, #0x68]
1004c2764:      sturb   w9, [x29, #-0x68]
1004c2768:      cmp x23, #0x1
1004c276c:      b.eq    0x1004c27c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c2770:      ldr x9, [sp, #0x68]
1004c2774:      lsr x9, x9, #8
1004c2778:      sturb   w9, [x29, #-0x67]
1004c277c:      cmp x23, #0x2
1004c2780:      b.eq    0x1004c27c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c2784:      ldr x9, [sp, #0x68]
1004c2788:      lsr x9, x9, #16
1004c278c:      sturb   w9, [x29, #-0x66]
1004c2790:      cmp x23, #0x3
1004c2794:      b.eq    0x1004c27c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c2798:      ldr x9, [sp, #0x68]
1004c279c:      lsr x9, x9, #24
1004c27a0:      sturb   w9, [x29, #-0x65]
1004c27a4:      cmp x23, #0x4
1004c27a8:      b.eq    0x1004c27c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c27ac:      ldr x9, [sp, #0x68]
1004c27b0:      lsr x9, x9, #32
1004c27b4:      sturb   w9, [x29, #-0x64]
1004c27b8:      cmp x23, #0x5
1004c27bc:      b.ne    0x1004c30f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c27c0:      ldr w9, [x8, #0x4]
1004c27c4:      cmp x23, x9
1004c27c8:      b.ne    0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c27cc:      add x0, x8, #0x14
1004c27d0:      sub x1, x29, #0x68
1004c27d4:      mov x2, x23
1004c27d8:      bl  0x100af9160 <_writev+0x100af9160>
1004c27dc:      cbnz    w0, 0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c27e0:      cmp x20, #0x2
1004c27e4:      b.eq    0x1004c2bd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c27e8:      ldr x8, [x26, #0x10]
1004c27ec:      ldr x9, [sp, #0x60]
1004c27f0:      cbz x9, 0x1004c2864 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c27f4:      ldp x10, x9, [sp, #0x58]
1004c27f8:      sturb   w10, [x29, #-0x68]
1004c27fc:      cmp x9, #0x1
1004c2800:      b.eq    0x1004c2864 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c2804:      ldr x9, [sp, #0x58]
1004c2808:      lsr x9, x9, #8
1004c280c:      sturb   w9, [x29, #-0x67]
1004c2810:      ldr x9, [sp, #0x60]
1004c2814:      cmp x9, #0x2
1004c2818:      b.eq    0x1004c2864 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c281c:      ldr x9, [sp, #0x58]
1004c2820:      lsr x9, x9, #16
1004c2824:      sturb   w9, [x29, #-0x66]
1004c2828:      ldr x9, [sp, #0x60]
1004c282c:      cmp x9, #0x3
1004c2830:      b.eq    0x1004c2864 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c2834:      ldr x9, [sp, #0x58]
1004c2838:      lsr x9, x9, #24
1004c283c:      sturb   w9, [x29, #-0x65]
1004c2840:      ldr x9, [sp, #0x60]
1004c2844:      cmp x9, #0x4
1004c2848:      b.eq    0x1004c2864 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c284c:      ldr x9, [sp, #0x58]
1004c2850:      lsr x9, x9, #32
1004c2854:      sturb   w9, [x29, #-0x64]
1004c2858:      ldr x9, [sp, #0x60]
1004c285c:      cmp x9, #0x5
1004c2860:      b.ne    0x1004c30f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c2864:      ldr w9, [x8, #0x4]
1004c2868:      ldr x10, [sp, #0x60]
1004c286c:      cmp x10, x9
1004c2870:      b.ne    0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2874:      add x0, x8, #0x14
1004c2878:      sub x1, x29, #0x68
1004c287c:      ldr x2, [sp, #0x60]
1004c2880:      bl  0x100af9160 <_writev+0x100af9160>
1004c2884:      cbnz    w0, 0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2888:      cmp x20, #0x3
1004c288c:      b.eq    0x1004c2bd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c2890:      ldr x8, [x26, #0x18]
1004c2894:      ldr x9, [sp, #0x50]
1004c2898:      cbz x9, 0x1004c290c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c289c:      ldp x10, x9, [sp, #0x48]
1004c28a0:      sturb   w10, [x29, #-0x68]
1004c28a4:      cmp x9, #0x1
1004c28a8:      b.eq    0x1004c290c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c28ac:      ldr x9, [sp, #0x48]
1004c28b0:      lsr x9, x9, #8
1004c28b4:      sturb   w9, [x29, #-0x67]
1004c28b8:      ldr x9, [sp, #0x50]
1004c28bc:      cmp x9, #0x2
1004c28c0:      b.eq    0x1004c290c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c28c4:      ldr x9, [sp, #0x48]
1004c28c8:      lsr x9, x9, #16
1004c28cc:      sturb   w9, [x29, #-0x66]
1004c28d0:      ldr x9, [sp, #0x50]
1004c28d4:      cmp x9, #0x3
1004c28d8:      b.eq    0x1004c290c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c28dc:      ldr x9, [sp, #0x48]
1004c28e0:      lsr x9, x9, #24
1004c28e4:      sturb   w9, [x29, #-0x65]
1004c28e8:      ldr x9, [sp, #0x50]
1004c28ec:      cmp x9, #0x4
1004c28f0:      b.eq    0x1004c290c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c28f4:      ldr x9, [sp, #0x48]
1004c28f8:      lsr x9, x9, #32
1004c28fc:      sturb   w9, [x29, #-0x64]
1004c2900:      ldr x9, [sp, #0x50]
1004c2904:      cmp x9, #0x5
1004c2908:      b.ne    0x1004c30f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c290c:      ldr w9, [x8, #0x4]
1004c2910:      ldr x10, [sp, #0x50]
1004c2914:      cmp x10, x9
1004c2918:      b.ne    0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c291c:      add x0, x8, #0x14
1004c2920:      sub x1, x29, #0x68
1004c2924:      ldr x2, [sp, #0x50]
1004c2928:      bl  0x100af9160 <_writev+0x100af9160>
1004c292c:      cbnz    w0, 0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2930:      cmp x20, #0x4
1004c2934:      b.eq    0x1004c2bd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c2938:      ldr x8, [x26, #0x20]
1004c293c:      ldr x9, [sp, #0x40]
1004c2940:      cbz x9, 0x1004c29b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c2944:      ldp x10, x9, [sp, #0x38]
1004c2948:      sturb   w10, [x29, #-0x68]
1004c294c:      cmp x9, #0x1
1004c2950:      b.eq    0x1004c29b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c2954:      ldr x9, [sp, #0x38]
1004c2958:      lsr x9, x9, #8
1004c295c:      sturb   w9, [x29, #-0x67]
1004c2960:      ldr x9, [sp, #0x40]
1004c2964:      cmp x9, #0x2
1004c2968:      b.eq    0x1004c29b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c296c:      ldr x9, [sp, #0x38]
1004c2970:      lsr x9, x9, #16
1004c2974:      sturb   w9, [x29, #-0x66]
1004c2978:      ldr x9, [sp, #0x40]
1004c297c:      cmp x9, #0x3
1004c2980:      b.eq    0x1004c29b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c2984:      ldr x9, [sp, #0x38]
1004c2988:      lsr x9, x9, #24
1004c298c:      sturb   w9, [x29, #-0x65]
1004c2990:      ldr x9, [sp, #0x40]
1004c2994:      cmp x9, #0x4
1004c2998:      b.eq    0x1004c29b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c299c:      ldr x9, [sp, #0x38]
1004c29a0:      lsr x9, x9, #32
1004c29a4:      sturb   w9, [x29, #-0x64]
1004c29a8:      ldr x9, [sp, #0x40]
1004c29ac:      cmp x9, #0x5
1004c29b0:      b.ne    0x1004c30f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c29b4:      ldr w9, [x8, #0x4]
1004c29b8:      ldr x10, [sp, #0x40]
1004c29bc:      cmp x10, x9
1004c29c0:      b.ne    0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c29c4:      add x0, x8, #0x14
1004c29c8:      sub x1, x29, #0x68
1004c29cc:      ldr x2, [sp, #0x40]
1004c29d0:      bl  0x100af9160 <_writev+0x100af9160>
1004c29d4:      cbnz    w0, 0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c29d8:      cmp x20, #0x5
1004c29dc:      b.eq    0x1004c2bd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c29e0:      ldr x8, [x26, #0x28]
1004c29e4:      ldr x9, [sp, #0x30]
1004c29e8:      cbz x9, 0x1004c2a5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c29ec:      ldp x10, x9, [sp, #0x28]
1004c29f0:      sturb   w10, [x29, #-0x68]
1004c29f4:      cmp x9, #0x1
1004c29f8:      b.eq    0x1004c2a5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c29fc:      ldr x9, [sp, #0x28]
1004c2a00:      lsr x9, x9, #8
1004c2a04:      sturb   w9, [x29, #-0x67]
1004c2a08:      ldr x9, [sp, #0x30]
1004c2a0c:      cmp x9, #0x2
1004c2a10:      b.eq    0x1004c2a5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c2a14:      ldr x9, [sp, #0x28]
1004c2a18:      lsr x9, x9, #16
1004c2a1c:      sturb   w9, [x29, #-0x66]
1004c2a20:      ldr x9, [sp, #0x30]
1004c2a24:      cmp x9, #0x3
1004c2a28:      b.eq    0x1004c2a5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c2a2c:      ldr x9, [sp, #0x28]
1004c2a30:      lsr x9, x9, #24
1004c2a34:      sturb   w9, [x29, #-0x65]
1004c2a38:      ldr x9, [sp, #0x30]
1004c2a3c:      cmp x9, #0x4
1004c2a40:      b.eq    0x1004c2a5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c2a44:      ldr x9, [sp, #0x28]
1004c2a48:      lsr x9, x9, #32
1004c2a4c:      sturb   w9, [x29, #-0x64]
1004c2a50:      ldr x9, [sp, #0x30]
1004c2a54:      cmp x9, #0x5
1004c2a58:      b.ne    0x1004c30f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c2a5c:      ldr w9, [x8, #0x4]
1004c2a60:      ldr x10, [sp, #0x30]
1004c2a64:      cmp x10, x9
1004c2a68:      b.ne    0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2a6c:      add x0, x8, #0x14
1004c2a70:      sub x1, x29, #0x68
1004c2a74:      ldr x2, [sp, #0x30]
1004c2a78:      bl  0x100af9160 <_writev+0x100af9160>
1004c2a7c:      cbnz    w0, 0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2a80:      cmp x20, #0x6
1004c2a84:      b.eq    0x1004c2bd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c2a88:      ldr x8, [x26, #0x30]
1004c2a8c:      ldr x9, [sp, #0x20]
1004c2a90:      cbz x9, 0x1004c2b04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c2a94:      ldp x10, x9, [sp, #0x18]
1004c2a98:      sturb   w10, [x29, #-0x68]
1004c2a9c:      cmp x9, #0x1
1004c2aa0:      b.eq    0x1004c2b04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c2aa4:      ldr x9, [sp, #0x18]
1004c2aa8:      lsr x9, x9, #8
1004c2aac:      sturb   w9, [x29, #-0x67]
1004c2ab0:      ldr x9, [sp, #0x20]
1004c2ab4:      cmp x9, #0x2
1004c2ab8:      b.eq    0x1004c2b04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c2abc:      ldr x9, [sp, #0x18]
1004c2ac0:      lsr x9, x9, #16
1004c2ac4:      sturb   w9, [x29, #-0x66]
1004c2ac8:      ldr x9, [sp, #0x20]
1004c2acc:      cmp x9, #0x3
1004c2ad0:      b.eq    0x1004c2b04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c2ad4:      ldr x9, [sp, #0x18]
1004c2ad8:      lsr x9, x9, #24
1004c2adc:      sturb   w9, [x29, #-0x65]
1004c2ae0:      ldr x9, [sp, #0x20]
1004c2ae4:      cmp x9, #0x4
1004c2ae8:      b.eq    0x1004c2b04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c2aec:      ldr x9, [sp, #0x18]
1004c2af0:      lsr x9, x9, #32
1004c2af4:      sturb   w9, [x29, #-0x64]
1004c2af8:      ldr x9, [sp, #0x20]
1004c2afc:      cmp x9, #0x5
1004c2b00:      b.ne    0x1004c30f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c2b04:      ldr w9, [x8, #0x4]
1004c2b08:      ldr x10, [sp, #0x20]
1004c2b0c:      cmp x10, x9
1004c2b10:      b.ne    0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2b14:      add x0, x8, #0x14
1004c2b18:      sub x1, x29, #0x68
1004c2b1c:      ldr x2, [sp, #0x20]
1004c2b20:      bl  0x100af9160 <_writev+0x100af9160>
1004c2b24:      cbnz    w0, 0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2b28:      cmp x20, #0x7
1004c2b2c:      b.eq    0x1004c2bd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c2b30:      ldr x8, [x26, #0x38]
1004c2b34:      ldr x9, [sp, #0x10]
1004c2b38:      cbz x9, 0x1004c2bac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c2b3c:      ldp x10, x9, [sp, #0x8]
1004c2b40:      sturb   w10, [x29, #-0x68]
1004c2b44:      cmp x9, #0x1
1004c2b48:      b.eq    0x1004c2bac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c2b4c:      ldr x9, [sp, #0x8]
1004c2b50:      lsr x9, x9, #8
1004c2b54:      sturb   w9, [x29, #-0x67]
1004c2b58:      ldr x9, [sp, #0x10]
1004c2b5c:      cmp x9, #0x2
1004c2b60:      b.eq    0x1004c2bac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c2b64:      ldr x9, [sp, #0x8]
1004c2b68:      lsr x9, x9, #16
1004c2b6c:      sturb   w9, [x29, #-0x66]
1004c2b70:      ldr x9, [sp, #0x10]
1004c2b74:      cmp x9, #0x3
1004c2b78:      b.eq    0x1004c2bac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c2b7c:      ldr x9, [sp, #0x8]
1004c2b80:      lsr x9, x9, #24
1004c2b84:      sturb   w9, [x29, #-0x65]
1004c2b88:      ldr x9, [sp, #0x10]
1004c2b8c:      cmp x9, #0x4
1004c2b90:      b.eq    0x1004c2bac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c2b94:      ldr x9, [sp, #0x8]
1004c2b98:      lsr x9, x9, #32
1004c2b9c:      sturb   w9, [x29, #-0x64]
1004c2ba0:      ldr x9, [sp, #0x10]
1004c2ba4:      cmp x9, #0x5
1004c2ba8:      b.ne    0x1004c30f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c2bac:      ldr w9, [x8, #0x4]
1004c2bb0:      ldr x10, [sp, #0x10]
1004c2bb4:      cmp x10, x9
1004c2bb8:      b.ne    0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2bbc:      add x0, x8, #0x14
1004c2bc0:      sub x1, x29, #0x68
1004c2bc4:      ldr x2, [sp, #0x10]
1004c2bc8:      bl  0x100af9160 <_writev+0x100af9160>
1004c2bcc:      cbnz    w0, 0x1004c26b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2bd0:      ldr x22, [x24]
1004c2bd4:      str x28, [x21]
1004c2bd8:      bl  0x10026a3d0 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
1004c2bdc:      str x0, [sp, #0x70]
1004c2be0:      mov x23, #0x7ffd000000000000 ; =9222527611924643840
1004c2be4:      stp x22, x23, [x29, #-0x60]
1004c2be8:      mov w8, #0x1                ; =1
1004c2bec:      stur    x8, [x29, #-0x68]
1004c2bf0:      sub x0, x29, #0x68
1004c2bf4:      bl  0x10026a4d8 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1004c2bf8:      mov x22, x0
1004c2bfc:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c2c00:      add x0, x0, #0x9d0
1004c2c04:      ldr x8, [x0]
1004c2c08:      blr x8
1004c2c0c:      mov x21, x0
1004c2c10:      ldrb    w8, [x0]
1004c2c14:      strb    wzr, [x0]
1004c2c18:      cbz w8, 0x1004c2c54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x698>
1004c2c1c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c2c20:      add x0, x0, #0x9e8
1004c2c24:      ldr x8, [x0]
1004c2c28:      blr x8
1004c2c2c:      ldrb    w8, [x0]
1004c2c30:      tst w8, #0x3
1004c2c34:      b.ne    0x1004c2c4c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x690>
1004c2c38:      adrp    x8, 0x100fd9000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfeec>
1004c2c3c:      add x8, x8, #0x8fc
1004c2c40:      ldapr   w8, [x8]
1004c2c44:      cmp w8, #0x0
1004c2c48:      b.le    0x1004c2f7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9c0>
1004c2c4c:      mov w8, #0x1                ; =1
1004c2c50:      strb    w8, [x21]
1004c2c54:      mov w0, #0x0                ; =0
1004c2c58:      mov w1, #0x0                ; =0
1004c2c5c:      mov x2, x20
1004c2c60:      bl  0x10081d7c0 <_js_object_alloc_with_parent>
1004c2c64:      stp x0, x23, [x29, #-0x60]
1004c2c68:      mov w8, #0x1                ; =1
1004c2c6c:      stur    x8, [x29, #-0x68]
1004c2c70:      sub x0, x29, #0x68
1004c2c74:      bl  0x10026a4d8 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1004c2c78:      mov x23, x0
1004c2c7c:      adrp    x25, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2c80:      ldr x8, [x25, #0x48]
1004c2c84:      cmn x8, #0x1
1004c2c88:      b.eq    0x1004c2cec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x730>
1004c2c8c:      mrs x9, TPIDRRO_EL0
1004c2c90:      and x9, x9, #0xfffffffffffffff8
1004c2c94:      ldr x8, [x9, x8, lsl #3]
1004c2c98:      cbz x8, 0x1004c2cec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x730>
1004c2c9c:      ldr x8, [x8, #0x19e8]
1004c2ca0:      cbz x8, 0x1004c2cec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x730>
1004c2ca4:      ldr x9, [x8]
1004c2ca8:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004c2cac:      cmp x9, x10
1004c2cb0:      b.hs    0x1004c30b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xaf8>
1004c2cb4:      add x10, x9, #0x1
1004c2cb8:      str x10, [x8]
1004c2cbc:      ldr x10, [x8, #0x18]
1004c2cc0:      cmp x22, x10
1004c2cc4:      b.hs    0x1004c30c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb04>
1004c2cc8:      ldr x10, [x8, #0x10]
1004c2ccc:      mov w11, #0x18              ; =24
1004c2cd0:      madd    x10, x22, x11, x10
1004c2cd4:      ldr x11, [x10]
1004c2cd8:      cmp x11, #0x1
1004c2cdc:      b.ne    0x1004c30c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb08>
1004c2ce0:      ldr x0, [x10, #0x8]
1004c2ce4:      str x9, [x8]
1004c2ce8:      b   0x1004c2cf4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x738>
1004c2cec:      mov x0, x22
1004c2cf0:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004c2cf4:      mov x1, x20
1004c2cf8:      mov x2, x20
1004c2cfc:      mov x3, #0x0                ; =0
1004c2d00:      mov w4, #0x0                ; =0
1004c2d04:      mov w5, #0x0                ; =0
1004c2d08:      bl  0x10059bc00 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes34shape_descriptor_ensure_with_holes>
1004c2d0c:      mov x24, x0
1004c2d10:      tbnz    w24, #0x0, 0x1004c304c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa90>
1004c2d14:      ldr x8, [x25, #0x48]
1004c2d18:      cmn x8, #0x1
1004c2d1c:      b.eq    0x1004c2d8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x7d0>
1004c2d20:      mrs x9, TPIDRRO_EL0
1004c2d24:      and x9, x9, #0xfffffffffffffff8
1004c2d28:      ldr x8, [x9, x8, lsl #3]
1004c2d2c:      cbz x8, 0x1004c2d8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x7d0>
1004c2d30:      ldr x8, [x8, #0x19e8]
1004c2d34:      cbz x8, 0x1004c2d8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x7d0>
1004c2d38:      ldr x9, [x8]
1004c2d3c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004c2d40:      cmp x9, x10
1004c2d44:      b.hs    0x1004c30b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xaf8>
1004c2d48:      add x10, x9, #0x1
1004c2d4c:      str x10, [x8]
1004c2d50:      ldr x10, [x8, #0x18]
1004c2d54:      cmp x23, x10
1004c2d58:      b.hs    0x1004c30c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb04>
1004c2d5c:      ldr x10, [x8, #0x10]
1004c2d60:      mov w11, #0x18              ; =24
1004c2d64:      madd    x10, x23, x11, x10
1004c2d68:      ldr x11, [x10]
1004c2d6c:      cmp x11, #0x1
1004c2d70:      b.ne    0x1004c30c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb08>
1004c2d74:      ldr x23, [x10, #0x8]
1004c2d78:      str x9, [x8]
1004c2d7c:      ldr x8, [x25, #0x48]
1004c2d80:      cmn x8, #0x1
1004c2d84:      b.ne    0x1004c2da4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x7e8>
1004c2d88:      b   0x1004c2e04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x848>
1004c2d8c:      mov x0, x23
1004c2d90:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004c2d94:      mov x23, x0
1004c2d98:      ldr x8, [x25, #0x48]
1004c2d9c:      cmn x8, #0x1
1004c2da0:      b.eq    0x1004c2e04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x848>
1004c2da4:      mrs x9, TPIDRRO_EL0
1004c2da8:      and x9, x9, #0xfffffffffffffff8
1004c2dac:      ldr x8, [x9, x8, lsl #3]
1004c2db0:      cbz x8, 0x1004c2e04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x848>
1004c2db4:      ldr x8, [x8, #0x19e8]
1004c2db8:      cbz x8, 0x1004c2e04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x848>
1004c2dbc:      ldr x9, [x8]
1004c2dc0:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004c2dc4:      cmp x9, x10
1004c2dc8:      b.hs    0x1004c30b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xaf8>
1004c2dcc:      add x10, x9, #0x1
1004c2dd0:      str x10, [x8]
1004c2dd4:      ldr x10, [x8, #0x18]
1004c2dd8:      cmp x22, x10
1004c2ddc:      b.hs    0x1004c30c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb04>
1004c2de0:      ldr x10, [x8, #0x10]
1004c2de4:      mov w11, #0x18              ; =24
1004c2de8:      madd    x10, x22, x11, x10
1004c2dec:      ldr x11, [x10]
1004c2df0:      cmp x11, #0x1
1004c2df4:      b.ne    0x1004c30c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb08>
1004c2df8:      ldr x2, [x10, #0x8]
1004c2dfc:      str x9, [x8]
1004c2e00:      b   0x1004c2e10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x854>
1004c2e04:      mov x0, x22
1004c2e08:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004c2e0c:      mov x2, x0
1004c2e10:      lsr x1, x24, #32
1004c2e14:      mov x0, x23
1004c2e18:      mov x3, x20
1004c2e1c:      bl  0x10059c13c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes34try_birth_stamp_preinstalled_shape>
1004c2e20:      tbz w0, #0x0, 0x1004c3054 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa98>
1004c2e24:      cbz x23, 0x1004c2e34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x878>
1004c2e28:      ldurh   w8, [x23, #-0x6]
1004c2e2c:      orr w8, w8, #0x200
1004c2e30:      sturh   w8, [x23, #-0x6]
1004c2e34:      cbz x20, 0x1004c2ee4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x928>
1004c2e38:      lsl x8, x20, #4
1004c2e3c:      sub x9, x8, #0x10
1004c2e40:      cmp x9, #0x80
1004c2e44:      b.hs    0x1004c2e54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x898>
1004c2e48:      mov x10, #0x0               ; =0
1004c2e4c:      mov x9, x19
1004c2e50:      b   0x1004c2ec4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x908>
1004c2e54:      lsr x9, x9, #4
1004c2e58:      add x9, x9, #0x1
1004c2e5c:      ands    x10, x9, #0x7
1004c2e60:      mov w11, #0x8               ; =8
1004c2e64:      csel    x10, x11, x10, eq
1004c2e68:      sub x10, x9, x10
1004c2e6c:      add x9, x19, x10, lsl #4
1004c2e70:      add x11, x19, #0x48
1004c2e74:      add x12, x23, #0x20
1004c2e78:      mov x13, x10
1004c2e7c:      sub x14, x11, #0x30
1004c2e80:      ldur    q0, [x11, #-0x40]
1004c2e84:      ld1.d   { v0 }[1], [x14]
1004c2e88:      sub x14, x11, #0x10
1004c2e8c:      ldur    q1, [x11, #-0x20]
1004c2e90:      ld1.d   { v1 }[1], [x14]
1004c2e94:      add x14, x11, #0x10
1004c2e98:      ldr q2, [x11]
1004c2e9c:      ld1.d   { v2 }[1], [x14]
1004c2ea0:      add x14, x11, #0x30
1004c2ea4:      ldr q3, [x11, #0x20]
1004c2ea8:      ld1.d   { v3 }[1], [x14]
1004c2eac:      stp q0, q1, [x12, #-0x10]
1004c2eb0:      stp q2, q3, [x12, #0x10]
1004c2eb4:      add x11, x11, #0x80
1004c2eb8:      add x12, x12, #0x40
1004c2ebc:      subs    x13, x13, #0x8
1004c2ec0:      b.ne    0x1004c2e7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x8c0>
1004c2ec4:      add x8, x19, x8
1004c2ec8:      add x10, x23, x10, lsl #3
1004c2ecc:      add x10, x10, #0x10
1004c2ed0:      ldr x11, [x9, #0x8]
1004c2ed4:      add x9, x9, #0x10
1004c2ed8:      str x11, [x10], #0x8
1004c2edc:      cmp x9, x8
1004c2ee0:      b.ne    0x1004c2ed0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x914>
1004c2ee4:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004c2ee8:      add x0, x0, #0xb60
1004c2eec:      ldr x8, [x0]
1004c2ef0:      blr x8
1004c2ef4:      ldrb    w8, [x0, #0x38]
1004c2ef8:      cmp w8, #0x1
1004c2efc:      b.ne    0x1004c306c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xab0>
1004c2f00:      ldr x8, [x0]
1004c2f04:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1004c2f08:      cmp x8, x9
1004c2f0c:      b.hs    0x1004c3080 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xac4>
1004c2f10:      ldr x8, [x0, #0x20]
1004c2f14:      cmp x8, #0x1, lsl #12       ; =0x1000
1004c2f18:      b.hi    0x1004c308c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xad0>
1004c2f1c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2f20:      add x8, x8, #0x610
1004c2f24:      ldapr   x8, [x8]
1004c2f28:      cbnz    x8, 0x1004c30a0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xae4>
1004c2f2c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2f30:      ldrb    w8, [x8, #0x618]
1004c2f34:      cbz w8, 0x1004c2f64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a8>
1004c2f38:      bl  0x1004e0d7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena4walk18arena_in_use_bytes>
1004c2f3c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2f40:      add x8, x8, #0xdb0
1004c2f44:      ldapr   x8, [x8]
1004c2f48:      cbnz    x8, 0x1004c30d8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb1c>
1004c2f4c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2f50:      ldr x8, [x8, #0xdb8]
1004c2f54:      cmp x0, x8
1004c2f58:      b.lo    0x1004c2f64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a8>
1004c2f5c:      mov w8, #0x1                ; =1
1004c2f60:      strb    w8, [x21]
1004c2f64:      mov x19, #0x7ffd000000000000 ; =9222527611924643840
1004c2f68:      bfxil   x19, x23, #0, #48
1004c2f6c:      add x0, sp, #0x70
1004c2f70:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004c2f74:      mov w0, #0x1                ; =1
1004c2f78:      b   0x1004c2ff0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa34>
1004c2f7c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c2f80:      add x0, x0, #0xda8
1004c2f84:      ldr x8, [x0]
1004c2f88:      blr x8
1004c2f8c:      ldr x8, [x0]
1004c2f90:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c2f94:      add x0, x0, #0x808
1004c2f98:      ldr x9, [x0]
1004c2f9c:      blr x9
1004c2fa0:      ldr x9, [x0]
1004c2fa4:      cmp x9, x8
1004c2fa8:      b.ls    0x1004c2fc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa0c>
1004c2fac:      str x8, [x0]
1004c2fb0:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c2fb4:      add x0, x0, #0x778
1004c2fb8:      ldr x8, [x0]
1004c2fbc:      blr x8
1004c2fc0:      mov w8, #0x1                ; =1
1004c2fc4:      strb    w8, [x0]
1004c2fc8:      bl  0x1004346c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc6policy16gc_check_trigger>
1004c2fcc:      b   0x1004c2c54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x698>
1004c2fd0:      sub x9, x10, #0x10
1004c2fd4:      ldr x10, [x9, x8]
1004c2fd8:      cmp x10, x20
1004c2fdc:      b.eq    0x1004c3108 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb4c>
1004c2fe0:      subs    x8, x8, #0x20
1004c2fe4:      b.ne    0x1004c2fd4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa18>
1004c2fe8:      mov x0, #0x0                ; =0
1004c2fec:      str x28, [x21]
1004c2ff0:      mov x1, x19
1004c2ff4:      ldp x29, x30, [sp, #0xe0]
1004c2ff8:      ldp x20, x19, [sp, #0xd0]
1004c2ffc:      ldp x22, x21, [sp, #0xc0]
1004c3000:      ldp x24, x23, [sp, #0xb0]
1004c3004:      ldp x26, x25, [sp, #0xa0]
1004c3008:      ldp x28, x27, [sp, #0x90]
1004c300c:      add sp, sp, #0xf0
1004c3010:      ret
1004c3014:      cmp w8, #0x1
1004c3018:      b.ne    0x1004c3074 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xab8>
1004c301c:      adrp    x1, 0x10018b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs7wa3bwJW6LN_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x34>
1004c3020:      add x1, x1, #0xa0
1004c3024:      mov x0, x21
1004c3028:      bl  0x1009bf25c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1004c302c:      strb    wzr, [x21, #0x20]
1004c3030:      ldr x28, [x21]
1004c3034:      mov x8, #0x7fffffffffffffff ; =9223372036854775807
1004c3038:      cmp x28, x8
1004c303c:      b.lo    0x1004c260c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x50>
1004c3040:      adrp    x0, 0x100e7d000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x7fc8>
1004c3044:      add x0, x0, #0x428
1004c3048:      bl  0x100ab151c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004c304c:      tbnz    w24, #0x8, 0x1004c30d4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb18>
1004c3050:      bl  0x100ae2274 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes24shape_id_exhausted_abort>
1004c3054:      adrp    x0, 0x100c0b000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime10bun_compat12width_tables8EAW_WIDE+0x6ae0>
1004c3058:      add x0, x0, #0x174
1004c305c:      adrp    x2, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ec0>
1004c3060:      add x2, x2, #0x0
1004c3064:      mov w1, #0x75               ; =117
1004c3068:      bl  0x100ab17a8 <__RNvNtCsjgY6bXVaRmE_4core9panicking5panic>
1004c306c:      bl  0x100abcdf0 <__RINvMs0_NtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native4lazyINtB6_7StorageINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBe_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs7wa3bwJW6LN_13perry_runtime6string12StringHeaderEEuE16get_or_init_slowNvNvNtB39_4json15PARSE_KEY_CACHE27___rust_std_internal_init_fnEB39_>
1004c3070:      cbnz    x0, 0x1004c2f00 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x944>
1004c3074:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
1004c3078:      add x0, x0, #0x850
1004c307c:      bl  0x100aef91c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1004c3080:      adrp    x0, 0x100e7d000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x7fc8>
1004c3084:      add x0, x0, #0x3f8
1004c3088:      bl  0x100ab151c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004c308c:      bl  0x100ade908 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json12parse_scalar15clear_key_cache>
1004c3090:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3094:      add x8, x8, #0x610
1004c3098:      ldapr   x8, [x8]
1004c309c:      cbz x8, 0x1004c2f2c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x970>
1004c30a0:      bl  0x100ab80cc <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtCs7wa3bwJW6LN_13perry_runtime2gc14gen_gc_enabled0E0zEB1y_>
1004c30a4:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c30a8:      ldrb    w8, [x8, #0x618]
1004c30ac:      cbnz    w8, 0x1004c2f38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x97c>
1004c30b0:      b   0x1004c2f64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a8>
1004c30b4:      adrp    x0, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004c30b8:      add x0, x0, #0x8a8
1004c30bc:      bl  0x100ab151c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004c30c0:      bl  0x100ae2dc8 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1004c30c4:      adrp    x0, 0x100bd9000 <_PERRY_EMPTY_STRING+0x9b0>
1004c30c8:      add x0, x0, #0x2ac
1004c30cc:      mov w1, #0xb                ; =11
1004c30d0:      bl  0x100ae2d90 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1004c30d4:      bl  0x100ae2290 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes25invalid_shape_facts_abort>
1004c30d8:      mov x19, x0
1004c30dc:      bl  0x100ab8b84 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockjE10initializeNCINvB2_11get_or_initNCNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc11heap_budget38gc_tiny_parse_in_use_trigger_dyn_bytes0E0zEB1A_>
1004c30e0:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c30e4:      ldr x8, [x8, #0xdb8]
1004c30e8:      cmp x19, x8
1004c30ec:      b.hs    0x1004c2f5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a0>
1004c30f0:      b   0x1004c2f64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a8>
1004c30f4:      adrp    x2, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004c30f8:      add x2, x2, #0xe58
1004c30fc:      mov w0, #0x5                ; =5
1004c3100:      mov w1, #0x5                ; =5
1004c3104:      bl  0x100ab164c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1004c3108:      adrp    x3, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004c310c:      add x3, x3, #0xe70
1004c3110:      mov x0, #0x0                ; =0
1004c3114:      mov x1, x20
1004c3118:      mov w2, #0x8                ; =8
1004c311c:      bl  0x100ab180c <__RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail>
1004c3120:      nop
1004c3124:      nop
1004c3128:      nop
1004c312c:      nop
1004c3130:      nop
1004c3134:      nop
1004c3138:      nop
1004c313c:      nop
