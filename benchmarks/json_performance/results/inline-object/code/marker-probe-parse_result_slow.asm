/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/marker-probe-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100767b70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow>:
100767b70:      sub sp, sp, #0x120
100767b74:      stp x28, x27, [sp, #0xc0]
100767b78:      stp x26, x25, [sp, #0xd0]
100767b7c:      stp x24, x23, [sp, #0xe0]
100767b80:      stp x22, x21, [sp, #0xf0]
100767b84:      stp x20, x19, [sp, #0x100]
100767b88:      stp x29, x30, [sp, #0x110]
100767b8c:      add x29, sp, #0x110
100767b90:      mov x21, x2
100767b94:      mov x19, x0
100767b98:      add x20, x1, #0x14
100767b9c:      cmp x2, #0x2
100767ba0:      b.ne    0x100767bb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x48>
100767ba4:      ldrh    w8, [x20]
100767ba8:      mov w9, #0x7d7b             ; =32123
100767bac:      cmp w8, w9
100767bb0:      b.eq    0x100767bf4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x84>
100767bb4:      b   0x100767ce0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x170>
100767bb8:      cmp x21, #0x3
100767bbc:      b.lo    0x100767ce0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x170>
100767bc0:      ldrb    w8, [x20]
100767bc4:      cmp w8, #0x20
100767bc8:      b.hi    0x100767c00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x90>
100767bcc:      mov x9, #0x2600             ; =9728
100767bd0:      movk    x9, #0x1, lsl #32
100767bd4:      lsr x9, x9, x8
100767bd8:      tbz w9, #0x0, 0x100767c00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x90>
100767bdc:      add x0, x1, #0x14
100767be0:      mov x22, x1
100767be4:      mov x1, x21
100767be8:      bl  0x10074e65c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty19padded_empty_object>
100767bec:      mov x1, x22
100767bf0:      tbz w0, #0x0, 0x100767c2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xbc>
100767bf4:      bl  0x10074e728 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty21allocate_empty_object>
100767bf8:      stp xzr, x0, [x19]
100767bfc:      b   0x100767f88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x418>
100767c00:      cmp w8, #0x7b
100767c04:      b.ne    0x100767c2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xbc>
100767c08:      ldrb    w8, [x1, #0x15]
100767c0c:      cmp w8, #0x20
100767c10:      b.hi    0x100767c24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xb4>
100767c14:      mov x9, #0x2600             ; =9728
100767c18:      movk    x9, #0x1, lsl #32
100767c1c:      lsr x9, x9, x8
100767c20:      tbnz    w9, #0x0, 0x100767bdc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x6c>
100767c24:      cmp w8, #0x7d
100767c28:      b.eq    0x100767bdc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x6c>
100767c2c:      cmp x21, #0x3e9
100767c30:      b.lo    0x100767ce0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x170>
100767c34:      add x0, x1, #0x14
100767c38:      mov x22, x1
100767c3c:      mov x1, x21
100767c40:      mov w2, #0x3e8              ; =1000
100767c44:      bl  0x100758e14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
100767c48:      mov x1, x22
100767c4c:      tbz w0, #0x0, 0x100767ce0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x170>
100767c50:      add x0, x1, #0x14
100767c54:      mov x1, x21
100767c58:      mov w2, #0xa120             ; =41248
100767c5c:      movk    w2, #0x7, lsl #16
100767c60:      bl  0x100758e14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
100767c64:      tbz w0, #0x0, 0x100768040 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4d0>
100767c68:      adrp    x8, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
100767c6c:      add x8, x8, #0xf80
100767c70:      adrp    x9, 0x100e07000 <_anon.fd7e678389f6d6013308189123b84ec8.2025+0x389>
100767c74:      add x9, x9, #0xd78
100767c78:      stp x9, x8, [x29, #-0x60]
100767c7c:      adrp    x0, 0x100ef1000 <_anon.fd7e678389f6d6013308189123b84ec8.630+0x48f>
100767c80:      add x0, x0, #0x33a
100767c84:      mov x8, sp
100767c88:      sub x1, x29, #0x60
100767c8c:      bl  0x100023808 <__RNvNvNtCsctvjasLqLe9_5alloc3fmt6format12format_inner>
100767c90:      ldr x20, [sp, #0x8]
100767c94:      ldr w1, [sp, #0x10]
100767c98:      mov x0, x20
100767c9c:      mov x2, x1
100767ca0:      bl  0x1007b6140 <_js_string_from_bytes_with_capacity>
100767ca4:      mov x3, x0
100767ca8:      adrp    x1, 0x100e03000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text17SB_X_USER_DEFINED+0x160>
100767cac:      add x1, x1, #0x5f5
100767cb0:      mov w21, #0x1               ; =1
100767cb4:      mov w0, #0x2                ; =2
100767cb8:      mov w2, #0xa                ; =10
100767cbc:      mov w4, #0x1                ; =1
100767cc0:      bl  0x10071c538 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
100767cc4:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100767cc8:      bfxil   x8, x0, #0, #48
100767ccc:      stp x21, x8, [x19]
100767cd0:      ldr x8, [sp]
100767cd4:      cbz x8, 0x100767f88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x418>
100767cd8:      mov x0, x20
100767cdc:      b   0x100767f84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x414>
100767ce0:      adrp    x0, 0x101130000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins7globals31STRUCTURED_CLONE_TRANSFER_STATE0023___RUST_STD_INTERNAL_VAL>
100767ce4:      add x0, x0, #0xc0
100767ce8:      ldr x8, [x0]
100767cec:      blr x8
100767cf0:      mov x20, x0
100767cf4:      ldrb    w8, [x0, #0x20]
100767cf8:      cbnz    w8, 0x10076810c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x59c>
100767cfc:      ldr x8, [x20]
100767d00:      cbnz    x8, 0x100768160 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5f0>
100767d04:      mov x23, #0x7fff000000000000 ; =9223090561878065152
100767d08:      bfxil   x23, x1, #0, #48
100767d0c:      mov x8, #-0x1               ; =-1
100767d10:      str x8, [x20]
100767d14:      mov x22, x20
100767d18:      ldr x8, [x22, #0x8]!
100767d1c:      ldr x24, [x20, #0x18]
100767d20:      cmp x24, x8
100767d24:      b.ne    0x100767d30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1c0>
100767d28:      mov x0, x22
100767d2c:      bl  0x100cb2c30 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
100767d30:      ldr x8, [x20, #0x10]
100767d34:      str x23, [x8, x24, lsl #3]
100767d38:      add x8, x24, #0x1
100767d3c:      str x8, [x20, #0x18]
100767d40:      ldr x8, [x20]
100767d44:      add x8, x8, #0x1
100767d48:      str x8, [x20]
100767d4c:      adrp    x0, 0x10112f000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime10event_pump11SPIN_STREAK0s_023___RUST_STD_INTERNAL_VAL+0x8>
100767d50:      add x0, x0, #0xfa0
100767d54:      ldr x8, [x0]
100767d58:      blr x8
100767d5c:      ldrb    w9, [x0]
100767d60:      strb    wzr, [x0]
100767d64:      adrp    x23, 0x101130000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins7globals31STRUCTURED_CLONE_TRANSFER_STATE0023___RUST_STD_INTERNAL_VAL>
100767d68:      add x23, x23, #0x228
100767d6c:      cbz w9, 0x100767da8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x238>
100767d70:      mov x8, x0
100767d74:      ldr x9, [x23]
100767d78:      mov x0, x23
100767d7c:      blr x9
100767d80:      ldrb    w9, [x0]
100767d84:      tst w9, #0x3
100767d88:      b.ne    0x100767da0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x230>
100767d8c:      adrp    x9, 0x101205000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x120>
100767d90:      add x9, x9, #0x8a8
100767d94:      ldapr   w9, [x9]
100767d98:      cmp w9, #0x0
100767d9c:      b.le    0x100767fa8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x438>
100767da0:      mov w9, #0x1                ; =1
100767da4:      strb    w9, [x8]
100767da8:      bl  0x100747fc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
100767dac:      bl  0x1007479d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
100767db0:      ldrb    w8, [x20, #0x20]
100767db4:      cbnz    w8, 0x100768008 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x498>
100767db8:      ldr x8, [x20]
100767dbc:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100767dc0:      cmp x8, x9
100767dc4:      b.hs    0x100768034 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4c4>
100767dc8:      add x9, x8, #0x1
100767dcc:      str x9, [x20]
100767dd0:      ldr x10, [x20, #0x18]
100767dd4:      mov w9, #0x1                ; =1
100767dd8:      cmp x24, x10
100767ddc:      b.hs    0x100767df0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x280>
100767de0:      ldr x10, [x20, #0x10]
100767de4:      ldr x10, [x10, x24, lsl #3]
100767de8:      and x10, x10, #0xffffffffffff
100767dec:      b   0x100767df4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x284>
100767df0:      mov w10, #0x1               ; =1
100767df4:      str x8, [x20]
100767df8:      add x8, x10, #0x14
100767dfc:      movi.2d v0, #0000000000000000
100767e00:      stur    q0, [sp, #0x78]
100767e04:      stur    q0, [sp, #0x68]
100767e08:      stur    q0, [sp, #0x58]
100767e0c:      stur    q0, [sp, #0x48]
100767e10:      strb    w9, [sp, #0x90]
100767e14:      mov x9, #-0x1               ; =-1
100767e18:      stp x8, x21, [sp, #0x28]
100767e1c:      str x9, [sp]
100767e20:      stp xzr, xzr, [sp, #0x38]
100767e24:      str xzr, [sp, #0x88]
100767e28:      mov x0, sp
100767e2c:      bl  0x10070e500 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
100767e30:      mov x21, x0
100767e34:      ldp x8, x9, [sp, #0x30]
100767e38:      cmp x9, x8
100767e3c:      b.hs    0x100767e70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x300>
100767e40:      ldr x10, [sp, #0x28]
100767e44:      mov x11, #0x2600            ; =9728
100767e48:      movk    x11, #0x1, lsl #32
100767e4c:      ldrb    w12, [x10, x9]
100767e50:      cmp w12, #0x20
100767e54:      b.hi    0x100767e70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x300>
100767e58:      lsr x12, x11, x12
100767e5c:      tbz w12, #0x0, 0x100767e70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x300>
100767e60:      add x9, x9, #0x1
100767e64:      cmp x8, x9
100767e68:      b.ne    0x100767e4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2dc>
100767e6c:      mov x9, x8
100767e70:      ldrb    w25, [sp, #0x90]
100767e74:      cmp x9, x8
100767e78:      cset    w26, eq
100767e7c:      ldrb    w8, [x20, #0x20]
100767e80:      cbnz    w8, 0x10076813c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5cc>
100767e84:      ldr x8, [x20]
100767e88:      cbnz    x8, 0x100768160 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5f0>
100767e8c:      mov x8, #-0x1               ; =-1
100767e90:      str x8, [x20]
100767e94:      ldr x27, [x20, #0x18]
100767e98:      ldr x8, [x20, #0x8]
100767e9c:      cmp x27, x8
100767ea0:      b.ne    0x100767eac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x33c>
100767ea4:      mov x0, x22
100767ea8:      bl  0x100cb2c30 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
100767eac:      ldr x8, [x20, #0x10]
100767eb0:      str x21, [x8, x27, lsl #3]
100767eb4:      add x8, x27, #0x1
100767eb8:      str x8, [x20, #0x18]
100767ebc:      ldr x8, [x20]
100767ec0:      add x8, x8, #0x1
100767ec4:      str x8, [x20]
100767ec8:      ldr x8, [x23]
100767ecc:      mov x0, x23
100767ed0:      blr x8
100767ed4:      ldrb    w8, [x0]
100767ed8:      and w8, w8, #0xfffffffd
100767edc:      strb    w8, [x0]
100767ee0:      bl  0x100748930 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy22gc_bump_malloc_trigger>
100767ee4:      bl  0x10074d9fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy49gc_schedule_parse_boundary_collection_if_pressure>
100767ee8:      ldrb    w8, [x20, #0x20]
100767eec:      cbnz    w8, 0x10076816c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5fc>
100767ef0:      ldr x8, [x20]
100767ef4:      cbnz    x8, 0x10076819c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x62c>
100767ef8:      and w22, w26, w25
100767efc:      ldr x8, [x20, #0x18]
100767f00:      cmp x24, x8
100767f04:      b.hi    0x100767f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x39c>
100767f08:      str x24, [x20, #0x18]
100767f0c:      adrp    x0, 0x1010bf000 <_anon.fd7e678389f6d6013308189123b84ec8.144+0x50>
100767f10:      add x0, x0, #0x318
100767f14:      bl  0x10013985c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api17parse_result_slows_0uEB2P_>
100767f18:      tbz w22, #0x0, 0x100767f30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x3c0>
100767f1c:      stp xzr, x21, [x19]
100767f20:      ldr x8, [sp]
100767f24:      cmn x8, #0x1
100767f28:      b.ne    0x100767f7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x40c>
100767f2c:      b   0x100767f88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x418>
100767f30:      adrp    x0, 0x100e04000 <_anon.fd7e678389f6d6013308189123b84ec8.899+0x2>
100767f34:      add x0, x0, #0x5a1
100767f38:      mov w1, #0x21               ; =33
100767f3c:      mov w2, #0x21               ; =33
100767f40:      bl  0x1007b6140 <_js_string_from_bytes_with_capacity>
100767f44:      mov x3, x0
100767f48:      adrp    x1, 0x100e03000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text17SB_X_USER_DEFINED+0x160>
100767f4c:      add x1, x1, #0x60d
100767f50:      mov w20, #0x1               ; =1
100767f54:      mov w0, #0x4                ; =4
100767f58:      mov w2, #0xb                ; =11
100767f5c:      mov w4, #0x1                ; =1
100767f60:      bl  0x10071c538 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
100767f64:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100767f68:      bfxil   x8, x0, #0, #48
100767f6c:      stp x20, x8, [x19]
100767f70:      ldr x8, [sp]
100767f74:      cmn x8, #0x1
100767f78:      b.eq    0x100767f88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x418>
100767f7c:      cbz x8, 0x100767f88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x418>
100767f80:      ldr x0, [sp, #0x8]
100767f84:      bl  0x100cda940 <_mi_free>
100767f88:      ldp x29, x30, [sp, #0x110]
100767f8c:      ldp x20, x19, [sp, #0x100]
100767f90:      ldp x22, x21, [sp, #0xf0]
100767f94:      ldp x24, x23, [sp, #0xe0]
100767f98:      ldp x26, x25, [sp, #0xd0]
100767f9c:      ldp x28, x27, [sp, #0xc0]
100767fa0:      add sp, sp, #0x120
100767fa4:      ret
100767fa8:      adrp    x0, 0x10112e000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime6object40ASYNC_GENERATOR_INTRINSIC_PROTO_PTR_SLOT7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
100767fac:      add x0, x0, #0x8d8
100767fb0:      ldr x8, [x0]
100767fb4:      blr x8
100767fb8:      ldr x8, [x0]
100767fbc:      adrp    x0, 0x101130000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins7globals31STRUCTURED_CLONE_TRANSFER_STATE0023___RUST_STD_INTERNAL_VAL>
100767fc0:      add x0, x0, #0x1b0
100767fc4:      ldr x9, [x0]
100767fc8:      blr x9
100767fcc:      ldr x9, [x0]
100767fd0:      cmp x9, x8
100767fd4:      b.ls    0x100767ff4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x484>
100767fd8:      str x8, [x0]
100767fdc:      adrp    x0, 0x101130000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins7globals31STRUCTURED_CLONE_TRANSFER_STATE0023___RUST_STD_INTERNAL_VAL>
100767fe0:      add x0, x0, #0xf0
100767fe4:      ldr x8, [x0]
100767fe8:      blr x8
100767fec:      mov w8, #0x1                ; =1
100767ff0:      strb    w8, [x0]
100767ff4:      bl  0x100747fc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
100767ff8:      bl  0x100747fc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
100767ffc:      bl  0x1007479d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
100768000:      ldrb    w8, [x20, #0x20]
100768004:      cbz w8, 0x100767db8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x248>
100768008:      cmp w8, #0x2
10076800c:      b.eq    0x100768174 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x604>
100768010:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
100768014:      add x1, x1, #0x850
100768018:      mov x0, x20
10076801c:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100768020:      strb    wzr, [x20, #0x20]
100768024:      ldr x8, [x20]
100768028:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10076802c:      cmp x8, x9
100768030:      b.lo    0x100767dc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x258>
100768034:      adrp    x0, 0x101098000 <_anon.68a532d94142320e15103d7866c451bd.21>
100768038:      add x0, x0, #0xd98
10076803c:      bl  0x100c8d25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100768040:      stur    x21, [x29, #-0x78]
100768044:      mov x8, #0x7fff000000000000 ; =9223090561878065152
100768048:      bfxil   x8, x22, #0, #48
10076804c:      str x8, [sp]
100768050:      adrp    x22, 0x1010bf000 <_anon.fd7e678389f6d6013308189123b84ec8.144+0x50>
100768054:      add x22, x22, #0x308
100768058:      mov x1, sp
10076805c:      mov x0, x22
100768060:      bl  0x100137110 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json15parse_root_push0jEB2h_>
100768064:      mov x23, x0
100768068:      stp x0, x20, [x29, #-0x70]
10076806c:      stur    x21, [x29, #-0x60]
100768070:      sub x8, x29, #0x68
100768074:      sub x9, x29, #0x60
100768078:      stp x8, x9, [sp]
10076807c:      sub x8, x29, #0x70
100768080:      sub x9, x29, #0x78
100768084:      stp x8, x9, [sp, #0x10]
100768088:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
10076808c:      add x0, x0, #0xa68
100768090:      mov x1, sp
100768094:      bl  0x10012c630 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawIB1t_NtNtNtB1S_5value7jsvalue7JSValueENCINvB1Q_19with_built_tape_rawB3o_NCNvNtNtB1S_4json9parse_api24try_parse_deep_iterative0E0E0IB1t_B3o_EEB1S_>
100768098:      mov x21, x0
10076809c:      mov x20, x1
1007680a0:      str x23, [sp]
1007680a4:      mov x1, sp
1007680a8:      mov x0, x22
1007680ac:      bl  0x1001371a8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json18parse_root_restore0uEB2h_>
1007680b0:      adrp    x0, 0x1010bf000 <_anon.fd7e678389f6d6013308189123b84ec8.144+0x50>
1007680b4:      add x0, x0, #0x318
1007680b8:      bl  0x1001399c4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api24try_parse_deep_iteratives_0uEB2P_>
1007680bc:      tbnz    w21, #0x0, 0x100768100 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x590>
1007680c0:      adrp    x0, 0x100dfd000 <_anon.4ff118d01ccdc9bd41517af7abf33093.1077+0xe2>
1007680c4:      add x0, x0, #0xaf6
1007680c8:      mov w1, #0x29               ; =41
1007680cc:      mov w2, #0x29               ; =41
1007680d0:      bl  0x1007b6140 <_js_string_from_bytes_with_capacity>
1007680d4:      mov x3, x0
1007680d8:      adrp    x1, 0x100e03000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text17SB_X_USER_DEFINED+0x160>
1007680dc:      add x1, x1, #0x60d
1007680e0:      mov w21, #0x1               ; =1
1007680e4:      mov w0, #0x4                ; =4
1007680e8:      mov w2, #0xb                ; =11
1007680ec:      mov w4, #0x1                ; =1
1007680f0:      bl  0x10071c538 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
1007680f4:      mov x20, #0x7ffd000000000000 ; =9222527611924643840
1007680f8:      bfxil   x20, x0, #0, #48
1007680fc:      b   0x100768104 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x594>
100768100:      mov x21, #0x0               ; =0
100768104:      stp x21, x20, [x19]
100768108:      b   0x100767f88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x418>
10076810c:      cmp w8, #0x1
100768110:      b.ne    0x100768174 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x604>
100768114:      mov x22, x1
100768118:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
10076811c:      add x1, x1, #0x850
100768120:      mov x0, x20
100768124:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100768128:      strb    wzr, [x20, #0x20]
10076812c:      mov x1, x22
100768130:      ldr x8, [x20]
100768134:      cbz x8, 0x100767d04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x194>
100768138:      b   0x100768160 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5f0>
10076813c:      cmp w8, #0x2
100768140:      b.eq    0x100768174 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x604>
100768144:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
100768148:      add x1, x1, #0x850
10076814c:      mov x0, x20
100768150:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100768154:      strb    wzr, [x20, #0x20]
100768158:      ldr x8, [x20]
10076815c:      cbz x8, 0x100767e8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x31c>
100768160:      adrp    x0, 0x101098000 <_anon.68a532d94142320e15103d7866c451bd.21>
100768164:      add x0, x0, #0xdc8
100768168:      bl  0x100c8d22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10076816c:      cmp w8, #0x2
100768170:      b.ne    0x100768180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x610>
100768174:      adrp    x0, 0x101097000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
100768178:      add x0, x0, #0xed8
10076817c:      bl  0x100cd3f9c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
100768180:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
100768184:      add x1, x1, #0x850
100768188:      mov x0, x20
10076818c:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100768190:      strb    wzr, [x20, #0x20]
100768194:      ldr x8, [x20]
100768198:      cbz x8, 0x100767ef8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x388>
10076819c:      adrp    x0, 0x101098000 <_anon.68a532d94142320e15103d7866c451bd.21>
1007681a0:      add x0, x0, #0xe28
1007681a4:      bl  0x100c8d22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
