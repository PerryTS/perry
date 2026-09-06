/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/short-array-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010089a350 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array>:
10089a350:      sub sp, sp, #0x80
10089a354:      stp d9, d8, [sp, #0x10]
10089a358:      stp x28, x27, [sp, #0x20]
10089a35c:      stp x26, x25, [sp, #0x30]
10089a360:      stp x24, x23, [sp, #0x40]
10089a364:      stp x22, x21, [sp, #0x50]
10089a368:      stp x20, x19, [sp, #0x60]
10089a36c:      stp x29, x30, [sp, #0x70]
10089a370:      add x29, sp, #0x70
10089a374:      str x2, [sp, #0x8]
10089a378:      mov x23, x1
10089a37c:      mov x21, x0
10089a380:      mov x0, x23
10089a384:      bl  0x1008dcf90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array5alloc32js_array_alloc_with_length_exact>
10089a388:      mov x20, x0
10089a38c:      bl  0x1008dea38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header24set_array_numeric_layout>
10089a390:      cbz x23, 0x10089a560 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x210>
10089a394:      mov x22, #0x0               ; =0
10089a398:      lsl x26, x23, #3
10089a39c:      add x23, x20, #0x8
10089a3a0:      mov w27, #0x8               ; =8
10089a3a4:      mov w28, #0x7ffe            ; =32766
10089a3a8:      mov x19, #0x7ff8ffffffffffff ; =9221401712017801215
10089a3ac:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
10089a3b0:      fmov    d8, x8
10089a3b4:      lsr x24, x20, #3
10089a3b8:      b   0x10089a3e4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x94>
10089a3bc:      mov x0, x20
10089a3c0:      mov x1, x23
10089a3c4:      mov x2, x25
10089a3c8:      mov w3, #0x0                ; =0
10089a3cc:      bl  0x100553db4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
10089a3d0:      add x23, x23, #0x8
10089a3d4:      add x27, x27, #0x8
10089a3d8:      add x22, x22, #0x1
10089a3dc:      subs    x26, x26, #0x8
10089a3e0:      b.eq    0x10089a560 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x210>
10089a3e4:      ldr x25, [x21, x22, lsl #3]
10089a3e8:      str x25, [x20, x27]
10089a3ec:      mov x0, x20
10089a3f0:      bl  0x1008ddb44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
10089a3f4:      tbz w0, #0x0, 0x10089a414 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xc4>
10089a3f8:      cmp x28, x25, lsr #48
10089a3fc:      b.ne    0x10089a434 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xe4>
10089a400:      mov x0, x25
10089a404:      bl  0x10024367c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10089a408:      tbnz    w0, #0x0, 0x10089a44c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10089a40c:      scvtf   d0, w25
10089a410:      b   0x10089a448 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xf8>
10089a414:      cmp x24, #0x201
10089a418:      b.lo    0x10089a44c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10089a41c:      ldurb   w8, [x20, #-0x8]
10089a420:      cmp w8, #0x1
10089a424:      b.ne    0x10089a44c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10089a428:      ldurh   w8, [x20, #-0x6]
10089a42c:      tbnz    w8, #0xc, 0x10089a3f8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xa8>
10089a430:      b   0x10089a44c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10089a434:      cmp x25, x19
10089a438:      b.gt    0x10089a44c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0xfc>
10089a43c:      fmov    d0, x25
10089a440:      fcmp    d0, d0
10089a444:      fcsel   d0, d8, d0, vs
10089a448:      fmov    x25, d0
10089a44c:      str x25, [x20, x27]
10089a450:      cmp x28, x25, lsr #48
10089a454:      b.ne    0x10089a474 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x124>
10089a458:      mov x0, x25
10089a45c:      bl  0x10024367c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10089a460:      tbnz    w0, #0x0, 0x10089a47c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x12c>
10089a464:      scvtf   d0, w25
10089a468:      cmp x24, #0x201
10089a46c:      b.hs    0x10089a4cc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x17c>
10089a470:      b   0x10089a4f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10089a474:      cmp x25, x19
10089a478:      b.le    0x10089a4b8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x168>
10089a47c:      cmp x24, #0x201
10089a480:      b.lo    0x10089a4f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10089a484:      ldurb   w8, [x20, #-0x8]
10089a488:      cmp w8, #0x1
10089a48c:      b.ne    0x10089a4f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10089a490:      ldurh   w8, [x20, #-0x6]
10089a494:      mov w9, #0xef7f             ; =61311
10089a498:      and w9, w8, w9
10089a49c:      sturh   w9, [x20, #-0x6]
10089a4a0:      mov w9, #0x1080             ; =4224
10089a4a4:      tst w8, w9
10089a4a8:      b.eq    0x10089a4f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10089a4ac:      mov x0, x20
10089a4b0:      bl  0x1007bd668 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
10089a4b4:      b   0x10089a4f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10089a4b8:      fmov    d0, x25
10089a4bc:      fcmp    d0, d0
10089a4c0:      fcsel   d0, d8, d0, vs
10089a4c4:      cmp x24, #0x201
10089a4c8:      b.lo    0x10089a4f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10089a4cc:      ldurb   w8, [x20, #-0x8]
10089a4d0:      cmp w8, #0x1
10089a4d4:      b.ne    0x10089a4f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10089a4d8:      ldurh   w8, [x20, #-0x6]
10089a4dc:      tbz w8, #0x7, 0x10089a4f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10089a4e0:      ldr w8, [x20]
10089a4e4:      cmp x22, x8
10089a4e8:      b.hs    0x10089a4f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1a0>
10089a4ec:      str d0, [x20, x27]
10089a4f0:      mov x0, x20
10089a4f4:      mov x1, x22
10089a4f8:      mov x2, x25
10089a4fc:      bl  0x1007ea6c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
10089a500:      mov x0, x20
10089a504:      bl  0x1008cc038 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
10089a508:      cbz w0, 0x10089a3d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
10089a50c:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
10089a510:      add x8, x8, #0x28
10089a514:      ldapr   x8, [x8]
10089a518:      cbnz    x8, 0x10089a544 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1f4>
10089a51c:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
10089a520:      ldrb    w8, [x8, #0x30]
10089a524:      tbnz    w8, #0x0, 0x10089a3bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x6c>
10089a528:      adrp    x8, 0x1011f8000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f448>
10089a52c:      add x8, x8, #0xfe0
10089a530:      ldr w8, [x8]
10089a534:      cbz w8, 0x10089a3d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
10089a538:      mov x0, x25
10089a53c:      bl  0x100555294 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
10089a540:      b   0x10089a3d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x80>
10089a544:      adrp    x0, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
10089a548:      add x0, x0, #0x28
10089a54c:      bl  0x100cc1e44 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
10089a550:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
10089a554:      ldrb    w8, [x8, #0x30]
10089a558:      tbnz    w8, #0x0, 0x10089a3bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x6c>
10089a55c:      b   0x10089a528 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x1d8>
10089a560:      adrp    x0, 0x101134000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json17PARSE_SHAPE_CACHE0023___RUST_STD_INTERNAL_VAL>
10089a564:      add x0, x0, #0x660
10089a568:      ldr x8, [x0]
10089a56c:      blr x8
10089a570:      ldrb    w8, [x0, #0x20]
10089a574:      cbnz    w8, 0x10089a5c0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x270>
10089a578:      ldr x8, [x0]
10089a57c:      cbnz    x8, 0x10089a5e8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x298>
10089a580:      ldr x8, [x0, #0x18]
10089a584:      ldr x9, [sp, #0x8]
10089a588:      cmp x9, x8
10089a58c:      b.hi    0x10089a594 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x244>
10089a590:      str x9, [x0, #0x18]
10089a594:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
10089a598:      bfxil   x0, x20, #0, #48
10089a59c:      ldp x29, x30, [sp, #0x70]
10089a5a0:      ldp x20, x19, [sp, #0x60]
10089a5a4:      ldp x22, x21, [sp, #0x50]
10089a5a8:      ldp x24, x23, [sp, #0x40]
10089a5ac:      ldp x26, x25, [sp, #0x30]
10089a5b0:      ldp x28, x27, [sp, #0x20]
10089a5b4:      ldp d9, d8, [sp, #0x10]
10089a5b8:      add sp, sp, #0x80
10089a5bc:      ret
10089a5c0:      cmp w8, #0x1
10089a5c4:      b.ne    0x10089a5f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x2a4>
10089a5c8:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
10089a5cc:      add x1, x1, #0xd0
10089a5d0:      mov x21, x0
10089a5d4:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10089a5d8:      mov x0, x21
10089a5dc:      strb    wzr, [x21, #0x20]
10089a5e0:      ldr x8, [x21]
10089a5e4:      cbz x8, 0x10089a580 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array+0x230>
10089a5e8:      adrp    x0, 0x10109c000 <_anon.438b28c8644b10f28676d307896bf03a.21>
10089a5ec:      add x0, x0, #0xe58
10089a5f0:      bl  0x100c99aac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10089a5f4:      adrp    x0, 0x10109b000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10089a5f8:      add x0, x0, #0xed8
10089a5fc:      bl  0x100cdb71c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
