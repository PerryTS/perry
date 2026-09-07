/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010036a4e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow>:
10036a4e8:      sub sp, sp, #0x1a0
10036a4ec:      stp x28, x27, [sp, #0x140]
10036a4f0:      stp x26, x25, [sp, #0x150]
10036a4f4:      stp x24, x23, [sp, #0x160]
10036a4f8:      stp x22, x21, [sp, #0x170]
10036a4fc:      stp x20, x19, [sp, #0x180]
10036a500:      stp x29, x30, [sp, #0x190]
10036a504:      add x29, sp, #0x190
10036a508:      mov x20, x2
10036a50c:      mov x22, x1
10036a510:      mov x19, x0
10036a514:      add x24, sp, #0x90
10036a518:      add x23, x1, #0x14
10036a51c:      cmp x2, #0x2
10036a520:      b.ne    0x10036a538 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x50>
10036a524:      ldrh    w8, [x23]
10036a528:      mov w9, #0x7d7b             ; =32123
10036a52c:      cmp w8, w9
10036a530:      b.eq    0x10036a56c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x84>
10036a534:      b   0x10036a5ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xc4>
10036a538:      cmp x20, #0x3
10036a53c:      b.lo    0x10036a5ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xc4>
10036a540:      ldrb    w8, [x23]
10036a544:      cmp w8, #0x20
10036a548:      b.hi    0x10036a578 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x90>
10036a54c:      mov x9, #0x2600             ; =9728
10036a550:      movk    x9, #0x1, lsl #32
10036a554:      lsr x9, x9, x8
10036a558:      tbz w9, #0x0, 0x10036a578 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x90>
10036a55c:      add x0, x22, #0x14
10036a560:      mov x1, x20
10036a564:      bl  0x10036156c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty19padded_empty_object>
10036a568:      tbz w0, #0x0, 0x10036a5a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xbc>
10036a56c:      bl  0x100361638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty21allocate_empty_object>
10036a570:      stp xzr, x0, [x19]
10036a574:      b   0x10036a94c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
10036a578:      cmp w8, #0x7b
10036a57c:      b.ne    0x10036a5a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xbc>
10036a580:      ldrb    w8, [x22, #0x15]
10036a584:      cmp w8, #0x20
10036a588:      b.hi    0x10036a59c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xb4>
10036a58c:      mov x9, #0x2600             ; =9728
10036a590:      movk    x9, #0x1, lsl #32
10036a594:      lsr x9, x9, x8
10036a598:      tbnz    w9, #0x0, 0x10036a55c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x74>
10036a59c:      cmp w8, #0x7d
10036a5a0:      b.eq    0x10036a55c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x74>
10036a5a4:      cmp x20, #0x41
10036a5a8:      b.hs    0x10036a610 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x128>
10036a5ac:      add x0, sp, #0x90
10036a5b0:      add x1, x22, #0x14
10036a5b4:      mov x2, x20
10036a5b8:      bl  0x100361b04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode>
10036a5bc:      ldr x8, [sp, #0x90]
10036a5c0:      cbz x8, 0x10036a6bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
10036a5c4:      ldr x8, [sp, #0x118]
10036a5c8:      str x8, [sp, #0x80]
10036a5cc:      ldur    q0, [x24, #0x48]
10036a5d0:      ldur    q1, [x24, #0x58]
10036a5d4:      stp q0, q1, [sp, #0x40]
10036a5d8:      ldur    q0, [x24, #0x68]
10036a5dc:      ldur    q1, [x24, #0x78]
10036a5e0:      stp q0, q1, [sp, #0x60]
10036a5e4:      ldur    q0, [x24, #0x8]
10036a5e8:      ldur    q1, [x24, #0x18]
10036a5ec:      stp q0, q1, [sp]
10036a5f0:      ldur    q0, [x24, #0x28]
10036a5f4:      ldur    q1, [x24, #0x38]
10036a5f8:      stp q0, q1, [sp, #0x20]
10036a5fc:      mov x0, sp
10036a600:      bl  0x100362564 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate>
10036a604:      tbz w0, #0x0, 0x10036a6bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
10036a608:      stp xzr, x1, [x19]
10036a60c:      b   0x10036a94c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
10036a610:      cmp x20, #0x3e9
10036a614:      b.lo    0x10036a6bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
10036a618:      add x0, x22, #0x14
10036a61c:      mov x1, x20
10036a620:      mov w2, #0x3e8              ; =1000
10036a624:      bl  0x100363078 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
10036a628:      tbz w0, #0x0, 0x10036a6bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
10036a62c:      add x0, x22, #0x14
10036a630:      mov x1, x20
10036a634:      mov w2, #0xa120             ; =41248
10036a638:      movk    w2, #0x7, lsl #16
10036a63c:      bl  0x100363078 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
10036a640:      tbz w0, #0x0, 0x10036a9f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x50c>
10036a644:      adrp    x8, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
10036a648:      add x8, x8, #0xf80
10036a64c:      adrp    x9, 0x100dd7000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.1688+0x188>
10036a650:      add x9, x9, #0xae8
10036a654:      stp x9, x8, [sp]
10036a658:      adrp    x0, 0x100ef8000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.256+0x58>
10036a65c:      add x0, x0, #0x685
10036a660:      add x8, sp, #0x90
10036a664:      mov x1, sp
10036a668:      bl  0x100023808 <__RNvNvNtCsctvjasLqLe9_5alloc3fmt6format12format_inner>
10036a66c:      ldr x20, [sp, #0x98]
10036a670:      ldr w1, [sp, #0xa0]
10036a674:      mov x0, x20
10036a678:      mov x2, x1
10036a67c:      bl  0x100958040 <_js_string_from_bytes_with_capacity>
10036a680:      mov x3, x0
10036a684:      adrp    x1, 0x100dd1000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.52+0x78>
10036a688:      add x1, x1, #0x22a
10036a68c:      mov w21, #0x1               ; =1
10036a690:      mov w0, #0x2                ; =2
10036a694:      mov w2, #0xa                ; =10
10036a698:      mov w4, #0x1                ; =1
10036a69c:      bl  0x1003536bc <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
10036a6a0:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
10036a6a4:      bfxil   x8, x0, #0, #48
10036a6a8:      stp x21, x8, [x19]
10036a6ac:      ldr x8, [sp, #0x90]
10036a6b0:      cbz x8, 0x10036a94c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
10036a6b4:      mov x0, x20
10036a6b8:      b   0x10036a948 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x460>
10036a6bc:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
10036a6c0:      add x0, x0, #0x398
10036a6c4:      ldr x8, [x0]
10036a6c8:      blr x8
10036a6cc:      mov x21, x0
10036a6d0:      ldrb    w8, [x0, #0x20]
10036a6d4:      cbnz    w8, 0x10036aac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5d8>
10036a6d8:      ldr x8, [x21]
10036a6dc:      cbnz    x8, 0x10036ab0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x624>
10036a6e0:      mov x23, #0x7fff000000000000 ; =9223090561878065152
10036a6e4:      bfxil   x23, x22, #0, #48
10036a6e8:      mov x8, #-0x1               ; =-1
10036a6ec:      str x8, [x21]
10036a6f0:      mov x22, x21
10036a6f4:      ldr x8, [x22, #0x8]!
10036a6f8:      ldr x25, [x21, #0x18]
10036a6fc:      cmp x25, x8
10036a700:      b.ne    0x10036a70c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x224>
10036a704:      mov x0, x22
10036a708:      bl  0x100cd4250 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
10036a70c:      ldr x8, [x21, #0x10]
10036a710:      str x23, [x8, x25, lsl #3]
10036a714:      add x8, x25, #0x1
10036a718:      str x8, [x21, #0x18]
10036a71c:      ldr x8, [x21]
10036a720:      add x8, x8, #0x1
10036a724:      str x8, [x21]
10036a728:      mov x0, #0x0                ; =0
10036a72c:      bl  0x100482150 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy43GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1J_6option6OptionQIB2o_INtNtB1J_4cell4CellbEEEEE9call_onceBc_>
10036a730:      ldrb    w8, [x0]
10036a734:      strb    wzr, [x0]
10036a738:      cbz w8, 0x10036a770 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x288>
10036a73c:      mov x23, x0
10036a740:      mov x0, #0x0                ; =0
10036a744:      bl  0x100482170 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
10036a748:      ldrb    w8, [x0]
10036a74c:      tst w8, #0x3
10036a750:      b.ne    0x10036a768 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x280>
10036a754:      adrp    x8, 0x101200000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7a8>
10036a758:      add x8, x8, #0xa8c
10036a75c:      ldapr   w8, [x8]
10036a760:      cmp w8, #0x0
10036a764:      b.le    0x10036a96c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x484>
10036a768:      mov w8, #0x1                ; =1
10036a76c:      strb    w8, [x23]
10036a770:      bl  0x100447f80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
10036a774:      bl  0x100447988 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
10036a778:      ldrb    w8, [x21, #0x20]
10036a77c:      cbnz    w8, 0x10036a9bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4d4>
10036a780:      ldr x8, [x21]
10036a784:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10036a788:      cmp x8, x9
10036a78c:      b.hs    0x10036a9e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x500>
10036a790:      add x9, x8, #0x1
10036a794:      str x9, [x21]
10036a798:      ldr x10, [x21, #0x18]
10036a79c:      mov w9, #0x1                ; =1
10036a7a0:      cmp x25, x10
10036a7a4:      b.hs    0x10036a7b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2d0>
10036a7a8:      ldr x10, [x21, #0x10]
10036a7ac:      ldr x10, [x10, x25, lsl #3]
10036a7b0:      and x10, x10, #0xffffffffffff
10036a7b4:      b   0x10036a7bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2d4>
10036a7b8:      mov w10, #0x1               ; =1
10036a7bc:      str x8, [x21]
10036a7c0:      add x8, x10, #0x14
10036a7c4:      movi.2d v0, #0000000000000000
10036a7c8:      stur    q0, [x24, #0x78]
10036a7cc:      stur    q0, [x24, #0x68]
10036a7d0:      stur    q0, [x24, #0x58]
10036a7d4:      stur    q0, [x24, #0x48]
10036a7d8:      strb    w9, [sp, #0x120]
10036a7dc:      mov x9, #-0x1               ; =-1
10036a7e0:      stp x8, x20, [sp, #0xb8]
10036a7e4:      str x9, [sp, #0x90]
10036a7e8:      stp xzr, xzr, [sp, #0xc8]
10036a7ec:      str xzr, [sp, #0x118]
10036a7f0:      add x0, sp, #0x90
10036a7f4:      bl  0x10033aaa0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10036a7f8:      mov x20, x0
10036a7fc:      ldp x8, x9, [sp, #0xc0]
10036a800:      cmp x9, x8
10036a804:      b.hs    0x10036a838 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
10036a808:      ldr x10, [sp, #0xb8]
10036a80c:      mov x11, #0x2600            ; =9728
10036a810:      movk    x11, #0x1, lsl #32
10036a814:      ldrb    w12, [x10, x9]
10036a818:      cmp w12, #0x20
10036a81c:      b.hi    0x10036a838 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
10036a820:      lsr x12, x11, x12
10036a824:      tbz w12, #0x0, 0x10036a838 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
10036a828:      add x9, x9, #0x1
10036a82c:      cmp x8, x9
10036a830:      b.ne    0x10036a814 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x32c>
10036a834:      mov x9, x8
10036a838:      ldrb    w23, [sp, #0x120]
10036a83c:      cmp x9, x8
10036a840:      cset    w24, eq
10036a844:      ldrb    w8, [x21, #0x20]
10036a848:      cbnz    w8, 0x10036aae8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x600>
10036a84c:      ldr x8, [x21]
10036a850:      cbnz    x8, 0x10036ab0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x624>
10036a854:      mov x8, #-0x1               ; =-1
10036a858:      str x8, [x21]
10036a85c:      ldr x26, [x21, #0x18]
10036a860:      ldr x8, [x21, #0x8]
10036a864:      cmp x26, x8
10036a868:      b.ne    0x10036a874 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x38c>
10036a86c:      mov x0, x22
10036a870:      bl  0x100cd4250 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
10036a874:      ldr x8, [x21, #0x10]
10036a878:      str x20, [x8, x26, lsl #3]
10036a87c:      add x8, x26, #0x1
10036a880:      str x8, [x21, #0x18]
10036a884:      ldr x8, [x21]
10036a888:      add x8, x8, #0x1
10036a88c:      str x8, [x21]
10036a890:      mov x0, #0x0                ; =0
10036a894:      bl  0x100482170 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
10036a898:      ldrb    w8, [x0]
10036a89c:      and w8, w8, #0xfffffffd
10036a8a0:      strb    w8, [x0]
10036a8a4:      bl  0x100448a90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy22gc_bump_malloc_trigger>
10036a8a8:      bl  0x10044d36c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy49gc_schedule_parse_boundary_collection_if_pressure>
10036a8ac:      ldrb    w8, [x21, #0x20]
10036a8b0:      cbnz    w8, 0x10036ab18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x630>
10036a8b4:      ldr x8, [x21]
10036a8b8:      cbnz    x8, 0x10036ab48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x660>
10036a8bc:      and w22, w24, w23
10036a8c0:      ldr x8, [x21, #0x18]
10036a8c4:      cmp x25, x8
10036a8c8:      b.hi    0x10036a8d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x3e8>
10036a8cc:      str x25, [x21, #0x18]
10036a8d0:      adrp    x0, 0x1010b3000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.209>
10036a8d4:      add x0, x0, #0xd30
10036a8d8:      bl  0x10013a24c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api17parse_result_slows_0uEB2P_>
10036a8dc:      tbz w22, #0x0, 0x10036a8f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x40c>
10036a8e0:      stp xzr, x20, [x19]
10036a8e4:      ldr x8, [sp, #0x90]
10036a8e8:      cmn x8, #0x1
10036a8ec:      b.ne    0x10036a940 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x458>
10036a8f0:      b   0x10036a94c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
10036a8f4:      adrp    x0, 0x100dd4000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.297+0x2d5b>
10036a8f8:      add x0, x0, #0xe03
10036a8fc:      mov w1, #0x21               ; =33
10036a900:      mov w2, #0x21               ; =33
10036a904:      bl  0x100958040 <_js_string_from_bytes_with_capacity>
10036a908:      mov x3, x0
10036a90c:      adrp    x1, 0x100dd1000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.52+0x78>
10036a910:      add x1, x1, #0x242
10036a914:      mov w20, #0x1               ; =1
10036a918:      mov w0, #0x4                ; =4
10036a91c:      mov w2, #0xb                ; =11
10036a920:      mov w4, #0x1                ; =1
10036a924:      bl  0x1003536bc <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
10036a928:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
10036a92c:      bfxil   x8, x0, #0, #48
10036a930:      stp x20, x8, [x19]
10036a934:      ldr x8, [sp, #0x90]
10036a938:      cmn x8, #0x1
10036a93c:      b.eq    0x10036a94c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
10036a940:      cbz x8, 0x10036a94c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
10036a944:      ldr x0, [sp, #0x98]
10036a948:      bl  0x100ce70c0 <_mi_free>
10036a94c:      ldp x29, x30, [sp, #0x190]
10036a950:      ldp x20, x19, [sp, #0x180]
10036a954:      ldp x22, x21, [sp, #0x170]
10036a958:      ldp x24, x23, [sp, #0x160]
10036a95c:      ldp x26, x25, [sp, #0x150]
10036a960:      ldp x28, x27, [sp, #0x140]
10036a964:      add sp, sp, #0x1a0
10036a968:      ret
10036a96c:      mov x0, #0x0                ; =0
10036a970:      bl  0x100482370 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5block17ARENA_TOTAL_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1l_6option6OptionQIB20_INtNtB1l_4cell4CelljEEEEE9call_onceBc_>
10036a974:      ldr x23, [x0]
10036a978:      mov x0, #0x0                ; =0
10036a97c:      bl  0x100482050 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy21GC_NEXT_TRIGGER_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1n_6option6OptionQIB22_INtNtB1n_4cell4CelljEEEEE9call_onceBc_>
10036a980:      ldr x8, [x0]
10036a984:      cmp x8, x23
10036a988:      b.ls    0x10036a9a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4c0>
10036a98c:      str x23, [x0]
10036a990:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
10036a994:      add x0, x0, #0xad0
10036a998:      ldr x8, [x0]
10036a99c:      blr x8
10036a9a0:      mov w8, #0x1                ; =1
10036a9a4:      strb    w8, [x0]
10036a9a8:      bl  0x100447f80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
10036a9ac:      bl  0x100447f80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
10036a9b0:      bl  0x100447988 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
10036a9b4:      ldrb    w8, [x21, #0x20]
10036a9b8:      cbz w8, 0x10036a780 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x298>
10036a9bc:      cmp w8, #0x2
10036a9c0:      b.eq    0x10036ab20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x638>
10036a9c4:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10036a9c8:      add x1, x1, #0x87c
10036a9cc:      mov x0, x21
10036a9d0:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10036a9d4:      strb    wzr, [x21, #0x20]
10036a9d8:      ldr x8, [x21]
10036a9dc:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10036a9e0:      cmp x8, x9
10036a9e4:      b.lo    0x10036a790 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2a8>
10036a9e8:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10036a9ec:      add x0, x0, #0xdc8
10036a9f0:      bl  0x100c9de9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10036a9f4:      stur    x20, [x29, #-0x68]
10036a9f8:      mov x8, #0x7fff000000000000 ; =9223090561878065152
10036a9fc:      bfxil   x8, x22, #0, #48
10036aa00:      str x8, [sp, #0x90]
10036aa04:      adrp    x21, 0x1010b3000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.209>
10036aa08:      add x21, x21, #0xd28
10036aa0c:      add x1, sp, #0x90
10036aa10:      mov x0, x21
10036aa14:      bl  0x100137a90 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json15parse_root_push0jEB2h_>
10036aa18:      mov x22, x0
10036aa1c:      stp x0, x23, [x29, #-0x60]
10036aa20:      str x20, [sp]
10036aa24:      sub x8, x29, #0x58
10036aa28:      mov x9, sp
10036aa2c:      stp x8, x9, [sp, #0x90]
10036aa30:      sub x8, x29, #0x60
10036aa34:      sub x9, x29, #0x68
10036aa38:      stp x8, x9, [sp, #0xa0]
10036aa3c:      adrp    x0, 0x1010b2000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3map29KEEP_JS_MAP_DELETE_NUMBER_KEY>
10036aa40:      add x0, x0, #0x388
10036aa44:      add x1, sp, #0x90
10036aa48:      bl  0x10012c958 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawIB1t_NtNtNtB1S_5value7jsvalue7JSValueENCINvB1Q_19with_built_tape_rawB3o_NCNvNtNtB1S_4json9parse_api24try_parse_deep_iterative0E0E0IB1t_B3o_EEB1S_>
10036aa4c:      mov x23, x0
10036aa50:      mov x20, x1
10036aa54:      str x22, [sp, #0x90]
10036aa58:      add x1, sp, #0x90
10036aa5c:      mov x0, x21
10036aa60:      bl  0x100137b28 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json18parse_root_restore0uEB2h_>
10036aa64:      adrp    x0, 0x1010b3000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.209>
10036aa68:      add x0, x0, #0xd30
10036aa6c:      bl  0x10013a3b4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api24try_parse_deep_iteratives_0uEB2P_>
10036aa70:      tbnz    w23, #0x0, 0x10036aab4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5cc>
10036aa74:      adrp    x0, 0x100dd1000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.52+0x78>
10036aa78:      add x0, x0, #0x1d9
10036aa7c:      mov w1, #0x29               ; =41
10036aa80:      mov w2, #0x29               ; =41
10036aa84:      bl  0x100958040 <_js_string_from_bytes_with_capacity>
10036aa88:      mov x3, x0
10036aa8c:      adrp    x1, 0x100dd1000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.52+0x78>
10036aa90:      add x1, x1, #0x242
10036aa94:      mov w21, #0x1               ; =1
10036aa98:      mov w0, #0x4                ; =4
10036aa9c:      mov w2, #0xb                ; =11
10036aaa0:      mov w4, #0x1                ; =1
10036aaa4:      bl  0x1003536bc <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
10036aaa8:      mov x20, #0x7ffd000000000000 ; =9222527611924643840
10036aaac:      bfxil   x20, x0, #0, #48
10036aab0:      b   0x10036aab8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5d0>
10036aab4:      mov x21, #0x0               ; =0
10036aab8:      stp x21, x20, [x19]
10036aabc:      b   0x10036a94c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
10036aac0:      cmp w8, #0x1
10036aac4:      b.ne    0x10036ab20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x638>
10036aac8:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10036aacc:      add x1, x1, #0x87c
10036aad0:      mov x0, x21
10036aad4:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10036aad8:      strb    wzr, [x21, #0x20]
10036aadc:      ldr x8, [x21]
10036aae0:      cbz x8, 0x10036a6e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1f8>
10036aae4:      b   0x10036ab0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x624>
10036aae8:      cmp w8, #0x2
10036aaec:      b.eq    0x10036ab20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x638>
10036aaf0:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10036aaf4:      add x1, x1, #0x87c
10036aaf8:      mov x0, x21
10036aafc:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10036ab00:      strb    wzr, [x21, #0x20]
10036ab04:      ldr x8, [x21]
10036ab08:      cbz x8, 0x10036a854 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x36c>
10036ab0c:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10036ab10:      add x0, x0, #0xdf8
10036ab14:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10036ab18:      cmp w8, #0x2
10036ab1c:      b.ne    0x10036ab2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x644>
10036ab20:      adrp    x0, 0x1010a3000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10036ab24:      add x0, x0, #0xed8
10036ab28:      bl  0x100ce071c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10036ab2c:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10036ab30:      add x1, x1, #0x87c
10036ab34:      mov x0, x21
10036ab38:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10036ab3c:      strb    wzr, [x21, #0x20]
10036ab40:      ldr x8, [x21]
10036ab44:      cbz x8, 0x10036a8bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x3d4>
10036ab48:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10036ab4c:      add x0, x0, #0xe58
10036ab50:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
