/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/scalar31-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010081d380 <_js_object_alloc_with_shape>:
10081d380:      sub sp, sp, #0xb0
10081d384:      stp x28, x27, [sp, #0x50]
10081d388:      stp x26, x25, [sp, #0x60]
10081d38c:      stp x24, x23, [sp, #0x70]
10081d390:      stp x22, x21, [sp, #0x80]
10081d394:      stp x20, x19, [sp, #0x90]
10081d398:      stp x29, x30, [sp, #0xa0]
10081d39c:      add x29, sp, #0xa0
10081d3a0:      mov x23, x3
10081d3a4:      mov x22, x2
10081d3a8:      mov x19, x1
10081d3ac:      mov x20, x0
10081d3b0:      mov w8, #0x2                ; =2
10081d3b4:      cmp w1, #0x2
10081d3b8:      csel    w26, w1, w8, hi
10081d3bc:      ubfiz   x8, x26, #3, #32
10081d3c0:      mov w9, #0x3ffd             ; =16381
10081d3c4:      adrp    x27, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081d3c8:      cmp w1, w9
10081d3cc:      b.ls    0x10081d3f4 <_js_object_alloc_with_shape+0x74>
10081d3d0:      add x0, x8, #0x10
10081d3d4:      mov w1, #0x8                ; =8
10081d3d8:      mov w2, #0x2                ; =2
10081d3dc:      bl  0x1004d9f18 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators18arena_alloc_gc_old>
10081d3e0:      mov x24, x0
10081d3e4:      ldurb   w8, [x0, #-0x7]
10081d3e8:      orr w8, w8, #0x20
10081d3ec:      sturb   w8, [x0, #-0x7]
10081d3f0:      b   0x10081d560 <_js_object_alloc_with_shape+0x1e0>
10081d3f4:      add x21, x8, #0x18
10081d3f8:      ldr x8, [x27, #0x48]
10081d3fc:      cmn x8, #0x1
10081d400:      b.eq    0x10081df58 <_js_object_alloc_with_shape+0xbd8>
10081d404:      mrs x9, TPIDRRO_EL0
10081d408:      and x9, x9, #0xfffffffffffffff8
10081d40c:      ldr x0, [x9, x8, lsl #3]
10081d410:      cbz x0, 0x10081df58 <_js_object_alloc_with_shape+0xbd8>
10081d414:      ldr x8, [x0, #0x28]
10081d418:      ldrb    w8, [x8]
10081d41c:      tbz w8, #0x0, 0x10081d488 <_js_object_alloc_with_shape+0x108>
10081d420:      ldr x8, [x27, #0x48]
10081d424:      cmn x8, #0x1
10081d428:      b.eq    0x10081e0e8 <_js_object_alloc_with_shape+0xd68>
10081d42c:      mrs x9, TPIDRRO_EL0
10081d430:      and x9, x9, #0xfffffffffffffff8
10081d434:      ldr x0, [x9, x8, lsl #3]
10081d438:      cbz x0, 0x10081e0e8 <_js_object_alloc_with_shape+0xd68>
10081d43c:      ldr x25, [x0, #0x20]
10081d440:      ldr x8, [x25]
10081d444:      cbnz    x8, 0x10081e0f8 <_js_object_alloc_with_shape+0xd78>
10081d448:      mov x8, #-0x1               ; =-1
10081d44c:      str x8, [x25]
10081d450:      ldr x1, [x25, #0x18]
10081d454:      cbz x1, 0x10081d484 <_js_object_alloc_with_shape+0x104>
10081d458:      mov x0, #0x0                ; =0
10081d45c:      ldr x8, [x25, #0x10]
10081d460:      lsl x10, x1, #4
10081d464:      mov x9, x8
10081d468:      ldr x11, [x9, #0x8]
10081d46c:      cmp x11, x21
10081d470:      b.eq    0x10081d660 <_js_object_alloc_with_shape+0x2e0>
10081d474:      add x0, x0, #0x1
10081d478:      add x9, x9, #0x10
10081d47c:      subs    x10, x10, #0x10
10081d480:      b.ne    0x10081d468 <_js_object_alloc_with_shape+0xe8>
10081d484:      str xzr, [x25]
10081d488:      mov x0, x21
10081d48c:      bl  0x1004d9b5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators11arena_alloc>
10081d490:      mov w8, #0x2                ; =2
10081d494:      strb    w8, [x0]
10081d498:      ldr x8, [x27, #0x48]
10081d49c:      cmn x8, #0x1
10081d4a0:      b.eq    0x10081df6c <_js_object_alloc_with_shape+0xbec>
10081d4a4:      mrs x9, TPIDRRO_EL0
10081d4a8:      and x9, x9, #0xfffffffffffffff8
10081d4ac:      ldr x8, [x9, x8, lsl #3]
10081d4b0:      cbz x8, 0x10081df6c <_js_object_alloc_with_shape+0xbec>
10081d4b4:      ldr x8, [x8, #0x30]
10081d4b8:      ldrb    w8, [x8]
10081d4bc:      orr w8, w8, #0x2
10081d4c0:      strb    w8, [x0, #0x1]
10081d4c4:      ldr x8, [x27, #0x48]
10081d4c8:      cmn x8, #0x1
10081d4cc:      b.eq    0x10081df80 <_js_object_alloc_with_shape+0xc00>
10081d4d0:      mrs x9, TPIDRRO_EL0
10081d4d4:      and x9, x9, #0xfffffffffffffff8
10081d4d8:      ldr x8, [x9, x8, lsl #3]
10081d4dc:      cbz x8, 0x10081df80 <_js_object_alloc_with_shape+0xc00>
10081d4e0:      ldr x8, [x8, #0x30]
10081d4e4:      ldrb    w8, [x8]
10081d4e8:      tbz w8, #0x0, 0x10081d554 <_js_object_alloc_with_shape+0x1d4>
10081d4ec:      ldrb    w8, [x0]
10081d4f0:      sub w9, w8, #0x15
10081d4f4:      cmn w9, #0x14
10081d4f8:      b.lo    0x10081d554 <_js_object_alloc_with_shape+0x1d4>
10081d4fc:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081d500:      add x9, x9, #0xb10
10081d504:      add x8, x9, x8, lsl #5
10081d508:      ldrb    w8, [x8, #0x1b]
10081d50c:      tbnz    w8, #0x0, 0x10081d554 <_js_object_alloc_with_shape+0x1d4>
10081d510:      mov x25, x0
10081d514:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081d518:      add x0, x0, #0x658
10081d51c:      ldr x8, [x0]
10081d520:      blr x8
10081d524:      mov x24, x0
10081d528:      ldrb    w8, [x0, #0x18]
10081d52c:      cbnz    w8, 0x10081e228 <_js_object_alloc_with_shape+0xea8>
10081d530:      ldr x28, [x24, #0x10]
10081d534:      ldr x8, [x24]
10081d538:      cmp x28, x8
10081d53c:      mov x0, x25
10081d540:      b.eq    0x10081e258 <_js_object_alloc_with_shape+0xed8>
10081d544:      ldr x8, [x24, #0x8]
10081d548:      str x0, [x8, x28, lsl #3]
10081d54c:      add x8, x28, #0x1
10081d550:      str x8, [x24, #0x10]
10081d554:      strh    wzr, [x0, #0x2]
10081d558:      str w21, [x0, #0x4]
10081d55c:      add x24, x0, #0x8
10081d560:      stp xzr, xzr, [x24]
10081d564:      lsl x2, x26, #3
10081d568:      adrp    x1, 0x100c2b000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x61e8>
10081d56c:      add x1, x1, #0x3b0
10081d570:      add x0, x24, #0x10
10081d574:      bl  0x100af8850 <_writev+0x100af8850>
10081d578:      lsr x8, x24, #3
10081d57c:      cmp x8, #0x201
10081d580:      b.lo    0x10081d5d4 <_js_object_alloc_with_shape+0x254>
10081d584:      ldurb   w8, [x24, #-0x8]
10081d588:      sub w9, w8, #0x15
10081d58c:      cmn w9, #0x14
10081d590:      b.lo    0x10081d5d4 <_js_object_alloc_with_shape+0x254>
10081d594:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081d598:      add x9, x9, #0xb10
10081d59c:      add x8, x9, x8, lsl #5
10081d5a0:      ldrb    w8, [x8, #0x14]
10081d5a4:      mov w9, #0x16               ; =22
10081d5a8:      lsr w8, w9, w8
10081d5ac:      tbz w8, #0x0, 0x10081d5d4 <_js_object_alloc_with_shape+0x254>
10081d5b0:      ldurh   w8, [x24, #-0x6]
10081d5b4:      mov w9, #0x4000             ; =16384
10081d5b8:      bfxil   w9, w8, #0, #13
10081d5bc:      sturh   w9, [x24, #-0x6]
10081d5c0:      mov x0, x24
10081d5c4:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081d5c8:      ldurh   w8, [x24, #-0x6]
10081d5cc:      and w8, w8, #0xffffefff
10081d5d0:      sturh   w8, [x24, #-0x6]
10081d5d4:      bl  0x10026a350 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
10081d5d8:      str x0, [sp, #0x18]
10081d5dc:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
10081d5e0:      stp x24, x8, [sp, #0x30]
10081d5e4:      mov w8, #0x1                ; =1
10081d5e8:      str x8, [sp, #0x28]
10081d5ec:      add x0, sp, #0x28
10081d5f0:      bl  0x10026a458 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
10081d5f4:      mov x28, x0
10081d5f8:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081d5fc:      ldr w21, [x24, #0x634]
10081d600:      cmp w21, #0x300
10081d604:      b.hs    0x10081d788 <_js_object_alloc_with_shape+0x408>
10081d608:      ldr x8, [x27, #0x48]
10081d60c:      cmn x8, #0x1
10081d610:      b.eq    0x10081d778 <_js_object_alloc_with_shape+0x3f8>
10081d614:      mrs x9, TPIDRRO_EL0
10081d618:      and x9, x9, #0xfffffffffffffff8
10081d61c:      ldr x0, [x9, x8, lsl #3]
10081d620:      cbz x0, 0x10081d778 <_js_object_alloc_with_shape+0x3f8>
10081d624:      add x8, x0, x21, lsl #3
10081d628:      ldr x0, [x8, #0x1e8]
10081d62c:      cbz x0, 0x10081d788 <_js_object_alloc_with_shape+0x408>
10081d630:      ldr x0, [x0]
10081d634:      cbz x0, 0x10081d79c <_js_object_alloc_with_shape+0x41c>
10081d638:      and w8, w20, #0xff
10081d63c:      add x8, x0, w8, uxtw #4
10081d640:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081d644:      ldr w9, [x8]
10081d648:      cmp w9, w20
10081d64c:      b.ne    0x10081d7b8 <_js_object_alloc_with_shape+0x438>
10081d650:      ldr x25, [x8, #0x8]
10081d654:      ldr w26, [x8, #0x4]
10081d658:      cbnz    x25, 0x10081dd1c <_js_object_alloc_with_shape+0x99c>
10081d65c:      b   0x10081d8ac <_js_object_alloc_with_shape+0x52c>
10081d660:      cmp x0, x1
10081d664:      b.hs    0x10081e224 <_js_object_alloc_with_shape+0xea4>
10081d668:      ldr x24, [x9]
10081d66c:      subs    x10, x1, #0x1
10081d670:      ldr q0, [x8, x10, lsl #4]
10081d674:      str q0, [x9]
10081d678:      str x10, [x25, #0x18]
10081d67c:      b.ne    0x10081d6a4 <_js_object_alloc_with_shape+0x324>
10081d680:      ldr x8, [x27, #0x48]
10081d684:      cmn x8, #0x1
10081d688:      b.eq    0x10081e1f0 <_js_object_alloc_with_shape+0xe70>
10081d68c:      mrs x9, TPIDRRO_EL0
10081d690:      and x9, x9, #0xfffffffffffffff8
10081d694:      ldr x0, [x9, x8, lsl #3]
10081d698:      cbz x0, 0x10081e1f0 <_js_object_alloc_with_shape+0xe70>
10081d69c:      ldr x8, [x0, #0x28]
10081d6a0:      strb    wzr, [x8]
10081d6a4:      ldr x8, [x25]
10081d6a8:      add x8, x8, #0x1
10081d6ac:      str x8, [x25]
10081d6b0:      mov w8, #0x2                ; =2
10081d6b4:      mov x28, x24
10081d6b8:      strb    w8, [x28, #-0x8]!
10081d6bc:      ldr x8, [x27, #0x48]
10081d6c0:      cmn x8, #0x1
10081d6c4:      b.eq    0x10081e1d4 <_js_object_alloc_with_shape+0xe54>
10081d6c8:      mrs x9, TPIDRRO_EL0
10081d6cc:      and x9, x9, #0xfffffffffffffff8
10081d6d0:      ldr x0, [x9, x8, lsl #3]
10081d6d4:      cbz x0, 0x10081e1d4 <_js_object_alloc_with_shape+0xe54>
10081d6d8:      ldr x8, [x0, #0x30]
10081d6dc:      ldrb    w8, [x8]
10081d6e0:      orr w8, w8, #0x2
10081d6e4:      sturb   w8, [x24, #-0x7]
10081d6e8:      ldr x8, [x27, #0x48]
10081d6ec:      cmn x8, #0x1
10081d6f0:      b.eq    0x10081e1dc <_js_object_alloc_with_shape+0xe5c>
10081d6f4:      mrs x9, TPIDRRO_EL0
10081d6f8:      and x9, x9, #0xfffffffffffffff8
10081d6fc:      ldr x0, [x9, x8, lsl #3]
10081d700:      cbz x0, 0x10081e1dc <_js_object_alloc_with_shape+0xe5c>
10081d704:      ldr x8, [x0, #0x30]
10081d708:      ldrb    w8, [x8]
10081d70c:      tbz w8, #0x0, 0x10081d76c <_js_object_alloc_with_shape+0x3ec>
10081d710:      ldrb    w8, [x28]
10081d714:      sub w9, w8, #0x15
10081d718:      cmn w9, #0x14
10081d71c:      b.lo    0x10081d76c <_js_object_alloc_with_shape+0x3ec>
10081d720:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081d724:      add x9, x9, #0xb10
10081d728:      add x8, x9, x8, lsl #5
10081d72c:      ldrb    w8, [x8, #0x1b]
10081d730:      tbnz    w8, #0x0, 0x10081d76c <_js_object_alloc_with_shape+0x3ec>
10081d734:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081d738:      add x0, x0, #0x658
10081d73c:      ldr x8, [x0]
10081d740:      blr x8
10081d744:      ldrb    w8, [x0, #0x18]
10081d748:      cbnz    w8, 0x10081e268 <_js_object_alloc_with_shape+0xee8>
10081d74c:      ldr x25, [x0, #0x10]
10081d750:      ldr x8, [x0]
10081d754:      cmp x25, x8
10081d758:      b.eq    0x10081e2a4 <_js_object_alloc_with_shape+0xf24>
10081d75c:      ldr x8, [x0, #0x8]
10081d760:      str x28, [x8, x25, lsl #3]
10081d764:      add x8, x25, #0x1
10081d768:      str x8, [x0, #0x10]
10081d76c:      sturh   wzr, [x24, #-0x6]
10081d770:      stur    w21, [x24, #-0x4]
10081d774:      b   0x10081d560 <_js_object_alloc_with_shape+0x1e0>
10081d778:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081d77c:      add x8, x0, x21, lsl #3
10081d780:      ldr x0, [x8, #0x1e8]
10081d784:      cbnz    x0, 0x10081d630 <_js_object_alloc_with_shape+0x2b0>
10081d788:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081d78c:      add x0, x0, #0x330
10081d790:      bl  0x100ad8a1c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081d794:      ldr x0, [x0]
10081d798:      cbnz    x0, 0x10081d638 <_js_object_alloc_with_shape+0x2b8>
10081d79c:      bl  0x100ada6ec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081d7a0:      and w8, w20, #0xff
10081d7a4:      add x8, x0, w8, uxtw #4
10081d7a8:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081d7ac:      ldr w9, [x8]
10081d7b0:      cmp w9, w20
10081d7b4:      b.eq    0x10081d650 <_js_object_alloc_with_shape+0x2d0>
10081d7b8:      ldr x8, [x0, #0x5088]
10081d7bc:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081d7c0:      cmp x8, x9
10081d7c4:      b.hs    0x10081e218 <_js_object_alloc_with_shape+0xe98>
10081d7c8:      add x9, x8, #0x1
10081d7cc:      str x9, [x0, #0x5088]
10081d7d0:      ldr x9, [x0, #0x50a8]
10081d7d4:      cbz x9, 0x10081d888 <_js_object_alloc_with_shape+0x508>
10081d7d8:      mov x9, #0x0                ; =0
10081d7dc:      mov w10, w20
10081d7e0:      mov x11, #0x7c15            ; =31765
10081d7e4:      movk    x11, #0x7f4a, lsl #16
10081d7e8:      movk    x11, #0x79b9, lsl #32
10081d7ec:      movk    x11, #0x9e37, lsl #48
10081d7f0:      mul x10, x10, x11
10081d7f4:      eor x13, x10, x10, lsr #32
10081d7f8:      lsr x12, x10, #57
10081d7fc:      ldr x10, [x0, #0x5098]
10081d800:      ldr x11, [x0, #0x5090]
10081d804:      dup.8b  v0, w12
10081d808:      movi.2d v1, #0xffffffffffffffff
10081d80c:      mov w12, #0x18              ; =24
10081d810:      and x13, x13, x10
10081d814:      ldr d2, [x11, x13]
10081d818:      cmeq.8b v3, v2, v0
10081d81c:      fmov    x14, d3
10081d820:      ands    x14, x14, #0x8080808080808080
10081d824:      b.eq    0x10081d858 <_js_object_alloc_with_shape+0x4d8>
10081d828:      rbit    x15, x14
10081d82c:      clz x15, x15
10081d830:      add x15, x13, x15, lsr #3
10081d834:      and x15, x15, x10
10081d838:      mneg    x15, x15, x12
10081d83c:      add x15, x11, x15
10081d840:      ldur    w16, [x15, #-0x18]
10081d844:      cmp w20, w16
10081d848:      b.eq    0x10081d89c <_js_object_alloc_with_shape+0x51c>
10081d84c:      sub x15, x14, #0x2
10081d850:      ands    x14, x15, x14
10081d854:      b.ne    0x10081d828 <_js_object_alloc_with_shape+0x4a8>
10081d858:      cmeq.8b v2, v2, v1
10081d85c:      fmov    x14, d2
10081d860:      cbnz    x14, 0x10081d888 <_js_object_alloc_with_shape+0x508>
10081d864:      add x9, x9, #0x8
10081d868:      add x13, x13, x9
10081d86c:      and x13, x13, x10
10081d870:      ldr d2, [x11, x13]
10081d874:      cmeq.8b v3, v2, v0
10081d878:      fmov    x14, d3
10081d87c:      ands    x14, x14, #0x8080808080808080
10081d880:      b.ne    0x10081d828 <_js_object_alloc_with_shape+0x4a8>
10081d884:      b   0x10081d858 <_js_object_alloc_with_shape+0x4d8>
10081d888:      mov w26, #0x0               ; =0
10081d88c:      mov x25, #0x0               ; =0
10081d890:      str x8, [x0, #0x5088]
10081d894:      cbnz    x25, 0x10081dd1c <_js_object_alloc_with_shape+0x99c>
10081d898:      b   0x10081d8ac <_js_object_alloc_with_shape+0x52c>
10081d89c:      ldur    x25, [x15, #-0x10]
10081d8a0:      ldur    w26, [x15, #-0x8]
10081d8a4:      str x8, [x0, #0x5088]
10081d8a8:      cbnz    x25, 0x10081dd1c <_js_object_alloc_with_shape+0x99c>
10081d8ac:      and w8, w20, #0xff
10081d8b0:      str x8, [sp, #0x8]
10081d8b4:      mov w26, #0x0               ; =0
10081d8b8:      mov w8, #0x0                ; =0
10081d8bc:      mov w9, #0x8                ; =8
10081d8c0:      str x9, [sp, #0x10]
10081d8c4:      mov w25, w23
10081d8c8:      b   0x10081d8dc <_js_object_alloc_with_shape+0x55c>
10081d8cc:      mov w26, #0x1               ; =1
10081d8d0:      mov x22, x21
10081d8d4:      mov w8, #0x1                ; =1
10081d8d8:      cbnz    x23, 0x10081d934 <_js_object_alloc_with_shape+0x5b4>
10081d8dc:      tbnz    w8, #0x0, 0x10081d928 <_js_object_alloc_with_shape+0x5a8>
10081d8e0:      mov x21, x22
10081d8e4:      mov x23, #0x0               ; =0
10081d8e8:      cbz x25, 0x10081d8cc <_js_object_alloc_with_shape+0x54c>
10081d8ec:      ldrb    w8, [x21, x23]
10081d8f0:      cbz w8, 0x10081d914 <_js_object_alloc_with_shape+0x594>
10081d8f4:      add x23, x23, #0x1
10081d8f8:      cmp x25, x23
10081d8fc:      b.ne    0x10081d8ec <_js_object_alloc_with_shape+0x56c>
10081d900:      mov w26, #0x1               ; =1
10081d904:      mov x22, x21
10081d908:      mov w8, #0x1                ; =1
10081d90c:      mov x23, x25
10081d910:      b   0x10081d8d8 <_js_object_alloc_with_shape+0x558>
10081d914:      mvn x9, x23
10081d918:      add x25, x9, x25
10081d91c:      add x9, x21, x23
10081d920:      add x22, x9, #0x1
10081d924:      b   0x10081d8d8 <_js_object_alloc_with_shape+0x558>
10081d928:      mov x23, #0x0               ; =0
10081d92c:      mov w21, #0x1               ; =1
10081d930:      b   0x10081da34 <_js_object_alloc_with_shape+0x6b4>
10081d934:      cbz x21, 0x10081da24 <_js_object_alloc_with_shape+0x6a4>
10081d938:      mov w0, #0x40               ; =64
10081d93c:      mov w1, #0x8                ; =8
10081d940:      bl  0x100af5c08 <_mi_malloc_aligned>
10081d944:      cbz x0, 0x10081e2b4 <_js_object_alloc_with_shape+0xf34>
10081d948:      stp x21, x23, [x0]
10081d94c:      mov w8, #0x4                ; =4
10081d950:      stp x8, x0, [sp, #0x28]
10081d954:      mov w23, #0x1               ; =1
10081d958:      str x23, [sp, #0x38]
10081d95c:      mov x8, x25
10081d960:      mov x10, x22
10081d964:      mov x9, x26
10081d968:      b   0x10081d97c <_js_object_alloc_with_shape+0x5fc>
10081d96c:      mov w26, #0x1               ; =1
10081d970:      mov x10, x24
10081d974:      mov w9, #0x1                ; =1
10081d978:      cbnz    x21, 0x10081d9d0 <_js_object_alloc_with_shape+0x650>
10081d97c:      tbnz    w9, #0x0, 0x10081da10 <_js_object_alloc_with_shape+0x690>
10081d980:      mov x24, x10
10081d984:      mov x21, #0x0               ; =0
10081d988:      cbz x8, 0x10081d96c <_js_object_alloc_with_shape+0x5ec>
10081d98c:      ldrb    w9, [x24, x21]
10081d990:      cbz w9, 0x10081d9b4 <_js_object_alloc_with_shape+0x634>
10081d994:      add x21, x21, #0x1
10081d998:      cmp x8, x21
10081d99c:      b.ne    0x10081d98c <_js_object_alloc_with_shape+0x60c>
10081d9a0:      mov w26, #0x1               ; =1
10081d9a4:      mov x10, x24
10081d9a8:      mov w9, #0x1                ; =1
10081d9ac:      mov x21, x8
10081d9b0:      b   0x10081d978 <_js_object_alloc_with_shape+0x5f8>
10081d9b4:      mvn x10, x21
10081d9b8:      add x25, x10, x8
10081d9bc:      add x8, x24, x21
10081d9c0:      add x22, x8, #0x1
10081d9c4:      mov x8, x25
10081d9c8:      mov x10, x22
10081d9cc:      b   0x10081d978 <_js_object_alloc_with_shape+0x5f8>
10081d9d0:      cbz x24, 0x10081da10 <_js_object_alloc_with_shape+0x690>
10081d9d4:      ldr x8, [sp, #0x28]
10081d9d8:      cmp x23, x8
10081d9dc:      b.eq    0x10081d9f0 <_js_object_alloc_with_shape+0x670>
10081d9e0:      add x8, x0, x23, lsl #4
10081d9e4:      stp x24, x21, [x8]
10081d9e8:      add x23, x23, #0x1
10081d9ec:      b   0x10081d958 <_js_object_alloc_with_shape+0x5d8>
10081d9f0:      add x0, sp, #0x28
10081d9f4:      mov x1, x23
10081d9f8:      mov w2, #0x1                ; =1
10081d9fc:      mov w3, #0x8                ; =8
10081da00:      mov w4, #0x10               ; =16
10081da04:      bl  0x100ad5290 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs7wa3bwJW6LN_13perry_runtime>
10081da08:      ldr x0, [sp, #0x30]
10081da0c:      b   0x10081d9e0 <_js_object_alloc_with_shape+0x660>
10081da10:      ldp x8, x9, [sp, #0x28]
10081da14:      str x9, [sp, #0x10]
10081da18:      cmp x8, #0x0
10081da1c:      cset    w21, eq
10081da20:      b   0x10081da34 <_js_object_alloc_with_shape+0x6b4>
10081da24:      mov x23, #0x0               ; =0
10081da28:      mov w21, #0x1               ; =1
10081da2c:      mov w8, #0x8                ; =8
10081da30:      str x8, [sp, #0x10]
10081da34:      ubfiz   x8, x23, #3, #32
10081da38:      add x0, x8, #0x8
10081da3c:      mov w1, #0x8                ; =8
10081da40:      mov w2, #0x1                ; =1
10081da44:      bl  0x1004da9a0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators24arena_alloc_gc_longlived>
10081da48:      mov x22, x0
10081da4c:      stp w23, w23, [x0]
10081da50:      lsr x8, x0, #3
10081da54:      cmp x8, #0x201
10081da58:      b.lo    0x10081dae8 <_js_object_alloc_with_shape+0x768>
10081da5c:      ldurb   w8, [x22, #-0x8]
10081da60:      cmp w8, #0x1
10081da64:      b.ne    0x10081da90 <_js_object_alloc_with_shape+0x710>
10081da68:      ldurh   w8, [x22, #-0x6]
10081da6c:      mov w9, #0x1080             ; =4224
10081da70:      mov w10, #0xef7f            ; =61311
10081da74:      and w10, w8, w10
10081da78:      sturh   w10, [x22, #-0x6]
10081da7c:      tst w8, w9
10081da80:      b.eq    0x10081daa0 <_js_object_alloc_with_shape+0x720>
10081da84:      mov x0, x22
10081da88:      bl  0x1002b6ec0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime14typed_feedback32invalidate_representation_change>
10081da8c:      ldurb   w8, [x22, #-0x8]
10081da90:      sub w9, w8, #0x15
10081da94:      cmn w9, #0x14
10081da98:      b.hs    0x10081daa4 <_js_object_alloc_with_shape+0x724>
10081da9c:      b   0x10081dae8 <_js_object_alloc_with_shape+0x768>
10081daa0:      mov w8, #0x1                ; =1
10081daa4:      mov w8, w8
10081daa8:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081daac:      add x9, x9, #0xb10
10081dab0:      add x8, x9, x8, lsl #5
10081dab4:      ldrb    w8, [x8, #0x14]
10081dab8:      mov w9, #0x16               ; =22
10081dabc:      lsr w8, w9, w8
10081dac0:      tbz w8, #0x0, 0x10081dae8 <_js_object_alloc_with_shape+0x768>
10081dac4:      ldurh   w8, [x22, #-0x6]
10081dac8:      mov w9, #0x4000             ; =16384
10081dacc:      bfxil   w9, w8, #0, #13
10081dad0:      sturh   w9, [x22, #-0x6]
10081dad4:      mov x0, x22
10081dad8:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081dadc:      ldurh   w8, [x22, #-0x6]
10081dae0:      and w8, w8, #0xffffefff
10081dae4:      sturh   w8, [x22, #-0x6]
10081dae8:      stp w21, w19, [sp]
10081daec:      mov x19, x20
10081daf0:      mov x20, x28
10081daf4:      bl  0x10026a350 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
10081daf8:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
10081dafc:      stp x22, x8, [sp, #0x30]
10081db00:      mov w8, #0x1                ; =1
10081db04:      stp x0, x8, [sp, #0x20]
10081db08:      add x0, sp, #0x28
10081db0c:      bl  0x10026a458 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
10081db10:      mov x22, x0
10081db14:      cbz x23, 0x10081dbd8 <_js_object_alloc_with_shape+0x858>
10081db18:      mov x25, #0x0               ; =0
10081db1c:      mov x26, #0x7fffffffffffffff ; =9223372036854775807
10081db20:      mov w28, #0x18              ; =24
10081db24:      ldr x8, [sp, #0x10]
10081db28:      mov x24, x8
10081db2c:      add x21, x8, x23, lsl #4
10081db30:      b   0x10081db64 <_js_object_alloc_with_shape+0x7e4>
10081db34:      mov x0, x22
10081db38:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081db3c:      mov x2, #0x7fff000000000000 ; =9223090561878065152
10081db40:      bfxil   x2, x23, #0, #48
10081db44:      add x8, x0, x25, lsl #3
10081db48:      str x2, [x8, #0x8]
10081db4c:      mov x1, x25
10081db50:      bl  0x1004f079c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array15header_gc_slots27note_array_slot_layout_only>
10081db54:      add x24, x24, #0x10
10081db58:      add x25, x25, #0x1
10081db5c:      cmp x24, x21
10081db60:      b.eq    0x10081dbd8 <_js_object_alloc_with_shape+0x858>
10081db64:      ldr x0, [x24]
10081db68:      ldr w1, [x24, #0x8]
10081db6c:      bl  0x100884854 <_js_string_from_bytes_longlived>
10081db70:      mov x23, x0
10081db74:      ldr x8, [x27, #0x48]
10081db78:      cmn x8, #0x1
10081db7c:      b.eq    0x10081db34 <_js_object_alloc_with_shape+0x7b4>
10081db80:      mrs x9, TPIDRRO_EL0
10081db84:      and x9, x9, #0xfffffffffffffff8
10081db88:      ldr x8, [x9, x8, lsl #3]
10081db8c:      cbz x8, 0x10081db34 <_js_object_alloc_with_shape+0x7b4>
10081db90:      ldr x8, [x8, #0x19e8]
10081db94:      cbz x8, 0x10081db34 <_js_object_alloc_with_shape+0x7b4>
10081db98:      ldr x9, [x8]
10081db9c:      cmp x9, x26
10081dba0:      b.hs    0x10081e1f8 <_js_object_alloc_with_shape+0xe78>
10081dba4:      add x10, x9, #0x1
10081dba8:      str x10, [x8]
10081dbac:      ldr x10, [x8, #0x18]
10081dbb0:      cmp x22, x10
10081dbb4:      b.hs    0x10081e204 <_js_object_alloc_with_shape+0xe84>
10081dbb8:      ldr x10, [x8, #0x10]
10081dbbc:      madd    x10, x22, x28, x10
10081dbc0:      ldr x11, [x10]
10081dbc4:      cmp x11, #0x1
10081dbc8:      b.ne    0x10081e208 <_js_object_alloc_with_shape+0xe88>
10081dbcc:      ldr x0, [x10, #0x8]
10081dbd0:      str x9, [x8]
10081dbd4:      b   0x10081db3c <_js_object_alloc_with_shape+0x7bc>
10081dbd8:      ldr x8, [x27, #0x48]
10081dbdc:      cmn x8, #0x1
10081dbe0:      b.eq    0x10081dc50 <_js_object_alloc_with_shape+0x8d0>
10081dbe4:      mrs x9, TPIDRRO_EL0
10081dbe8:      and x9, x9, #0xfffffffffffffff8
10081dbec:      ldr x8, [x9, x8, lsl #3]
10081dbf0:      cbz x8, 0x10081dc50 <_js_object_alloc_with_shape+0x8d0>
10081dbf4:      ldr x8, [x8, #0x19e8]
10081dbf8:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081dbfc:      cbz x8, 0x10081dc88 <_js_object_alloc_with_shape+0x908>
10081dc00:      ldr x9, [x8]
10081dc04:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10081dc08:      cmp x9, x10
10081dc0c:      b.hs    0x10081e1f8 <_js_object_alloc_with_shape+0xe78>
10081dc10:      add x10, x9, #0x1
10081dc14:      str x10, [x8]
10081dc18:      ldr x10, [x8, #0x18]
10081dc1c:      cmp x22, x10
10081dc20:      b.hs    0x10081e204 <_js_object_alloc_with_shape+0xe84>
10081dc24:      ldr x10, [x8, #0x10]
10081dc28:      mov w11, #0x18              ; =24
10081dc2c:      madd    x10, x22, x11, x10
10081dc30:      ldr x11, [x10]
10081dc34:      cmp x11, #0x1
10081dc38:      b.ne    0x10081e208 <_js_object_alloc_with_shape+0xe88>
10081dc3c:      mov x28, x20
10081dc40:      mov x20, x19
10081dc44:      ldr x25, [x10, #0x8]
10081dc48:      str x9, [x8]
10081dc4c:      b   0x10081dc9c <_js_object_alloc_with_shape+0x91c>
10081dc50:      mov x0, x22
10081dc54:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081dc58:      mov x25, x0
10081dc5c:      mov x28, x20
10081dc60:      mov x20, x19
10081dc64:      ldr w19, [sp, #0x4]
10081dc68:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081dc6c:      mov x0, x20
10081dc70:      mov x1, x25
10081dc74:      bl  0x10032203c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object18shape_cache_insert>
10081dc78:      ldr w21, [x24, #0x634]
10081dc7c:      cmp w21, #0x300
10081dc80:      b.lo    0x10081dcb8 <_js_object_alloc_with_shape+0x938>
10081dc84:      b   0x10081dfb0 <_js_object_alloc_with_shape+0xc30>
10081dc88:      mov x0, x22
10081dc8c:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081dc90:      mov x25, x0
10081dc94:      mov x28, x20
10081dc98:      mov x20, x19
10081dc9c:      ldr w19, [sp, #0x4]
10081dca0:      mov x0, x20
10081dca4:      mov x1, x25
10081dca8:      bl  0x10032203c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object18shape_cache_insert>
10081dcac:      ldr w21, [x24, #0x634]
10081dcb0:      cmp w21, #0x300
10081dcb4:      b.hs    0x10081dfb0 <_js_object_alloc_with_shape+0xc30>
10081dcb8:      ldr x8, [x27, #0x48]
10081dcbc:      cmn x8, #0x1
10081dcc0:      b.eq    0x10081dfa0 <_js_object_alloc_with_shape+0xc20>
10081dcc4:      mrs x9, TPIDRRO_EL0
10081dcc8:      and x9, x9, #0xfffffffffffffff8
10081dccc:      ldr x0, [x9, x8, lsl #3]
10081dcd0:      cbz x0, 0x10081dfa0 <_js_object_alloc_with_shape+0xc20>
10081dcd4:      add x8, x0, x21, lsl #3
10081dcd8:      ldr x0, [x8, #0x1e8]
10081dcdc:      cbz x0, 0x10081dfb0 <_js_object_alloc_with_shape+0xc30>
10081dce0:      ldr x0, [x0]
10081dce4:      cbz x0, 0x10081dfc4 <_js_object_alloc_with_shape+0xc44>
10081dce8:      ldr x8, [sp, #0x8]
10081dcec:      add x8, x0, x8, lsl #4
10081dcf0:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081dcf4:      ldr w9, [x8]
10081dcf8:      cmp w9, w20
10081dcfc:      b.ne    0x10081dfe0 <_js_object_alloc_with_shape+0xc60>
10081dd00:      ldr w26, [x8, #0x4]
10081dd04:      add x0, sp, #0x20
10081dd08:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
10081dd0c:      ldr w8, [sp]
10081dd10:      tbnz    w8, #0x0, 0x10081dd1c <_js_object_alloc_with_shape+0x99c>
10081dd14:      ldr x0, [sp, #0x10]
10081dd18:      bl  0x100af5980 <_mi_free>
10081dd1c:      ldr x8, [x27, #0x48]
10081dd20:      cmn x8, #0x1
10081dd24:      b.eq    0x10081dd8c <_js_object_alloc_with_shape+0xa0c>
10081dd28:      mrs x9, TPIDRRO_EL0
10081dd2c:      and x9, x9, #0xfffffffffffffff8
10081dd30:      ldr x8, [x9, x8, lsl #3]
10081dd34:      cbz x8, 0x10081dd8c <_js_object_alloc_with_shape+0xa0c>
10081dd38:      ldr x8, [x8, #0x19e8]
10081dd3c:      cbz x8, 0x10081dd8c <_js_object_alloc_with_shape+0xa0c>
10081dd40:      ldr x9, [x8]
10081dd44:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10081dd48:      cmp x9, x10
10081dd4c:      b.hs    0x10081e1f8 <_js_object_alloc_with_shape+0xe78>
10081dd50:      add x10, x9, #0x1
10081dd54:      str x10, [x8]
10081dd58:      ldr x10, [x8, #0x18]
10081dd5c:      cmp x28, x10
10081dd60:      b.hs    0x10081e204 <_js_object_alloc_with_shape+0xe84>
10081dd64:      ldr x10, [x8, #0x10]
10081dd68:      mov w11, #0x18              ; =24
10081dd6c:      madd    x10, x28, x11, x10
10081dd70:      ldr x11, [x10]
10081dd74:      cmp x11, #0x1
10081dd78:      b.ne    0x10081e208 <_js_object_alloc_with_shape+0xe88>
10081dd7c:      ldr x20, [x10, #0x8]
10081dd80:      str x9, [x8]
10081dd84:      cbnz    w26, 0x10081dd9c <_js_object_alloc_with_shape+0xa1c>
10081dd88:      b   0x10081ddb4 <_js_object_alloc_with_shape+0xa34>
10081dd8c:      mov x0, x28
10081dd90:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081dd94:      mov x20, x0
10081dd98:      cbz w26, 0x10081ddb4 <_js_object_alloc_with_shape+0xa34>
10081dd9c:      mov x0, x20
10081dda0:      mov x1, x26
10081dda4:      mov x2, x25
10081dda8:      mov x3, x19
10081ddac:      bl  0x10059b7fc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes34try_birth_stamp_preinstalled_shape>
10081ddb0:      tbnz    w0, #0x0, 0x10081e130 <_js_object_alloc_with_shape+0xdb0>
10081ddb4:      ldr w21, [x20, #0x4]
10081ddb8:      ldr w22, [x24, #0x634]
10081ddbc:      cmp w22, #0x300
10081ddc0:      b.hs    0x10081de90 <_js_object_alloc_with_shape+0xb10>
10081ddc4:      ldr x8, [x27, #0x48]
10081ddc8:      cmn x8, #0x1
10081ddcc:      b.eq    0x10081de80 <_js_object_alloc_with_shape+0xb00>
10081ddd0:      mrs x9, TPIDRRO_EL0
10081ddd4:      and x9, x9, #0xfffffffffffffff8
10081ddd8:      ldr x0, [x9, x8, lsl #3]
10081dddc:      cbz x0, 0x10081de80 <_js_object_alloc_with_shape+0xb00>
10081dde0:      add x8, x0, x22, lsl #3
10081dde4:      ldr x0, [x8, #0x1e8]
10081dde8:      cbz x0, 0x10081de90 <_js_object_alloc_with_shape+0xb10>
10081ddec:      ldr x0, [x0]
10081ddf0:      cbz x0, 0x10081dea4 <_js_object_alloc_with_shape+0xb24>
10081ddf4:      mov w8, #-0x40000001        ; =-1073741825
10081ddf8:      cmp w21, w8
10081ddfc:      b.gt    0x10081deb4 <_js_object_alloc_with_shape+0xb34>
10081de00:      ldr x9, [x0, #0x5198]
10081de04:      ubfx    x8, x21, #15, #15
10081de08:      cmp x8, x9
10081de0c:      b.hs    0x10081deb4 <_js_object_alloc_with_shape+0xb34>
10081de10:      ldr x9, [x0, #0x5190]
10081de14:      ldr x8, [x9, x8, lsl #3]
10081de18:      cbz x8, 0x10081deb4 <_js_object_alloc_with_shape+0xb34>
10081de1c:      ubfx    x9, x21, #5, #10
10081de20:      ldr x8, [x8, x9, lsl #3]
10081de24:      cbz x8, 0x10081deb4 <_js_object_alloc_with_shape+0xb34>
10081de28:      and x9, x21, #0x1f
10081de2c:      add x8, x8, x9, lsl #5
10081de30:      ldrb    w9, [x8, #0x1c]
10081de34:      tbz w9, #0x0, 0x10081deb4 <_js_object_alloc_with_shape+0xb34>
10081de38:      mov w10, #0x90              ; =144
10081de3c:      tst w9, w10
10081de40:      cset    w10, ne
10081de44:      ldp x11, x12, [x8]
10081de48:      ubfx    w13, w9, #5, #1
10081de4c:      ldr w14, [x8, #0x18]
10081de50:      ubfx    w9, w9, #2, #1
10081de54:      stp x11, x8, [sp, #0x28]
10081de58:      str x12, [sp, #0x38]
10081de5c:      ldr d0, [x8, #0x10]
10081de60:      str d0, [sp, #0x40]
10081de64:      str w14, [sp, #0x48]
10081de68:      strb    w9, [sp, #0x4c]
10081de6c:      strb    w10, [sp, #0x4d]
10081de70:      strb    w13, [sp, #0x4e]
10081de74:      cmp x11, x25
10081de78:      b.ne    0x10081dec0 <_js_object_alloc_with_shape+0xb40>
10081de7c:      b   0x10081e10c <_js_object_alloc_with_shape+0xd8c>
10081de80:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081de84:      add x8, x0, x22, lsl #3
10081de88:      ldr x0, [x8, #0x1e8]
10081de8c:      cbnz    x0, 0x10081ddec <_js_object_alloc_with_shape+0xa6c>
10081de90:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081de94:      add x0, x0, #0x330
10081de98:      bl  0x100ad8a1c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081de9c:      ldr x0, [x0]
10081dea0:      cbnz    x0, 0x10081ddf4 <_js_object_alloc_with_shape+0xa74>
10081dea4:      bl  0x100ada6ec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081dea8:      mov w8, #-0x40000001        ; =-1073741825
10081deac:      cmp w21, w8
10081deb0:      b.le    0x10081de00 <_js_object_alloc_with_shape+0xa80>
10081deb4:      mov w8, #0x2                ; =2
10081deb8:      strb    w8, [sp, #0x4e]
10081debc:      cbz x25, 0x10081e10c <_js_object_alloc_with_shape+0xd8c>
10081dec0:      lsr x8, x20, #3
10081dec4:      cmp x8, #0x201
10081dec8:      b.lo    0x10081e10c <_js_object_alloc_with_shape+0xd8c>
10081decc:      ldursh  w8, [x20, #-0x6]
10081ded0:      tst w8, #0x1000
10081ded4:      mov w9, #-0x4001            ; =-16385
10081ded8:      ccmp    w8, w9, #0x4, eq
10081dedc:      b.gt    0x10081e10c <_js_object_alloc_with_shape+0xd8c>
10081dee0:      lsr x9, x20, #3
10081dee4:      cmp x9, #0x201
10081dee8:      b.lo    0x10081e10c <_js_object_alloc_with_shape+0xd8c>
10081deec:      ldurb   w9, [x20, #-0x8]
10081def0:      sub w10, w9, #0x15
10081def4:      cmn w10, #0x14
10081def8:      b.lo    0x10081e10c <_js_object_alloc_with_shape+0xd8c>
10081defc:      and w8, w8, #0xffff
10081df00:      adrp    x10, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081df04:      add x10, x10, #0xb10
10081df08:      add x9, x10, x9, lsl #5
10081df0c:      ldrb    w9, [x9, #0x14]
10081df10:      mov w10, #0x16              ; =22
10081df14:      lsr w9, w10, w9
10081df18:      tbz w9, #0x0, 0x10081e10c <_js_object_alloc_with_shape+0xd8c>
10081df1c:      and w9, w8, #0xffffefff
10081df20:      sturh   w9, [x20, #-0x6]
10081df24:      ands    w21, w8, #0xc000
10081df28:      b.eq    0x10081e104 <_js_object_alloc_with_shape+0xd84>
10081df2c:      and w8, w8, #0xfff
10081df30:      sturh   w8, [x20, #-0x6]
10081df34:      mov x0, x20
10081df38:      bl  0x1004141e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20typed_layouts_remove>
10081df3c:      cmp w21, #0x4, lsl #12      ; =0x4000
10081df40:      b.eq    0x10081df4c <_js_object_alloc_with_shape+0xbcc>
10081df44:      mov x0, x20
10081df48:      bl  0x1004138e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables17slot_masks_remove>
10081df4c:      mov x0, x20
10081df50:      bl  0x1002b6ec0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime14typed_feedback32invalidate_representation_change>
10081df54:      b   0x10081e10c <_js_object_alloc_with_shape+0xd8c>
10081df58:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081df5c:      ldr x8, [x0, #0x28]
10081df60:      ldrb    w8, [x8]
10081df64:      tbnz    w8, #0x0, 0x10081d420 <_js_object_alloc_with_shape+0xa0>
10081df68:      b   0x10081d488 <_js_object_alloc_with_shape+0x108>
10081df6c:      mov x24, x0
10081df70:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081df74:      mov x8, x0
10081df78:      mov x0, x24
10081df7c:      b   0x10081d4b4 <_js_object_alloc_with_shape+0x134>
10081df80:      mov x24, x0
10081df84:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081df88:      mov x8, x0
10081df8c:      mov x0, x24
10081df90:      ldr x8, [x8, #0x30]
10081df94:      ldrb    w8, [x8]
10081df98:      tbnz    w8, #0x0, 0x10081d4ec <_js_object_alloc_with_shape+0x16c>
10081df9c:      b   0x10081d554 <_js_object_alloc_with_shape+0x1d4>
10081dfa0:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081dfa4:      add x8, x0, x21, lsl #3
10081dfa8:      ldr x0, [x8, #0x1e8]
10081dfac:      cbnz    x0, 0x10081dce0 <_js_object_alloc_with_shape+0x960>
10081dfb0:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081dfb4:      add x0, x0, #0x330
10081dfb8:      bl  0x100ad8a1c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081dfbc:      ldr x0, [x0]
10081dfc0:      cbnz    x0, 0x10081dce8 <_js_object_alloc_with_shape+0x968>
10081dfc4:      bl  0x100ada6ec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081dfc8:      ldr x8, [sp, #0x8]
10081dfcc:      add x8, x0, x8, lsl #4
10081dfd0:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081dfd4:      ldr w9, [x8]
10081dfd8:      cmp w9, w20
10081dfdc:      b.eq    0x10081dd00 <_js_object_alloc_with_shape+0x980>
10081dfe0:      ldr x8, [x0, #0x5088]
10081dfe4:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081dfe8:      cmp x8, x9
10081dfec:      b.hs    0x10081e218 <_js_object_alloc_with_shape+0xe98>
10081dff0:      add x9, x8, #0x1
10081dff4:      str x9, [x0, #0x5088]
10081dff8:      ldr x9, [x0, #0x50a8]
10081dffc:      cbz x9, 0x10081e0b0 <_js_object_alloc_with_shape+0xd30>
10081e000:      mov x9, #0x0                ; =0
10081e004:      mov w10, w20
10081e008:      mov x11, #0x7c15            ; =31765
10081e00c:      movk    x11, #0x7f4a, lsl #16
10081e010:      movk    x11, #0x79b9, lsl #32
10081e014:      movk    x11, #0x9e37, lsl #48
10081e018:      mul x10, x10, x11
10081e01c:      eor x13, x10, x10, lsr #32
10081e020:      lsr x12, x10, #57
10081e024:      ldr x10, [x0, #0x5098]
10081e028:      ldr x11, [x0, #0x5090]
10081e02c:      dup.8b  v0, w12
10081e030:      movi.2d v1, #0xffffffffffffffff
10081e034:      mov w12, #0x18              ; =24
10081e038:      and x13, x13, x10
10081e03c:      ldr d2, [x11, x13]
10081e040:      cmeq.8b v3, v2, v0
10081e044:      fmov    x14, d3
10081e048:      ands    x14, x14, #0x8080808080808080
10081e04c:      b.eq    0x10081e080 <_js_object_alloc_with_shape+0xd00>
10081e050:      rbit    x15, x14
10081e054:      clz x15, x15
10081e058:      add x15, x13, x15, lsr #3
10081e05c:      and x15, x15, x10
10081e060:      mneg    x15, x15, x12
10081e064:      add x15, x11, x15
10081e068:      ldur    w16, [x15, #-0x18]
10081e06c:      cmp w20, w16
10081e070:      b.eq    0x10081e0cc <_js_object_alloc_with_shape+0xd4c>
10081e074:      sub x15, x14, #0x2
10081e078:      ands    x14, x15, x14
10081e07c:      b.ne    0x10081e050 <_js_object_alloc_with_shape+0xcd0>
10081e080:      cmeq.8b v2, v2, v1
10081e084:      fmov    x14, d2
10081e088:      cbnz    x14, 0x10081e0b0 <_js_object_alloc_with_shape+0xd30>
10081e08c:      add x9, x9, #0x8
10081e090:      add x13, x13, x9
10081e094:      and x13, x13, x10
10081e098:      ldr d2, [x11, x13]
10081e09c:      cmeq.8b v3, v2, v0
10081e0a0:      fmov    x14, d3
10081e0a4:      ands    x14, x14, #0x8080808080808080
10081e0a8:      b.ne    0x10081e050 <_js_object_alloc_with_shape+0xcd0>
10081e0ac:      b   0x10081e080 <_js_object_alloc_with_shape+0xd00>
10081e0b0:      mov w26, #0x0               ; =0
10081e0b4:      str x8, [x0, #0x5088]
10081e0b8:      add x0, sp, #0x20
10081e0bc:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
10081e0c0:      ldr w8, [sp]
10081e0c4:      tbz w8, #0x0, 0x10081dd14 <_js_object_alloc_with_shape+0x994>
10081e0c8:      b   0x10081dd1c <_js_object_alloc_with_shape+0x99c>
10081e0cc:      ldur    w26, [x15, #-0x8]
10081e0d0:      str x8, [x0, #0x5088]
10081e0d4:      add x0, sp, #0x20
10081e0d8:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
10081e0dc:      ldr w8, [sp]
10081e0e0:      tbz w8, #0x0, 0x10081dd14 <_js_object_alloc_with_shape+0x994>
10081e0e4:      b   0x10081dd1c <_js_object_alloc_with_shape+0x99c>
10081e0e8:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e0ec:      ldr x25, [x0, #0x20]
10081e0f0:      ldr x8, [x25]
10081e0f4:      cbz x8, 0x10081d448 <_js_object_alloc_with_shape+0xc8>
10081e0f8:      adrp    x0, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ef0>
10081e0fc:      add x0, x0, #0x120
10081e100:      bl  0x100ab0bac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10081e104:      mov x0, x20
10081e108:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081e10c:      add x1, sp, #0x28
10081e110:      mov x0, x20
10081e114:      mov x2, x25
10081e118:      mov x3, x19
10081e11c:      bl  0x10059802c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes25publish_object_shape_from>
10081e120:      mov x0, x20
10081e124:      mov x1, x26
10081e128:      mov x2, x19
10081e12c:      bl  0x100597880 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes24birth_stamp_object_shape>
10081e130:      ldr x8, [x27, #0x48]
10081e134:      cmn x8, #0x1
10081e138:      b.eq    0x10081e19c <_js_object_alloc_with_shape+0xe1c>
10081e13c:      mrs x9, TPIDRRO_EL0
10081e140:      and x9, x9, #0xfffffffffffffff8
10081e144:      ldr x8, [x9, x8, lsl #3]
10081e148:      cbz x8, 0x10081e19c <_js_object_alloc_with_shape+0xe1c>
10081e14c:      ldr x8, [x8, #0x19e8]
10081e150:      cbz x8, 0x10081e19c <_js_object_alloc_with_shape+0xe1c>
10081e154:      ldr x9, [x8]
10081e158:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10081e15c:      cmp x9, x10
10081e160:      b.hs    0x10081e1f8 <_js_object_alloc_with_shape+0xe78>
10081e164:      add x10, x9, #0x1
10081e168:      str x10, [x8]
10081e16c:      ldr x10, [x8, #0x18]
10081e170:      cmp x28, x10
10081e174:      b.hs    0x10081e204 <_js_object_alloc_with_shape+0xe84>
10081e178:      ldr x10, [x8, #0x10]
10081e17c:      mov w11, #0x18              ; =24
10081e180:      madd    x10, x28, x11, x10
10081e184:      ldr x11, [x10]
10081e188:      cmp x11, #0x1
10081e18c:      b.ne    0x10081e208 <_js_object_alloc_with_shape+0xe88>
10081e190:      ldr x19, [x10, #0x8]
10081e194:      str x9, [x8]
10081e198:      b   0x10081e1a8 <_js_object_alloc_with_shape+0xe28>
10081e19c:      mov x0, x28
10081e1a0:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
10081e1a4:      mov x19, x0
10081e1a8:      add x0, sp, #0x18
10081e1ac:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
10081e1b0:      mov x0, x19
10081e1b4:      ldp x29, x30, [sp, #0xa0]
10081e1b8:      ldp x20, x19, [sp, #0x90]
10081e1bc:      ldp x22, x21, [sp, #0x80]
10081e1c0:      ldp x24, x23, [sp, #0x70]
10081e1c4:      ldp x26, x25, [sp, #0x60]
10081e1c8:      ldp x28, x27, [sp, #0x50]
10081e1cc:      add sp, sp, #0xb0
10081e1d0:      ret
10081e1d4:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e1d8:      b   0x10081d6d8 <_js_object_alloc_with_shape+0x358>
10081e1dc:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e1e0:      ldr x8, [x0, #0x30]
10081e1e4:      ldrb    w8, [x8]
10081e1e8:      tbnz    w8, #0x0, 0x10081d710 <_js_object_alloc_with_shape+0x390>
10081e1ec:      b   0x10081d76c <_js_object_alloc_with_shape+0x3ec>
10081e1f0:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081e1f4:      b   0x10081d69c <_js_object_alloc_with_shape+0x31c>
10081e1f8:      adrp    x0, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
10081e1fc:      add x0, x0, #0x8a8
10081e200:      bl  0x100ab0bdc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10081e204:      bl  0x100ae2488 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
10081e208:      adrp    x0, 0x100bd8000 <_PERRY_EMPTY_STRING+0x2f0>
10081e20c:      add x0, x0, #0x96c
10081e210:      mov w1, #0xb                ; =11
10081e214:      bl  0x100ae2450 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
10081e218:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081e21c:      add x0, x0, #0x798
10081e220:      bl  0x100ab0bdc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10081e224:      bl  0x100ab087c <__RNvNvMs_NtCsctvjasLqLe9_5alloc3vecINtB6_3VecppE11swap_remove13assert_failed>
10081e228:      cmp w8, #0x1
10081e22c:      b.ne    0x10081e270 <_js_object_alloc_with_shape+0xef0>
10081e230:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081e234:      add x1, x1, #0xbb8
10081e238:      mov x0, x24
10081e23c:      bl  0x1009be91c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081e240:      strb    wzr, [x24, #0x18]
10081e244:      ldr x28, [x24, #0x10]
10081e248:      ldr x8, [x24]
10081e24c:      cmp x28, x8
10081e250:      mov x0, x25
10081e254:      b.ne    0x10081d544 <_js_object_alloc_with_shape+0x1c4>
10081e258:      mov x0, x24
10081e25c:      bl  0x100ad7b24 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081e260:      mov x0, x25
10081e264:      b   0x10081d544 <_js_object_alloc_with_shape+0x1c4>
10081e268:      cmp w8, #0x2
10081e26c:      b.ne    0x10081e27c <_js_object_alloc_with_shape+0xefc>
10081e270:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
10081e274:      add x0, x0, #0x850
10081e278:      bl  0x100aeefdc <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10081e27c:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081e280:      add x1, x1, #0xbb8
10081e284:      mov x25, x0
10081e288:      bl  0x1009be91c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081e28c:      mov x0, x25
10081e290:      strb    wzr, [x25, #0x18]
10081e294:      ldr x25, [x25, #0x10]
10081e298:      ldr x8, [x0]
10081e29c:      cmp x25, x8
10081e2a0:      b.ne    0x10081d75c <_js_object_alloc_with_shape+0x3dc>
10081e2a4:      str x0, [sp, #0x10]
10081e2a8:      bl  0x100ad7b24 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081e2ac:      ldr x0, [sp, #0x10]
10081e2b0:      b   0x10081d75c <_js_object_alloc_with_shape+0x3dc>
10081e2b4:      mov w0, #0x8                ; =8
10081e2b8:      mov w1, #0x40               ; =64
10081e2bc:      bl  0x100ab0824 <__RNvNtCsctvjasLqLe9_5alloc7raw_vec12handle_error>
