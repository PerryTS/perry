/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/short-array-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008c9940 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow>:
1008c9940:      sub sp, sp, #0x1a0
1008c9944:      stp x28, x27, [sp, #0x140]
1008c9948:      stp x26, x25, [sp, #0x150]
1008c994c:      stp x24, x23, [sp, #0x160]
1008c9950:      stp x22, x21, [sp, #0x170]
1008c9954:      stp x20, x19, [sp, #0x180]
1008c9958:      stp x29, x30, [sp, #0x190]
1008c995c:      add x29, sp, #0x190
1008c9960:      mov x20, x1
1008c9964:      mov x21, x0
1008c9968:      add x25, sp, #0x90
1008c996c:      add x22, x0, #0x14
1008c9970:      cmp x1, #0x2
1008c9974:      b.ne    0x1008c998c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4c>
1008c9978:      ldrh    w8, [x22]
1008c997c:      mov w9, #0x7d7b             ; =32123
1008c9980:      cmp w8, w9
1008c9984:      b.eq    0x1008c99c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x80>
1008c9988:      b   0x1008c9a14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xd4>
1008c998c:      cmp x20, #0x3
1008c9990:      b.lo    0x1008c9a14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xd4>
1008c9994:      ldrb    w8, [x22]
1008c9998:      cmp w8, #0x20
1008c999c:      b.hi    0x1008c99e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xa0>
1008c99a0:      mov x9, #0x2600             ; =9728
1008c99a4:      movk    x9, #0x1, lsl #32
1008c99a8:      lsr x9, x9, x8
1008c99ac:      tbz w9, #0x0, 0x1008c99e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xa0>
1008c99b0:      add x0, x21, #0x14
1008c99b4:      mov x1, x20
1008c99b8:      bl  0x1008c102c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty19padded_empty_object>
1008c99bc:      tbz w0, #0x0, 0x1008c9a0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xcc>
1008c99c0:      ldp x29, x30, [sp, #0x190]
1008c99c4:      ldp x20, x19, [sp, #0x180]
1008c99c8:      ldp x22, x21, [sp, #0x170]
1008c99cc:      ldp x24, x23, [sp, #0x160]
1008c99d0:      ldp x26, x25, [sp, #0x150]
1008c99d4:      ldp x28, x27, [sp, #0x140]
1008c99d8:      add sp, sp, #0x1a0
1008c99dc:      b   0x1008c10f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty21allocate_empty_object>
1008c99e0:      cmp w8, #0x7b
1008c99e4:      b.ne    0x1008c9a0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xcc>
1008c99e8:      ldrb    w8, [x21, #0x15]
1008c99ec:      cmp w8, #0x20
1008c99f0:      b.hi    0x1008c9a04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xc4>
1008c99f4:      mov x9, #0x2600             ; =9728
1008c99f8:      movk    x9, #0x1, lsl #32
1008c99fc:      lsr x9, x9, x8
1008c9a00:      tbnz    w9, #0x0, 0x1008c99b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x70>
1008c9a04:      cmp w8, #0x7d
1008c9a08:      b.eq    0x1008c99b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x70>
1008c9a0c:      cmp x20, #0x41
1008c9a10:      b.hs    0x1008c9a78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x138>
1008c9a14:      add x0, sp, #0x90
1008c9a18:      add x1, x21, #0x14
1008c9a1c:      mov x2, x20
1008c9a20:      bl  0x1008c1658 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode>
1008c9a24:      ldr x8, [sp, #0x90]
1008c9a28:      cbz x8, 0x1008c9b3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1fc>
1008c9a2c:      ldr x8, [sp, #0x118]
1008c9a30:      str x8, [sp, #0x80]
1008c9a34:      ldur    q0, [x25, #0x48]
1008c9a38:      ldur    q1, [x25, #0x58]
1008c9a3c:      stp q0, q1, [sp, #0x40]
1008c9a40:      ldur    q0, [x25, #0x68]
1008c9a44:      ldur    q1, [x25, #0x78]
1008c9a48:      stp q0, q1, [sp, #0x60]
1008c9a4c:      ldur    q0, [x25, #0x8]
1008c9a50:      ldur    q1, [x25, #0x18]
1008c9a54:      stp q0, q1, [sp]
1008c9a58:      ldur    q0, [x25, #0x28]
1008c9a5c:      ldur    q1, [x25, #0x38]
1008c9a60:      stp q0, q1, [sp, #0x20]
1008c9a64:      mov x0, sp
1008c9a68:      bl  0x1008c20b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate>
1008c9a6c:      tbz w0, #0x0, 0x1008c9b3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1fc>
1008c9a70:      mov x23, x1
1008c9a74:      b   0x1008c9f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x650>
1008c9a78:      cmp x20, #0x3e9
1008c9a7c:      b.lo    0x1008c9b3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1fc>
1008c9a80:      add x0, x21, #0x14
1008c9a84:      mov x1, x20
1008c9a88:      mov w2, #0x3e8              ; =1000
1008c9a8c:      bl  0x1008c2c5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1008c9a90:      tbz w0, #0x0, 0x1008c9b3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1fc>
1008c9a94:      add x0, x21, #0x14
1008c9a98:      mov x1, x20
1008c9a9c:      mov w2, #0xa120             ; =41248
1008c9aa0:      movk    w2, #0x7, lsl #16
1008c9aa4:      bl  0x1008c2c5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1008c9aa8:      tbnz    w0, #0x0, 0x1008ca148 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x808>
1008c9aac:      stur    x20, [x29, #-0x68]
1008c9ab0:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1008c9ab4:      bfxil   x8, x21, #0, #48
1008c9ab8:      str x8, [sp, #0x90]
1008c9abc:      adrp    x19, 0x1010ce000 <_anon.b8734461ce4fd0c908478712a5ac704e.197+0x20>
1008c9ac0:      add x19, x19, #0xb40
1008c9ac4:      add x1, sp, #0x90
1008c9ac8:      mov x0, x19
1008c9acc:      bl  0x100137410 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json15parse_root_push0jEB2h_>
1008c9ad0:      mov x21, x0
1008c9ad4:      stp x0, x22, [x29, #-0x60]
1008c9ad8:      str x20, [sp]
1008c9adc:      sub x8, x29, #0x58
1008c9ae0:      mov x9, sp
1008c9ae4:      stp x8, x9, [sp, #0x90]
1008c9ae8:      sub x8, x29, #0x60
1008c9aec:      sub x9, x29, #0x68
1008c9af0:      stp x8, x9, [sp, #0xa0]
1008c9af4:      adrp    x0, 0x1010cd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5types18GC_TYPE_INFO_BY_ID+0x4c8>
1008c9af8:      add x0, x0, #0x400
1008c9afc:      add x1, sp, #0x90
1008c9b00:      bl  0x10012c430 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawIB1t_NtNtNtB1S_5value7jsvalue7JSValueENCINvB1Q_19with_built_tape_rawB3o_NCNvNtNtB1S_4json9parse_api24try_parse_deep_iterative0E0E0IB1t_B3o_EEB1S_>
1008c9b04:      mov x20, x0
1008c9b08:      mov x23, x1
1008c9b0c:      str x21, [sp, #0x90]
1008c9b10:      add x1, sp, #0x90
1008c9b14:      mov x0, x19
1008c9b18:      bl  0x1001374a8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json18parse_root_restore0uEB2h_>
1008c9b1c:      adrp    x0, 0x1010ce000 <_anon.b8734461ce4fd0c908478712a5ac704e.197+0x20>
1008c9b20:      add x0, x0, #0xb48
1008c9b24:      bl  0x100139cc4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api24try_parse_deep_iteratives_0uEB2P_>
1008c9b28:      tbnz    w20, #0x0, 0x1008c9f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x650>
1008c9b2c:      adrp    x0, 0x100e17000 <_anon.78a33a9fe279ced61d81da3c9b3c7fad.1076+0xdb>
1008c9b30:      add x0, x0, #0xd82
1008c9b34:      mov w1, #0x29               ; =41
1008c9b38:      bl  0x1008cae88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
1008c9b3c:      adrp    x0, 0x101134000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json17PARSE_SHAPE_CACHE0023___RUST_STD_INTERNAL_VAL>
1008c9b40:      add x0, x0, #0x660
1008c9b44:      ldr x8, [x0]
1008c9b48:      blr x8
1008c9b4c:      mov x19, x0
1008c9b50:      ldrb    w8, [x0, #0x20]
1008c9b54:      cbnz    w8, 0x1008c9fec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6ac>
1008c9b58:      ldr x8, [x19]
1008c9b5c:      cbnz    x8, 0x1008ca068 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x728>
1008c9b60:      mov x23, #0x7fff000000000000 ; =9223090561878065152
1008c9b64:      bfxil   x23, x21, #0, #48
1008c9b68:      mov x8, #-0x1               ; =-1
1008c9b6c:      str x8, [x19]
1008c9b70:      mov x22, x19
1008c9b74:      ldr x8, [x22, #0x8]!
1008c9b78:      ldr x24, [x19, #0x18]
1008c9b7c:      cmp x24, x8
1008c9b80:      b.ne    0x1008c9b8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x24c>
1008c9b84:      mov x0, x22
1008c9b88:      bl  0x100ccc608 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1008c9b8c:      ldr x8, [x19, #0x10]
1008c9b90:      str x23, [x8, x24, lsl #3]
1008c9b94:      add x8, x24, #0x1
1008c9b98:      str x8, [x19, #0x18]
1008c9b9c:      ldr x8, [x19]
1008c9ba0:      add x8, x8, #0x1
1008c9ba4:      str x8, [x19]
1008c9ba8:      mov x0, #0x0                ; =0
1008c9bac:      bl  0x10039e7b0 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy43GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1J_6option6OptionQIB2o_INtNtB1J_4cell4CellbEEEEE9call_onceBc_>
1008c9bb0:      mov x21, x0
1008c9bb4:      ldrb    w8, [x0]
1008c9bb8:      strb    wzr, [x0]
1008c9bbc:      cbz w8, 0x1008c9bf0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2b0>
1008c9bc0:      mov x0, #0x0                ; =0
1008c9bc4:      bl  0x10039e7d0 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1008c9bc8:      ldrb    w8, [x0]
1008c9bcc:      tst w8, #0x3
1008c9bd0:      b.ne    0x1008c9be8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2a8>
1008c9bd4:      adrp    x8, 0x101178000 <_out_buf+0x3f08>
1008c9bd8:      add x8, x8, #0xb38
1008c9bdc:      ldapr   w8, [x8]
1008c9be0:      cmp w8, #0x0
1008c9be4:      b.le    0x1008c9d48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x408>
1008c9be8:      mov w8, #0x1                ; =1
1008c9bec:      strb    w8, [x21]
1008c9bf0:      ldrb    w8, [x19, #0x20]
1008c9bf4:      cbnz    w8, 0x1008c9d90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x450>
1008c9bf8:      ldr x8, [x19]
1008c9bfc:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008c9c00:      cmp x8, x9
1008c9c04:      b.hs    0x1008ca0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7b8>
1008c9c08:      add x9, x8, #0x1
1008c9c0c:      str x9, [x19]
1008c9c10:      ldr x9, [x19, #0x18]
1008c9c14:      cmp x24, x9
1008c9c18:      b.hs    0x1008c9c2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2ec>
1008c9c1c:      ldr x9, [x19, #0x10]
1008c9c20:      ldr x9, [x9, x24, lsl #3]
1008c9c24:      and x23, x9, #0xffffffffffff
1008c9c28:      b   0x1008c9c30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2f0>
1008c9c2c:      mov w23, #0x1               ; =1
1008c9c30:      str x8, [x19]
1008c9c34:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
1008c9c38:      add x8, x8, #0xaa0
1008c9c3c:      ldapr   x8, [x8]
1008c9c40:      cbnz    x8, 0x1008c9fb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x674>
1008c9c44:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
1008c9c48:      ldrb    w8, [x8, #0xaa8]
1008c9c4c:      cmp w8, #0x2
1008c9c50:      b.eq    0x1008c9dc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x488>
1008c9c54:      cmp w8, #0x1
1008c9c58:      b.ne    0x1008c9c9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x35c>
1008c9c5c:      stp x24, x20, [x29, #-0x68]
1008c9c60:      ldrb    w8, [x19, #0x20]
1008c9c64:      cbnz    w8, 0x1008ca0cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x78c>
1008c9c68:      ldr x8, [x19]
1008c9c6c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008c9c70:      cmp x8, x9
1008c9c74:      b.hs    0x1008ca0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7b8>
1008c9c78:      add x9, x8, #0x1
1008c9c7c:      str x9, [x19]
1008c9c80:      ldr x9, [x19, #0x18]
1008c9c84:      cmp x24, x9
1008c9c88:      b.hs    0x1008c9cdc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x39c>
1008c9c8c:      ldr x9, [x19, #0x10]
1008c9c90:      ldr x9, [x9, x24, lsl #3]
1008c9c94:      and x9, x9, #0xffffffffffff
1008c9c98:      b   0x1008c9ce0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x3a0>
1008c9c9c:      add x8, x23, #0x14
1008c9ca0:      sub x9, x20, #0x400
1008c9ca4:      mov w10, #0xfffc00          ; =16776192
1008c9ca8:      cmp x9, x10
1008c9cac:      b.hi    0x1008c9dc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x488>
1008c9cb0:      mov x9, #0x2600             ; =9728
1008c9cb4:      movk    x9, #0x1, lsl #32
1008c9cb8:      mov x10, x20
1008c9cbc:      ldrb    w11, [x8], #0x1
1008c9cc0:      cmp w11, #0x20
1008c9cc4:      b.hi    0x1008c9dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
1008c9cc8:      lsr x12, x9, x11
1008c9ccc:      tbz w12, #0x0, 0x1008c9dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
1008c9cd0:      subs    x10, x10, #0x1
1008c9cd4:      b.ne    0x1008c9cbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x37c>
1008c9cd8:      b   0x1008c9dc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x488>
1008c9cdc:      mov w9, #0x1                ; =1
1008c9ce0:      str x8, [x19]
1008c9ce4:      add x8, x9, #0x14
1008c9ce8:      stur    x8, [x29, #-0x58]
1008c9cec:      str x20, [sp]
1008c9cf0:      sub x8, x29, #0x58
1008c9cf4:      mov x9, sp
1008c9cf8:      stp x8, x9, [sp, #0x90]
1008c9cfc:      sub x8, x29, #0x68
1008c9d00:      sub x9, x29, #0x60
1008c9d04:      stp x8, x9, [sp, #0xa0]
1008c9d08:      adrp    x0, 0x1010cd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5types18GC_TYPE_INFO_BY_ID+0x4c8>
1008c9d0c:      add x0, x0, #0x400
1008c9d10:      add x1, sp, #0x90
1008c9d14:      bl  0x10012c884 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawNtNtNtB1S_5value7jsvalue7JSValueNCNvNtNtB1S_4json9parse_api18try_parse_via_tape0E0IB1t_B3o_EEB1S_>
1008c9d18:      cmp x0, #0x1
1008c9d1c:      b.ne    0x1008c9dc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x488>
1008c9d20:      mov x23, x1
1008c9d24:      ldrb    w8, [x19, #0x20]
1008c9d28:      cbnz    w8, 0x1008ca104 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7c4>
1008c9d2c:      ldr x8, [x19]
1008c9d30:      cbnz    x8, 0x1008ca098 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x758>
1008c9d34:      ldr x8, [x19, #0x18]
1008c9d38:      cmp x24, x8
1008c9d3c:      b.hi    0x1008c9f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x650>
1008c9d40:      str x24, [x19, #0x18]
1008c9d44:      b   0x1008c9f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x650>
1008c9d48:      mov x0, #0x0                ; =0
1008c9d4c:      bl  0x10039e968 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5block17ARENA_TOTAL_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1l_6option6OptionQIB20_INtNtB1l_4cell4CelljEEEEE9call_onceBc_>
1008c9d50:      ldr x23, [x0]
1008c9d54:      mov x0, #0x0                ; =0
1008c9d58:      bl  0x10039e630 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy21GC_NEXT_TRIGGER_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1n_6option6OptionQIB22_INtNtB1n_4cell4CelljEEEEE9call_onceBc_>
1008c9d5c:      ldr x8, [x0]
1008c9d60:      cmp x8, x23
1008c9d64:      b.ls    0x1008c9d84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x444>
1008c9d68:      str x23, [x0]
1008c9d6c:      adrp    x0, 0x101132000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime8dyn_eval3env13ENV_KEY_CACHE0023___RUST_STD_INTERNAL_VAL+0x10>
1008c9d70:      add x0, x0, #0x9c8
1008c9d74:      ldr x8, [x0]
1008c9d78:      blr x8
1008c9d7c:      mov w8, #0x1                ; =1
1008c9d80:      strb    w8, [x0]
1008c9d84:      bl  0x100347d00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1008c9d88:      ldrb    w8, [x19, #0x20]
1008c9d8c:      cbz w8, 0x1008c9bf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2b8>
1008c9d90:      cmp w8, #0x2
1008c9d94:      b.eq    0x1008ca10c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7cc>
1008c9d98:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
1008c9d9c:      add x1, x1, #0xd0
1008c9da0:      mov x0, x19
1008c9da4:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008c9da8:      strb    wzr, [x19, #0x20]
1008c9dac:      ldr x8, [x19]
1008c9db0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008c9db4:      cmp x8, x9
1008c9db8:      b.lo    0x1008c9c08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2c8>
1008c9dbc:      b   0x1008ca0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7b8>
1008c9dc0:      cmp w11, #0x5b
1008c9dc4:      b.eq    0x1008c9c5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x31c>
1008c9dc8:      bl  0x100347d00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1008c9dcc:      bl  0x100347bec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
1008c9dd0:      ldrb    w8, [x19, #0x20]
1008c9dd4:      cbnz    w8, 0x1008ca014 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6d4>
1008c9dd8:      ldr x8, [x19]
1008c9ddc:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008c9de0:      cmp x8, x9
1008c9de4:      b.hs    0x1008ca0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7b8>
1008c9de8:      add x9, x8, #0x1
1008c9dec:      str x9, [x19]
1008c9df0:      ldr x10, [x19, #0x18]
1008c9df4:      mov w9, #0x1                ; =1
1008c9df8:      cmp x24, x10
1008c9dfc:      b.hs    0x1008c9e10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4d0>
1008c9e00:      ldr x10, [x19, #0x10]
1008c9e04:      ldr x10, [x10, x24, lsl #3]
1008c9e08:      and x10, x10, #0xffffffffffff
1008c9e0c:      b   0x1008c9e14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4d4>
1008c9e10:      mov w10, #0x1               ; =1
1008c9e14:      str x8, [x19]
1008c9e18:      add x8, x10, #0x14
1008c9e1c:      movi.2d v0, #0000000000000000
1008c9e20:      stur    q0, [x25, #0x78]
1008c9e24:      stur    q0, [x25, #0x68]
1008c9e28:      stur    q0, [x25, #0x58]
1008c9e2c:      stur    q0, [x25, #0x48]
1008c9e30:      strb    w9, [sp, #0x120]
1008c9e34:      mov x9, #-0x1               ; =-1
1008c9e38:      stp x8, x20, [sp, #0xb8]
1008c9e3c:      str x9, [sp, #0x90]
1008c9e40:      stp xzr, xzr, [sp, #0xc8]
1008c9e44:      str xzr, [sp, #0x118]
1008c9e48:      add x0, sp, #0x90
1008c9e4c:      bl  0x10089987c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1008c9e50:      mov x23, x0
1008c9e54:      ldp x8, x9, [sp, #0xc0]
1008c9e58:      cmp x9, x8
1008c9e5c:      b.hs    0x1008c9e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x550>
1008c9e60:      ldr x10, [sp, #0xb8]
1008c9e64:      mov x11, #0x2600            ; =9728
1008c9e68:      movk    x11, #0x1, lsl #32
1008c9e6c:      ldrb    w12, [x10, x9]
1008c9e70:      cmp w12, #0x20
1008c9e74:      b.hi    0x1008c9e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x550>
1008c9e78:      lsr x12, x11, x12
1008c9e7c:      tbz w12, #0x0, 0x1008c9e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x550>
1008c9e80:      add x9, x9, #0x1
1008c9e84:      cmp x8, x9
1008c9e88:      b.ne    0x1008c9e6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x52c>
1008c9e8c:      mov x9, x8
1008c9e90:      ldrb    w20, [sp, #0x120]
1008c9e94:      cmp x9, x8
1008c9e98:      cset    w25, eq
1008c9e9c:      ldrb    w8, [x19, #0x20]
1008c9ea0:      cbnz    w8, 0x1008ca044 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x704>
1008c9ea4:      ldr x8, [x19]
1008c9ea8:      cbnz    x8, 0x1008ca068 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x728>
1008c9eac:      mov x8, #-0x1               ; =-1
1008c9eb0:      str x8, [x19]
1008c9eb4:      ldr x26, [x19, #0x18]
1008c9eb8:      ldr x8, [x19, #0x8]
1008c9ebc:      cmp x26, x8
1008c9ec0:      b.ne    0x1008c9ecc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x58c>
1008c9ec4:      mov x0, x22
1008c9ec8:      bl  0x100ccc608 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1008c9ecc:      ldr x8, [x19, #0x10]
1008c9ed0:      str x23, [x8, x26, lsl #3]
1008c9ed4:      add x8, x26, #0x1
1008c9ed8:      str x8, [x19, #0x18]
1008c9edc:      ldr x8, [x19]
1008c9ee0:      add x8, x8, #0x1
1008c9ee4:      str x8, [x19]
1008c9ee8:      mov x0, #0x0                ; =0
1008c9eec:      bl  0x10039e7d0 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1008c9ef0:      ldrb    w8, [x0]
1008c9ef4:      and w8, w8, #0xfffffffd
1008c9ef8:      strb    w8, [x0]
1008c9efc:      bl  0x100348798 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy22gc_bump_malloc_trigger>
1008c9f00:      adrp    x22, 0x10112c000 <_perry_global_baseline_worker_ts__1>
1008c9f04:      add x22, x22, #0x5c0
1008c9f08:      ldapr   x8, [x22]
1008c9f0c:      cbnz    x8, 0x1008c9fd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x694>
1008c9f10:      ldrb    w8, [x22, #0x8]
1008c9f14:      cbz w8, 0x1008c9f44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x604>
1008c9f18:      bl  0x100195d64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena4walk18arena_in_use_bytes>
1008c9f1c:      adrp    x8, 0x10112c000 <_perry_global_baseline_worker_ts__1>
1008c9f20:      add x8, x8, #0x658
1008c9f24:      ldapr   x8, [x8]
1008c9f28:      cbnz    x8, 0x1008ca0a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x764>
1008c9f2c:      adrp    x8, 0x10112c000 <_perry_global_baseline_worker_ts__1>
1008c9f30:      ldr x8, [x8, #0x660]
1008c9f34:      cmp x0, x8
1008c9f38:      b.lo    0x1008c9f44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x604>
1008c9f3c:      mov w8, #0x1                ; =1
1008c9f40:      strb    w8, [x21]
1008c9f44:      ldrb    w8, [x19, #0x20]
1008c9f48:      cbnz    w8, 0x1008ca074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x734>
1008c9f4c:      ldr x8, [x19]
1008c9f50:      cbnz    x8, 0x1008ca098 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x758>
1008c9f54:      and w20, w25, w20
1008c9f58:      ldr x8, [x19, #0x18]
1008c9f5c:      cmp x24, x8
1008c9f60:      b.hi    0x1008c9f68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x628>
1008c9f64:      str x24, [x19, #0x18]
1008c9f68:      adrp    x0, 0x1010ce000 <_anon.b8734461ce4fd0c908478712a5ac704e.197+0x20>
1008c9f6c:      add x0, x0, #0xb48
1008c9f70:      bl  0x1001399f4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api10parse_slow0uEB2P_>
1008c9f74:      tbz w20, #0x0, 0x1008ca138 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7f8>
1008c9f78:      ldr x8, [sp, #0x90]
1008c9f7c:      cmn x8, #0x1
1008c9f80:      b.eq    0x1008c9f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x650>
1008c9f84:      cbz x8, 0x1008c9f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x650>
1008c9f88:      ldr x0, [sp, #0x98]
1008c9f8c:      bl  0x100ce20c0 <_mi_free>
1008c9f90:      mov x0, x23
1008c9f94:      ldp x29, x30, [sp, #0x190]
1008c9f98:      ldp x20, x19, [sp, #0x180]
1008c9f9c:      ldp x22, x21, [sp, #0x170]
1008c9fa0:      ldp x24, x23, [sp, #0x160]
1008c9fa4:      ldp x26, x25, [sp, #0x150]
1008c9fa8:      ldp x28, x27, [sp, #0x140]
1008c9fac:      add sp, sp, #0x1a0
1008c9fb0:      ret
1008c9fb4:      adrp    x0, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
1008c9fb8:      add x0, x0, #0xaa0
1008c9fbc:      bl  0x100cc18c4 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockNtNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api8TapeModeE10initializeNCINvB2_11get_or_initNCNvBV_18tape_mode_from_env0E0zEBZ_>
1008c9fc0:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
1008c9fc4:      ldrb    w8, [x8, #0xaa8]
1008c9fc8:      cmp w8, #0x2
1008c9fcc:      b.ne    0x1008c9c54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x314>
1008c9fd0:      b   0x1008c9dc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x488>
1008c9fd4:      adrp    x0, 0x10112c000 <_perry_global_baseline_worker_ts__1>
1008c9fd8:      add x0, x0, #0x5c0
1008c9fdc:      bl  0x100cc1a54 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtCs5gMwpk3Cs4e_13perry_runtime2gc14gen_gc_enabled0E0zEB1y_>
1008c9fe0:      ldrb    w8, [x22, #0x8]
1008c9fe4:      cbnz    w8, 0x1008c9f18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x5d8>
1008c9fe8:      b   0x1008c9f44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x604>
1008c9fec:      cmp w8, #0x1
1008c9ff0:      b.ne    0x1008ca10c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7cc>
1008c9ff4:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
1008c9ff8:      add x1, x1, #0xd0
1008c9ffc:      mov x0, x19
1008ca000:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008ca004:      strb    wzr, [x19, #0x20]
1008ca008:      ldr x8, [x19]
1008ca00c:      cbz x8, 0x1008c9b60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x220>
1008ca010:      b   0x1008ca068 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x728>
1008ca014:      cmp w8, #0x2
1008ca018:      b.eq    0x1008ca10c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7cc>
1008ca01c:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
1008ca020:      add x1, x1, #0xd0
1008ca024:      mov x0, x19
1008ca028:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008ca02c:      strb    wzr, [x19, #0x20]
1008ca030:      ldr x8, [x19]
1008ca034:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008ca038:      cmp x8, x9
1008ca03c:      b.lo    0x1008c9de8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4a8>
1008ca040:      b   0x1008ca0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7b8>
1008ca044:      cmp w8, #0x2
1008ca048:      b.eq    0x1008ca10c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7cc>
1008ca04c:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
1008ca050:      add x1, x1, #0xd0
1008ca054:      mov x0, x19
1008ca058:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008ca05c:      strb    wzr, [x19, #0x20]
1008ca060:      ldr x8, [x19]
1008ca064:      cbz x8, 0x1008c9eac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x56c>
1008ca068:      adrp    x0, 0x10109c000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1008ca06c:      add x0, x0, #0xdf8
1008ca070:      bl  0x100c99aac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008ca074:      cmp w8, #0x2
1008ca078:      b.eq    0x1008ca10c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7cc>
1008ca07c:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
1008ca080:      add x1, x1, #0xd0
1008ca084:      mov x0, x19
1008ca088:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008ca08c:      strb    wzr, [x19, #0x20]
1008ca090:      ldr x8, [x19]
1008ca094:      cbz x8, 0x1008c9f54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x614>
1008ca098:      adrp    x0, 0x10109c000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1008ca09c:      add x0, x0, #0xe58
1008ca0a0:      bl  0x100c99aac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008ca0a4:      adrp    x8, 0x10112c000 <_perry_global_baseline_worker_ts__1>
1008ca0a8:      add x8, x8, #0x658
1008ca0ac:      mov x22, x0
1008ca0b0:      mov x0, x8
1008ca0b4:      bl  0x100cc2384 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockjE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc11heap_budget38gc_tiny_parse_in_use_trigger_dyn_bytes0E0zEB1A_>
1008ca0b8:      adrp    x8, 0x10112c000 <_perry_global_baseline_worker_ts__1>
1008ca0bc:      ldr x8, [x8, #0x660]
1008ca0c0:      cmp x22, x8
1008ca0c4:      b.hs    0x1008c9f3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x5fc>
1008ca0c8:      b   0x1008c9f44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x604>
1008ca0cc:      cmp w8, #0x2
1008ca0d0:      b.eq    0x1008ca10c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7cc>
1008ca0d4:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
1008ca0d8:      add x1, x1, #0xd0
1008ca0dc:      mov x0, x19
1008ca0e0:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008ca0e4:      strb    wzr, [x19, #0x20]
1008ca0e8:      ldr x8, [x19]
1008ca0ec:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008ca0f0:      cmp x8, x9
1008ca0f4:      b.lo    0x1008c9c78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x338>
1008ca0f8:      adrp    x0, 0x10109c000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1008ca0fc:      add x0, x0, #0xdc8
1008ca100:      bl  0x100c99adc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008ca104:      cmp w8, #0x2
1008ca108:      b.ne    0x1008ca118 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7d8>
1008ca10c:      adrp    x0, 0x10109b000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1008ca110:      add x0, x0, #0xed8
1008ca114:      bl  0x100cdb71c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1008ca118:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
1008ca11c:      add x1, x1, #0xd0
1008ca120:      mov x0, x19
1008ca124:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008ca128:      strb    wzr, [x19, #0x20]
1008ca12c:      ldr x8, [x19]
1008ca130:      cbz x8, 0x1008c9d34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x3f4>
1008ca134:      b   0x1008ca098 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x758>
1008ca138:      adrp    x0, 0x100e18000 <_anon.b8734461ce4fd0c908478712a5ac704e.318+0x138>
1008ca13c:      add x0, x0, #0x639
1008ca140:      mov w1, #0x21               ; =33
1008ca144:      bl  0x1008cae88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
1008ca148:      add x0, sp, #0x90
1008ca14c:      bl  0x1008caf30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api24iterative_budget_message>
1008ca150:      ldp x0, x1, [sp, #0x98]
1008ca154:      bl  0x1008ca884 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17throw_range_error>
