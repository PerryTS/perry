/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/nested-records-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100891474 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array>:
100891474:      sub sp, sp, #0x80
100891478:      stp d9, d8, [sp, #0x10]
10089147c:      stp x28, x27, [sp, #0x20]
100891480:      stp x26, x25, [sp, #0x30]
100891484:      stp x24, x23, [sp, #0x40]
100891488:      stp x22, x21, [sp, #0x50]
10089148c:      stp x20, x19, [sp, #0x60]
100891490:      stp x29, x30, [sp, #0x70]
100891494:      add x29, sp, #0x70
100891498:      str x2, [sp, #0x8]
10089149c:      mov x23, x1
1008914a0:      mov x21, x0
1008914a4:      mov x0, x23
1008914a8:      bl  0x1008cfa34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array5alloc32js_array_alloc_with_length_exact>
1008914ac:      mov x20, x0
1008914b0:      bl  0x1008d14b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header24set_array_numeric_layout>
1008914b4:      cbz x23, 0x100891684 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x210>
1008914b8:      mov x22, #0x0               ; =0
1008914bc:      lsl x26, x23, #3
1008914c0:      add x23, x20, #0x8
1008914c4:      mov w27, #0x8               ; =8
1008914c8:      mov w28, #0x7ffe            ; =32766
1008914cc:      mov x19, #0x7ff8ffffffffffff ; =9221401712017801215
1008914d0:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
1008914d4:      fmov    d8, x8
1008914d8:      lsr x24, x20, #3
1008914dc:      b   0x100891508 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x94>
1008914e0:      mov x0, x20
1008914e4:      mov x1, x23
1008914e8:      mov x2, x25
1008914ec:      mov w3, #0x0                ; =0
1008914f0:      bl  0x1005b943c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
1008914f4:      add x23, x23, #0x8
1008914f8:      add x27, x27, #0x8
1008914fc:      add x22, x22, #0x1
100891500:      subs    x26, x26, #0x8
100891504:      b.eq    0x100891684 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x210>
100891508:      ldr x25, [x21, x22, lsl #3]
10089150c:      str x25, [x20, x27]
100891510:      mov x0, x20
100891514:      bl  0x1008d05c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
100891518:      tbz w0, #0x0, 0x100891538 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xc4>
10089151c:      cmp x28, x25, lsr #48
100891520:      b.ne    0x100891558 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xe4>
100891524:      mov x0, x25
100891528:      bl  0x100845448 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10089152c:      tbnz    w0, #0x0, 0x100891570 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
100891530:      scvtf   d0, w25
100891534:      b   0x10089156c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xf8>
100891538:      cmp x24, #0x201
10089153c:      b.lo    0x100891570 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
100891540:      ldurb   w8, [x20, #-0x8]
100891544:      cmp w8, #0x1
100891548:      b.ne    0x100891570 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10089154c:      ldurh   w8, [x20, #-0x6]
100891550:      tbnz    w8, #0xc, 0x10089151c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xa8>
100891554:      b   0x100891570 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
100891558:      cmp x25, x19
10089155c:      b.gt    0x100891570 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
100891560:      fmov    d0, x25
100891564:      fcmp    d0, d0
100891568:      fcsel   d0, d8, d0, vs
10089156c:      fmov    x25, d0
100891570:      str x25, [x20, x27]
100891574:      cmp x28, x25, lsr #48
100891578:      b.ne    0x100891598 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x124>
10089157c:      mov x0, x25
100891580:      bl  0x100845448 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
100891584:      tbnz    w0, #0x0, 0x1008915a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x12c>
100891588:      scvtf   d0, w25
10089158c:      cmp x24, #0x201
100891590:      b.hs    0x1008915f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x17c>
100891594:      b   0x100891614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
100891598:      cmp x25, x19
10089159c:      b.le    0x1008915dc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x168>
1008915a0:      cmp x24, #0x201
1008915a4:      b.lo    0x100891614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1008915a8:      ldurb   w8, [x20, #-0x8]
1008915ac:      cmp w8, #0x1
1008915b0:      b.ne    0x100891614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1008915b4:      ldurh   w8, [x20, #-0x6]
1008915b8:      mov w9, #0xef7f             ; =61311
1008915bc:      and w9, w8, w9
1008915c0:      sturh   w9, [x20, #-0x6]
1008915c4:      mov w9, #0x1080             ; =4224
1008915c8:      tst w8, w9
1008915cc:      b.eq    0x100891614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1008915d0:      mov x0, x20
1008915d4:      bl  0x1005aaee0 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
1008915d8:      b   0x100891614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1008915dc:      fmov    d0, x25
1008915e0:      fcmp    d0, d0
1008915e4:      fcsel   d0, d8, d0, vs
1008915e8:      cmp x24, #0x201
1008915ec:      b.lo    0x100891614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1008915f0:      ldurb   w8, [x20, #-0x8]
1008915f4:      cmp w8, #0x1
1008915f8:      b.ne    0x100891614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
1008915fc:      ldurh   w8, [x20, #-0x6]
100891600:      tbz w8, #0x7, 0x100891614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
100891604:      ldr w8, [x20]
100891608:      cmp x22, x8
10089160c:      b.hs    0x100891614 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
100891610:      str d0, [x20, x27]
100891614:      mov x0, x20
100891618:      mov x1, x22
10089161c:      mov x2, x25
100891620:      bl  0x100319c40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
100891624:      mov x0, x20
100891628:      bl  0x1008c2b8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
10089162c:      cbz w0, 0x1008914f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
100891630:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
100891634:      add x8, x8, #0x58
100891638:      ldapr   x8, [x8]
10089163c:      cbnz    x8, 0x100891668 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1f4>
100891640:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
100891644:      ldrb    w8, [x8, #0x60]
100891648:      tbnz    w8, #0x0, 0x1008914e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x6c>
10089164c:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7fb58>
100891650:      add x8, x8, #0x9c4
100891654:      ldr w8, [x8]
100891658:      cbz w8, 0x1008914f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
10089165c:      mov x0, x25
100891660:      bl  0x1005ba638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
100891664:      b   0x1008914f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
100891668:      adrp    x0, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
10089166c:      add x0, x0, #0x58
100891670:      bl  0x100cd0144 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
100891674:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
100891678:      ldrb    w8, [x8, #0x60]
10089167c:      tbnz    w8, #0x0, 0x1008914e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x6c>
100891680:      b   0x10089164c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1d8>
100891684:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
100891688:      add x0, x0, #0x2a0
10089168c:      ldr x8, [x0]
100891690:      blr x8
100891694:      ldrb    w8, [x0, #0x20]
100891698:      cbnz    w8, 0x1008916e4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x270>
10089169c:      ldr x8, [x0]
1008916a0:      cbnz    x8, 0x10089170c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x298>
1008916a4:      ldr x8, [x0, #0x18]
1008916a8:      ldr x9, [sp, #0x8]
1008916ac:      cmp x9, x8
1008916b0:      b.hi    0x1008916b8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x244>
1008916b4:      str x9, [x0, #0x18]
1008916b8:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
1008916bc:      bfxil   x0, x20, #0, #48
1008916c0:      ldp x29, x30, [sp, #0x70]
1008916c4:      ldp x20, x19, [sp, #0x60]
1008916c8:      ldp x22, x21, [sp, #0x50]
1008916cc:      ldp x24, x23, [sp, #0x40]
1008916d0:      ldp x26, x25, [sp, #0x30]
1008916d4:      ldp x28, x27, [sp, #0x20]
1008916d8:      ldp d9, d8, [sp, #0x10]
1008916dc:      add sp, sp, #0x80
1008916e0:      ret
1008916e4:      cmp w8, #0x1
1008916e8:      b.ne    0x100891718 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x2a4>
1008916ec:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1008916f0:      add x1, x1, #0xeec
1008916f4:      mov x21, x0
1008916f8:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008916fc:      mov x0, x21
100891700:      strb    wzr, [x21, #0x20]
100891704:      ldr x8, [x21]
100891708:      cbz x8, 0x1008916a4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x230>
10089170c:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
100891710:      add x0, x0, #0xe58
100891714:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100891718:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10089171c:      add x0, x0, #0xed8
100891720:      bl  0x100cdc11c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
