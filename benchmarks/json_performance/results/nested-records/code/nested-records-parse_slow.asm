/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/nested-records-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008c0c84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow>:
1008c0c84:      sub sp, sp, #0x190
1008c0c88:      stp x26, x25, [sp, #0x140]
1008c0c8c:      stp x24, x23, [sp, #0x150]
1008c0c90:      stp x22, x21, [sp, #0x160]
1008c0c94:      stp x20, x19, [sp, #0x170]
1008c0c98:      stp x29, x30, [sp, #0x180]
1008c0c9c:      add x29, sp, #0x180
1008c0ca0:      mov x20, x1
1008c0ca4:      mov x21, x0
1008c0ca8:      add x24, sp, #0x90
1008c0cac:      add x22, x0, #0x14
1008c0cb0:      cmp x1, #0x2
1008c0cb4:      b.ne    0x1008c0ccc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x48>
1008c0cb8:      ldrh    w8, [x22]
1008c0cbc:      mov w9, #0x7d7b             ; =32123
1008c0cc0:      cmp w8, w9
1008c0cc4:      b.eq    0x1008c0d00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7c>
1008c0cc8:      b   0x1008c0d50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xcc>
1008c0ccc:      cmp x20, #0x3
1008c0cd0:      b.lo    0x1008c0d50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xcc>
1008c0cd4:      ldrb    w8, [x22]
1008c0cd8:      cmp w8, #0x20
1008c0cdc:      b.hi    0x1008c0d1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x98>
1008c0ce0:      mov x9, #0x2600             ; =9728
1008c0ce4:      movk    x9, #0x1, lsl #32
1008c0ce8:      lsr x9, x9, x8
1008c0cec:      tbz w9, #0x0, 0x1008c0d1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x98>
1008c0cf0:      add x0, x21, #0x14
1008c0cf4:      mov x1, x20
1008c0cf8:      bl  0x1008b8458 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty19padded_empty_object>
1008c0cfc:      tbz w0, #0x0, 0x1008c0d48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xc4>
1008c0d00:      ldp x29, x30, [sp, #0x180]
1008c0d04:      ldp x20, x19, [sp, #0x170]
1008c0d08:      ldp x22, x21, [sp, #0x160]
1008c0d0c:      ldp x24, x23, [sp, #0x150]
1008c0d10:      ldp x26, x25, [sp, #0x140]
1008c0d14:      add sp, sp, #0x190
1008c0d18:      b   0x1008b8524 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty21allocate_empty_object>
1008c0d1c:      cmp w8, #0x7b
1008c0d20:      b.ne    0x1008c0d48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xc4>
1008c0d24:      ldrb    w8, [x21, #0x15]
1008c0d28:      cmp w8, #0x20
1008c0d2c:      b.hi    0x1008c0d40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xbc>
1008c0d30:      mov x9, #0x2600             ; =9728
1008c0d34:      movk    x9, #0x1, lsl #32
1008c0d38:      lsr x9, x9, x8
1008c0d3c:      tbnz    w9, #0x0, 0x1008c0cf0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c>
1008c0d40:      cmp w8, #0x7d
1008c0d44:      b.eq    0x1008c0cf0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c>
1008c0d48:      cmp x20, #0x41
1008c0d4c:      b.hs    0x1008c0db4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x130>
1008c0d50:      add x0, sp, #0x90
1008c0d54:      add x1, x21, #0x14
1008c0d58:      mov x2, x20
1008c0d5c:      bl  0x1008b89f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode>
1008c0d60:      ldr x8, [sp, #0x90]
1008c0d64:      cbz x8, 0x1008c0e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1f4>
1008c0d68:      ldr x8, [sp, #0x118]
1008c0d6c:      str x8, [sp, #0x80]
1008c0d70:      ldur    q0, [x24, #0x48]
1008c0d74:      ldur    q1, [x24, #0x58]
1008c0d78:      stp q0, q1, [sp, #0x40]
1008c0d7c:      ldur    q0, [x24, #0x68]
1008c0d80:      ldur    q1, [x24, #0x78]
1008c0d84:      stp q0, q1, [sp, #0x60]
1008c0d88:      ldur    q0, [x24, #0x8]
1008c0d8c:      ldur    q1, [x24, #0x18]
1008c0d90:      stp q0, q1, [sp]
1008c0d94:      ldur    q0, [x24, #0x28]
1008c0d98:      ldur    q1, [x24, #0x38]
1008c0d9c:      stp q0, q1, [sp, #0x20]
1008c0da0:      mov x0, sp
1008c0da4:      bl  0x1008b9480 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate>
1008c0da8:      tbz w0, #0x0, 0x1008c0e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1f4>
1008c0dac:      mov x22, x1
1008c0db0:      b   0x1008c128c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
1008c0db4:      cmp x20, #0x3e9
1008c0db8:      b.lo    0x1008c0e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1f4>
1008c0dbc:      add x0, x21, #0x14
1008c0dc0:      mov x1, x20
1008c0dc4:      mov w2, #0x3e8              ; =1000
1008c0dc8:      bl  0x1008b9fa0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1008c0dcc:      tbz w0, #0x0, 0x1008c0e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1f4>
1008c0dd0:      add x0, x21, #0x14
1008c0dd4:      mov x1, x20
1008c0dd8:      mov w2, #0xa120             ; =41248
1008c0ddc:      movk    w2, #0x7, lsl #16
1008c0de0:      bl  0x1008b9fa0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1008c0de4:      tbnz    w0, #0x0, 0x1008c1400 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x77c>
1008c0de8:      stur    x20, [x29, #-0x58]
1008c0dec:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1008c0df0:      bfxil   x8, x21, #0, #48
1008c0df4:      str x8, [sp, #0x90]
1008c0df8:      adrp    x19, 0x1010cf000 <_anon.0c78480e1ec3114c482e9770ddf18575.129+0x90>
1008c0dfc:      add x19, x19, #0xfd0
1008c0e00:      add x1, sp, #0x90
1008c0e04:      mov x0, x19
1008c0e08:      bl  0x100137610 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json15parse_root_push0jEB2h_>
1008c0e0c:      mov x21, x0
1008c0e10:      stp x0, x22, [x29, #-0x50]
1008c0e14:      str x20, [sp]
1008c0e18:      sub x8, x29, #0x48
1008c0e1c:      mov x9, sp
1008c0e20:      stp x8, x9, [sp, #0x90]
1008c0e24:      sub x8, x29, #0x50
1008c0e28:      sub x9, x29, #0x58
1008c0e2c:      stp x8, x9, [sp, #0xa0]
1008c0e30:      adrp    x0, 0x1010ce000 <_anon.a237fa49f331f28fb58ad898b36936d2.2234+0xf0>
1008c0e34:      add x0, x0, #0x7c0
1008c0e38:      add x1, sp, #0x90
1008c0e3c:      bl  0x10012c518 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawIB1t_NtNtNtB1S_5value7jsvalue7JSValueENCINvB1Q_19with_built_tape_rawB3o_NCNvNtNtB1S_4json9parse_api24try_parse_deep_iterative0E0E0IB1t_B3o_EEB1S_>
1008c0e40:      mov x20, x0
1008c0e44:      mov x22, x1
1008c0e48:      str x21, [sp, #0x90]
1008c0e4c:      add x1, sp, #0x90
1008c0e50:      mov x0, x19
1008c0e54:      bl  0x1001376a8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json18parse_root_restore0uEB2h_>
1008c0e58:      adrp    x0, 0x1010cf000 <_anon.0c78480e1ec3114c482e9770ddf18575.129+0x90>
1008c0e5c:      add x0, x0, #0xfd8
1008c0e60:      bl  0x100139f34 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api24try_parse_deep_iteratives_0uEB2P_>
1008c0e64:      tbnz    w20, #0x0, 0x1008c128c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
1008c0e68:      adrp    x0, 0x100e0c000 <_anon.a237fa49f331f28fb58ad898b36936d2.2333+0x209>
1008c0e6c:      add x0, x0, #0xd01
1008c0e70:      mov w1, #0x29               ; =41
1008c0e74:      bl  0x1008c20c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
1008c0e78:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
1008c0e7c:      add x0, x0, #0x2a0
1008c0e80:      ldr x8, [x0]
1008c0e84:      blr x8
1008c0e88:      mov x19, x0
1008c0e8c:      ldrb    w8, [x0, #0x20]
1008c0e90:      cbnz    w8, 0x1008c12cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x648>
1008c0e94:      ldr x8, [x19]
1008c0e98:      cbnz    x8, 0x1008c1348 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c4>
1008c0e9c:      mov x22, #0x7fff000000000000 ; =9223090561878065152
1008c0ea0:      bfxil   x22, x21, #0, #48
1008c0ea4:      mov x8, #-0x1               ; =-1
1008c0ea8:      str x8, [x19]
1008c0eac:      mov x21, x19
1008c0eb0:      ldr x8, [x21, #0x8]!
1008c0eb4:      ldr x23, [x19, #0x18]
1008c0eb8:      cmp x23, x8
1008c0ebc:      b.ne    0x1008c0ec8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x244>
1008c0ec0:      mov x0, x21
1008c0ec4:      bl  0x100cad9e0 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1008c0ec8:      ldr x8, [x19, #0x10]
1008c0ecc:      str x22, [x8, x23, lsl #3]
1008c0ed0:      add x8, x23, #0x1
1008c0ed4:      str x8, [x19, #0x18]
1008c0ed8:      ldr x8, [x19]
1008c0edc:      add x8, x8, #0x1
1008c0ee0:      str x8, [x19]
1008c0ee4:      mov x0, #0x0                ; =0
1008c0ee8:      bl  0x1002daea8 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy43GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1J_6option6OptionQIB2o_INtNtB1J_4cell4CellbEEEEE9call_onceBc_>
1008c0eec:      ldrb    w8, [x0]
1008c0ef0:      strb    wzr, [x0]
1008c0ef4:      cbz w8, 0x1008c0f2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2a8>
1008c0ef8:      mov x22, x0
1008c0efc:      mov x0, #0x0                ; =0
1008c0f00:      bl  0x1002daec8 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1008c0f04:      ldrb    w8, [x0]
1008c0f08:      tst w8, #0x3
1008c0f0c:      b.ne    0x1008c0f24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2a0>
1008c0f10:      adrp    x8, 0x10117c000 <_out_buf+0x3e08>
1008c0f14:      add x8, x8, #0x440
1008c0f18:      ldapr   w8, [x8]
1008c0f1c:      cmp w8, #0x0
1008c0f20:      b.le    0x1008c1084 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x400>
1008c0f24:      mov w8, #0x1                ; =1
1008c0f28:      strb    w8, [x22]
1008c0f2c:      ldrb    w8, [x19, #0x20]
1008c0f30:      cbnz    w8, 0x1008c10cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x448>
1008c0f34:      ldr x8, [x19]
1008c0f38:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008c0f3c:      cmp x8, x9
1008c0f40:      b.hs    0x1008c13b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
1008c0f44:      add x9, x8, #0x1
1008c0f48:      str x9, [x19]
1008c0f4c:      ldr x9, [x19, #0x18]
1008c0f50:      cmp x23, x9
1008c0f54:      b.hs    0x1008c0f68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2e4>
1008c0f58:      ldr x9, [x19, #0x10]
1008c0f5c:      ldr x9, [x9, x23, lsl #3]
1008c0f60:      and x22, x9, #0xffffffffffff
1008c0f64:      b   0x1008c0f6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2e8>
1008c0f68:      mov w22, #0x1               ; =1
1008c0f6c:      str x8, [x19]
1008c0f70:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
1008c0f74:      add x8, x8, #0xb88
1008c0f78:      ldapr   x8, [x8]
1008c0f7c:      cbnz    x8, 0x1008c12ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x628>
1008c0f80:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
1008c0f84:      ldrb    w8, [x8, #0xb90]
1008c0f88:      cmp w8, #0x2
1008c0f8c:      b.eq    0x1008c1104 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
1008c0f90:      cmp w8, #0x1
1008c0f94:      b.ne    0x1008c0fd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x354>
1008c0f98:      stp x23, x20, [x29, #-0x58]
1008c0f9c:      ldrb    w8, [x19, #0x20]
1008c0fa0:      cbnz    w8, 0x1008c1384 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x700>
1008c0fa4:      ldr x8, [x19]
1008c0fa8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008c0fac:      cmp x8, x9
1008c0fb0:      b.hs    0x1008c13b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
1008c0fb4:      add x9, x8, #0x1
1008c0fb8:      str x9, [x19]
1008c0fbc:      ldr x9, [x19, #0x18]
1008c0fc0:      cmp x23, x9
1008c0fc4:      b.hs    0x1008c1018 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x394>
1008c0fc8:      ldr x9, [x19, #0x10]
1008c0fcc:      ldr x9, [x9, x23, lsl #3]
1008c0fd0:      and x9, x9, #0xffffffffffff
1008c0fd4:      b   0x1008c101c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x398>
1008c0fd8:      add x8, x22, #0x14
1008c0fdc:      sub x9, x20, #0x400
1008c0fe0:      mov w10, #0xfffc00          ; =16776192
1008c0fe4:      cmp x9, x10
1008c0fe8:      b.hi    0x1008c1104 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
1008c0fec:      mov x9, #0x2600             ; =9728
1008c0ff0:      movk    x9, #0x1, lsl #32
1008c0ff4:      mov x10, x20
1008c0ff8:      ldrb    w11, [x8], #0x1
1008c0ffc:      cmp w11, #0x20
1008c1000:      b.hi    0x1008c10fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x478>
1008c1004:      lsr x12, x9, x11
1008c1008:      tbz w12, #0x0, 0x1008c10fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x478>
1008c100c:      subs    x10, x10, #0x1
1008c1010:      b.ne    0x1008c0ff8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x374>
1008c1014:      b   0x1008c1104 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
1008c1018:      mov w9, #0x1                ; =1
1008c101c:      str x8, [x19]
1008c1020:      add x8, x9, #0x14
1008c1024:      stur    x8, [x29, #-0x48]
1008c1028:      str x20, [sp]
1008c102c:      sub x8, x29, #0x48
1008c1030:      mov x9, sp
1008c1034:      stp x8, x9, [sp, #0x90]
1008c1038:      sub x8, x29, #0x58
1008c103c:      sub x9, x29, #0x50
1008c1040:      stp x8, x9, [sp, #0xa0]
1008c1044:      adrp    x0, 0x1010ce000 <_anon.a237fa49f331f28fb58ad898b36936d2.2234+0xf0>
1008c1048:      add x0, x0, #0x7c0
1008c104c:      add x1, sp, #0x90
1008c1050:      bl  0x10012c8e4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawNtNtNtB1S_5value7jsvalue7JSValueNCNvNtNtB1S_4json9parse_api18try_parse_via_tape0E0IB1t_B3o_EEB1S_>
1008c1054:      cmp x0, #0x1
1008c1058:      b.ne    0x1008c1104 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
1008c105c:      mov x22, x1
1008c1060:      ldrb    w8, [x19, #0x20]
1008c1064:      cbnz    w8, 0x1008c13bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x738>
1008c1068:      ldr x8, [x19]
1008c106c:      cbnz    x8, 0x1008c1378 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6f4>
1008c1070:      ldr x8, [x19, #0x18]
1008c1074:      cmp x23, x8
1008c1078:      b.hi    0x1008c128c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
1008c107c:      str x23, [x19, #0x18]
1008c1080:      b   0x1008c128c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
1008c1084:      mov x0, #0x0                ; =0
1008c1088:      bl  0x1002db010 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5block17ARENA_TOTAL_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1l_6option6OptionQIB20_INtNtB1l_4cell4CelljEEEEE9call_onceBc_>
1008c108c:      ldr x22, [x0]
1008c1090:      mov x0, #0x0                ; =0
1008c1094:      bl  0x1002dad68 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy21GC_NEXT_TRIGGER_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1n_6option6OptionQIB22_INtNtB1n_4cell4CelljEEEEE9call_onceBc_>
1008c1098:      ldr x8, [x0]
1008c109c:      cmp x8, x22
1008c10a0:      b.ls    0x1008c10c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x43c>
1008c10a4:      str x22, [x0]
1008c10a8:      adrp    x0, 0x101135000 <__MergedGlobals+0xc0>
1008c10ac:      add x0, x0, #0xf90
1008c10b0:      ldr x8, [x0]
1008c10b4:      blr x8
1008c10b8:      mov w8, #0x1                ; =1
1008c10bc:      strb    w8, [x0]
1008c10c0:      bl  0x1002a6240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1008c10c4:      ldrb    w8, [x19, #0x20]
1008c10c8:      cbz w8, 0x1008c0f34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2b0>
1008c10cc:      cmp w8, #0x2
1008c10d0:      b.eq    0x1008c13c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
1008c10d4:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1008c10d8:      add x1, x1, #0xeec
1008c10dc:      mov x0, x19
1008c10e0:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008c10e4:      strb    wzr, [x19, #0x20]
1008c10e8:      ldr x8, [x19]
1008c10ec:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008c10f0:      cmp x8, x9
1008c10f4:      b.lo    0x1008c0f44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2c0>
1008c10f8:      b   0x1008c13b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
1008c10fc:      cmp w11, #0x5b
1008c1100:      b.eq    0x1008c0f98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x314>
1008c1104:      bl  0x1002a6240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1008c1108:      bl  0x1002a5c48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
1008c110c:      ldrb    w8, [x19, #0x20]
1008c1110:      cbnz    w8, 0x1008c12f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x670>
1008c1114:      ldr x8, [x19]
1008c1118:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008c111c:      cmp x8, x9
1008c1120:      b.hs    0x1008c13b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
1008c1124:      add x9, x8, #0x1
1008c1128:      str x9, [x19]
1008c112c:      ldr x10, [x19, #0x18]
1008c1130:      mov w9, #0x1                ; =1
1008c1134:      cmp x23, x10
1008c1138:      b.hs    0x1008c114c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4c8>
1008c113c:      ldr x10, [x19, #0x10]
1008c1140:      ldr x10, [x10, x23, lsl #3]
1008c1144:      and x10, x10, #0xffffffffffff
1008c1148:      b   0x1008c1150 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4cc>
1008c114c:      mov w10, #0x1               ; =1
1008c1150:      str x8, [x19]
1008c1154:      add x8, x10, #0x14
1008c1158:      movi.2d v0, #0000000000000000
1008c115c:      stur    q0, [x24, #0x78]
1008c1160:      stur    q0, [x24, #0x68]
1008c1164:      stur    q0, [x24, #0x58]
1008c1168:      stur    q0, [x24, #0x48]
1008c116c:      strb    w9, [sp, #0x120]
1008c1170:      mov x9, #-0x1               ; =-1
1008c1174:      stp x8, x20, [sp, #0xb8]
1008c1178:      str x9, [sp, #0x90]
1008c117c:      stp xzr, xzr, [sp, #0xc8]
1008c1180:      str xzr, [sp, #0x118]
1008c1184:      add x0, sp, #0x90
1008c1188:      bl  0x1008909a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1008c118c:      mov x22, x0
1008c1190:      ldp x8, x9, [sp, #0xc0]
1008c1194:      cmp x9, x8
1008c1198:      b.hs    0x1008c11cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x548>
1008c119c:      ldr x10, [sp, #0xb8]
1008c11a0:      mov x11, #0x2600            ; =9728
1008c11a4:      movk    x11, #0x1, lsl #32
1008c11a8:      ldrb    w12, [x10, x9]
1008c11ac:      cmp w12, #0x20
1008c11b0:      b.hi    0x1008c11cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x548>
1008c11b4:      lsr x12, x11, x12
1008c11b8:      tbz w12, #0x0, 0x1008c11cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x548>
1008c11bc:      add x9, x9, #0x1
1008c11c0:      cmp x8, x9
1008c11c4:      b.ne    0x1008c11a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x524>
1008c11c8:      mov x9, x8
1008c11cc:      ldrb    w20, [sp, #0x120]
1008c11d0:      cmp x9, x8
1008c11d4:      cset    w24, eq
1008c11d8:      ldrb    w8, [x19, #0x20]
1008c11dc:      cbnz    w8, 0x1008c1324 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6a0>
1008c11e0:      ldr x8, [x19]
1008c11e4:      cbnz    x8, 0x1008c1348 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c4>
1008c11e8:      mov x8, #-0x1               ; =-1
1008c11ec:      str x8, [x19]
1008c11f0:      ldr x25, [x19, #0x18]
1008c11f4:      ldr x8, [x19, #0x8]
1008c11f8:      cmp x25, x8
1008c11fc:      b.ne    0x1008c1208 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x584>
1008c1200:      mov x0, x21
1008c1204:      bl  0x100cad9e0 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1008c1208:      ldr x8, [x19, #0x10]
1008c120c:      str x22, [x8, x25, lsl #3]
1008c1210:      add x8, x25, #0x1
1008c1214:      str x8, [x19, #0x18]
1008c1218:      ldr x8, [x19]
1008c121c:      add x8, x8, #0x1
1008c1220:      str x8, [x19]
1008c1224:      mov x0, #0x0                ; =0
1008c1228:      bl  0x1002daec8 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1008c122c:      ldrb    w8, [x0]
1008c1230:      and w8, w8, #0xfffffffd
1008c1234:      strb    w8, [x0]
1008c1238:      bl  0x1002a6d28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy22gc_bump_malloc_trigger>
1008c123c:      bl  0x1002abac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy49gc_schedule_parse_boundary_collection_if_pressure>
1008c1240:      ldrb    w8, [x19, #0x20]
1008c1244:      cbnz    w8, 0x1008c1354 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6d0>
1008c1248:      ldr x8, [x19]
1008c124c:      cbnz    x8, 0x1008c1378 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6f4>
1008c1250:      and w20, w24, w20
1008c1254:      ldr x8, [x19, #0x18]
1008c1258:      cmp x23, x8
1008c125c:      b.hi    0x1008c1264 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x5e0>
1008c1260:      str x23, [x19, #0x18]
1008c1264:      adrp    x0, 0x1010cf000 <_anon.0c78480e1ec3114c482e9770ddf18575.129+0x90>
1008c1268:      add x0, x0, #0xfd8
1008c126c:      bl  0x100139c64 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api10parse_slow0uEB2P_>
1008c1270:      tbz w20, #0x0, 0x1008c13f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x76c>
1008c1274:      ldr x8, [sp, #0x90]
1008c1278:      cmn x8, #0x1
1008c127c:      b.eq    0x1008c128c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
1008c1280:      cbz x8, 0x1008c128c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
1008c1284:      ldr x0, [sp, #0x98]
1008c1288:      bl  0x100ce2ac0 <_mi_free>
1008c128c:      mov x0, x22
1008c1290:      ldp x29, x30, [sp, #0x180]
1008c1294:      ldp x20, x19, [sp, #0x170]
1008c1298:      ldp x22, x21, [sp, #0x160]
1008c129c:      ldp x24, x23, [sp, #0x150]
1008c12a0:      ldp x26, x25, [sp, #0x140]
1008c12a4:      add sp, sp, #0x190
1008c12a8:      ret
1008c12ac:      adrp    x0, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
1008c12b0:      add x0, x0, #0xb88
1008c12b4:      bl  0x100ccfba8 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockNtNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api8TapeModeE10initializeNCINvB2_11get_or_initNCNvBV_18tape_mode_from_env0E0zEBZ_>
1008c12b8:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
1008c12bc:      ldrb    w8, [x8, #0xb90]
1008c12c0:      cmp w8, #0x2
1008c12c4:      b.ne    0x1008c0f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x30c>
1008c12c8:      b   0x1008c1104 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
1008c12cc:      cmp w8, #0x1
1008c12d0:      b.ne    0x1008c13c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
1008c12d4:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1008c12d8:      add x1, x1, #0xeec
1008c12dc:      mov x0, x19
1008c12e0:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008c12e4:      strb    wzr, [x19, #0x20]
1008c12e8:      ldr x8, [x19]
1008c12ec:      cbz x8, 0x1008c0e9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x218>
1008c12f0:      b   0x1008c1348 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c4>
1008c12f4:      cmp w8, #0x2
1008c12f8:      b.eq    0x1008c13c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
1008c12fc:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1008c1300:      add x1, x1, #0xeec
1008c1304:      mov x0, x19
1008c1308:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008c130c:      strb    wzr, [x19, #0x20]
1008c1310:      ldr x8, [x19]
1008c1314:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008c1318:      cmp x8, x9
1008c131c:      b.lo    0x1008c1124 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4a0>
1008c1320:      b   0x1008c13b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
1008c1324:      cmp w8, #0x2
1008c1328:      b.eq    0x1008c13c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
1008c132c:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1008c1330:      add x1, x1, #0xeec
1008c1334:      mov x0, x19
1008c1338:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008c133c:      strb    wzr, [x19, #0x20]
1008c1340:      ldr x8, [x19]
1008c1344:      cbz x8, 0x1008c11e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x564>
1008c1348:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1008c134c:      add x0, x0, #0xdf8
1008c1350:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008c1354:      cmp w8, #0x2
1008c1358:      b.eq    0x1008c13c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
1008c135c:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1008c1360:      add x1, x1, #0xeec
1008c1364:      mov x0, x19
1008c1368:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008c136c:      strb    wzr, [x19, #0x20]
1008c1370:      ldr x8, [x19]
1008c1374:      cbz x8, 0x1008c1250 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x5cc>
1008c1378:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1008c137c:      add x0, x0, #0xe58
1008c1380:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008c1384:      cmp w8, #0x2
1008c1388:      b.eq    0x1008c13c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
1008c138c:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1008c1390:      add x1, x1, #0xeec
1008c1394:      mov x0, x19
1008c1398:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008c139c:      strb    wzr, [x19, #0x20]
1008c13a0:      ldr x8, [x19]
1008c13a4:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008c13a8:      cmp x8, x9
1008c13ac:      b.lo    0x1008c0fb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x330>
1008c13b0:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1008c13b4:      add x0, x0, #0xdc8
1008c13b8:      bl  0x100c99c5c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008c13bc:      cmp w8, #0x2
1008c13c0:      b.ne    0x1008c13d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x74c>
1008c13c4:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1008c13c8:      add x0, x0, #0xed8
1008c13cc:      bl  0x100cdc11c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1008c13d0:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1008c13d4:      add x1, x1, #0xeec
1008c13d8:      mov x0, x19
1008c13dc:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008c13e0:      strb    wzr, [x19, #0x20]
1008c13e4:      ldr x8, [x19]
1008c13e8:      cbz x8, 0x1008c1070 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x3ec>
1008c13ec:      b   0x1008c1378 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6f4>
1008c13f0:      adrp    x0, 0x100e10000 <_anon.0c78480e1ec3114c482e9770ddf18575.278+0x324a>
1008c13f4:      add x0, x0, #0x626
1008c13f8:      mov w1, #0x21               ; =33
1008c13fc:      bl  0x1008c20c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
1008c1400:      add x0, sp, #0x90
1008c1404:      bl  0x1008c2168 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api24iterative_budget_message>
1008c1408:      ldp x0, x1, [sp, #0x98]
1008c140c:      bl  0x1008c1abc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17throw_range_error>
