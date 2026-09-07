/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100369d5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow>:
100369d5c:      sub sp, sp, #0x190
100369d60:      stp x26, x25, [sp, #0x140]
100369d64:      stp x24, x23, [sp, #0x150]
100369d68:      stp x22, x21, [sp, #0x160]
100369d6c:      stp x20, x19, [sp, #0x170]
100369d70:      stp x29, x30, [sp, #0x180]
100369d74:      add x29, sp, #0x180
100369d78:      mov x20, x1
100369d7c:      mov x21, x0
100369d80:      add x24, sp, #0x90
100369d84:      add x22, x0, #0x14
100369d88:      cmp x1, #0x2
100369d8c:      b.ne    0x100369da4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x48>
100369d90:      ldrh    w8, [x22]
100369d94:      mov w9, #0x7d7b             ; =32123
100369d98:      cmp w8, w9
100369d9c:      b.eq    0x100369dd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x7c>
100369da0:      b   0x100369e28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xcc>
100369da4:      cmp x20, #0x3
100369da8:      b.lo    0x100369e28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xcc>
100369dac:      ldrb    w8, [x22]
100369db0:      cmp w8, #0x20
100369db4:      b.hi    0x100369df4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x98>
100369db8:      mov x9, #0x2600             ; =9728
100369dbc:      movk    x9, #0x1, lsl #32
100369dc0:      lsr x9, x9, x8
100369dc4:      tbz w9, #0x0, 0x100369df4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x98>
100369dc8:      add x0, x21, #0x14
100369dcc:      mov x1, x20
100369dd0:      bl  0x10036156c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty19padded_empty_object>
100369dd4:      tbz w0, #0x0, 0x100369e20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xc4>
100369dd8:      ldp x29, x30, [sp, #0x180]
100369ddc:      ldp x20, x19, [sp, #0x170]
100369de0:      ldp x22, x21, [sp, #0x160]
100369de4:      ldp x24, x23, [sp, #0x150]
100369de8:      ldp x26, x25, [sp, #0x140]
100369dec:      add sp, sp, #0x190
100369df0:      b   0x100361638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty21allocate_empty_object>
100369df4:      cmp w8, #0x7b
100369df8:      b.ne    0x100369e20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xc4>
100369dfc:      ldrb    w8, [x21, #0x15]
100369e00:      cmp w8, #0x20
100369e04:      b.hi    0x100369e18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0xbc>
100369e08:      mov x9, #0x2600             ; =9728
100369e0c:      movk    x9, #0x1, lsl #32
100369e10:      lsr x9, x9, x8
100369e14:      tbnz    w9, #0x0, 0x100369dc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c>
100369e18:      cmp w8, #0x7d
100369e1c:      b.eq    0x100369dc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c>
100369e20:      cmp x20, #0x41
100369e24:      b.hs    0x100369e8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x130>
100369e28:      add x0, sp, #0x90
100369e2c:      add x1, x21, #0x14
100369e30:      mov x2, x20
100369e34:      bl  0x100361b04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode>
100369e38:      ldr x8, [sp, #0x90]
100369e3c:      cbz x8, 0x100369f50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1f4>
100369e40:      ldr x8, [sp, #0x118]
100369e44:      str x8, [sp, #0x80]
100369e48:      ldur    q0, [x24, #0x48]
100369e4c:      ldur    q1, [x24, #0x58]
100369e50:      stp q0, q1, [sp, #0x40]
100369e54:      ldur    q0, [x24, #0x68]
100369e58:      ldur    q1, [x24, #0x78]
100369e5c:      stp q0, q1, [sp, #0x60]
100369e60:      ldur    q0, [x24, #0x8]
100369e64:      ldur    q1, [x24, #0x18]
100369e68:      stp q0, q1, [sp]
100369e6c:      ldur    q0, [x24, #0x28]
100369e70:      ldur    q1, [x24, #0x38]
100369e74:      stp q0, q1, [sp, #0x20]
100369e78:      mov x0, sp
100369e7c:      bl  0x100362564 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate>
100369e80:      tbz w0, #0x0, 0x100369f50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1f4>
100369e84:      mov x22, x1
100369e88:      b   0x10036a364 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
100369e8c:      cmp x20, #0x3e9
100369e90:      b.lo    0x100369f50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1f4>
100369e94:      add x0, x21, #0x14
100369e98:      mov x1, x20
100369e9c:      mov w2, #0x3e8              ; =1000
100369ea0:      bl  0x100363078 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
100369ea4:      tbz w0, #0x0, 0x100369f50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x1f4>
100369ea8:      add x0, x21, #0x14
100369eac:      mov x1, x20
100369eb0:      mov w2, #0xa120             ; =41248
100369eb4:      movk    w2, #0x7, lsl #16
100369eb8:      bl  0x100363078 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
100369ebc:      tbnz    w0, #0x0, 0x10036a4d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x77c>
100369ec0:      stur    x20, [x29, #-0x58]
100369ec4:      mov x8, #0x7fff000000000000 ; =9223090561878065152
100369ec8:      bfxil   x8, x21, #0, #48
100369ecc:      str x8, [sp, #0x90]
100369ed0:      adrp    x19, 0x1010b3000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.209>
100369ed4:      add x19, x19, #0xd28
100369ed8:      add x1, sp, #0x90
100369edc:      mov x0, x19
100369ee0:      bl  0x100137a90 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json15parse_root_push0jEB2h_>
100369ee4:      mov x21, x0
100369ee8:      stp x0, x22, [x29, #-0x50]
100369eec:      str x20, [sp]
100369ef0:      sub x8, x29, #0x48
100369ef4:      mov x9, sp
100369ef8:      stp x8, x9, [sp, #0x90]
100369efc:      sub x8, x29, #0x50
100369f00:      sub x9, x29, #0x58
100369f04:      stp x8, x9, [sp, #0xa0]
100369f08:      adrp    x0, 0x1010b2000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3map29KEEP_JS_MAP_DELETE_NUMBER_KEY>
100369f0c:      add x0, x0, #0x388
100369f10:      add x1, sp, #0x90
100369f14:      bl  0x10012c958 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawIB1t_NtNtNtB1S_5value7jsvalue7JSValueENCINvB1Q_19with_built_tape_rawB3o_NCNvNtNtB1S_4json9parse_api24try_parse_deep_iterative0E0E0IB1t_B3o_EEB1S_>
100369f18:      mov x20, x0
100369f1c:      mov x22, x1
100369f20:      str x21, [sp, #0x90]
100369f24:      add x1, sp, #0x90
100369f28:      mov x0, x19
100369f2c:      bl  0x100137b28 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json18parse_root_restore0uEB2h_>
100369f30:      adrp    x0, 0x1010b3000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.209>
100369f34:      add x0, x0, #0xd30
100369f38:      bl  0x10013a3b4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api24try_parse_deep_iteratives_0uEB2P_>
100369f3c:      tbnz    w20, #0x0, 0x10036a364 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
100369f40:      adrp    x0, 0x100dd1000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.52+0x78>
100369f44:      add x0, x0, #0x1d9
100369f48:      mov w1, #0x29               ; =41
100369f4c:      bl  0x10036b198 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
100369f50:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
100369f54:      add x0, x0, #0x398
100369f58:      ldr x8, [x0]
100369f5c:      blr x8
100369f60:      mov x19, x0
100369f64:      ldrb    w8, [x0, #0x20]
100369f68:      cbnz    w8, 0x10036a3a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x648>
100369f6c:      ldr x8, [x19]
100369f70:      cbnz    x8, 0x10036a420 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c4>
100369f74:      mov x22, #0x7fff000000000000 ; =9223090561878065152
100369f78:      bfxil   x22, x21, #0, #48
100369f7c:      mov x8, #-0x1               ; =-1
100369f80:      str x8, [x19]
100369f84:      mov x21, x19
100369f88:      ldr x8, [x21, #0x8]!
100369f8c:      ldr x23, [x19, #0x18]
100369f90:      cmp x23, x8
100369f94:      b.ne    0x100369fa0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x244>
100369f98:      mov x0, x21
100369f9c:      bl  0x100cd4250 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
100369fa0:      ldr x8, [x19, #0x10]
100369fa4:      str x22, [x8, x23, lsl #3]
100369fa8:      add x8, x23, #0x1
100369fac:      str x8, [x19, #0x18]
100369fb0:      ldr x8, [x19]
100369fb4:      add x8, x8, #0x1
100369fb8:      str x8, [x19]
100369fbc:      mov x0, #0x0                ; =0
100369fc0:      bl  0x100482150 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy43GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1J_6option6OptionQIB2o_INtNtB1J_4cell4CellbEEEEE9call_onceBc_>
100369fc4:      ldrb    w8, [x0]
100369fc8:      strb    wzr, [x0]
100369fcc:      cbz w8, 0x10036a004 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2a8>
100369fd0:      mov x22, x0
100369fd4:      mov x0, #0x0                ; =0
100369fd8:      bl  0x100482170 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
100369fdc:      ldrb    w8, [x0]
100369fe0:      tst w8, #0x3
100369fe4:      b.ne    0x100369ffc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2a0>
100369fe8:      adrp    x8, 0x101200000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7a8>
100369fec:      add x8, x8, #0xa8c
100369ff0:      ldapr   w8, [x8]
100369ff4:      cmp w8, #0x0
100369ff8:      b.le    0x10036a15c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x400>
100369ffc:      mov w8, #0x1                ; =1
10036a000:      strb    w8, [x22]
10036a004:      ldrb    w8, [x19, #0x20]
10036a008:      cbnz    w8, 0x10036a1a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x448>
10036a00c:      ldr x8, [x19]
10036a010:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10036a014:      cmp x8, x9
10036a018:      b.hs    0x10036a488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
10036a01c:      add x9, x8, #0x1
10036a020:      str x9, [x19]
10036a024:      ldr x9, [x19, #0x18]
10036a028:      cmp x23, x9
10036a02c:      b.hs    0x10036a040 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2e4>
10036a030:      ldr x9, [x19, #0x10]
10036a034:      ldr x9, [x9, x23, lsl #3]
10036a038:      and x22, x9, #0xffffffffffff
10036a03c:      b   0x10036a044 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2e8>
10036a040:      mov w22, #0x1               ; =1
10036a044:      str x8, [x19]
10036a048:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
10036a04c:      add x8, x8, #0x6a0
10036a050:      ldapr   x8, [x8]
10036a054:      cbnz    x8, 0x10036a384 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x628>
10036a058:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
10036a05c:      ldrb    w8, [x8, #0x6a8]
10036a060:      cmp w8, #0x2
10036a064:      b.eq    0x10036a1dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
10036a068:      cmp w8, #0x1
10036a06c:      b.ne    0x10036a0b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x354>
10036a070:      stp x23, x20, [x29, #-0x58]
10036a074:      ldrb    w8, [x19, #0x20]
10036a078:      cbnz    w8, 0x10036a45c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x700>
10036a07c:      ldr x8, [x19]
10036a080:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10036a084:      cmp x8, x9
10036a088:      b.hs    0x10036a488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
10036a08c:      add x9, x8, #0x1
10036a090:      str x9, [x19]
10036a094:      ldr x9, [x19, #0x18]
10036a098:      cmp x23, x9
10036a09c:      b.hs    0x10036a0f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x394>
10036a0a0:      ldr x9, [x19, #0x10]
10036a0a4:      ldr x9, [x9, x23, lsl #3]
10036a0a8:      and x9, x9, #0xffffffffffff
10036a0ac:      b   0x10036a0f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x398>
10036a0b0:      add x8, x22, #0x14
10036a0b4:      sub x9, x20, #0x400
10036a0b8:      mov w10, #0xfffc00          ; =16776192
10036a0bc:      cmp x9, x10
10036a0c0:      b.hi    0x10036a1dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
10036a0c4:      mov x9, #0x2600             ; =9728
10036a0c8:      movk    x9, #0x1, lsl #32
10036a0cc:      mov x10, x20
10036a0d0:      ldrb    w11, [x8], #0x1
10036a0d4:      cmp w11, #0x20
10036a0d8:      b.hi    0x10036a1d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x478>
10036a0dc:      lsr x12, x9, x11
10036a0e0:      tbz w12, #0x0, 0x10036a1d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x478>
10036a0e4:      subs    x10, x10, #0x1
10036a0e8:      b.ne    0x10036a0d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x374>
10036a0ec:      b   0x10036a1dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
10036a0f0:      mov w9, #0x1                ; =1
10036a0f4:      str x8, [x19]
10036a0f8:      add x8, x9, #0x14
10036a0fc:      stur    x8, [x29, #-0x48]
10036a100:      str x20, [sp]
10036a104:      sub x8, x29, #0x48
10036a108:      mov x9, sp
10036a10c:      stp x8, x9, [sp, #0x90]
10036a110:      sub x8, x29, #0x58
10036a114:      sub x9, x29, #0x50
10036a118:      stp x8, x9, [sp, #0xa0]
10036a11c:      adrp    x0, 0x1010b2000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3map29KEEP_JS_MAP_DELETE_NUMBER_KEY>
10036a120:      add x0, x0, #0x388
10036a124:      add x1, sp, #0x90
10036a128:      bl  0x10012cd24 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawNtNtNtB1S_5value7jsvalue7JSValueNCNvNtNtB1S_4json9parse_api18try_parse_via_tape0E0IB1t_B3o_EEB1S_>
10036a12c:      cmp x0, #0x1
10036a130:      b.ne    0x10036a1dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
10036a134:      mov x22, x1
10036a138:      ldrb    w8, [x19, #0x20]
10036a13c:      cbnz    w8, 0x10036a494 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x738>
10036a140:      ldr x8, [x19]
10036a144:      cbnz    x8, 0x10036a450 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6f4>
10036a148:      ldr x8, [x19, #0x18]
10036a14c:      cmp x23, x8
10036a150:      b.hi    0x10036a364 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
10036a154:      str x23, [x19, #0x18]
10036a158:      b   0x10036a364 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
10036a15c:      mov x0, #0x0                ; =0
10036a160:      bl  0x100482370 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5block17ARENA_TOTAL_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1l_6option6OptionQIB20_INtNtB1l_4cell4CelljEEEEE9call_onceBc_>
10036a164:      ldr x22, [x0]
10036a168:      mov x0, #0x0                ; =0
10036a16c:      bl  0x100482050 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy21GC_NEXT_TRIGGER_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1n_6option6OptionQIB22_INtNtB1n_4cell4CelljEEEEE9call_onceBc_>
10036a170:      ldr x8, [x0]
10036a174:      cmp x8, x22
10036a178:      b.ls    0x10036a198 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x43c>
10036a17c:      str x22, [x0]
10036a180:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
10036a184:      add x0, x0, #0xad0
10036a188:      ldr x8, [x0]
10036a18c:      blr x8
10036a190:      mov w8, #0x1                ; =1
10036a194:      strb    w8, [x0]
10036a198:      bl  0x100447f80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
10036a19c:      ldrb    w8, [x19, #0x20]
10036a1a0:      cbz w8, 0x10036a00c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2b0>
10036a1a4:      cmp w8, #0x2
10036a1a8:      b.eq    0x10036a49c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
10036a1ac:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10036a1b0:      add x1, x1, #0x87c
10036a1b4:      mov x0, x19
10036a1b8:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10036a1bc:      strb    wzr, [x19, #0x20]
10036a1c0:      ldr x8, [x19]
10036a1c4:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10036a1c8:      cmp x8, x9
10036a1cc:      b.lo    0x10036a01c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x2c0>
10036a1d0:      b   0x10036a488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
10036a1d4:      cmp w11, #0x5b
10036a1d8:      b.eq    0x10036a070 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x314>
10036a1dc:      bl  0x100447f80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
10036a1e0:      bl  0x100447988 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
10036a1e4:      ldrb    w8, [x19, #0x20]
10036a1e8:      cbnz    w8, 0x10036a3cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x670>
10036a1ec:      ldr x8, [x19]
10036a1f0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10036a1f4:      cmp x8, x9
10036a1f8:      b.hs    0x10036a488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
10036a1fc:      add x9, x8, #0x1
10036a200:      str x9, [x19]
10036a204:      ldr x10, [x19, #0x18]
10036a208:      mov w9, #0x1                ; =1
10036a20c:      cmp x23, x10
10036a210:      b.hs    0x10036a224 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4c8>
10036a214:      ldr x10, [x19, #0x10]
10036a218:      ldr x10, [x10, x23, lsl #3]
10036a21c:      and x10, x10, #0xffffffffffff
10036a220:      b   0x10036a228 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4cc>
10036a224:      mov w10, #0x1               ; =1
10036a228:      str x8, [x19]
10036a22c:      add x8, x10, #0x14
10036a230:      movi.2d v0, #0000000000000000
10036a234:      stur    q0, [x24, #0x78]
10036a238:      stur    q0, [x24, #0x68]
10036a23c:      stur    q0, [x24, #0x58]
10036a240:      stur    q0, [x24, #0x48]
10036a244:      strb    w9, [sp, #0x120]
10036a248:      mov x9, #-0x1               ; =-1
10036a24c:      stp x8, x20, [sp, #0xb8]
10036a250:      str x9, [sp, #0x90]
10036a254:      stp xzr, xzr, [sp, #0xc8]
10036a258:      str xzr, [sp, #0x118]
10036a25c:      add x0, sp, #0x90
10036a260:      bl  0x10033aaa0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10036a264:      mov x22, x0
10036a268:      ldp x8, x9, [sp, #0xc0]
10036a26c:      cmp x9, x8
10036a270:      b.hs    0x10036a2a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x548>
10036a274:      ldr x10, [sp, #0xb8]
10036a278:      mov x11, #0x2600            ; =9728
10036a27c:      movk    x11, #0x1, lsl #32
10036a280:      ldrb    w12, [x10, x9]
10036a284:      cmp w12, #0x20
10036a288:      b.hi    0x10036a2a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x548>
10036a28c:      lsr x12, x11, x12
10036a290:      tbz w12, #0x0, 0x10036a2a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x548>
10036a294:      add x9, x9, #0x1
10036a298:      cmp x8, x9
10036a29c:      b.ne    0x10036a280 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x524>
10036a2a0:      mov x9, x8
10036a2a4:      ldrb    w20, [sp, #0x120]
10036a2a8:      cmp x9, x8
10036a2ac:      cset    w24, eq
10036a2b0:      ldrb    w8, [x19, #0x20]
10036a2b4:      cbnz    w8, 0x10036a3fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6a0>
10036a2b8:      ldr x8, [x19]
10036a2bc:      cbnz    x8, 0x10036a420 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c4>
10036a2c0:      mov x8, #-0x1               ; =-1
10036a2c4:      str x8, [x19]
10036a2c8:      ldr x25, [x19, #0x18]
10036a2cc:      ldr x8, [x19, #0x8]
10036a2d0:      cmp x25, x8
10036a2d4:      b.ne    0x10036a2e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x584>
10036a2d8:      mov x0, x21
10036a2dc:      bl  0x100cd4250 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
10036a2e0:      ldr x8, [x19, #0x10]
10036a2e4:      str x22, [x8, x25, lsl #3]
10036a2e8:      add x8, x25, #0x1
10036a2ec:      str x8, [x19, #0x18]
10036a2f0:      ldr x8, [x19]
10036a2f4:      add x8, x8, #0x1
10036a2f8:      str x8, [x19]
10036a2fc:      mov x0, #0x0                ; =0
10036a300:      bl  0x100482170 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
10036a304:      ldrb    w8, [x0]
10036a308:      and w8, w8, #0xfffffffd
10036a30c:      strb    w8, [x0]
10036a310:      bl  0x100448a90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy22gc_bump_malloc_trigger>
10036a314:      bl  0x10044d36c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy49gc_schedule_parse_boundary_collection_if_pressure>
10036a318:      ldrb    w8, [x19, #0x20]
10036a31c:      cbnz    w8, 0x10036a42c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6d0>
10036a320:      ldr x8, [x19]
10036a324:      cbnz    x8, 0x10036a450 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6f4>
10036a328:      and w20, w24, w20
10036a32c:      ldr x8, [x19, #0x18]
10036a330:      cmp x23, x8
10036a334:      b.hi    0x10036a33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x5e0>
10036a338:      str x23, [x19, #0x18]
10036a33c:      adrp    x0, 0x1010b3000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.209>
10036a340:      add x0, x0, #0xd30
10036a344:      bl  0x10013a0e4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api10parse_slow0uEB2P_>
10036a348:      tbz w20, #0x0, 0x10036a4c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x76c>
10036a34c:      ldr x8, [sp, #0x90]
10036a350:      cmn x8, #0x1
10036a354:      b.eq    0x10036a364 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
10036a358:      cbz x8, 0x10036a364 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x608>
10036a35c:      ldr x0, [sp, #0x98]
10036a360:      bl  0x100ce70c0 <_mi_free>
10036a364:      mov x0, x22
10036a368:      ldp x29, x30, [sp, #0x180]
10036a36c:      ldp x20, x19, [sp, #0x170]
10036a370:      ldp x22, x21, [sp, #0x160]
10036a374:      ldp x24, x23, [sp, #0x150]
10036a378:      ldp x26, x25, [sp, #0x140]
10036a37c:      add sp, sp, #0x190
10036a380:      ret
10036a384:      adrp    x0, 0x101134000 <_perry_global_baseline_worker_ts__1>
10036a388:      add x0, x0, #0x6a0
10036a38c:      bl  0x100cc3d98 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockNtNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api8TapeModeE10initializeNCINvB2_11get_or_initNCNvBV_18tape_mode_from_env0E0zEBZ_>
10036a390:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
10036a394:      ldrb    w8, [x8, #0x6a8]
10036a398:      cmp w8, #0x2
10036a39c:      b.ne    0x10036a068 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x30c>
10036a3a0:      b   0x10036a1dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x480>
10036a3a4:      cmp w8, #0x1
10036a3a8:      b.ne    0x10036a49c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
10036a3ac:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10036a3b0:      add x1, x1, #0x87c
10036a3b4:      mov x0, x19
10036a3b8:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10036a3bc:      strb    wzr, [x19, #0x20]
10036a3c0:      ldr x8, [x19]
10036a3c4:      cbz x8, 0x100369f74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x218>
10036a3c8:      b   0x10036a420 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6c4>
10036a3cc:      cmp w8, #0x2
10036a3d0:      b.eq    0x10036a49c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
10036a3d4:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10036a3d8:      add x1, x1, #0x87c
10036a3dc:      mov x0, x19
10036a3e0:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10036a3e4:      strb    wzr, [x19, #0x20]
10036a3e8:      ldr x8, [x19]
10036a3ec:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10036a3f0:      cmp x8, x9
10036a3f4:      b.lo    0x10036a1fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x4a0>
10036a3f8:      b   0x10036a488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x72c>
10036a3fc:      cmp w8, #0x2
10036a400:      b.eq    0x10036a49c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
10036a404:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10036a408:      add x1, x1, #0x87c
10036a40c:      mov x0, x19
10036a410:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10036a414:      strb    wzr, [x19, #0x20]
10036a418:      ldr x8, [x19]
10036a41c:      cbz x8, 0x10036a2c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x564>
10036a420:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10036a424:      add x0, x0, #0xdf8
10036a428:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10036a42c:      cmp w8, #0x2
10036a430:      b.eq    0x10036a49c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
10036a434:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10036a438:      add x1, x1, #0x87c
10036a43c:      mov x0, x19
10036a440:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10036a444:      strb    wzr, [x19, #0x20]
10036a448:      ldr x8, [x19]
10036a44c:      cbz x8, 0x10036a328 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x5cc>
10036a450:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10036a454:      add x0, x0, #0xe58
10036a458:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10036a45c:      cmp w8, #0x2
10036a460:      b.eq    0x10036a49c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x740>
10036a464:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10036a468:      add x1, x1, #0x87c
10036a46c:      mov x0, x19
10036a470:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10036a474:      strb    wzr, [x19, #0x20]
10036a478:      ldr x8, [x19]
10036a47c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10036a480:      cmp x8, x9
10036a484:      b.lo    0x10036a08c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x330>
10036a488:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10036a48c:      add x0, x0, #0xdc8
10036a490:      bl  0x100c9de9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10036a494:      cmp w8, #0x2
10036a498:      b.ne    0x10036a4a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x74c>
10036a49c:      adrp    x0, 0x1010a3000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10036a4a0:      add x0, x0, #0xed8
10036a4a4:      bl  0x100ce071c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10036a4a8:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10036a4ac:      add x1, x1, #0x87c
10036a4b0:      mov x0, x19
10036a4b4:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10036a4b8:      strb    wzr, [x19, #0x20]
10036a4bc:      ldr x8, [x19]
10036a4c0:      cbz x8, 0x10036a148 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x3ec>
10036a4c4:      b   0x10036a450 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow+0x6f4>
10036a4c8:      adrp    x0, 0x100dd4000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.297+0x2d5b>
10036a4cc:      add x0, x0, #0xe03
10036a4d0:      mov w1, #0x21               ; =33
10036a4d4:      bl  0x10036b198 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
10036a4d8:      add x0, sp, #0x90
10036a4dc:      bl  0x10036b240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api24iterative_budget_message>
10036a4e0:      ldp x0, x1, [sp, #0x98]
10036a4e4:      bl  0x10036ab94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17throw_range_error>
