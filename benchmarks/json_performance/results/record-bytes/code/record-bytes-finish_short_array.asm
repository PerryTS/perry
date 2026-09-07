/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010033b574 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array>:
10033b574:      sub sp, sp, #0x80
10033b578:      stp d9, d8, [sp, #0x10]
10033b57c:      stp x28, x27, [sp, #0x20]
10033b580:      stp x26, x25, [sp, #0x30]
10033b584:      stp x24, x23, [sp, #0x40]
10033b588:      stp x22, x21, [sp, #0x50]
10033b58c:      stp x20, x19, [sp, #0x60]
10033b590:      stp x29, x30, [sp, #0x70]
10033b594:      add x29, sp, #0x70
10033b598:      str x2, [sp, #0x8]
10033b59c:      mov x23, x1
10033b5a0:      mov x21, x0
10033b5a4:      mov x0, x23
10033b5a8:      bl  0x10037a438 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array5alloc32js_array_alloc_with_length_exact>
10033b5ac:      mov x20, x0
10033b5b0:      bl  0x10037beb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header24set_array_numeric_layout>
10033b5b4:      cbz x23, 0x10033b784 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x210>
10033b5b8:      mov x22, #0x0               ; =0
10033b5bc:      lsl x26, x23, #3
10033b5c0:      add x23, x20, #0x8
10033b5c4:      mov w27, #0x8               ; =8
10033b5c8:      mov w28, #0x7ffe            ; =32766
10033b5cc:      mov x19, #0x7ff8ffffffffffff ; =9221401712017801215
10033b5d0:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
10033b5d4:      fmov    d8, x8
10033b5d8:      lsr x24, x20, #3
10033b5dc:      b   0x10033b608 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x94>
10033b5e0:      mov x0, x20
10033b5e4:      mov x1, x23
10033b5e8:      mov x2, x25
10033b5ec:      mov w3, #0x0                ; =0
10033b5f0:      bl  0x100680e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
10033b5f4:      add x23, x23, #0x8
10033b5f8:      add x27, x27, #0x8
10033b5fc:      add x22, x22, #0x1
10033b600:      subs    x26, x26, #0x8
10033b604:      b.eq    0x10033b784 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x210>
10033b608:      ldr x25, [x21, x22, lsl #3]
10033b60c:      str x25, [x20, x27]
10033b610:      mov x0, x20
10033b614:      bl  0x10037afc4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
10033b618:      tbz w0, #0x0, 0x10033b638 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xc4>
10033b61c:      cmp x28, x25, lsr #48
10033b620:      b.ne    0x10033b658 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xe4>
10033b624:      mov x0, x25
10033b628:      bl  0x1008033fc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10033b62c:      tbnz    w0, #0x0, 0x10033b670 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10033b630:      scvtf   d0, w25
10033b634:      b   0x10033b66c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xf8>
10033b638:      cmp x24, #0x201
10033b63c:      b.lo    0x10033b670 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10033b640:      ldurb   w8, [x20, #-0x8]
10033b644:      cmp w8, #0x1
10033b648:      b.ne    0x10033b670 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10033b64c:      ldurh   w8, [x20, #-0x6]
10033b650:      tbnz    w8, #0xc, 0x10033b61c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xa8>
10033b654:      b   0x10033b670 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10033b658:      cmp x25, x19
10033b65c:      b.gt    0x10033b670 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10033b660:      fmov    d0, x25
10033b664:      fcmp    d0, d0
10033b668:      fcsel   d0, d8, d0, vs
10033b66c:      fmov    x25, d0
10033b670:      str x25, [x20, x27]
10033b674:      cmp x28, x25, lsr #48
10033b678:      b.ne    0x10033b698 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x124>
10033b67c:      mov x0, x25
10033b680:      bl  0x1008033fc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10033b684:      tbnz    w0, #0x0, 0x10033b6a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x12c>
10033b688:      scvtf   d0, w25
10033b68c:      cmp x24, #0x201
10033b690:      b.hs    0x10033b6f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x17c>
10033b694:      b   0x10033b714 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10033b698:      cmp x25, x19
10033b69c:      b.le    0x10033b6dc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x168>
10033b6a0:      cmp x24, #0x201
10033b6a4:      b.lo    0x10033b714 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10033b6a8:      ldurb   w8, [x20, #-0x8]
10033b6ac:      cmp w8, #0x1
10033b6b0:      b.ne    0x10033b714 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10033b6b4:      ldurh   w8, [x20, #-0x6]
10033b6b8:      mov w9, #0xef7f             ; =61311
10033b6bc:      and w9, w8, w9
10033b6c0:      sturh   w9, [x20, #-0x6]
10033b6c4:      mov w9, #0x1080             ; =4224
10033b6c8:      tst w8, w9
10033b6cc:      b.eq    0x10033b714 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10033b6d0:      mov x0, x20
10033b6d4:      bl  0x10066de20 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
10033b6d8:      b   0x10033b714 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10033b6dc:      fmov    d0, x25
10033b6e0:      fcmp    d0, d0
10033b6e4:      fcsel   d0, d8, d0, vs
10033b6e8:      cmp x24, #0x201
10033b6ec:      b.lo    0x10033b714 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10033b6f0:      ldurb   w8, [x20, #-0x8]
10033b6f4:      cmp w8, #0x1
10033b6f8:      b.ne    0x10033b714 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10033b6fc:      ldurh   w8, [x20, #-0x6]
10033b700:      tbz w8, #0x7, 0x10033b714 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10033b704:      ldr w8, [x20]
10033b708:      cmp x22, x8
10033b70c:      b.hs    0x10033b714 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10033b710:      str d0, [x20, x27]
10033b714:      mov x0, x20
10033b718:      mov x1, x22
10033b71c:      mov x2, x25
10033b720:      bl  0x100242640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
10033b724:      mov x0, x20
10033b728:      bl  0x10036cee4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
10033b72c:      cbz w0, 0x10033b5f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
10033b730:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
10033b734:      add x8, x8, #0x1b8
10033b738:      ldapr   x8, [x8]
10033b73c:      cbnz    x8, 0x10033b768 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1f4>
10033b740:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
10033b744:      ldrb    w8, [x8, #0x1c0]
10033b748:      tbnz    w8, #0x0, 0x10033b5e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x6c>
10033b74c:      adrp    x8, 0x101201000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc11instruments24INCREMENTAL_CYCLE_STARTS>
10033b750:      add x8, x8, #0x184
10033b754:      ldr w8, [x8]
10033b758:      cbz w8, 0x10033b5f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
10033b75c:      mov x0, x25
10033b760:      bl  0x100682074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
10033b764:      b   0x10033b5f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
10033b768:      adrp    x0, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
10033b76c:      add x0, x0, #0x1b8
10033b770:      bl  0x100cc433c <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
10033b774:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
10033b778:      ldrb    w8, [x8, #0x1c0]
10033b77c:      tbnz    w8, #0x0, 0x10033b5e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x6c>
10033b780:      b   0x10033b74c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1d8>
10033b784:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
10033b788:      add x0, x0, #0x398
10033b78c:      ldr x8, [x0]
10033b790:      blr x8
10033b794:      ldrb    w8, [x0, #0x20]
10033b798:      cbnz    w8, 0x10033b7e4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x270>
10033b79c:      ldr x8, [x0]
10033b7a0:      cbnz    x8, 0x10033b80c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x298>
10033b7a4:      ldr x8, [x0, #0x18]
10033b7a8:      ldr x9, [sp, #0x8]
10033b7ac:      cmp x9, x8
10033b7b0:      b.hi    0x10033b7b8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x244>
10033b7b4:      str x9, [x0, #0x18]
10033b7b8:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
10033b7bc:      bfxil   x0, x20, #0, #48
10033b7c0:      ldp x29, x30, [sp, #0x70]
10033b7c4:      ldp x20, x19, [sp, #0x60]
10033b7c8:      ldp x22, x21, [sp, #0x50]
10033b7cc:      ldp x24, x23, [sp, #0x40]
10033b7d0:      ldp x26, x25, [sp, #0x30]
10033b7d4:      ldp x28, x27, [sp, #0x20]
10033b7d8:      ldp d9, d8, [sp, #0x10]
10033b7dc:      add sp, sp, #0x80
10033b7e0:      ret
10033b7e4:      cmp w8, #0x1
10033b7e8:      b.ne    0x10033b818 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x2a4>
10033b7ec:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10033b7f0:      add x1, x1, #0x87c
10033b7f4:      mov x21, x0
10033b7f8:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10033b7fc:      mov x0, x21
10033b800:      strb    wzr, [x21, #0x20]
10033b804:      ldr x8, [x21]
10033b808:      cbz x8, 0x10033b7a4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x230>
10033b80c:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10033b810:      add x0, x0, #0xe58
10033b814:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10033b818:      adrp    x0, 0x1010a3000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10033b81c:      add x0, x0, #0xed8
10033b820:      bl  0x100ce071c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
