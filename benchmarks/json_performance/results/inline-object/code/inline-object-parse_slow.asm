/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/inline-object-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100834788 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow>:
100834788:      sub sp, sp, #0x1a0
10083478c:      stp x28, x27, [sp, #0x140]
100834790:      stp x26, x25, [sp, #0x150]
100834794:      stp x24, x23, [sp, #0x160]
100834798:      stp x22, x21, [sp, #0x170]
10083479c:      stp x20, x19, [sp, #0x180]
1008347a0:      stp x29, x30, [sp, #0x190]
1008347a4:      add x29, sp, #0x190
1008347a8:      mov x20, x1
1008347ac:      mov x21, x0
1008347b0:      add x25, sp, #0x90
1008347b4:      add x22, x0, #0x14
1008347b8:      cmp x1, #0x2
1008347bc:      b.ne    0x1008347d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4c>
1008347c0:      ldrh    w8, [x22]
1008347c4:      mov w9, #0x7d7b             ; =32123
1008347c8:      cmp w8, w9
1008347cc:      b.eq    0x100834808 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x80>
1008347d0:      b   0x10083485c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xd4>
1008347d4:      cmp x20, #0x3
1008347d8:      b.lo    0x10083485c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xd4>
1008347dc:      ldrb    w8, [x22]
1008347e0:      cmp w8, #0x20
1008347e4:      b.hi    0x100834828 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xa0>
1008347e8:      mov x9, #0x2600             ; =9728
1008347ec:      movk    x9, #0x1, lsl #32
1008347f0:      lsr x9, x9, x8
1008347f4:      tbz w9, #0x0, 0x100834828 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xa0>
1008347f8:      add x0, x21, #0x14
1008347fc:      mov x1, x20
100834800:      bl  0x10081a228 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty19padded_empty_object>
100834804:      tbz w0, #0x0, 0x100834854 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xcc>
100834808:      ldp x29, x30, [sp, #0x190]
10083480c:      ldp x20, x19, [sp, #0x180]
100834810:      ldp x22, x21, [sp, #0x170]
100834814:      ldp x24, x23, [sp, #0x160]
100834818:      ldp x26, x25, [sp, #0x150]
10083481c:      ldp x28, x27, [sp, #0x140]
100834820:      add sp, sp, #0x1a0
100834824:      b   0x10081a2f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty21allocate_empty_object>
100834828:      cmp w8, #0x7b
10083482c:      b.ne    0x100834854 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xcc>
100834830:      ldrb    w8, [x21, #0x15]
100834834:      cmp w8, #0x20
100834838:      b.hi    0x10083484c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xc4>
10083483c:      mov x9, #0x2600             ; =9728
100834840:      movk    x9, #0x1, lsl #32
100834844:      lsr x9, x9, x8
100834848:      tbnz    w9, #0x0, 0x1008347f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x70>
10083484c:      cmp w8, #0x7d
100834850:      b.eq    0x1008347f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x70>
100834854:      cmp x20, #0x41
100834858:      b.hs    0x1008348c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x138>
10083485c:      add x0, sp, #0x90
100834860:      add x1, x21, #0x14
100834864:      mov x2, x20
100834868:      bl  0x100821084 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode>
10083486c:      ldr x8, [sp, #0x90]
100834870:      cbz x8, 0x100834984 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1fc>
100834874:      ldr x8, [sp, #0x118]
100834878:      str x8, [sp, #0x80]
10083487c:      ldur    q0, [x25, #0x48]
100834880:      ldur    q1, [x25, #0x58]
100834884:      stp q0, q1, [sp, #0x40]
100834888:      ldur    q0, [x25, #0x68]
10083488c:      ldur    q1, [x25, #0x78]
100834890:      stp q0, q1, [sp, #0x60]
100834894:      ldur    q0, [x25, #0x8]
100834898:      ldur    q1, [x25, #0x18]
10083489c:      stp q0, q1, [sp]
1008348a0:      ldur    q0, [x25, #0x28]
1008348a4:      ldur    q1, [x25, #0x38]
1008348a8:      stp q0, q1, [sp, #0x20]
1008348ac:      mov x0, sp
1008348b0:      bl  0x100821ae4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate>
1008348b4:      tbz w0, #0x0, 0x100834984 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1fc>
1008348b8:      mov x23, x1
1008348bc:      b   0x100834dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x638>
1008348c0:      cmp x20, #0x3e9
1008348c4:      b.lo    0x100834984 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1fc>
1008348c8:      add x0, x21, #0x14
1008348cc:      mov x1, x20
1008348d0:      mov w2, #0x3e8              ; =1000
1008348d4:      bl  0x100826214 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1008348d8:      tbz w0, #0x0, 0x100834984 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1fc>
1008348dc:      add x0, x21, #0x14
1008348e0:      mov x1, x20
1008348e4:      mov w2, #0xa120             ; =41248
1008348e8:      movk    w2, #0x7, lsl #16
1008348ec:      bl  0x100826214 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1008348f0:      tbnz    w0, #0x0, 0x100834f38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7b0>
1008348f4:      stur    x20, [x29, #-0x68]
1008348f8:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1008348fc:      bfxil   x8, x21, #0, #48
100834900:      str x8, [sp, #0x90]
100834904:      adrp    x19, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
100834908:      add x19, x19, #0xc00
10083490c:      add x1, sp, #0x90
100834910:      mov x0, x19
100834914:      bl  0x100137b50 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json15parse_root_push0jEB2h_>
100834918:      mov x21, x0
10083491c:      stp x0, x22, [x29, #-0x60]
100834920:      str x20, [sp]
100834924:      sub x8, x29, #0x58
100834928:      mov x9, sp
10083492c:      stp x8, x9, [sp, #0x90]
100834930:      sub x8, x29, #0x60
100834934:      sub x9, x29, #0x68
100834938:      stp x8, x9, [sp, #0xa0]
10083493c:      adrp    x0, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
100834940:      add x0, x0, #0x3d0
100834944:      add x1, sp, #0x90
100834948:      bl  0x10012c970 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawIB1t_NtNtNtB1S_5value7jsvalue7JSValueENCINvB1Q_19with_built_tape_rawB3o_NCNvNtNtB1S_4json9parse_api24try_parse_deep_iterative0E0E0IB1t_B3o_EEB1S_>
10083494c:      mov x20, x0
100834950:      mov x23, x1
100834954:      str x21, [sp, #0x90]
100834958:      add x1, sp, #0x90
10083495c:      mov x0, x19
100834960:      bl  0x100137be8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json18parse_root_restore0uEB2h_>
100834964:      adrp    x0, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
100834968:      add x0, x0, #0xc10
10083496c:      bl  0x10013a404 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api24try_parse_deep_iteratives_0uEB2P_>
100834970:      tbnz    w20, #0x0, 0x100834dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x638>
100834974:      adrp    x0, 0x100e03000 <_anon.d13bca72c7c43155356dec6763133824.2879+0x6dd>
100834978:      add x0, x0, #0xd2e
10083497c:      mov w1, #0x29               ; =41
100834980:      bl  0x100835cb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
100834984:      adrp    x0, 0x10112c000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy29GC_SAFEPOINT_DEFER_ARENA_BASE0s_023___RUST_STD_INTERNAL_VAL>
100834988:      add x0, x0, #0x1b0
10083498c:      ldr x8, [x0]
100834990:      blr x8
100834994:      mov x19, x0
100834998:      ldrb    w8, [x0, #0x20]
10083499c:      cbnz    w8, 0x100834e04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x67c>
1008349a0:      ldr x8, [x19]
1008349a4:      cbnz    x8, 0x100834e80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6f8>
1008349a8:      mov x22, #0x7fff000000000000 ; =9223090561878065152
1008349ac:      bfxil   x22, x21, #0, #48
1008349b0:      mov x8, #-0x1               ; =-1
1008349b4:      str x8, [x19]
1008349b8:      mov x21, x19
1008349bc:      ldr x8, [x21, #0x8]!
1008349c0:      ldr x24, [x19, #0x18]
1008349c4:      cmp x24, x8
1008349c8:      b.ne    0x1008349d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x24c>
1008349cc:      mov x0, x21
1008349d0:      bl  0x100cbaee0 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1008349d4:      ldr x8, [x19, #0x10]
1008349d8:      str x22, [x8, x24, lsl #3]
1008349dc:      add x8, x24, #0x1
1008349e0:      str x8, [x19, #0x18]
1008349e4:      ldr x8, [x19]
1008349e8:      add x8, x8, #0x1
1008349ec:      str x8, [x19]
1008349f0:      adrp    x0, 0x10112c000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy29GC_SAFEPOINT_DEFER_ARENA_BASE0s_023___RUST_STD_INTERNAL_VAL>
1008349f4:      add x0, x0, #0xa8
1008349f8:      ldr x8, [x0]
1008349fc:      blr x8
100834a00:      ldrb    w9, [x0]
100834a04:      strb    wzr, [x0]
100834a08:      adrp    x22, 0x10112c000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy29GC_SAFEPOINT_DEFER_ARENA_BASE0s_023___RUST_STD_INTERNAL_VAL>
100834a0c:      add x22, x22, #0x270
100834a10:      cbz w9, 0x100834a4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2c4>
100834a14:      mov x8, x0
100834a18:      ldr x9, [x22]
100834a1c:      mov x0, x22
100834a20:      blr x9
100834a24:      ldrb    w9, [x0]
100834a28:      tst w9, #0x3
100834a2c:      b.ne    0x100834a44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2bc>
100834a30:      adrp    x9, 0x101201000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xf6e8>
100834a34:      add x9, x9, #0x918
100834a38:      ldapr   w9, [x9]
100834a3c:      cmp w9, #0x0
100834a40:      b.le    0x100834ba4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x41c>
100834a44:      mov w9, #0x1                ; =1
100834a48:      strb    w9, [x8]
100834a4c:      ldrb    w8, [x19, #0x20]
100834a50:      cbnz    w8, 0x100834bfc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x474>
100834a54:      ldr x8, [x19]
100834a58:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100834a5c:      cmp x8, x9
100834a60:      b.hs    0x100834ee8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x760>
100834a64:      add x9, x8, #0x1
100834a68:      str x9, [x19]
100834a6c:      ldr x9, [x19, #0x18]
100834a70:      cmp x24, x9
100834a74:      b.hs    0x100834a88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x300>
100834a78:      ldr x9, [x19, #0x10]
100834a7c:      ldr x9, [x9, x24, lsl #3]
100834a80:      and x23, x9, #0xffffffffffff
100834a84:      b   0x100834a8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x304>
100834a88:      mov w23, #0x1               ; =1
100834a8c:      str x8, [x19]
100834a90:      adrp    x8, 0x101125000 <__MergedGlobals+0xd8>
100834a94:      add x8, x8, #0x488
100834a98:      ldapr   x8, [x8]
100834a9c:      cbnz    x8, 0x100834de4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x65c>
100834aa0:      adrp    x8, 0x101125000 <__MergedGlobals+0xd8>
100834aa4:      ldrb    w8, [x8, #0x490]
100834aa8:      cmp w8, #0x2
100834aac:      b.eq    0x100834c34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4ac>
100834ab0:      cmp w8, #0x1
100834ab4:      b.ne    0x100834af8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x370>
100834ab8:      stp x24, x20, [x29, #-0x68]
100834abc:      ldrb    w8, [x19, #0x20]
100834ac0:      cbnz    w8, 0x100834ebc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x734>
100834ac4:      ldr x8, [x19]
100834ac8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100834acc:      cmp x8, x9
100834ad0:      b.hs    0x100834ee8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x760>
100834ad4:      add x9, x8, #0x1
100834ad8:      str x9, [x19]
100834adc:      ldr x9, [x19, #0x18]
100834ae0:      cmp x24, x9
100834ae4:      b.hs    0x100834b38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x3b0>
100834ae8:      ldr x9, [x19, #0x10]
100834aec:      ldr x9, [x9, x24, lsl #3]
100834af0:      and x9, x9, #0xffffffffffff
100834af4:      b   0x100834b3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x3b4>
100834af8:      add x8, x23, #0x14
100834afc:      sub x9, x20, #0x400
100834b00:      mov w10, #0xfffc00          ; =16776192
100834b04:      cmp x9, x10
100834b08:      b.hi    0x100834c34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4ac>
100834b0c:      mov x9, #0x2600             ; =9728
100834b10:      movk    x9, #0x1, lsl #32
100834b14:      mov x10, x20
100834b18:      ldrb    w11, [x8], #0x1
100834b1c:      cmp w11, #0x20
100834b20:      b.hi    0x100834c2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4a4>
100834b24:      lsr x12, x9, x11
100834b28:      tbz w12, #0x0, 0x100834c2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4a4>
100834b2c:      subs    x10, x10, #0x1
100834b30:      b.ne    0x100834b18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x390>
100834b34:      b   0x100834c34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4ac>
100834b38:      mov w9, #0x1                ; =1
100834b3c:      str x8, [x19]
100834b40:      add x8, x9, #0x14
100834b44:      stur    x8, [x29, #-0x58]
100834b48:      str x20, [sp]
100834b4c:      sub x8, x29, #0x58
100834b50:      mov x9, sp
100834b54:      stp x8, x9, [sp, #0x90]
100834b58:      sub x8, x29, #0x68
100834b5c:      sub x9, x29, #0x60
100834b60:      stp x8, x9, [sp, #0xa0]
100834b64:      adrp    x0, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
100834b68:      add x0, x0, #0x3d0
100834b6c:      add x1, sp, #0x90
100834b70:      bl  0x10012cd3c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawNtNtNtB1S_5value7jsvalue7JSValueNCNvNtNtB1S_4json9parse_api18try_parse_via_tape0E0IB1t_B3o_EEB1S_>
100834b74:      cmp x0, #0x1
100834b78:      b.ne    0x100834c34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4ac>
100834b7c:      mov x23, x1
100834b80:      ldrb    w8, [x19, #0x20]
100834b84:      cbnz    w8, 0x100834ef4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x76c>
100834b88:      ldr x8, [x19]
100834b8c:      cbnz    x8, 0x100834eb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x728>
100834b90:      ldr x8, [x19, #0x18]
100834b94:      cmp x24, x8
100834b98:      b.hi    0x100834dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x638>
100834b9c:      str x24, [x19, #0x18]
100834ba0:      b   0x100834dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x638>
100834ba4:      adrp    x0, 0x10112a000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc8tenuring17NURSERY_CAP_SCALE0s_023___RUST_STD_INTERNAL_VAL+0x10>
100834ba8:      add x0, x0, #0x608
100834bac:      ldr x8, [x0]
100834bb0:      blr x8
100834bb4:      ldr x8, [x0]
100834bb8:      adrp    x0, 0x10112b000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json8raw_json12RAW_JSON_KEY0s_023___RUST_STD_INTERNAL_VAL+0x8>
100834bbc:      add x0, x0, #0xf58
100834bc0:      ldr x9, [x0]
100834bc4:      blr x9
100834bc8:      ldr x9, [x0]
100834bcc:      cmp x9, x8
100834bd0:      b.ls    0x100834bf0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x468>
100834bd4:      str x8, [x0]
100834bd8:      adrp    x0, 0x10112c000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy29GC_SAFEPOINT_DEFER_ARENA_BASE0s_023___RUST_STD_INTERNAL_VAL>
100834bdc:      add x0, x0, #0x1e0
100834be0:      ldr x8, [x0]
100834be4:      blr x8
100834be8:      mov w8, #0x1                ; =1
100834bec:      strb    w8, [x0]
100834bf0:      bl  0x100813240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
100834bf4:      ldrb    w8, [x19, #0x20]
100834bf8:      cbz w8, 0x100834a54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2cc>
100834bfc:      cmp w8, #0x2
100834c00:      b.eq    0x100834efc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x774>
100834c04:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
100834c08:      add x1, x1, #0x36c
100834c0c:      mov x0, x19
100834c10:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100834c14:      strb    wzr, [x19, #0x20]
100834c18:      ldr x8, [x19]
100834c1c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100834c20:      cmp x8, x9
100834c24:      b.lo    0x100834a64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2dc>
100834c28:      b   0x100834ee8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x760>
100834c2c:      cmp w11, #0x5b
100834c30:      b.eq    0x100834ab8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x330>
100834c34:      bl  0x100813240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
100834c38:      bl  0x100812c38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
100834c3c:      ldrb    w8, [x19, #0x20]
100834c40:      cbnz    w8, 0x100834e2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6a4>
100834c44:      ldr x8, [x19]
100834c48:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100834c4c:      cmp x8, x9
100834c50:      b.hs    0x100834ee8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x760>
100834c54:      add x9, x8, #0x1
100834c58:      str x9, [x19]
100834c5c:      ldr x10, [x19, #0x18]
100834c60:      mov w9, #0x1                ; =1
100834c64:      cmp x24, x10
100834c68:      b.hs    0x100834c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4f4>
100834c6c:      ldr x10, [x19, #0x10]
100834c70:      ldr x10, [x10, x24, lsl #3]
100834c74:      and x10, x10, #0xffffffffffff
100834c78:      b   0x100834c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4f8>
100834c7c:      mov w10, #0x1               ; =1
100834c80:      str x8, [x19]
100834c84:      add x8, x10, #0x14
100834c88:      movi.2d v0, #0000000000000000
100834c8c:      stur    q0, [x25, #0x78]
100834c90:      stur    q0, [x25, #0x68]
100834c94:      stur    q0, [x25, #0x58]
100834c98:      stur    q0, [x25, #0x48]
100834c9c:      strb    w9, [sp, #0x120]
100834ca0:      mov x9, #-0x1               ; =-1
100834ca4:      stp x8, x20, [sp, #0xb8]
100834ca8:      str x9, [sp, #0x90]
100834cac:      stp xzr, xzr, [sp, #0xc8]
100834cb0:      str xzr, [sp, #0x118]
100834cb4:      add x0, sp, #0x90
100834cb8:      bl  0x1007db080 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
100834cbc:      mov x23, x0
100834cc0:      ldp x8, x9, [sp, #0xc0]
100834cc4:      cmp x9, x8
100834cc8:      b.hs    0x100834cfc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x574>
100834ccc:      ldr x10, [sp, #0xb8]
100834cd0:      mov x11, #0x2600            ; =9728
100834cd4:      movk    x11, #0x1, lsl #32
100834cd8:      ldrb    w12, [x10, x9]
100834cdc:      cmp w12, #0x20
100834ce0:      b.hi    0x100834cfc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x574>
100834ce4:      lsr x12, x11, x12
100834ce8:      tbz w12, #0x0, 0x100834cfc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x574>
100834cec:      add x9, x9, #0x1
100834cf0:      cmp x8, x9
100834cf4:      b.ne    0x100834cd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x550>
100834cf8:      mov x9, x8
100834cfc:      ldrb    w20, [sp, #0x120]
100834d00:      cmp x9, x8
100834d04:      cset    w25, eq
100834d08:      ldrb    w8, [x19, #0x20]
100834d0c:      cbnz    w8, 0x100834e5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6d4>
100834d10:      ldr x8, [x19]
100834d14:      cbnz    x8, 0x100834e80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6f8>
100834d18:      mov x8, #-0x1               ; =-1
100834d1c:      str x8, [x19]
100834d20:      ldr x26, [x19, #0x18]
100834d24:      ldr x8, [x19, #0x8]
100834d28:      cmp x26, x8
100834d2c:      b.ne    0x100834d38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x5b0>
100834d30:      mov x0, x21
100834d34:      bl  0x100cbaee0 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
100834d38:      ldr x8, [x19, #0x10]
100834d3c:      str x23, [x8, x26, lsl #3]
100834d40:      add x8, x26, #0x1
100834d44:      str x8, [x19, #0x18]
100834d48:      ldr x8, [x19]
100834d4c:      add x8, x8, #0x1
100834d50:      str x8, [x19]
100834d54:      ldr x8, [x22]
100834d58:      mov x0, x22
100834d5c:      blr x8
100834d60:      ldrb    w8, [x0]
100834d64:      and w8, w8, #0xfffffffd
100834d68:      strb    w8, [x0]
100834d6c:      bl  0x100813cd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy22gc_bump_malloc_trigger>
100834d70:      bl  0x100818820 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy49gc_schedule_parse_boundary_collection_if_pressure>
100834d74:      ldrb    w8, [x19, #0x20]
100834d78:      cbnz    w8, 0x100834e8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x704>
100834d7c:      ldr x8, [x19]
100834d80:      cbnz    x8, 0x100834eb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x728>
100834d84:      and w20, w25, w20
100834d88:      ldr x8, [x19, #0x18]
100834d8c:      cmp x24, x8
100834d90:      b.hi    0x100834d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x610>
100834d94:      str x24, [x19, #0x18]
100834d98:      adrp    x0, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
100834d9c:      add x0, x0, #0xc10
100834da0:      bl  0x10013a134 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api10parse_slow0uEB2P_>
100834da4:      tbz w20, #0x0, 0x100834f28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7a0>
100834da8:      ldr x8, [sp, #0x90]
100834dac:      cmn x8, #0x1
100834db0:      b.eq    0x100834dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x638>
100834db4:      cbz x8, 0x100834dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x638>
100834db8:      ldr x0, [sp, #0x98]
100834dbc:      bl  0x100cd8880 <_mi_free>
100834dc0:      mov x0, x23
100834dc4:      ldp x29, x30, [sp, #0x190]
100834dc8:      ldp x20, x19, [sp, #0x180]
100834dcc:      ldp x22, x21, [sp, #0x170]
100834dd0:      ldp x24, x23, [sp, #0x160]
100834dd4:      ldp x26, x25, [sp, #0x150]
100834dd8:      ldp x28, x27, [sp, #0x140]
100834ddc:      add sp, sp, #0x1a0
100834de0:      ret
100834de4:      adrp    x0, 0x101125000 <__MergedGlobals+0xd8>
100834de8:      add x0, x0, #0x488
100834dec:      bl  0x100cb6f10 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockNtNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api8TapeModeE10initializeNCINvB2_11get_or_initNCNvBV_18tape_mode_from_env0E0zEBZ_>
100834df0:      adrp    x8, 0x101125000 <__MergedGlobals+0xd8>
100834df4:      ldrb    w8, [x8, #0x490]
100834df8:      cmp w8, #0x2
100834dfc:      b.ne    0x100834ab0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x328>
100834e00:      b   0x100834c34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4ac>
100834e04:      cmp w8, #0x1
100834e08:      b.ne    0x100834efc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x774>
100834e0c:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
100834e10:      add x1, x1, #0x36c
100834e14:      mov x0, x19
100834e18:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100834e1c:      strb    wzr, [x19, #0x20]
100834e20:      ldr x8, [x19]
100834e24:      cbz x8, 0x1008349a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x220>
100834e28:      b   0x100834e80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6f8>
100834e2c:      cmp w8, #0x2
100834e30:      b.eq    0x100834efc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x774>
100834e34:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
100834e38:      add x1, x1, #0x36c
100834e3c:      mov x0, x19
100834e40:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100834e44:      strb    wzr, [x19, #0x20]
100834e48:      ldr x8, [x19]
100834e4c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100834e50:      cmp x8, x9
100834e54:      b.lo    0x100834c54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4cc>
100834e58:      b   0x100834ee8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x760>
100834e5c:      cmp w8, #0x2
100834e60:      b.eq    0x100834efc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x774>
100834e64:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
100834e68:      add x1, x1, #0x36c
100834e6c:      mov x0, x19
100834e70:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100834e74:      strb    wzr, [x19, #0x20]
100834e78:      ldr x8, [x19]
100834e7c:      cbz x8, 0x100834d18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x590>
100834e80:      adrp    x0, 0x101094000 <_anon.438b28c8644b10f28676d307896bf03a.21>
100834e84:      add x0, x0, #0xdf8
100834e88:      bl  0x100c8b22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100834e8c:      cmp w8, #0x2
100834e90:      b.eq    0x100834efc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x774>
100834e94:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
100834e98:      add x1, x1, #0x36c
100834e9c:      mov x0, x19
100834ea0:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100834ea4:      strb    wzr, [x19, #0x20]
100834ea8:      ldr x8, [x19]
100834eac:      cbz x8, 0x100834d84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x5fc>
100834eb0:      adrp    x0, 0x101094000 <_anon.438b28c8644b10f28676d307896bf03a.21>
100834eb4:      add x0, x0, #0xe58
100834eb8:      bl  0x100c8b22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100834ebc:      cmp w8, #0x2
100834ec0:      b.eq    0x100834efc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x774>
100834ec4:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
100834ec8:      add x1, x1, #0x36c
100834ecc:      mov x0, x19
100834ed0:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100834ed4:      strb    wzr, [x19, #0x20]
100834ed8:      ldr x8, [x19]
100834edc:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100834ee0:      cmp x8, x9
100834ee4:      b.lo    0x100834ad4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x34c>
100834ee8:      adrp    x0, 0x101094000 <_anon.438b28c8644b10f28676d307896bf03a.21>
100834eec:      add x0, x0, #0xdc8
100834ef0:      bl  0x100c8b25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100834ef4:      cmp w8, #0x2
100834ef8:      b.ne    0x100834f08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x780>
100834efc:      adrp    x0, 0x101093000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
100834f00:      add x0, x0, #0xed8
100834f04:      bl  0x100cd1edc <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
100834f08:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
100834f0c:      add x1, x1, #0x36c
100834f10:      mov x0, x19
100834f14:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100834f18:      strb    wzr, [x19, #0x20]
100834f1c:      ldr x8, [x19]
100834f20:      cbz x8, 0x100834b90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x408>
100834f24:      b   0x100834eb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x728>
100834f28:      adrp    x0, 0x100e0b000 <_anon.c2a4b59a01bfaad05414bd5c213a645e.880+0x102>
100834f2c:      add x0, x0, #0x557
100834f30:      mov w1, #0x21               ; =33
100834f34:      bl  0x100835cb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
100834f38:      add x0, sp, #0x90
100834f3c:      bl  0x100835d58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api24iterative_budget_message>
100834f40:      ldp x0, x1, [sp, #0x98]
100834f44:      bl  0x10083561c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17throw_range_error>
