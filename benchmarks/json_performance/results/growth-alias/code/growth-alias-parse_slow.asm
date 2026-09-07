/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/growth-alias-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001003e85f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow>:
1003e85f4:      sub sp, sp, #0x1a0
1003e85f8:      stp x28, x27, [sp, #0x140]
1003e85fc:      stp x26, x25, [sp, #0x150]
1003e8600:      stp x24, x23, [sp, #0x160]
1003e8604:      stp x22, x21, [sp, #0x170]
1003e8608:      stp x20, x19, [sp, #0x180]
1003e860c:      stp x29, x30, [sp, #0x190]
1003e8610:      add x29, sp, #0x190
1003e8614:      mov x20, x1
1003e8618:      mov x21, x0
1003e861c:      add x25, sp, #0x90
1003e8620:      add x22, x0, #0x14
1003e8624:      cmp x1, #0x2
1003e8628:      b.ne    0x1003e8640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4c>
1003e862c:      ldrh    w8, [x22]
1003e8630:      mov w9, #0x7d7b             ; =32123
1003e8634:      cmp w8, w9
1003e8638:      b.eq    0x1003e8674 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x80>
1003e863c:      b   0x1003e86c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xd4>
1003e8640:      cmp x20, #0x3
1003e8644:      b.lo    0x1003e86c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xd4>
1003e8648:      ldrb    w8, [x22]
1003e864c:      cmp w8, #0x20
1003e8650:      b.hi    0x1003e8694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xa0>
1003e8654:      mov x9, #0x2600             ; =9728
1003e8658:      movk    x9, #0x1, lsl #32
1003e865c:      lsr x9, x9, x8
1003e8660:      tbz w9, #0x0, 0x1003e8694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xa0>
1003e8664:      add x0, x21, #0x14
1003e8668:      mov x1, x20
1003e866c:      bl  0x10020dc88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty19padded_empty_object>
1003e8670:      tbz w0, #0x0, 0x1003e86c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xcc>
1003e8674:      ldp x29, x30, [sp, #0x190]
1003e8678:      ldp x20, x19, [sp, #0x180]
1003e867c:      ldp x22, x21, [sp, #0x170]
1003e8680:      ldp x24, x23, [sp, #0x160]
1003e8684:      ldp x26, x25, [sp, #0x150]
1003e8688:      ldp x28, x27, [sp, #0x140]
1003e868c:      add sp, sp, #0x1a0
1003e8690:      b   0x10020dd58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty21allocate_empty_object>
1003e8694:      cmp w8, #0x7b
1003e8698:      b.ne    0x1003e86c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xcc>
1003e869c:      ldrb    w8, [x21, #0x15]
1003e86a0:      cmp w8, #0x20
1003e86a4:      b.hi    0x1003e86b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xc4>
1003e86a8:      mov x9, #0x2600             ; =9728
1003e86ac:      movk    x9, #0x1, lsl #32
1003e86b0:      lsr x9, x9, x8
1003e86b4:      tbnz    w9, #0x0, 0x1003e8664 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x70>
1003e86b8:      cmp w8, #0x7d
1003e86bc:      b.eq    0x1003e8664 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x70>
1003e86c0:      cmp x20, #0x41
1003e86c4:      b.hs    0x1003e872c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x138>
1003e86c8:      add x0, sp, #0x90
1003e86cc:      add x1, x21, #0x14
1003e86d0:      mov x2, x20
1003e86d4:      bl  0x1003e030c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode>
1003e86d8:      ldr x8, [sp, #0x90]
1003e86dc:      cbz x8, 0x1003e87f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1fc>
1003e86e0:      ldr x8, [sp, #0x118]
1003e86e4:      str x8, [sp, #0x80]
1003e86e8:      ldur    q0, [x25, #0x48]
1003e86ec:      ldur    q1, [x25, #0x58]
1003e86f0:      stp q0, q1, [sp, #0x40]
1003e86f4:      ldur    q0, [x25, #0x68]
1003e86f8:      ldur    q1, [x25, #0x78]
1003e86fc:      stp q0, q1, [sp, #0x60]
1003e8700:      ldur    q0, [x25, #0x8]
1003e8704:      ldur    q1, [x25, #0x18]
1003e8708:      stp q0, q1, [sp]
1003e870c:      ldur    q0, [x25, #0x28]
1003e8710:      ldur    q1, [x25, #0x38]
1003e8714:      stp q0, q1, [sp, #0x20]
1003e8718:      mov x0, sp
1003e871c:      bl  0x1003e0d6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate>
1003e8720:      tbz w0, #0x0, 0x1003e87f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1fc>
1003e8724:      mov x23, x1
1003e8728:      b   0x1003e8c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x650>
1003e872c:      cmp x20, #0x3e9
1003e8730:      b.lo    0x1003e87f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1fc>
1003e8734:      add x0, x21, #0x14
1003e8738:      mov x1, x20
1003e873c:      mov w2, #0x3e8              ; =1000
1003e8740:      bl  0x1003e1910 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1003e8744:      tbz w0, #0x0, 0x1003e87f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1fc>
1003e8748:      add x0, x21, #0x14
1003e874c:      mov x1, x20
1003e8750:      mov w2, #0xa120             ; =41248
1003e8754:      movk    w2, #0x7, lsl #16
1003e8758:      bl  0x1003e1910 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1003e875c:      tbnz    w0, #0x0, 0x1003e8dfc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x808>
1003e8760:      stur    x20, [x29, #-0x68]
1003e8764:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1003e8768:      bfxil   x8, x21, #0, #48
1003e876c:      str x8, [sp, #0x90]
1003e8770:      adrp    x19, 0x10109e000 <_anon.32ca3690520b3140c3df72b88a347d65.100+0x178>
1003e8774:      add x19, x19, #0xf90
1003e8778:      add x1, sp, #0x90
1003e877c:      mov x0, x19
1003e8780:      bl  0x100137490 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json15parse_root_push0jEB2h_>
1003e8784:      mov x21, x0
1003e8788:      stp x0, x22, [x29, #-0x60]
1003e878c:      str x20, [sp]
1003e8790:      sub x8, x29, #0x58
1003e8794:      mov x9, sp
1003e8798:      stp x8, x9, [sp, #0x90]
1003e879c:      sub x8, x29, #0x60
1003e87a0:      sub x9, x29, #0x68
1003e87a4:      stp x8, x9, [sp, #0xa0]
1003e87a8:      adrp    x0, 0x10109d000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi4read17READ_OBJECT_CACHE+0x78>
1003e87ac:      add x0, x0, #0x938
1003e87b0:      add x1, sp, #0x90
1003e87b4:      bl  0x10012c4d8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawIB1t_NtNtNtB1S_5value7jsvalue7JSValueENCINvB1Q_19with_built_tape_rawB3o_NCNvNtNtB1S_4json9parse_api24try_parse_deep_iterative0E0E0IB1t_B3o_EEB1S_>
1003e87b8:      mov x20, x0
1003e87bc:      mov x23, x1
1003e87c0:      str x21, [sp, #0x90]
1003e87c4:      add x1, sp, #0x90
1003e87c8:      mov x0, x19
1003e87cc:      bl  0x100137528 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json18parse_root_restore0uEB2h_>
1003e87d0:      adrp    x0, 0x10109e000 <_anon.32ca3690520b3140c3df72b88a347d65.100+0x178>
1003e87d4:      add x0, x0, #0xf98
1003e87d8:      bl  0x100139d44 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api24try_parse_deep_iteratives_0uEB2P_>
1003e87dc:      tbnz    w20, #0x0, 0x1003e8c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x650>
1003e87e0:      adrp    x0, 0x100dd0000 <_PERRY_EMPTY_STRING+0x44>
1003e87e4:      add x0, x0, #0x265
1003e87e8:      mov w1, #0x29               ; =41
1003e87ec:      bl  0x1003e9b3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
1003e87f0:      adrp    x0, 0x101126000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json25OBJECT_PROTO_TOJSON_STATE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1003e87f4:      add x0, x0, #0x8
1003e87f8:      ldr x8, [x0]
1003e87fc:      blr x8
1003e8800:      mov x19, x0
1003e8804:      ldrb    w8, [x0, #0x20]
1003e8808:      cbnz    w8, 0x1003e8ca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6ac>
1003e880c:      ldr x8, [x19]
1003e8810:      cbnz    x8, 0x1003e8d1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x728>
1003e8814:      mov x23, #0x7fff000000000000 ; =9223090561878065152
1003e8818:      bfxil   x23, x21, #0, #48
1003e881c:      mov x8, #-0x1               ; =-1
1003e8820:      str x8, [x19]
1003e8824:      mov x22, x19
1003e8828:      ldr x8, [x22, #0x8]!
1003e882c:      ldr x24, [x19, #0x18]
1003e8830:      cmp x24, x8
1003e8834:      b.ne    0x1003e8840 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x24c>
1003e8838:      mov x0, x22
1003e883c:      bl  0x100cb708c <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1003e8840:      ldr x8, [x19, #0x10]
1003e8844:      str x23, [x8, x24, lsl #3]
1003e8848:      add x8, x24, #0x1
1003e884c:      str x8, [x19, #0x18]
1003e8850:      ldr x8, [x19]
1003e8854:      add x8, x8, #0x1
1003e8858:      str x8, [x19]
1003e885c:      mov x0, #0x0                ; =0
1003e8860:      bl  0x10030335c <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy43GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1J_6option6OptionQIB2o_INtNtB1J_4cell4CellbEEEEE9call_onceBc_>
1003e8864:      mov x21, x0
1003e8868:      ldrb    w8, [x0]
1003e886c:      strb    wzr, [x0]
1003e8870:      cbz w8, 0x1003e88a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2b0>
1003e8874:      mov x0, #0x0                ; =0
1003e8878:      bl  0x10030337c <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1003e887c:      ldrb    w8, [x0]
1003e8880:      tst w8, #0x3
1003e8884:      b.ne    0x1003e889c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2a8>
1003e8888:      adrp    x8, 0x10116c000 <_out_buf+0x3f08>
1003e888c:      add x8, x8, #0x790
1003e8890:      ldapr   w8, [x8]
1003e8894:      cmp w8, #0x0
1003e8898:      b.le    0x1003e89fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x408>
1003e889c:      mov w8, #0x1                ; =1
1003e88a0:      strb    w8, [x21]
1003e88a4:      ldrb    w8, [x19, #0x20]
1003e88a8:      cbnz    w8, 0x1003e8a44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x450>
1003e88ac:      ldr x8, [x19]
1003e88b0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1003e88b4:      cmp x8, x9
1003e88b8:      b.hs    0x1003e8dac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7b8>
1003e88bc:      add x9, x8, #0x1
1003e88c0:      str x9, [x19]
1003e88c4:      ldr x9, [x19, #0x18]
1003e88c8:      cmp x24, x9
1003e88cc:      b.hs    0x1003e88e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2ec>
1003e88d0:      ldr x9, [x19, #0x10]
1003e88d4:      ldr x9, [x9, x24, lsl #3]
1003e88d8:      and x23, x9, #0xffffffffffff
1003e88dc:      b   0x1003e88e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2f0>
1003e88e0:      mov w23, #0x1               ; =1
1003e88e4:      str x8, [x19]
1003e88e8:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e88ec:      add x8, x8, #0xa58
1003e88f0:      ldapr   x8, [x8]
1003e88f4:      cbnz    x8, 0x1003e8c68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x674>
1003e88f8:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e88fc:      ldrb    w8, [x8, #0xa60]
1003e8900:      cmp w8, #0x2
1003e8904:      b.eq    0x1003e8a7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x488>
1003e8908:      cmp w8, #0x1
1003e890c:      b.ne    0x1003e8950 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x35c>
1003e8910:      stp x24, x20, [x29, #-0x68]
1003e8914:      ldrb    w8, [x19, #0x20]
1003e8918:      cbnz    w8, 0x1003e8d80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x78c>
1003e891c:      ldr x8, [x19]
1003e8920:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1003e8924:      cmp x8, x9
1003e8928:      b.hs    0x1003e8dac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7b8>
1003e892c:      add x9, x8, #0x1
1003e8930:      str x9, [x19]
1003e8934:      ldr x9, [x19, #0x18]
1003e8938:      cmp x24, x9
1003e893c:      b.hs    0x1003e8990 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x39c>
1003e8940:      ldr x9, [x19, #0x10]
1003e8944:      ldr x9, [x9, x24, lsl #3]
1003e8948:      and x9, x9, #0xffffffffffff
1003e894c:      b   0x1003e8994 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x3a0>
1003e8950:      add x8, x23, #0x14
1003e8954:      sub x9, x20, #0x400
1003e8958:      mov w10, #0xfffc00          ; =16776192
1003e895c:      cmp x9, x10
1003e8960:      b.hi    0x1003e8a7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x488>
1003e8964:      mov x9, #0x2600             ; =9728
1003e8968:      movk    x9, #0x1, lsl #32
1003e896c:      mov x10, x20
1003e8970:      ldrb    w11, [x8], #0x1
1003e8974:      cmp w11, #0x20
1003e8978:      b.hi    0x1003e8a74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
1003e897c:      lsr x12, x9, x11
1003e8980:      tbz w12, #0x0, 0x1003e8a74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
1003e8984:      subs    x10, x10, #0x1
1003e8988:      b.ne    0x1003e8970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x37c>
1003e898c:      b   0x1003e8a7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x488>
1003e8990:      mov w9, #0x1                ; =1
1003e8994:      str x8, [x19]
1003e8998:      add x8, x9, #0x14
1003e899c:      stur    x8, [x29, #-0x58]
1003e89a0:      str x20, [sp]
1003e89a4:      sub x8, x29, #0x58
1003e89a8:      mov x9, sp
1003e89ac:      stp x8, x9, [sp, #0x90]
1003e89b0:      sub x8, x29, #0x68
1003e89b4:      sub x9, x29, #0x60
1003e89b8:      stp x8, x9, [sp, #0xa0]
1003e89bc:      adrp    x0, 0x10109d000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi4read17READ_OBJECT_CACHE+0x78>
1003e89c0:      add x0, x0, #0x938
1003e89c4:      add x1, sp, #0x90
1003e89c8:      bl  0x10012c92c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawNtNtNtB1S_5value7jsvalue7JSValueNCNvNtNtB1S_4json9parse_api18try_parse_via_tape0E0IB1t_B3o_EEB1S_>
1003e89cc:      cmp x0, #0x1
1003e89d0:      b.ne    0x1003e8a7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x488>
1003e89d4:      mov x23, x1
1003e89d8:      ldrb    w8, [x19, #0x20]
1003e89dc:      cbnz    w8, 0x1003e8db8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7c4>
1003e89e0:      ldr x8, [x19]
1003e89e4:      cbnz    x8, 0x1003e8d4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x758>
1003e89e8:      ldr x8, [x19, #0x18]
1003e89ec:      cmp x24, x8
1003e89f0:      b.hi    0x1003e8c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x650>
1003e89f4:      str x24, [x19, #0x18]
1003e89f8:      b   0x1003e8c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x650>
1003e89fc:      mov x0, #0x0                ; =0
1003e8a00:      bl  0x100303434 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5block17ARENA_TOTAL_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1l_6option6OptionQIB20_INtNtB1l_4cell4CelljEEEEE9call_onceBc_>
1003e8a04:      ldr x23, [x0]
1003e8a08:      mov x0, #0x0                ; =0
1003e8a0c:      bl  0x1003031dc <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy21GC_NEXT_TRIGGER_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1n_6option6OptionQIB22_INtNtB1n_4cell4CelljEEEEE9call_onceBc_>
1003e8a10:      ldr x8, [x0]
1003e8a14:      cmp x8, x23
1003e8a18:      b.ls    0x1003e8a38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x444>
1003e8a1c:      str x23, [x0]
1003e8a20:      adrp    x0, 0x101126000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json25OBJECT_PROTO_TOJSON_STATE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1003e8a24:      add x0, x0, #0x860
1003e8a28:      ldr x8, [x0]
1003e8a2c:      blr x8
1003e8a30:      mov w8, #0x1                ; =1
1003e8a34:      strb    w8, [x0]
1003e8a38:      bl  0x1002af600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1003e8a3c:      ldrb    w8, [x19, #0x20]
1003e8a40:      cbz w8, 0x1003e88ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2b8>
1003e8a44:      cmp w8, #0x2
1003e8a48:      b.eq    0x1003e8dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7cc>
1003e8a4c:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003e8a50:      add x1, x1, #0x824
1003e8a54:      mov x0, x19
1003e8a58:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003e8a5c:      strb    wzr, [x19, #0x20]
1003e8a60:      ldr x8, [x19]
1003e8a64:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1003e8a68:      cmp x8, x9
1003e8a6c:      b.lo    0x1003e88bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2c8>
1003e8a70:      b   0x1003e8dac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7b8>
1003e8a74:      cmp w11, #0x5b
1003e8a78:      b.eq    0x1003e8910 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x31c>
1003e8a7c:      bl  0x1002af600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1003e8a80:      bl  0x1002af4ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
1003e8a84:      ldrb    w8, [x19, #0x20]
1003e8a88:      cbnz    w8, 0x1003e8cc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6d4>
1003e8a8c:      ldr x8, [x19]
1003e8a90:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1003e8a94:      cmp x8, x9
1003e8a98:      b.hs    0x1003e8dac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7b8>
1003e8a9c:      add x9, x8, #0x1
1003e8aa0:      str x9, [x19]
1003e8aa4:      ldr x10, [x19, #0x18]
1003e8aa8:      mov w9, #0x1                ; =1
1003e8aac:      cmp x24, x10
1003e8ab0:      b.hs    0x1003e8ac4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4d0>
1003e8ab4:      ldr x10, [x19, #0x10]
1003e8ab8:      ldr x10, [x10, x24, lsl #3]
1003e8abc:      and x10, x10, #0xffffffffffff
1003e8ac0:      b   0x1003e8ac8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4d4>
1003e8ac4:      mov w10, #0x1               ; =1
1003e8ac8:      str x8, [x19]
1003e8acc:      add x8, x10, #0x14
1003e8ad0:      movi.2d v0, #0000000000000000
1003e8ad4:      stur    q0, [x25, #0x78]
1003e8ad8:      stur    q0, [x25, #0x68]
1003e8adc:      stur    q0, [x25, #0x58]
1003e8ae0:      stur    q0, [x25, #0x48]
1003e8ae4:      strb    w9, [sp, #0x120]
1003e8ae8:      mov x9, #-0x1               ; =-1
1003e8aec:      stp x8, x20, [sp, #0xb8]
1003e8af0:      str x9, [sp, #0x90]
1003e8af4:      stp xzr, xzr, [sp, #0xc8]
1003e8af8:      str xzr, [sp, #0x118]
1003e8afc:      add x0, sp, #0x90
1003e8b00:      bl  0x1003b9e0c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1003e8b04:      mov x23, x0
1003e8b08:      ldp x8, x9, [sp, #0xc0]
1003e8b0c:      cmp x9, x8
1003e8b10:      b.hs    0x1003e8b44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x550>
1003e8b14:      ldr x10, [sp, #0xb8]
1003e8b18:      mov x11, #0x2600            ; =9728
1003e8b1c:      movk    x11, #0x1, lsl #32
1003e8b20:      ldrb    w12, [x10, x9]
1003e8b24:      cmp w12, #0x20
1003e8b28:      b.hi    0x1003e8b44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x550>
1003e8b2c:      lsr x12, x11, x12
1003e8b30:      tbz w12, #0x0, 0x1003e8b44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x550>
1003e8b34:      add x9, x9, #0x1
1003e8b38:      cmp x8, x9
1003e8b3c:      b.ne    0x1003e8b20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x52c>
1003e8b40:      mov x9, x8
1003e8b44:      ldrb    w20, [sp, #0x120]
1003e8b48:      cmp x9, x8
1003e8b4c:      cset    w25, eq
1003e8b50:      ldrb    w8, [x19, #0x20]
1003e8b54:      cbnz    w8, 0x1003e8cf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x704>
1003e8b58:      ldr x8, [x19]
1003e8b5c:      cbnz    x8, 0x1003e8d1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x728>
1003e8b60:      mov x8, #-0x1               ; =-1
1003e8b64:      str x8, [x19]
1003e8b68:      ldr x26, [x19, #0x18]
1003e8b6c:      ldr x8, [x19, #0x8]
1003e8b70:      cmp x26, x8
1003e8b74:      b.ne    0x1003e8b80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x58c>
1003e8b78:      mov x0, x22
1003e8b7c:      bl  0x100cb708c <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1003e8b80:      ldr x8, [x19, #0x10]
1003e8b84:      str x23, [x8, x26, lsl #3]
1003e8b88:      add x8, x26, #0x1
1003e8b8c:      str x8, [x19, #0x18]
1003e8b90:      ldr x8, [x19]
1003e8b94:      add x8, x8, #0x1
1003e8b98:      str x8, [x19]
1003e8b9c:      mov x0, #0x0                ; =0
1003e8ba0:      bl  0x10030337c <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1003e8ba4:      ldrb    w8, [x0]
1003e8ba8:      and w8, w8, #0xfffffffd
1003e8bac:      strb    w8, [x0]
1003e8bb0:      bl  0x1002b0098 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy22gc_bump_malloc_trigger>
1003e8bb4:      adrp    x22, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e8bb8:      add x22, x22, #0x4b8
1003e8bbc:      ldapr   x8, [x22]
1003e8bc0:      cbnz    x8, 0x1003e8c88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x694>
1003e8bc4:      ldrb    w8, [x22, #0x8]
1003e8bc8:      cbz w8, 0x1003e8bf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x604>
1003e8bcc:      bl  0x1001971c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena4walk18arena_in_use_bytes>
1003e8bd0:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e8bd4:      add x8, x8, #0x550
1003e8bd8:      ldapr   x8, [x8]
1003e8bdc:      cbnz    x8, 0x1003e8d58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x764>
1003e8be0:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e8be4:      ldr x8, [x8, #0x558]
1003e8be8:      cmp x0, x8
1003e8bec:      b.lo    0x1003e8bf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x604>
1003e8bf0:      mov w8, #0x1                ; =1
1003e8bf4:      strb    w8, [x21]
1003e8bf8:      ldrb    w8, [x19, #0x20]
1003e8bfc:      cbnz    w8, 0x1003e8d28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x734>
1003e8c00:      ldr x8, [x19]
1003e8c04:      cbnz    x8, 0x1003e8d4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x758>
1003e8c08:      and w20, w25, w20
1003e8c0c:      ldr x8, [x19, #0x18]
1003e8c10:      cmp x24, x8
1003e8c14:      b.hi    0x1003e8c1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x628>
1003e8c18:      str x24, [x19, #0x18]
1003e8c1c:      adrp    x0, 0x10109e000 <_anon.32ca3690520b3140c3df72b88a347d65.100+0x178>
1003e8c20:      add x0, x0, #0xf98
1003e8c24:      bl  0x100139a74 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api10parse_slow0uEB2P_>
1003e8c28:      tbz w20, #0x0, 0x1003e8dec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7f8>
1003e8c2c:      ldr x8, [sp, #0x90]
1003e8c30:      cmn x8, #0x1
1003e8c34:      b.eq    0x1003e8c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x650>
1003e8c38:      cbz x8, 0x1003e8c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x650>
1003e8c3c:      ldr x0, [sp, #0x98]
1003e8c40:      bl  0x100cd5f00 <_mi_free>
1003e8c44:      mov x0, x23
1003e8c48:      ldp x29, x30, [sp, #0x190]
1003e8c4c:      ldp x20, x19, [sp, #0x180]
1003e8c50:      ldp x22, x21, [sp, #0x170]
1003e8c54:      ldp x24, x23, [sp, #0x160]
1003e8c58:      ldp x26, x25, [sp, #0x150]
1003e8c5c:      ldp x28, x27, [sp, #0x140]
1003e8c60:      add sp, sp, #0x1a0
1003e8c64:      ret
1003e8c68:      adrp    x0, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e8c6c:      add x0, x0, #0xa58
1003e8c70:      bl  0x100cb5e3c <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockNtNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api8TapeModeE10initializeNCINvB2_11get_or_initNCNvBV_18tape_mode_from_env0E0zEBZ_>
1003e8c74:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e8c78:      ldrb    w8, [x8, #0xa60]
1003e8c7c:      cmp w8, #0x2
1003e8c80:      b.ne    0x1003e8908 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x314>
1003e8c84:      b   0x1003e8a7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x488>
1003e8c88:      adrp    x0, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e8c8c:      add x0, x0, #0x4b8
1003e8c90:      bl  0x100cb5fcc <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtCs5gMwpk3Cs4e_13perry_runtime2gc14gen_gc_enabled0E0zEB1y_>
1003e8c94:      ldrb    w8, [x22, #0x8]
1003e8c98:      cbnz    w8, 0x1003e8bcc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x5d8>
1003e8c9c:      b   0x1003e8bf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x604>
1003e8ca0:      cmp w8, #0x1
1003e8ca4:      b.ne    0x1003e8dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7cc>
1003e8ca8:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003e8cac:      add x1, x1, #0x824
1003e8cb0:      mov x0, x19
1003e8cb4:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003e8cb8:      strb    wzr, [x19, #0x20]
1003e8cbc:      ldr x8, [x19]
1003e8cc0:      cbz x8, 0x1003e8814 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x220>
1003e8cc4:      b   0x1003e8d1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x728>
1003e8cc8:      cmp w8, #0x2
1003e8ccc:      b.eq    0x1003e8dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7cc>
1003e8cd0:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003e8cd4:      add x1, x1, #0x824
1003e8cd8:      mov x0, x19
1003e8cdc:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003e8ce0:      strb    wzr, [x19, #0x20]
1003e8ce4:      ldr x8, [x19]
1003e8ce8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1003e8cec:      cmp x8, x9
1003e8cf0:      b.lo    0x1003e8a9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4a8>
1003e8cf4:      b   0x1003e8dac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7b8>
1003e8cf8:      cmp w8, #0x2
1003e8cfc:      b.eq    0x1003e8dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7cc>
1003e8d00:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003e8d04:      add x1, x1, #0x824
1003e8d08:      mov x0, x19
1003e8d0c:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003e8d10:      strb    wzr, [x19, #0x20]
1003e8d14:      ldr x8, [x19]
1003e8d18:      cbz x8, 0x1003e8b60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x56c>
1003e8d1c:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1003e8d20:      add x0, x0, #0xdf8
1003e8d24:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1003e8d28:      cmp w8, #0x2
1003e8d2c:      b.eq    0x1003e8dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7cc>
1003e8d30:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003e8d34:      add x1, x1, #0x824
1003e8d38:      mov x0, x19
1003e8d3c:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003e8d40:      strb    wzr, [x19, #0x20]
1003e8d44:      ldr x8, [x19]
1003e8d48:      cbz x8, 0x1003e8c08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x614>
1003e8d4c:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1003e8d50:      add x0, x0, #0xe58
1003e8d54:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1003e8d58:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e8d5c:      add x8, x8, #0x550
1003e8d60:      mov x22, x0
1003e8d64:      mov x0, x8
1003e8d68:      bl  0x100cb68fc <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockjE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc11heap_budget38gc_tiny_parse_in_use_trigger_dyn_bytes0E0zEB1A_>
1003e8d6c:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1003e8d70:      ldr x8, [x8, #0x558]
1003e8d74:      cmp x22, x8
1003e8d78:      b.hs    0x1003e8bf0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x5fc>
1003e8d7c:      b   0x1003e8bf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x604>
1003e8d80:      cmp w8, #0x2
1003e8d84:      b.eq    0x1003e8dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7cc>
1003e8d88:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003e8d8c:      add x1, x1, #0x824
1003e8d90:      mov x0, x19
1003e8d94:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003e8d98:      strb    wzr, [x19, #0x20]
1003e8d9c:      ldr x8, [x19]
1003e8da0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1003e8da4:      cmp x8, x9
1003e8da8:      b.lo    0x1003e892c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x338>
1003e8dac:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1003e8db0:      add x0, x0, #0xdc8
1003e8db4:      bl  0x100c8d1dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1003e8db8:      cmp w8, #0x2
1003e8dbc:      b.ne    0x1003e8dcc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7d8>
1003e8dc0:      adrp    x0, 0x10108f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1003e8dc4:      add x0, x0, #0xed8
1003e8dc8:      bl  0x100ccf55c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1003e8dcc:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003e8dd0:      add x1, x1, #0x824
1003e8dd4:      mov x0, x19
1003e8dd8:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003e8ddc:      strb    wzr, [x19, #0x20]
1003e8de0:      ldr x8, [x19]
1003e8de4:      cbz x8, 0x1003e89e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x3f4>
1003e8de8:      b   0x1003e8d4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x758>
1003e8dec:      adrp    x0, 0x100dd0000 <_PERRY_EMPTY_STRING+0x44>
1003e8df0:      add x0, x0, #0xa25
1003e8df4:      mov w1, #0x21               ; =33
1003e8df8:      bl  0x1003e9b3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
1003e8dfc:      add x0, sp, #0x90
1003e8e00:      bl  0x1003e9be4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api24iterative_budget_message>
1003e8e04:      ldp x0, x1, [sp, #0x98]
1003e8e08:      bl  0x1003e9538 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17throw_range_error>
