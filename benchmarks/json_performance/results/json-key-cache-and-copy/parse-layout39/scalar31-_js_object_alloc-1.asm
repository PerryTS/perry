/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/scalar31-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010081c380 <_js_object_alloc_class_with_keys>:
10081c380:      sub sp, sp, #0x80
10081c384:      stp x28, x27, [sp, #0x20]
10081c388:      stp x26, x25, [sp, #0x30]
10081c38c:      stp x24, x23, [sp, #0x40]
10081c390:      stp x22, x21, [sp, #0x50]
10081c394:      stp x20, x19, [sp, #0x60]
10081c398:      stp x29, x30, [sp, #0x70]
10081c39c:      add x29, sp, #0x70
10081c3a0:      mov x24, x4
10081c3a4:      mov x22, x3
10081c3a8:      mov x19, x2
10081c3ac:      mov x23, x1
10081c3b0:      mov x20, x0
10081c3b4:      cbz w1, 0x10081c3c4 <_js_object_alloc_class_with_keys+0x44>
10081c3b8:      mov x0, x20
10081c3bc:      mov x1, x23
10081c3c0:      bl  0x1006d12f0 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime6object14class_registry13parent_static14register_class>
10081c3c4:      mov w8, #0x2                ; =2
10081c3c8:      cmp w19, #0x2
10081c3cc:      csel    w8, w19, w8, hi
10081c3d0:      ubfiz   x8, x8, #3, #32
10081c3d4:      mov w9, #0x3ffd             ; =16381
10081c3d8:      adrp    x28, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081c3dc:      cmp w19, w9
10081c3e0:      b.ls    0x10081c41c <_js_object_alloc_class_with_keys+0x9c>
10081c3e4:      add x0, x8, #0x10
10081c3e8:      mov w1, #0x8                ; =8
10081c3ec:      mov w2, #0x2                ; =2
10081c3f0:      bl  0x1004d9f18 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators18arena_alloc_gc_old>
10081c3f4:      mov x21, x0
10081c3f8:      ldurb   w8, [x0, #-0x7]
10081c3fc:      orr w8, w8, #0x20
10081c400:      sturb   w8, [x0, #-0x7]
10081c404:      stp w20, w23, [x0]
10081c408:      str xzr, [x0, #0x8]
10081c40c:      lsr x8, x0, #3
10081c410:      cmp x8, #0x201
10081c414:      b.hs    0x10081c59c <_js_object_alloc_class_with_keys+0x21c>
10081c418:      b   0x10081c720 <_js_object_alloc_class_with_keys+0x3a0>
10081c41c:      add x25, x8, #0x18
10081c420:      ldr x8, [x28, #0x48]
10081c424:      cmn x8, #0x1
10081c428:      b.eq    0x10081cc24 <_js_object_alloc_class_with_keys+0x8a4>
10081c42c:      mrs x9, TPIDRRO_EL0
10081c430:      and x9, x9, #0xfffffffffffffff8
10081c434:      ldr x0, [x9, x8, lsl #3]
10081c438:      cbz x0, 0x10081cc24 <_js_object_alloc_class_with_keys+0x8a4>
10081c43c:      ldr x8, [x0, #0x28]
10081c440:      ldrb    w8, [x8]
10081c444:      tbz w8, #0x0, 0x10081c4b0 <_js_object_alloc_class_with_keys+0x130>
10081c448:      ldr x8, [x28, #0x48]
10081c44c:      cmn x8, #0x1
10081c450:      b.eq    0x10081cd98 <_js_object_alloc_class_with_keys+0xa18>
10081c454:      mrs x9, TPIDRRO_EL0
10081c458:      and x9, x9, #0xfffffffffffffff8
10081c45c:      ldr x0, [x9, x8, lsl #3]
10081c460:      cbz x0, 0x10081cd98 <_js_object_alloc_class_with_keys+0xa18>
10081c464:      ldr x26, [x0, #0x20]
10081c468:      ldr x8, [x26]
10081c46c:      cbnz    x8, 0x10081cda8 <_js_object_alloc_class_with_keys+0xa28>
10081c470:      mov x8, #-0x1               ; =-1
10081c474:      str x8, [x26]
10081c478:      ldr x1, [x26, #0x18]
10081c47c:      cbz x1, 0x10081c4ac <_js_object_alloc_class_with_keys+0x12c>
10081c480:      mov x0, #0x0                ; =0
10081c484:      ldr x8, [x26, #0x10]
10081c488:      lsl x10, x1, #4
10081c48c:      mov x9, x8
10081c490:      ldr x11, [x9, #0x8]
10081c494:      cmp x11, x25
10081c498:      b.eq    0x10081c5f0 <_js_object_alloc_class_with_keys+0x270>
10081c49c:      add x0, x0, #0x1
10081c4a0:      add x9, x9, #0x10
10081c4a4:      subs    x10, x10, #0x10
10081c4a8:      b.ne    0x10081c490 <_js_object_alloc_class_with_keys+0x110>
10081c4ac:      str xzr, [x26]
10081c4b0:      mov x0, x25
10081c4b4:      bl  0x1004d9b5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators11arena_alloc>
10081c4b8:      mov w8, #0x2                ; =2
10081c4bc:      strb    w8, [x0]
10081c4c0:      ldr x8, [x28, #0x48]
10081c4c4:      cmn x8, #0x1
10081c4c8:      b.eq    0x10081cc38 <_js_object_alloc_class_with_keys+0x8b8>
10081c4cc:      mrs x9, TPIDRRO_EL0
10081c4d0:      and x9, x9, #0xfffffffffffffff8
10081c4d4:      ldr x8, [x9, x8, lsl #3]
10081c4d8:      cbz x8, 0x10081cc38 <_js_object_alloc_class_with_keys+0x8b8>
10081c4dc:      ldr x8, [x8, #0x30]
10081c4e0:      ldrb    w8, [x8]
10081c4e4:      orr w8, w8, #0x2
10081c4e8:      strb    w8, [x0, #0x1]
10081c4ec:      ldr x8, [x28, #0x48]
10081c4f0:      cmn x8, #0x1
10081c4f4:      b.eq    0x10081cc4c <_js_object_alloc_class_with_keys+0x8cc>
10081c4f8:      mrs x9, TPIDRRO_EL0
10081c4fc:      and x9, x9, #0xfffffffffffffff8
10081c500:      ldr x8, [x9, x8, lsl #3]
10081c504:      cbz x8, 0x10081cc4c <_js_object_alloc_class_with_keys+0x8cc>
10081c508:      ldr x8, [x8, #0x30]
10081c50c:      ldrb    w8, [x8]
10081c510:      tbz w8, #0x0, 0x10081c57c <_js_object_alloc_class_with_keys+0x1fc>
10081c514:      ldrb    w8, [x0]
10081c518:      sub w9, w8, #0x15
10081c51c:      cmn w9, #0x14
10081c520:      b.lo    0x10081c57c <_js_object_alloc_class_with_keys+0x1fc>
10081c524:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081c528:      add x9, x9, #0xb10
10081c52c:      add x8, x9, x8, lsl #5
10081c530:      ldrb    w8, [x8, #0x1b]
10081c534:      tbnz    w8, #0x0, 0x10081c57c <_js_object_alloc_class_with_keys+0x1fc>
10081c538:      mov x26, x0
10081c53c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081c540:      add x0, x0, #0x658
10081c544:      ldr x8, [x0]
10081c548:      blr x8
10081c54c:      mov x21, x0
10081c550:      ldrb    w8, [x0, #0x18]
10081c554:      cbnz    w8, 0x10081cde8 <_js_object_alloc_class_with_keys+0xa68>
10081c558:      ldr x27, [x21, #0x10]
10081c55c:      ldr x8, [x21]
10081c560:      cmp x27, x8
10081c564:      mov x0, x26
10081c568:      b.eq    0x10081ce18 <_js_object_alloc_class_with_keys+0xa98>
10081c56c:      ldr x8, [x21, #0x8]
10081c570:      str x0, [x8, x27, lsl #3]
10081c574:      add x8, x27, #0x1
10081c578:      str x8, [x21, #0x10]
10081c57c:      strh    wzr, [x0, #0x2]
10081c580:      str w25, [x0, #0x4]
10081c584:      add x21, x0, #0x8
10081c588:      stp w20, w23, [x21]
10081c58c:      str xzr, [x21, #0x8]
10081c590:      lsr x8, x21, #3
10081c594:      cmp x8, #0x201
10081c598:      b.lo    0x10081c720 <_js_object_alloc_class_with_keys+0x3a0>
10081c59c:      ldurb   w8, [x21, #-0x8]
10081c5a0:      sub w9, w8, #0x15
10081c5a4:      cmn w9, #0x14
10081c5a8:      b.lo    0x10081c720 <_js_object_alloc_class_with_keys+0x3a0>
10081c5ac:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081c5b0:      add x9, x9, #0xb10
10081c5b4:      add x8, x9, x8, lsl #5
10081c5b8:      ldrb    w8, [x8, #0x14]
10081c5bc:      mov w9, #0x16               ; =22
10081c5c0:      lsr w8, w9, w8
10081c5c4:      tbz w8, #0x0, 0x10081c720 <_js_object_alloc_class_with_keys+0x3a0>
10081c5c8:      ldurh   w8, [x21, #-0x6]
10081c5cc:      mov w9, #0x4000             ; =16384
10081c5d0:      bfxil   w9, w8, #0, #13
10081c5d4:      sturh   w9, [x21, #-0x6]
10081c5d8:      mov x0, x21
10081c5dc:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081c5e0:      ldurh   w8, [x21, #-0x6]
10081c5e4:      and w8, w8, #0xffffefff
10081c5e8:      sturh   w8, [x21, #-0x6]
10081c5ec:      b   0x10081c720 <_js_object_alloc_class_with_keys+0x3a0>
10081c5f0:      cmp x0, x1
10081c5f4:      b.hs    0x10081cde4 <_js_object_alloc_class_with_keys+0xa64>
10081c5f8:      ldr x21, [x9]
10081c5fc:      subs    x10, x1, #0x1
10081c600:      ldr q0, [x8, x10, lsl #4]
10081c604:      str q0, [x9]
10081c608:      str x10, [x26, #0x18]
10081c60c:      b.ne    0x10081c634 <_js_object_alloc_class_with_keys+0x2b4>
10081c610:      ldr x8, [x28, #0x48]
10081c614:      cmn x8, #0x1
10081c618:      b.eq    0x10081cdd0 <_js_object_alloc_class_with_keys+0xa50>
10081c61c:      mrs x9, TPIDRRO_EL0
10081c620:      and x9, x9, #0xfffffffffffffff8
10081c624:      ldr x0, [x9, x8, lsl #3]
10081c628:      cbz x0, 0x10081cdd0 <_js_object_alloc_class_with_keys+0xa50>
10081c62c:      ldr x8, [x0, #0x28]
10081c630:      strb    wzr, [x8]
10081c634:      ldr x8, [x26]
10081c638:      add x8, x8, #0x1
10081c63c:      str x8, [x26]
10081c640:      mov w8, #0x2                ; =2
10081c644:      mov x27, x21
10081c648:      strb    w8, [x27, #-0x8]!
10081c64c:      ldr x8, [x28, #0x48]
10081c650:      cmn x8, #0x1
10081c654:      b.eq    0x10081cdb4 <_js_object_alloc_class_with_keys+0xa34>
10081c658:      mrs x9, TPIDRRO_EL0
10081c65c:      and x9, x9, #0xfffffffffffffff8
10081c660:      ldr x0, [x9, x8, lsl #3]
10081c664:      cbz x0, 0x10081cdb4 <_js_object_alloc_class_with_keys+0xa34>
10081c668:      ldr x8, [x0, #0x30]
10081c66c:      ldrb    w8, [x8]
10081c670:      orr w8, w8, #0x2
10081c674:      sturb   w8, [x21, #-0x7]
10081c678:      ldr x8, [x28, #0x48]
10081c67c:      cmn x8, #0x1
10081c680:      b.eq    0x10081cdbc <_js_object_alloc_class_with_keys+0xa3c>
10081c684:      mrs x9, TPIDRRO_EL0
10081c688:      and x9, x9, #0xfffffffffffffff8
10081c68c:      ldr x0, [x9, x8, lsl #3]
10081c690:      cbz x0, 0x10081cdbc <_js_object_alloc_class_with_keys+0xa3c>
10081c694:      ldr x8, [x0, #0x30]
10081c698:      ldrb    w8, [x8]
10081c69c:      tbz w8, #0x0, 0x10081c704 <_js_object_alloc_class_with_keys+0x384>
10081c6a0:      ldrb    w8, [x27]
10081c6a4:      sub w9, w8, #0x15
10081c6a8:      cmn w9, #0x14
10081c6ac:      b.lo    0x10081c704 <_js_object_alloc_class_with_keys+0x384>
10081c6b0:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081c6b4:      add x9, x9, #0xb10
10081c6b8:      add x8, x9, x8, lsl #5
10081c6bc:      ldrb    w8, [x8, #0x1b]
10081c6c0:      tbnz    w8, #0x0, 0x10081c704 <_js_object_alloc_class_with_keys+0x384>
10081c6c4:      mov x26, x28
10081c6c8:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081c6cc:      add x0, x0, #0x658
10081c6d0:      ldr x8, [x0]
10081c6d4:      blr x8
10081c6d8:      ldrb    w8, [x0, #0x18]
10081c6dc:      cbnz    w8, 0x10081ce28 <_js_object_alloc_class_with_keys+0xaa8>
10081c6e0:      ldr x28, [x0, #0x10]
10081c6e4:      ldr x8, [x0]
10081c6e8:      cmp x28, x8
10081c6ec:      b.eq    0x10081ce64 <_js_object_alloc_class_with_keys+0xae4>
10081c6f0:      ldr x8, [x0, #0x8]
10081c6f4:      str x27, [x8, x28, lsl #3]
10081c6f8:      add x8, x28, #0x1
10081c6fc:      str x8, [x0, #0x10]
10081c700:      mov x28, x26
10081c704:      sturh   wzr, [x21, #-0x6]
10081c708:      stur    w25, [x21, #-0x4]
10081c70c:      stp w20, w23, [x21]
10081c710:      str xzr, [x21, #0x8]
10081c714:      lsr x8, x21, #3
10081c718:      cmp x8, #0x201
10081c71c:      b.hs    0x10081c59c <_js_object_alloc_class_with_keys+0x21c>
10081c720:      mov w8, #0x2717             ; =10007
10081c724:      mov w9, #0x86a3             ; =34467
10081c728:      movk    w9, #0x1, lsl #16
10081c72c:      mul w9, w19, w9
10081c730:      madd    w23, w20, w8, w9
10081c734:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081c738:      ldr w25, [x8, #0x634]
10081c73c:      cmp w25, #0x300
10081c740:      b.hs    0x10081c7b4 <_js_object_alloc_class_with_keys+0x434>
10081c744:      ldr x8, [x28, #0x48]
10081c748:      cmn x8, #0x1
10081c74c:      b.eq    0x10081c7a4 <_js_object_alloc_class_with_keys+0x424>
10081c750:      mrs x9, TPIDRRO_EL0
10081c754:      and x9, x9, #0xfffffffffffffff8
10081c758:      ldr x0, [x9, x8, lsl #3]
10081c75c:      cbz x0, 0x10081c7a4 <_js_object_alloc_class_with_keys+0x424>
10081c760:      add x8, x0, x25, lsl #3
10081c764:      ldr x0, [x8, #0x1e8]
10081c768:      cbz x0, 0x10081c7b4 <_js_object_alloc_class_with_keys+0x434>
10081c76c:      add w8, w23, #0xf4, lsl #12 ; =0xf4000
10081c770:      add w23, w8, #0x240
10081c774:      ldr x0, [x0]
10081c778:      cbz x0, 0x10081c7d0 <_js_object_alloc_class_with_keys+0x450>
10081c77c:      and w8, w23, #0xff
10081c780:      add x8, x0, w8, uxtw #4
10081c784:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081c788:      ldr w9, [x8]
10081c78c:      cmp w9, w23
10081c790:      b.ne    0x10081c7ec <_js_object_alloc_class_with_keys+0x46c>
10081c794:      ldr x26, [x8, #0x8]
10081c798:      cbz x26, 0x10081c8dc <_js_object_alloc_class_with_keys+0x55c>
10081c79c:      ldr w27, [x8, #0x4]
10081c7a0:      b   0x10081cbd0 <_js_object_alloc_class_with_keys+0x850>
10081c7a4:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c7a8:      add x8, x0, x25, lsl #3
10081c7ac:      ldr x0, [x8, #0x1e8]
10081c7b0:      cbnz    x0, 0x10081c76c <_js_object_alloc_class_with_keys+0x3ec>
10081c7b4:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081c7b8:      add x0, x0, #0x330
10081c7bc:      bl  0x100ad8a1c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081c7c0:      add w8, w23, #0xf4, lsl #12 ; =0xf4000
10081c7c4:      add w23, w8, #0x240
10081c7c8:      ldr x0, [x0]
10081c7cc:      cbnz    x0, 0x10081c77c <_js_object_alloc_class_with_keys+0x3fc>
10081c7d0:      bl  0x100ada6ec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081c7d4:      and w8, w23, #0xff
10081c7d8:      add x8, x0, w8, uxtw #4
10081c7dc:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081c7e0:      ldr w9, [x8]
10081c7e4:      cmp w9, w23
10081c7e8:      b.eq    0x10081c794 <_js_object_alloc_class_with_keys+0x414>
10081c7ec:      ldr x8, [x0, #0x5088]
10081c7f0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081c7f4:      cmp x8, x9
10081c7f8:      b.hs    0x10081cdd8 <_js_object_alloc_class_with_keys+0xa58>
10081c7fc:      add x9, x8, #0x1
10081c800:      str x9, [x0, #0x5088]
10081c804:      ldr x9, [x0, #0x50a8]
10081c808:      cbz x9, 0x10081c8b8 <_js_object_alloc_class_with_keys+0x538>
10081c80c:      mov x9, #0x0                ; =0
10081c810:      mov x10, #0x7c15            ; =31765
10081c814:      movk    x10, #0x7f4a, lsl #16
10081c818:      movk    x10, #0x79b9, lsl #32
10081c81c:      movk    x10, #0x9e37, lsl #48
10081c820:      mul x10, x23, x10
10081c824:      eor x13, x10, x10, lsr #32
10081c828:      lsr x12, x10, #57
10081c82c:      ldr x10, [x0, #0x5098]
10081c830:      ldr x11, [x0, #0x5090]
10081c834:      dup.8b  v0, w12
10081c838:      movi.2d v1, #0xffffffffffffffff
10081c83c:      mov w12, #0x18              ; =24
10081c840:      and x13, x13, x10
10081c844:      ldr d2, [x11, x13]
10081c848:      cmeq.8b v3, v2, v0
10081c84c:      fmov    x14, d3
10081c850:      ands    x14, x14, #0x8080808080808080
10081c854:      b.eq    0x10081c888 <_js_object_alloc_class_with_keys+0x508>
10081c858:      rbit    x15, x14
10081c85c:      clz x15, x15
10081c860:      add x15, x13, x15, lsr #3
10081c864:      and x15, x15, x10
10081c868:      mneg    x15, x15, x12
10081c86c:      add x15, x11, x15
10081c870:      ldur    w16, [x15, #-0x18]
10081c874:      cmp w23, w16
10081c878:      b.eq    0x10081c8cc <_js_object_alloc_class_with_keys+0x54c>
10081c87c:      sub x15, x14, #0x2
10081c880:      ands    x14, x15, x14
10081c884:      b.ne    0x10081c858 <_js_object_alloc_class_with_keys+0x4d8>
10081c888:      cmeq.8b v2, v2, v1
10081c88c:      fmov    x14, d2
10081c890:      cbnz    x14, 0x10081c8b8 <_js_object_alloc_class_with_keys+0x538>
10081c894:      add x9, x9, #0x8
10081c898:      add x13, x13, x9
10081c89c:      and x13, x13, x10
10081c8a0:      ldr d2, [x11, x13]
10081c8a4:      cmeq.8b v3, v2, v0
10081c8a8:      fmov    x14, d3
10081c8ac:      ands    x14, x14, #0x8080808080808080
10081c8b0:      b.ne    0x10081c858 <_js_object_alloc_class_with_keys+0x4d8>
10081c8b4:      b   0x10081c888 <_js_object_alloc_class_with_keys+0x508>
10081c8b8:      mov w27, #0x0               ; =0
10081c8bc:      mov x26, #0x0               ; =0
10081c8c0:      str x8, [x0, #0x5088]
10081c8c4:      cbnz    x26, 0x10081cbd0 <_js_object_alloc_class_with_keys+0x850>
10081c8c8:      b   0x10081c8dc <_js_object_alloc_class_with_keys+0x55c>
10081c8cc:      ldur    x26, [x15, #-0x10]
10081c8d0:      ldur    w27, [x15, #-0x8]
10081c8d4:      str x8, [x0, #0x5088]
10081c8d8:      cbnz    x26, 0x10081cbd0 <_js_object_alloc_class_with_keys+0x850>
10081c8dc:      mov w26, #0x0               ; =0
10081c8e0:      mov w8, #0x0                ; =0
10081c8e4:      mov w9, #0x8                ; =8
10081c8e8:      str x9, [sp]
10081c8ec:      mov w27, w24
10081c8f0:      b   0x10081c904 <_js_object_alloc_class_with_keys+0x584>
10081c8f4:      mov w26, #0x1               ; =1
10081c8f8:      mov x22, x24
10081c8fc:      mov w8, #0x1                ; =1
10081c900:      cbnz    x28, 0x10081c95c <_js_object_alloc_class_with_keys+0x5dc>
10081c904:      tbnz    w8, #0x0, 0x10081c950 <_js_object_alloc_class_with_keys+0x5d0>
10081c908:      mov x24, x22
10081c90c:      mov x28, #0x0               ; =0
10081c910:      cbz x27, 0x10081c8f4 <_js_object_alloc_class_with_keys+0x574>
10081c914:      ldrb    w8, [x24, x28]
10081c918:      cbz w8, 0x10081c93c <_js_object_alloc_class_with_keys+0x5bc>
10081c91c:      add x28, x28, #0x1
10081c920:      cmp x27, x28
10081c924:      b.ne    0x10081c914 <_js_object_alloc_class_with_keys+0x594>
10081c928:      mov w26, #0x1               ; =1
10081c92c:      mov x22, x24
10081c930:      mov w8, #0x1                ; =1
10081c934:      mov x28, x27
10081c938:      b   0x10081c900 <_js_object_alloc_class_with_keys+0x580>
10081c93c:      mvn x9, x28
10081c940:      add x27, x9, x27
10081c944:      add x9, x24, x28
10081c948:      add x22, x9, #0x1
10081c94c:      b   0x10081c900 <_js_object_alloc_class_with_keys+0x580>
10081c950:      mov x24, #0x0               ; =0
10081c954:      mov w25, #0x1               ; =1
10081c958:      b   0x10081ca58 <_js_object_alloc_class_with_keys+0x6d8>
10081c95c:      cbz x24, 0x10081ca4c <_js_object_alloc_class_with_keys+0x6cc>
10081c960:      mov w0, #0x40               ; =64
10081c964:      mov w1, #0x8                ; =8
10081c968:      bl  0x100af5c08 <_mi_malloc_aligned>
10081c96c:      cbz x0, 0x10081ce74 <_js_object_alloc_class_with_keys+0xaf4>
10081c970:      stp x24, x28, [x0]
10081c974:      mov w8, #0x4                ; =4
10081c978:      stp x8, x0, [sp, #0x8]
10081c97c:      mov w24, #0x1               ; =1
10081c980:      str x24, [sp, #0x18]
10081c984:      mov x8, x27
10081c988:      mov x10, x22
10081c98c:      mov x9, x26
10081c990:      b   0x10081c9a4 <_js_object_alloc_class_with_keys+0x624>
10081c994:      mov w26, #0x1               ; =1
10081c998:      mov x10, x25
10081c99c:      mov w9, #0x1                ; =1
10081c9a0:      cbnz    x28, 0x10081c9f8 <_js_object_alloc_class_with_keys+0x678>
10081c9a4:      tbnz    w9, #0x0, 0x10081ca38 <_js_object_alloc_class_with_keys+0x6b8>
10081c9a8:      mov x25, x10
10081c9ac:      mov x28, #0x0               ; =0
10081c9b0:      cbz x8, 0x10081c994 <_js_object_alloc_class_with_keys+0x614>
10081c9b4:      ldrb    w9, [x25, x28]
10081c9b8:      cbz w9, 0x10081c9dc <_js_object_alloc_class_with_keys+0x65c>
10081c9bc:      add x28, x28, #0x1
10081c9c0:      cmp x8, x28
10081c9c4:      b.ne    0x10081c9b4 <_js_object_alloc_class_with_keys+0x634>
10081c9c8:      mov w26, #0x1               ; =1
10081c9cc:      mov x10, x25
10081c9d0:      mov w9, #0x1                ; =1
10081c9d4:      mov x28, x8
10081c9d8:      b   0x10081c9a0 <_js_object_alloc_class_with_keys+0x620>
10081c9dc:      mvn x10, x28
10081c9e0:      add x27, x10, x8
10081c9e4:      add x8, x25, x28
10081c9e8:      add x22, x8, #0x1
10081c9ec:      mov x8, x27
10081c9f0:      mov x10, x22
10081c9f4:      b   0x10081c9a0 <_js_object_alloc_class_with_keys+0x620>
10081c9f8:      cbz x25, 0x10081ca38 <_js_object_alloc_class_with_keys+0x6b8>
10081c9fc:      ldr x8, [sp, #0x8]
10081ca00:      cmp x24, x8
10081ca04:      b.eq    0x10081ca18 <_js_object_alloc_class_with_keys+0x698>
10081ca08:      add x8, x0, x24, lsl #4
10081ca0c:      stp x25, x28, [x8]
10081ca10:      add x24, x24, #0x1
10081ca14:      b   0x10081c980 <_js_object_alloc_class_with_keys+0x600>
10081ca18:      add x0, sp, #0x8
10081ca1c:      mov x1, x24
10081ca20:      mov w2, #0x1                ; =1
10081ca24:      mov w3, #0x8                ; =8
10081ca28:      mov w4, #0x10               ; =16
10081ca2c:      bl  0x100ad5290 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs7wa3bwJW6LN_13perry_runtime>
10081ca30:      ldr x0, [sp, #0x10]
10081ca34:      b   0x10081ca08 <_js_object_alloc_class_with_keys+0x688>
10081ca38:      ldp x8, x9, [sp, #0x8]
10081ca3c:      str x9, [sp]
10081ca40:      cmp x8, #0x0
10081ca44:      cset    w25, eq
10081ca48:      b   0x10081ca58 <_js_object_alloc_class_with_keys+0x6d8>
10081ca4c:      mov w25, #0x1               ; =1
10081ca50:      mov w8, #0x8                ; =8
10081ca54:      str x8, [sp]
10081ca58:      ubfiz   x8, x24, #3, #32
10081ca5c:      add x0, x8, #0x8
10081ca60:      mov w1, #0x8                ; =8
10081ca64:      mov w2, #0x1                ; =1
10081ca68:      bl  0x1004da9a0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators24arena_alloc_gc_longlived>
10081ca6c:      mov x26, x0
10081ca70:      stp w24, w24, [x0]
10081ca74:      lsr x8, x0, #3
10081ca78:      cmp x8, #0x201
10081ca7c:      b.lo    0x10081cb0c <_js_object_alloc_class_with_keys+0x78c>
10081ca80:      ldurb   w8, [x26, #-0x8]
10081ca84:      cmp w8, #0x1
10081ca88:      b.ne    0x10081cab4 <_js_object_alloc_class_with_keys+0x734>
10081ca8c:      ldurh   w8, [x26, #-0x6]
10081ca90:      mov w9, #0x1080             ; =4224
10081ca94:      mov w10, #0xef7f            ; =61311
10081ca98:      and w10, w8, w10
10081ca9c:      sturh   w10, [x26, #-0x6]
10081caa0:      tst w8, w9
10081caa4:      b.eq    0x10081cac4 <_js_object_alloc_class_with_keys+0x744>
10081caa8:      mov x0, x26
10081caac:      bl  0x1002b6ec0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime14typed_feedback32invalidate_representation_change>
10081cab0:      ldurb   w8, [x26, #-0x8]
10081cab4:      sub w9, w8, #0x15
10081cab8:      cmn w9, #0x14
10081cabc:      b.hs    0x10081cac8 <_js_object_alloc_class_with_keys+0x748>
10081cac0:      b   0x10081cb0c <_js_object_alloc_class_with_keys+0x78c>
10081cac4:      mov w8, #0x1                ; =1
10081cac8:      mov w8, w8
10081cacc:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xd8>
10081cad0:      add x9, x9, #0xb10
10081cad4:      add x8, x9, x8, lsl #5
10081cad8:      ldrb    w8, [x8, #0x14]
10081cadc:      mov w9, #0x16               ; =22
10081cae0:      lsr w8, w9, w8
10081cae4:      tbz w8, #0x0, 0x10081cb0c <_js_object_alloc_class_with_keys+0x78c>
10081cae8:      ldurh   w8, [x26, #-0x6]
10081caec:      mov w9, #0x4000             ; =16384
10081caf0:      bfxil   w9, w8, #0, #13
10081caf4:      sturh   w9, [x26, #-0x6]
10081caf8:      mov x0, x26
10081cafc:      bl  0x100413b38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081cb00:      ldurh   w8, [x26, #-0x6]
10081cb04:      and w8, w8, #0xffffefff
10081cb08:      sturh   w8, [x26, #-0x6]
10081cb0c:      cbz x24, 0x10081cb58 <_js_object_alloc_class_with_keys+0x7d8>
10081cb10:      mov x22, #0x0               ; =0
10081cb14:      ldr x27, [sp]
10081cb18:      add x24, x27, x24, lsl #4
10081cb1c:      add x28, x22, #0x1
10081cb20:      ldr x0, [x27]
10081cb24:      ldr w1, [x27, #0x8]
10081cb28:      bl  0x100884854 <_js_string_from_bytes_longlived>
10081cb2c:      mov x2, #0x7fff000000000000 ; =9223090561878065152
10081cb30:      bfxil   x2, x0, #0, #48
10081cb34:      add x8, x26, x22, lsl #3
10081cb38:      str x2, [x8, #0x8]
10081cb3c:      mov x0, x26
10081cb40:      mov x1, x22
10081cb44:      bl  0x1004f079c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array15header_gc_slots27note_array_slot_layout_only>
10081cb48:      add x27, x27, #0x10
10081cb4c:      mov x22, x28
10081cb50:      cmp x27, x24
10081cb54:      b.ne    0x10081cb1c <_js_object_alloc_class_with_keys+0x79c>
10081cb58:      mov x0, x23
10081cb5c:      mov x1, x26
10081cb60:      bl  0x10032203c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object18shape_cache_insert>
10081cb64:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081cb68:      ldr w22, [x8, #0x634]
10081cb6c:      cmp w22, #0x300
10081cb70:      b.hs    0x10081cc7c <_js_object_alloc_class_with_keys+0x8fc>
10081cb74:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081cb78:      ldr x8, [x8, #0x48]
10081cb7c:      cmn x8, #0x1
10081cb80:      b.eq    0x10081cc6c <_js_object_alloc_class_with_keys+0x8ec>
10081cb84:      mrs x9, TPIDRRO_EL0
10081cb88:      and x9, x9, #0xfffffffffffffff8
10081cb8c:      ldr x0, [x9, x8, lsl #3]
10081cb90:      cbz x0, 0x10081cc6c <_js_object_alloc_class_with_keys+0x8ec>
10081cb94:      add x8, x0, x22, lsl #3
10081cb98:      ldr x0, [x8, #0x1e8]
10081cb9c:      cbz x0, 0x10081cc7c <_js_object_alloc_class_with_keys+0x8fc>
10081cba0:      ldr x0, [x0]
10081cba4:      cbz x0, 0x10081cc90 <_js_object_alloc_class_with_keys+0x910>
10081cba8:      and w8, w23, #0xff
10081cbac:      add x8, x0, x8, lsl #4
10081cbb0:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081cbb4:      ldr w9, [x8]
10081cbb8:      cmp w9, w23
10081cbbc:      b.ne    0x10081ccac <_js_object_alloc_class_with_keys+0x92c>
10081cbc0:      ldr w27, [x8, #0x4]
10081cbc4:      tbnz    w25, #0x0, 0x10081cbd0 <_js_object_alloc_class_with_keys+0x850>
10081cbc8:      ldr x0, [sp]
10081cbcc:      bl  0x100af5980 <_mi_free>
10081cbd0:      mov x0, x21
10081cbd4:      mov x1, x26
10081cbd8:      mov x2, x19
10081cbdc:      bl  0x100324600 <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object31set_object_keys_array_with_live>
10081cbe0:      mov x0, x21
10081cbe4:      mov x1, x27
10081cbe8:      mov x2, x19
10081cbec:      bl  0x100597880 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes24birth_stamp_object_shape>
10081cbf0:      mov x0, x20
10081cbf4:      mov x1, x19
10081cbf8:      mov x2, x26
10081cbfc:      bl  0x100591500 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object5alloc25remember_class_keys_array>
10081cc00:      mov x0, x21
10081cc04:      ldp x29, x30, [sp, #0x70]
10081cc08:      ldp x20, x19, [sp, #0x60]
10081cc0c:      ldp x22, x21, [sp, #0x50]
10081cc10:      ldp x24, x23, [sp, #0x40]
10081cc14:      ldp x26, x25, [sp, #0x30]
10081cc18:      ldp x28, x27, [sp, #0x20]
10081cc1c:      add sp, sp, #0x80
10081cc20:      ret
10081cc24:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081cc28:      ldr x8, [x0, #0x28]
10081cc2c:      ldrb    w8, [x8]
10081cc30:      tbnz    w8, #0x0, 0x10081c448 <_js_object_alloc_class_with_keys+0xc8>
10081cc34:      b   0x10081c4b0 <_js_object_alloc_class_with_keys+0x130>
10081cc38:      mov x21, x0
10081cc3c:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081cc40:      mov x8, x0
10081cc44:      mov x0, x21
10081cc48:      b   0x10081c4dc <_js_object_alloc_class_with_keys+0x15c>
10081cc4c:      mov x21, x0
10081cc50:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081cc54:      mov x8, x0
10081cc58:      mov x0, x21
10081cc5c:      ldr x8, [x8, #0x30]
10081cc60:      ldrb    w8, [x8]
10081cc64:      tbnz    w8, #0x0, 0x10081c514 <_js_object_alloc_class_with_keys+0x194>
10081cc68:      b   0x10081c57c <_js_object_alloc_class_with_keys+0x1fc>
10081cc6c:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081cc70:      add x8, x0, x22, lsl #3
10081cc74:      ldr x0, [x8, #0x1e8]
10081cc78:      cbnz    x0, 0x10081cba0 <_js_object_alloc_class_with_keys+0x820>
10081cc7c:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081cc80:      add x0, x0, #0x330
10081cc84:      bl  0x100ad8a1c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081cc88:      ldr x0, [x0]
10081cc8c:      cbnz    x0, 0x10081cba8 <_js_object_alloc_class_with_keys+0x828>
10081cc90:      bl  0x100ada6ec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081cc94:      and w8, w23, #0xff
10081cc98:      add x8, x0, x8, lsl #4
10081cc9c:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081cca0:      ldr w9, [x8]
10081cca4:      cmp w9, w23
10081cca8:      b.eq    0x10081cbc0 <_js_object_alloc_class_with_keys+0x840>
10081ccac:      ldr x8, [x0, #0x5088]
10081ccb0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081ccb4:      cmp x8, x9
10081ccb8:      b.hs    0x10081cdd8 <_js_object_alloc_class_with_keys+0xa58>
10081ccbc:      add x9, x8, #0x1
10081ccc0:      str x9, [x0, #0x5088]
10081ccc4:      ldr x9, [x0, #0x50a8]
10081ccc8:      cbz x9, 0x10081cd78 <_js_object_alloc_class_with_keys+0x9f8>
10081cccc:      mov x9, #0x0                ; =0
10081ccd0:      mov x10, #0x7c15            ; =31765
10081ccd4:      movk    x10, #0x7f4a, lsl #16
10081ccd8:      movk    x10, #0x79b9, lsl #32
10081ccdc:      movk    x10, #0x9e37, lsl #48
10081cce0:      mul x10, x23, x10
10081cce4:      eor x13, x10, x10, lsr #32
10081cce8:      lsr x12, x10, #57
10081ccec:      ldr x10, [x0, #0x5098]
10081ccf0:      ldr x11, [x0, #0x5090]
10081ccf4:      dup.8b  v0, w12
10081ccf8:      movi.2d v1, #0xffffffffffffffff
10081ccfc:      mov w12, #0x18              ; =24
10081cd00:      and x13, x13, x10
10081cd04:      ldr d2, [x11, x13]
10081cd08:      cmeq.8b v3, v2, v0
10081cd0c:      fmov    x14, d3
10081cd10:      ands    x14, x14, #0x8080808080808080
10081cd14:      b.eq    0x10081cd48 <_js_object_alloc_class_with_keys+0x9c8>
10081cd18:      rbit    x15, x14
10081cd1c:      clz x15, x15
10081cd20:      add x15, x13, x15, lsr #3
10081cd24:      and x15, x15, x10
10081cd28:      mneg    x15, x15, x12
10081cd2c:      add x15, x11, x15
10081cd30:      ldur    w16, [x15, #-0x18]
10081cd34:      cmp w23, w16
10081cd38:      b.eq    0x10081cd88 <_js_object_alloc_class_with_keys+0xa08>
10081cd3c:      sub x15, x14, #0x2
10081cd40:      ands    x14, x15, x14
10081cd44:      b.ne    0x10081cd18 <_js_object_alloc_class_with_keys+0x998>
10081cd48:      cmeq.8b v2, v2, v1
10081cd4c:      fmov    x14, d2
10081cd50:      cbnz    x14, 0x10081cd78 <_js_object_alloc_class_with_keys+0x9f8>
10081cd54:      add x9, x9, #0x8
10081cd58:      add x13, x13, x9
10081cd5c:      and x13, x13, x10
10081cd60:      ldr d2, [x11, x13]
10081cd64:      cmeq.8b v3, v2, v0
10081cd68:      fmov    x14, d3
10081cd6c:      ands    x14, x14, #0x8080808080808080
10081cd70:      b.ne    0x10081cd18 <_js_object_alloc_class_with_keys+0x998>
10081cd74:      b   0x10081cd48 <_js_object_alloc_class_with_keys+0x9c8>
10081cd78:      mov w27, #0x0               ; =0
10081cd7c:      str x8, [x0, #0x5088]
10081cd80:      tbz w25, #0x0, 0x10081cbc8 <_js_object_alloc_class_with_keys+0x848>
10081cd84:      b   0x10081cbd0 <_js_object_alloc_class_with_keys+0x850>
10081cd88:      ldur    w27, [x15, #-0x8]
10081cd8c:      str x8, [x0, #0x5088]
10081cd90:      tbz w25, #0x0, 0x10081cbc8 <_js_object_alloc_class_with_keys+0x848>
10081cd94:      b   0x10081cbd0 <_js_object_alloc_class_with_keys+0x850>
10081cd98:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081cd9c:      ldr x26, [x0, #0x20]
10081cda0:      ldr x8, [x26]
10081cda4:      cbz x8, 0x10081c470 <_js_object_alloc_class_with_keys+0xf0>
10081cda8:      adrp    x0, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ef0>
10081cdac:      add x0, x0, #0x120
10081cdb0:      bl  0x100ab0bac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10081cdb4:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081cdb8:      b   0x10081c668 <_js_object_alloc_class_with_keys+0x2e8>
10081cdbc:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081cdc0:      ldr x8, [x0, #0x30]
10081cdc4:      ldrb    w8, [x8]
10081cdc8:      tbnz    w8, #0x0, 0x10081c6a0 <_js_object_alloc_class_with_keys+0x320>
10081cdcc:      b   0x10081c704 <_js_object_alloc_class_with_keys+0x384>
10081cdd0:      bl  0x100adb5b0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081cdd4:      b   0x10081c62c <_js_object_alloc_class_with_keys+0x2ac>
10081cdd8:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
10081cddc:      add x0, x0, #0x798
10081cde0:      bl  0x100ab0bdc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10081cde4:      bl  0x100ab087c <__RNvNvMs_NtCsctvjasLqLe9_5alloc3vecINtB6_3VecppE11swap_remove13assert_failed>
10081cde8:      cmp w8, #0x1
10081cdec:      b.ne    0x10081ce30 <_js_object_alloc_class_with_keys+0xab0>
10081cdf0:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081cdf4:      add x1, x1, #0xbb8
10081cdf8:      mov x0, x21
10081cdfc:      bl  0x1009be91c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081ce00:      strb    wzr, [x21, #0x18]
10081ce04:      ldr x27, [x21, #0x10]
10081ce08:      ldr x8, [x21]
10081ce0c:      cmp x27, x8
10081ce10:      mov x0, x26
10081ce14:      b.ne    0x10081c56c <_js_object_alloc_class_with_keys+0x1ec>
10081ce18:      mov x0, x21
10081ce1c:      bl  0x100ad7b24 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081ce20:      mov x0, x26
10081ce24:      b   0x10081c56c <_js_object_alloc_class_with_keys+0x1ec>
10081ce28:      cmp w8, #0x2
10081ce2c:      b.ne    0x10081ce3c <_js_object_alloc_class_with_keys+0xabc>
10081ce30:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
10081ce34:      add x0, x0, #0x850
10081ce38:      bl  0x100aeefdc <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10081ce3c:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081ce40:      add x1, x1, #0xbb8
10081ce44:      mov x28, x0
10081ce48:      bl  0x1009be91c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081ce4c:      mov x0, x28
10081ce50:      strb    wzr, [x28, #0x18]
10081ce54:      ldr x28, [x28, #0x10]
10081ce58:      ldr x8, [x0]
10081ce5c:      cmp x28, x8
10081ce60:      b.ne    0x10081c6f0 <_js_object_alloc_class_with_keys+0x370>
10081ce64:      str x0, [sp]
10081ce68:      bl  0x100ad7b24 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081ce6c:      ldr x0, [sp]
10081ce70:      b   0x10081c6f0 <_js_object_alloc_class_with_keys+0x370>
10081ce74:      mov w0, #0x8                ; =8
10081ce78:      mov w1, #0x40               ; =64
10081ce7c:      bl  0x100ab0824 <__RNvNtCsctvjasLqLe9_5alloc7raw_vec12handle_error>
