/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/growth-alias-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001003ba384 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>:
1003ba384:      stp d9, d8, [sp, #-0x70]!
1003ba388:      stp x28, x27, [sp, #0x10]
1003ba38c:      stp x26, x25, [sp, #0x20]
1003ba390:      stp x24, x23, [sp, #0x30]
1003ba394:      stp x22, x21, [sp, #0x40]
1003ba398:      stp x20, x19, [sp, #0x50]
1003ba39c:      stp x29, x30, [sp, #0x60]
1003ba3a0:      add x29, sp, #0x60
1003ba3a4:      mov x19, x2
1003ba3a8:      mov x21, x0
1003ba3ac:      adrp    x0, 0x101126000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json25OBJECT_PROTO_TOJSON_STATE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1003ba3b0:      add x0, x0, #0x8
1003ba3b4:      ldr x8, [x0]
1003ba3b8:      blr x8
1003ba3bc:      mov x20, x0
1003ba3c0:      ldrb    w8, [x0, #0x20]
1003ba3c4:      cbnz    w8, 0x1003ba860 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4dc>
1003ba3c8:      ldr x8, [x20]
1003ba3cc:      cbnz    x8, 0x1003ba890 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x50c>
1003ba3d0:      mov x22, #0x7ffd000000000000 ; =9222527611924643840
1003ba3d4:      bfxil   x22, x1, #0, #48
1003ba3d8:      mov x8, #-0x1               ; =-1
1003ba3dc:      str x8, [x20]
1003ba3e0:      mov x0, x20
1003ba3e4:      ldr x8, [x0, #0x8]!
1003ba3e8:      ldr x28, [x20, #0x18]
1003ba3ec:      cmp x28, x8
1003ba3f0:      b.ne    0x1003ba3f8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x74>
1003ba3f4:      bl  0x100cb708c <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1003ba3f8:      ldr x8, [x20, #0x10]
1003ba3fc:      str x22, [x8, x28, lsl #3]
1003ba400:      add x8, x28, #0x1
1003ba404:      str x8, [x20, #0x18]
1003ba408:      ldr x8, [x20]
1003ba40c:      add x8, x8, #0x1
1003ba410:      str x8, [x20]
1003ba414:      mov x0, x21
1003ba418:      bl  0x1003b9e0c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1003ba41c:      ldrb    w8, [x21, #0x90]
1003ba420:      cbz w8, 0x1003ba77c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3f8>
1003ba424:      mov x24, x0
1003ba428:      mov x22, #0x7fffffffffffffff ; =9223372036854775807
1003ba42c:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
1003ba430:      fmov    d8, x8
1003ba434:      mov x27, #-0x1              ; =-1
1003ba438:      mov x23, #0x2600            ; =9728
1003ba43c:      movk    x23, #0x1, lsl #32
1003ba440:      ldrb    w8, [x20, #0x20]
1003ba444:      cbnz    w8, 0x1003ba620 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x29c>
1003ba448:      ldr x8, [x20]
1003ba44c:      cmp x8, x22
1003ba450:      b.hs    0x1003ba8d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
1003ba454:      add x9, x8, #0x1
1003ba458:      str x9, [x20]
1003ba45c:      ldr x9, [x20, #0x18]
1003ba460:      cmp x28, x9
1003ba464:      b.hs    0x1003ba498 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x114>
1003ba468:      ldr x9, [x20, #0x10]
1003ba46c:      ldr x9, [x9, x28, lsl #3]
1003ba470:      and x25, x9, #0xffffffffffff
1003ba474:      str x8, [x20]
1003ba478:      ldp w26, w8, [x25]
1003ba47c:      cmp w26, w8
1003ba480:      b.lo    0x1003ba4ac <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x128>
1003ba484:      fmov    d0, x24
1003ba488:      mov x0, x25
1003ba48c:      bl  0x1005e80b4 <_js_array_push_f64>
1003ba490:      and x25, x0, #0xffffffffffff
1003ba494:      b   0x1003ba688 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x304>
1003ba498:      mov w25, #0x1               ; =1
1003ba49c:      str x8, [x20]
1003ba4a0:      ldp w26, w8, [x25]
1003ba4a4:      cmp w26, w8
1003ba4a8:      b.hs    0x1003ba484 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x100>
1003ba4ac:      add x27, x25, x26, lsl #3
1003ba4b0:      str x24, [x27, #0x8]!
1003ba4b4:      mov x0, x25
1003ba4b8:      bl  0x1003fc3c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
1003ba4bc:      lsr x22, x25, #3
1003ba4c0:      tbz w0, #0x0, 0x1003ba4e4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x160>
1003ba4c4:      mov w8, #0x7ffe             ; =32766
1003ba4c8:      cmp x8, x24, lsr #48
1003ba4cc:      b.ne    0x1003ba504 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x180>
1003ba4d0:      mov x0, x24
1003ba4d4:      bl  0x10069fafc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
1003ba4d8:      tbnz    w0, #0x0, 0x1003ba520 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
1003ba4dc:      scvtf   d0, w24
1003ba4e0:      b   0x1003ba51c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x198>
1003ba4e4:      cmp x22, #0x201
1003ba4e8:      b.lo    0x1003ba520 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
1003ba4ec:      ldurb   w8, [x25, #-0x8]
1003ba4f0:      cmp w8, #0x1
1003ba4f4:      b.ne    0x1003ba520 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
1003ba4f8:      ldurh   w8, [x25, #-0x6]
1003ba4fc:      tbnz    w8, #0xc, 0x1003ba4c4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x140>
1003ba500:      b   0x1003ba520 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
1003ba504:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
1003ba508:      cmp x24, x8
1003ba50c:      b.gt    0x1003ba520 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
1003ba510:      fmov    d0, x24
1003ba514:      fcmp    d0, d0
1003ba518:      fcsel   d0, d8, d0, vs
1003ba51c:      fmov    x24, d0
1003ba520:      str x24, [x27]
1003ba524:      mov w8, #0x7ffe             ; =32766
1003ba528:      cmp x8, x24, lsr #48
1003ba52c:      b.ne    0x1003ba54c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x1c8>
1003ba530:      mov x0, x24
1003ba534:      bl  0x10069fafc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
1003ba538:      tbnz    w0, #0x0, 0x1003ba558 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x1d4>
1003ba53c:      scvtf   d0, w24
1003ba540:      cmp x22, #0x201
1003ba544:      b.hs    0x1003ba5a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x224>
1003ba548:      b   0x1003ba5cc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
1003ba54c:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
1003ba550:      cmp x24, x8
1003ba554:      b.le    0x1003ba594 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x210>
1003ba558:      cmp x22, #0x201
1003ba55c:      b.lo    0x1003ba5cc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
1003ba560:      ldurb   w8, [x25, #-0x8]
1003ba564:      cmp w8, #0x1
1003ba568:      b.ne    0x1003ba5cc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
1003ba56c:      ldurh   w8, [x25, #-0x6]
1003ba570:      mov w9, #0xef7f             ; =61311
1003ba574:      and w9, w8, w9
1003ba578:      sturh   w9, [x25, #-0x6]
1003ba57c:      mov w9, #0x1080             ; =4224
1003ba580:      tst w8, w9
1003ba584:      b.eq    0x1003ba5cc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
1003ba588:      mov x0, x25
1003ba58c:      bl  0x10055a3e8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
1003ba590:      b   0x1003ba5cc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
1003ba594:      fmov    d0, x24
1003ba598:      fcmp    d0, d0
1003ba59c:      fcsel   d0, d8, d0, vs
1003ba5a0:      cmp x22, #0x201
1003ba5a4:      b.lo    0x1003ba5cc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
1003ba5a8:      ldurb   w8, [x25, #-0x8]
1003ba5ac:      cmp w8, #0x1
1003ba5b0:      b.ne    0x1003ba5cc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
1003ba5b4:      ldurh   w8, [x25, #-0x6]
1003ba5b8:      tbz w8, #0x7, 0x1003ba5cc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
1003ba5bc:      ldr w8, [x25]
1003ba5c0:      cmp w26, w8
1003ba5c4:      b.hs    0x1003ba5cc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
1003ba5c8:      str d0, [x27]
1003ba5cc:      mov x0, x25
1003ba5d0:      mov x1, x26
1003ba5d4:      mov x2, x24
1003ba5d8:      bl  0x10058a300 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
1003ba5dc:      mov x0, x25
1003ba5e0:      bl  0x1003ead08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
1003ba5e4:      mov x22, #0x7fffffffffffffff ; =9223372036854775807
1003ba5e8:      cbz w0, 0x1003ba67c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
1003ba5ec:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
1003ba5f0:      add x8, x8, #0xd50
1003ba5f4:      ldapr   x8, [x8]
1003ba5f8:      cbnz    x8, 0x1003ba64c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2c8>
1003ba5fc:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
1003ba600:      ldrb    w8, [x8, #0xd58]
1003ba604:      tbz w8, #0x0, 0x1003ba664 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2e0>
1003ba608:      mov x0, x25
1003ba60c:      mov x1, x27
1003ba610:      mov x2, x24
1003ba614:      mov w3, #0x0                ; =0
1003ba618:      bl  0x10098f020 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
1003ba61c:      b   0x1003ba67c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
1003ba620:      cmp w8, #0x2
1003ba624:      b.eq    0x1003ba8a4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
1003ba628:      mov x0, x20
1003ba62c:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003ba630:      add x1, x1, #0x824
1003ba634:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003ba638:      strb    wzr, [x20, #0x20]
1003ba63c:      ldr x8, [x20]
1003ba640:      cmp x8, x22
1003ba644:      b.lo    0x1003ba454 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0xd0>
1003ba648:      b   0x1003ba8d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
1003ba64c:      adrp    x0, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
1003ba650:      add x0, x0, #0xd50
1003ba654:      bl  0x100cb63bc <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
1003ba658:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
1003ba65c:      ldrb    w8, [x8, #0xd58]
1003ba660:      tbnz    w8, #0x0, 0x1003ba608 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x284>
1003ba664:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x110>
1003ba668:      add x8, x8, #0xaa8
1003ba66c:      ldr w8, [x8]
1003ba670:      cbz w8, 0x1003ba67c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
1003ba674:      mov x0, x24
1003ba678:      bl  0x1009904f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
1003ba67c:      add w8, w26, #0x1
1003ba680:      str w8, [x25]
1003ba684:      mov x27, #-0x1              ; =-1
1003ba688:      ldrb    w8, [x20, #0x20]
1003ba68c:      cbnz    w8, 0x1003ba74c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3c8>
1003ba690:      ldr x8, [x20]
1003ba694:      cbnz    x8, 0x1003ba770 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3ec>
1003ba698:      str x27, [x20]
1003ba69c:      ldr x8, [x20, #0x18]
1003ba6a0:      cmp x28, x8
1003ba6a4:      b.hs    0x1003ba6d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x350>
1003ba6a8:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1003ba6ac:      orr x8, x25, x8
1003ba6b0:      ldr x9, [x20, #0x10]
1003ba6b4:      str x8, [x9, x28, lsl #3]
1003ba6b8:      ldr x8, [x20]
1003ba6bc:      add x8, x8, #0x1
1003ba6c0:      str x8, [x20]
1003ba6c4:      ldp x9, x8, [x21, #0x30]
1003ba6c8:      cmp x8, x9
1003ba6cc:      b.lo    0x1003ba6e8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x364>
1003ba6d0:      b   0x1003ba714 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
1003ba6d4:      mov x8, #0x0                ; =0
1003ba6d8:      str x8, [x20]
1003ba6dc:      ldp x9, x8, [x21, #0x30]
1003ba6e0:      cmp x8, x9
1003ba6e4:      b.hs    0x1003ba714 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
1003ba6e8:      ldr x10, [x21, #0x28]
1003ba6ec:      ldrb    w11, [x10, x8]
1003ba6f0:      cmp w11, #0x20
1003ba6f4:      b.hi    0x1003ba714 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
1003ba6f8:      lsr x11, x23, x11
1003ba6fc:      tbz w11, #0x0, 0x1003ba714 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
1003ba700:      add x8, x8, #0x1
1003ba704:      str x8, [x21, #0x38]
1003ba708:      cmp x9, x8
1003ba70c:      b.ne    0x1003ba6ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x368>
1003ba710:      b   0x1003ba7d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
1003ba714:      cmp x8, x9
1003ba718:      b.hs    0x1003ba780 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3fc>
1003ba71c:      ldr x10, [x21, #0x28]
1003ba720:      ldrb    w10, [x10, x8]
1003ba724:      cmp w10, #0x2c
1003ba728:      b.ne    0x1003ba780 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3fc>
1003ba72c:      add x8, x8, #0x1
1003ba730:      str x8, [x21, #0x38]
1003ba734:      mov x0, x21
1003ba738:      bl  0x1003b9e0c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1003ba73c:      mov x24, x0
1003ba740:      ldrb    w8, [x21, #0x90]
1003ba744:      tbnz    w8, #0x0, 0x1003ba440 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0xbc>
1003ba748:      b   0x1003ba77c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3f8>
1003ba74c:      cmp w8, #0x2
1003ba750:      b.eq    0x1003ba8a4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
1003ba754:      mov x0, x20
1003ba758:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003ba75c:      add x1, x1, #0x824
1003ba760:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003ba764:      strb    wzr, [x20, #0x20]
1003ba768:      ldr x8, [x20]
1003ba76c:      cbz x8, 0x1003ba698 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x314>
1003ba770:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1003ba774:      add x0, x0, #0xde0
1003ba778:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1003ba77c:      ldp x9, x8, [x21, #0x30]
1003ba780:      cmp x8, x9
1003ba784:      b.hs    0x1003ba7d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
1003ba788:      ldr x10, [x21, #0x28]
1003ba78c:      mov x11, #0x2600            ; =9728
1003ba790:      movk    x11, #0x1, lsl #32
1003ba794:      ldrb    w12, [x10, x8]
1003ba798:      cmp w12, #0x20
1003ba79c:      b.hi    0x1003ba7bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x438>
1003ba7a0:      lsr x13, x11, x12
1003ba7a4:      tbz w13, #0x0, 0x1003ba7bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x438>
1003ba7a8:      add x8, x8, #0x1
1003ba7ac:      str x8, [x21, #0x38]
1003ba7b0:      cmp x9, x8
1003ba7b4:      b.ne    0x1003ba794 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x410>
1003ba7b8:      b   0x1003ba7d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
1003ba7bc:      cmp w12, #0x5d
1003ba7c0:      b.ne    0x1003ba7d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
1003ba7c4:      add x8, x8, #0x1
1003ba7c8:      str x8, [x21, #0x38]
1003ba7cc:      b   0x1003ba7d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x450>
1003ba7d0:      strb    wzr, [x21, #0x90]
1003ba7d4:      ldrb    w8, [x20, #0x20]
1003ba7d8:      cbnz    w8, 0x1003ba89c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x518>
1003ba7dc:      ldr x8, [x20]
1003ba7e0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1003ba7e4:      cmp x8, x9
1003ba7e8:      b.hs    0x1003ba8d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
1003ba7ec:      add x9, x8, #0x1
1003ba7f0:      str x9, [x20]
1003ba7f4:      ldr x9, [x20, #0x18]
1003ba7f8:      cmp x28, x9
1003ba7fc:      b.hs    0x1003ba814 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x490>
1003ba800:      ldr x9, [x20, #0x10]
1003ba804:      ldr x9, [x9, x28, lsl #3]
1003ba808:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
1003ba80c:      bfxil   x0, x9, #0, #48
1003ba810:      b   0x1003ba81c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x498>
1003ba814:      mov x0, #0x1                ; =1
1003ba818:      movk    x0, #0x7ffd, lsl #48
1003ba81c:      str x8, [x20]
1003ba820:      cbnz    x8, 0x1003ba854 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4d0>
1003ba824:      ldr x8, [x20, #0x18]
1003ba828:      cmp x19, x8
1003ba82c:      b.hi    0x1003ba834 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4b0>
1003ba830:      str x19, [x20, #0x18]
1003ba834:      ldp x29, x30, [sp, #0x60]
1003ba838:      ldp x20, x19, [sp, #0x50]
1003ba83c:      ldp x22, x21, [sp, #0x40]
1003ba840:      ldp x24, x23, [sp, #0x30]
1003ba844:      ldp x26, x25, [sp, #0x20]
1003ba848:      ldp x28, x27, [sp, #0x10]
1003ba84c:      ldp d9, d8, [sp], #0x70
1003ba850:      ret
1003ba854:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1003ba858:      add x0, x0, #0xe58
1003ba85c:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1003ba860:      cmp w8, #0x1
1003ba864:      b.ne    0x1003ba8a4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
1003ba868:      adrp    x8, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003ba86c:      add x8, x8, #0x824
1003ba870:      mov x0, x20
1003ba874:      mov x22, x1
1003ba878:      mov x1, x8
1003ba87c:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003ba880:      mov x1, x22
1003ba884:      strb    wzr, [x20, #0x20]
1003ba888:      ldr x8, [x20]
1003ba88c:      cbz x8, 0x1003ba3d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4c>
1003ba890:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1003ba894:      add x0, x0, #0xdf8
1003ba898:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1003ba89c:      cmp w8, #0x2
1003ba8a0:      b.ne    0x1003ba8b0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x52c>
1003ba8a4:      adrp    x0, 0x10108f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1003ba8a8:      add x0, x0, #0xed8
1003ba8ac:      bl  0x100ccf55c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1003ba8b0:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003ba8b4:      add x1, x1, #0x824
1003ba8b8:      mov x0, x20
1003ba8bc:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003ba8c0:      strb    wzr, [x20, #0x20]
1003ba8c4:      ldr x8, [x20]
1003ba8c8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1003ba8cc:      cmp x8, x9
1003ba8d0:      b.lo    0x1003ba7ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x468>
1003ba8d4:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1003ba8d8:      add x0, x0, #0xdc8
1003ba8dc:      bl  0x100c8d1dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
