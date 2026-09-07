/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010028b474 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array>:
10028b474:      sub sp, sp, #0x80
10028b478:      stp d9, d8, [sp, #0x10]
10028b47c:      stp x28, x27, [sp, #0x20]
10028b480:      stp x26, x25, [sp, #0x30]
10028b484:      stp x24, x23, [sp, #0x40]
10028b488:      stp x22, x21, [sp, #0x50]
10028b48c:      stp x20, x19, [sp, #0x60]
10028b490:      stp x29, x30, [sp, #0x70]
10028b494:      add x29, sp, #0x70
10028b498:      str x2, [sp, #0x8]
10028b49c:      mov x23, x1
10028b4a0:      mov x21, x0
10028b4a4:      mov x0, x23
10028b4a8:      bl  0x1002cd8c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array5alloc32js_array_alloc_with_length_exact>
10028b4ac:      mov x20, x0
10028b4b0:      bl  0x1002cf378 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header24set_array_numeric_layout>
10028b4b4:      cbz x23, 0x10028b684 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x210>
10028b4b8:      mov x22, #0x0               ; =0
10028b4bc:      lsl x26, x23, #3
10028b4c0:      add x23, x20, #0x8
10028b4c4:      mov w27, #0x8               ; =8
10028b4c8:      mov w28, #0x7ffe            ; =32766
10028b4cc:      mov x19, #0x7ff8ffffffffffff ; =9221401712017801215
10028b4d0:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
10028b4d4:      fmov    d8, x8
10028b4d8:      lsr x24, x20, #3
10028b4dc:      b   0x10028b508 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x94>
10028b4e0:      mov x0, x20
10028b4e4:      mov x1, x23
10028b4e8:      mov x2, x25
10028b4ec:      mov w3, #0x0                ; =0
10028b4f0:      bl  0x1004357e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
10028b4f4:      add x23, x23, #0x8
10028b4f8:      add x27, x27, #0x8
10028b4fc:      add x22, x22, #0x1
10028b500:      subs    x26, x26, #0x8
10028b504:      b.eq    0x10028b684 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x210>
10028b508:      ldr x25, [x21, x22, lsl #3]
10028b50c:      str x25, [x20, x27]
10028b510:      mov x0, x20
10028b514:      bl  0x1002ce484 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
10028b518:      tbz w0, #0x0, 0x10028b538 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xc4>
10028b51c:      cmp x28, x25, lsr #48
10028b520:      b.ne    0x10028b558 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xe4>
10028b524:      mov x0, x25
10028b528:      bl  0x100aa65f8 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10028b52c:      tbnz    w0, #0x0, 0x10028b570 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10028b530:      scvtf   d0, w25
10028b534:      b   0x10028b56c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xf8>
10028b538:      cmp x24, #0x201
10028b53c:      b.lo    0x10028b570 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10028b540:      ldurb   w8, [x20, #-0x8]
10028b544:      cmp w8, #0x1
10028b548:      b.ne    0x10028b570 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10028b54c:      ldurh   w8, [x20, #-0x6]
10028b550:      tbnz    w8, #0xc, 0x10028b51c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xa8>
10028b554:      b   0x10028b570 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10028b558:      cmp x25, x19
10028b55c:      b.gt    0x10028b570 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10028b560:      fmov    d0, x25
10028b564:      fcmp    d0, d0
10028b568:      fcsel   d0, d8, d0, vs
10028b56c:      fmov    x25, d0
10028b570:      str x25, [x20, x27]
10028b574:      cmp x28, x25, lsr #48
10028b578:      b.ne    0x10028b598 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x124>
10028b57c:      mov x0, x25
10028b580:      bl  0x100aa65f8 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10028b584:      tbnz    w0, #0x0, 0x10028b5a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x12c>
10028b588:      scvtf   d0, w25
10028b58c:      cmp x24, #0x201
10028b590:      b.hs    0x10028b5f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x17c>
10028b594:      b   0x10028b614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10028b598:      cmp x25, x19
10028b59c:      b.le    0x10028b5dc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x168>
10028b5a0:      cmp x24, #0x201
10028b5a4:      b.lo    0x10028b614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10028b5a8:      ldurb   w8, [x20, #-0x8]
10028b5ac:      cmp w8, #0x1
10028b5b0:      b.ne    0x10028b614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10028b5b4:      ldurh   w8, [x20, #-0x6]
10028b5b8:      mov w9, #0xef7f             ; =61311
10028b5bc:      and w9, w8, w9
10028b5c0:      sturh   w9, [x20, #-0x6]
10028b5c4:      mov w9, #0x1080             ; =4224
10028b5c8:      tst w8, w9
10028b5cc:      b.eq    0x10028b614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10028b5d0:      mov x0, x20
10028b5d4:      bl  0x100422620 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
10028b5d8:      b   0x10028b614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10028b5dc:      fmov    d0, x25
10028b5e0:      fcmp    d0, d0
10028b5e4:      fcsel   d0, d8, d0, vs
10028b5e8:      cmp x24, #0x201
10028b5ec:      b.lo    0x10028b614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10028b5f0:      ldurb   w8, [x20, #-0x8]
10028b5f4:      cmp w8, #0x1
10028b5f8:      b.ne    0x10028b614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10028b5fc:      ldurh   w8, [x20, #-0x6]
10028b600:      tbz w8, #0x7, 0x10028b614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10028b604:      ldr w8, [x20]
10028b608:      cmp x22, x8
10028b60c:      b.hs    0x10028b614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10028b610:      str d0, [x20, x27]
10028b614:      mov x0, x20
10028b618:      mov x1, x22
10028b61c:      mov x2, x25
10028b620:      bl  0x1001d7e00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
10028b624:      mov x0, x20
10028b628:      bl  0x1002c0a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
10028b62c:      cbz w0, 0x10028b4f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
10028b630:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
10028b634:      add x8, x8, #0x910
10028b638:      ldapr   x8, [x8]
10028b63c:      cbnz    x8, 0x10028b668 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1f4>
10028b640:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
10028b644:      ldrb    w8, [x8, #0x918]
10028b648:      tbnz    w8, #0x0, 0x10028b4e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x6c>
10028b64c:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7e0>
10028b650:      add x8, x8, #0xb70
10028b654:      ldr w8, [x8]
10028b658:      cbz w8, 0x10028b4f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
10028b65c:      mov x0, x25
10028b660:      bl  0x1004369e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
10028b664:      b   0x10028b4f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
10028b668:      adrp    x0, 0x101130000 <_perry_global_baseline_worker_ts__1>
10028b66c:      add x0, x0, #0x910
10028b670:      bl  0x100cc5d84 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
10028b674:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
10028b678:      ldrb    w8, [x8, #0x918]
10028b67c:      tbnz    w8, #0x0, 0x10028b4e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x6c>
10028b680:      b   0x10028b64c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1d8>
10028b684:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
10028b688:      add x0, x0, #0x590
10028b68c:      ldr x8, [x0]
10028b690:      blr x8
10028b694:      ldrb    w8, [x0, #0x20]
10028b698:      cbnz    w8, 0x10028b6e4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x270>
10028b69c:      ldr x8, [x0]
10028b6a0:      cbnz    x8, 0x10028b70c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x298>
10028b6a4:      ldr x8, [x0, #0x18]
10028b6a8:      ldr x9, [sp, #0x8]
10028b6ac:      cmp x9, x8
10028b6b0:      b.hi    0x10028b6b8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x244>
10028b6b4:      str x9, [x0, #0x18]
10028b6b8:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
10028b6bc:      bfxil   x0, x20, #0, #48
10028b6c0:      ldp x29, x30, [sp, #0x70]
10028b6c4:      ldp x20, x19, [sp, #0x60]
10028b6c8:      ldp x22, x21, [sp, #0x50]
10028b6cc:      ldp x24, x23, [sp, #0x40]
10028b6d0:      ldp x26, x25, [sp, #0x30]
10028b6d4:      ldp x28, x27, [sp, #0x20]
10028b6d8:      ldp d9, d8, [sp, #0x10]
10028b6dc:      add sp, sp, #0x80
10028b6e0:      ret
10028b6e4:      cmp w8, #0x1
10028b6e8:      b.ne    0x10028b718 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x2a4>
10028b6ec:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
10028b6f0:      add x1, x1, #0xf78
10028b6f4:      mov x21, x0
10028b6f8:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10028b6fc:      mov x0, x21
10028b700:      strb    wzr, [x21, #0x20]
10028b704:      ldr x8, [x21]
10028b708:      cbz x8, 0x10028b6a4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x230>
10028b70c:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10028b710:      add x0, x0, #0xe58
10028b714:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10028b718:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10028b71c:      add x0, x0, #0xed8
10028b720:      bl  0x100cdab9c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
