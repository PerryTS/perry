/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/inline-object-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100821ae4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate>:
100821ae4:      sub sp, sp, #0x90
100821ae8:      stp x28, x27, [sp, #0x30]
100821aec:      stp x26, x25, [sp, #0x40]
100821af0:      stp x24, x23, [sp, #0x50]
100821af4:      stp x22, x21, [sp, #0x60]
100821af8:      stp x20, x19, [sp, #0x70]
100821afc:      stp x29, x30, [sp, #0x80]
100821b00:      add x29, sp, #0x80
100821b04:      mov x25, x0
100821b08:      adrp    x0, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
100821b0c:      add x0, x0, #0xc18
100821b10:      mov x1, x25
100821b14:      bl  0x1001338f8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime4json20ParseShapeCacheEntryEEE4withNCNvNtB24_19parse_inline_object8allocate0INtNtBZ_6option6OptionONtNtNtB26_5array6header11ArrayHeaderEEB26_>
100821b18:      cmp x0, #0x1
100821b1c:      b.ne    0x100821b68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x84>
100821b20:      adrp    x24, 0x101124000 <_perry_global_baseline_worker_ts__1>
100821b24:      add x24, x24, #0xe70
100821b28:      ldr x8, [x24]
100821b2c:      cmn x8, #0x1
100821b30:      b.eq    0x100821b70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x8c>
100821b34:      mrs x9, TPIDRRO_EL0
100821b38:      and x9, x9, #0xfffffffffffffff8
100821b3c:      ldr x8, [x9, x8, lsl #3]
100821b40:      cbz x8, 0x100821b70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x8c>
100821b44:      ldr x0, [x8, #0x19e8]
100821b48:      cbz x0, 0x100822018 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x534>
100821b4c:      ldr x8, [x0]
100821b50:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100821b54:      cmp x8, x9
100821b58:      b.lo    0x100821b98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xb4>
100821b5c:      adrp    x0, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
100821b60:      add x0, x0, #0x658
100821b64:      bl  0x100c8b25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100821b68:      mov x0, #0x0                ; =0
100821b6c:      b   0x10082248c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x9a8>
100821b70:      adrp    x0, 0x10112a000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc8tenuring17NURSERY_CAP_SCALE0s_023___RUST_STD_INTERNAL_VAL+0x10>
100821b74:      add x0, x0, #0x668
100821b78:      ldr x8, [x0]
100821b7c:      blr x8
100821b80:      ldrb    w8, [x0, #0x20]
100821b84:      cbnz    w8, 0x1008225d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xaf0>
100821b88:      ldr x8, [x0]
100821b8c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100821b90:      cmp x8, x9
100821b94:      b.hs    0x1008224ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x9c8>
100821b98:      ldr x19, [x0, #0x18]
100821b9c:      mov x21, #0x7ffd000000000000 ; =9222527611924643840
100821ba0:      stp x1, x21, [sp, #0x20]
100821ba4:      mov w8, #0x1                ; =1
100821ba8:      str x8, [sp, #0x18]
100821bac:      add x0, sp, #0x18
100821bb0:      bl  0x1007de328 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
100821bb4:      mov x22, x0
100821bb8:      adrp    x0, 0x10112c000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy29GC_SAFEPOINT_DEFER_ARENA_BASE0s_023___RUST_STD_INTERNAL_VAL>
100821bbc:      add x0, x0, #0xa8
100821bc0:      ldr x8, [x0]
100821bc4:      blr x8
100821bc8:      ldrb    w9, [x0]
100821bcc:      strb    wzr, [x0]
100821bd0:      cbz w9, 0x100821c10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x12c>
100821bd4:      mov x8, x0
100821bd8:      adrp    x0, 0x10112c000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy29GC_SAFEPOINT_DEFER_ARENA_BASE0s_023___RUST_STD_INTERNAL_VAL>
100821bdc:      add x0, x0, #0x270
100821be0:      ldr x9, [x0]
100821be4:      blr x9
100821be8:      ldrb    w9, [x0]
100821bec:      tst w9, #0x3
100821bf0:      b.ne    0x100821c08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x124>
100821bf4:      adrp    x9, 0x101201000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xf6e8>
100821bf8:      add x9, x9, #0x918
100821bfc:      ldapr   w9, [x9]
100821c00:      cmp w9, #0x0
100821c04:      b.le    0x10082203c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x558>
100821c08:      mov w9, #0x1                ; =1
100821c0c:      strb    w9, [x8]
100821c10:      ldr x20, [x25, #0x80]
100821c14:      mov w0, #0x0                ; =0
100821c18:      mov w1, #0x0                ; =0
100821c1c:      mov x2, x20
100821c20:      bl  0x1006f8640 <_js_object_alloc_with_parent>
100821c24:      stp x0, x21, [sp, #0x20]
100821c28:      mov w8, #0x1                ; =1
100821c2c:      str x8, [sp, #0x18]
100821c30:      add x0, sp, #0x18
100821c34:      bl  0x1007de328 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
100821c38:      mov x21, x0
100821c3c:      ldr x8, [x24]
100821c40:      cmn x8, #0x1
100821c44:      b.eq    0x100821ca8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x1c4>
100821c48:      mrs x9, TPIDRRO_EL0
100821c4c:      and x9, x9, #0xfffffffffffffff8
100821c50:      ldr x8, [x9, x8, lsl #3]
100821c54:      cbz x8, 0x100821ca8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x1c4>
100821c58:      ldr x9, [x8, #0x19e8]
100821c5c:      cbz x9, 0x100821ca8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x1c4>
100821c60:      ldr x10, [x9]
100821c64:      mov x8, #0x7fffffffffffffff ; =9223372036854775807
100821c68:      cmp x10, x8
100821c6c:      b.hs    0x1008224b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x9d4>
100821c70:      add x8, x10, #0x1
100821c74:      str x8, [x9]
100821c78:      ldr x8, [x9, #0x18]
100821c7c:      cmp x22, x8
100821c80:      b.hs    0x1008223ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x908>
100821c84:      ldr x8, [x9, #0x10]
100821c88:      mov w11, #0x18              ; =24
100821c8c:      madd    x8, x22, x11, x8
100821c90:      ldr x11, [x8]
100821c94:      cmp x11, #0x1
100821c98:      b.ne    0x1008224c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x9e0>
100821c9c:      ldr x8, [x8, #0x8]
100821ca0:      str x10, [x9]
100821ca4:      b   0x100821d04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x220>
100821ca8:      adrp    x0, 0x10112a000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc8tenuring17NURSERY_CAP_SCALE0s_023___RUST_STD_INTERNAL_VAL+0x10>
100821cac:      add x0, x0, #0x668
100821cb0:      ldr x8, [x0]
100821cb4:      blr x8
100821cb8:      ldrb    w8, [x0, #0x20]
100821cbc:      cbnz    w8, 0x1008224d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x9f0>
100821cc0:      ldr x9, [x0]
100821cc4:      mov x8, #0x7fffffffffffffff ; =9223372036854775807
100821cc8:      cmp x9, x8
100821ccc:      b.hs    0x100822584 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xaa0>
100821cd0:      add x8, x9, #0x1
100821cd4:      str x8, [x0]
100821cd8:      ldr x8, [x0, #0x18]
100821cdc:      cmp x22, x8
100821ce0:      b.hs    0x1008223ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x908>
100821ce4:      ldr x8, [x0, #0x10]
100821ce8:      mov w10, #0x18              ; =24
100821cec:      madd    x8, x22, x10, x8
100821cf0:      ldr x10, [x8]
100821cf4:      cmp x10, #0x1
100821cf8:      b.ne    0x1008223f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x90c>
100821cfc:      ldr x8, [x8, #0x8]
100821d00:      str x9, [x0]
100821d04:      mov x0, x8
100821d08:      mov x1, x20
100821d0c:      mov x2, x20
100821d10:      mov x3, #0x0                ; =0
100821d14:      mov w4, #0x0                ; =0
100821d18:      mov w5, #0x0                ; =0
100821d1c:      bl  0x10036c700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object6shapes34shape_descriptor_ensure_with_holes>
100821d20:      mov x23, x0
100821d24:      tbnz    w23, #0x0, 0x100822418 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x934>
100821d28:      ldr x8, [x24]
100821d2c:      cmn x8, #0x1
100821d30:      b.eq    0x100821da4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x2c0>
100821d34:      mrs x9, TPIDRRO_EL0
100821d38:      and x9, x9, #0xfffffffffffffff8
100821d3c:      ldr x8, [x9, x8, lsl #3]
100821d40:      cbz x8, 0x100821da4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x2c0>
100821d44:      ldr x8, [x8, #0x19e8]
100821d48:      cbz x8, 0x100821da4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x2c0>
100821d4c:      ldr x9, [x8]
100821d50:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100821d54:      cmp x9, x10
100821d58:      b.hs    0x1008224b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x9d4>
100821d5c:      add x10, x9, #0x1
100821d60:      str x10, [x8]
100821d64:      ldr x10, [x8, #0x18]
100821d68:      cmp x21, x10
100821d6c:      b.hs    0x1008223ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x908>
100821d70:      ldr x10, [x8, #0x10]
100821d74:      mov w11, #0x18              ; =24
100821d78:      madd    x10, x21, x11, x10
100821d7c:      ldr x11, [x10]
100821d80:      cmp x11, #0x1
100821d84:      b.ne    0x1008224c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x9e0>
100821d88:      ldr x21, [x10, #0x8]
100821d8c:      str x9, [x8]
100821d90:      ldr x8, [x24]
100821d94:      cmn x8, #0x1
100821d98:      stp x25, x20, [sp, #0x8]
100821d9c:      b.ne    0x100821e10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x32c>
100821da0:      b   0x100821e74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x390>
100821da4:      adrp    x0, 0x10112a000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc8tenuring17NURSERY_CAP_SCALE0s_023___RUST_STD_INTERNAL_VAL+0x10>
100821da8:      add x0, x0, #0x668
100821dac:      ldr x8, [x0]
100821db0:      blr x8
100821db4:      ldrb    w8, [x0, #0x20]
100821db8:      cbnz    w8, 0x100822508 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xa24>
100821dbc:      ldr x8, [x0]
100821dc0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100821dc4:      cmp x8, x9
100821dc8:      b.hs    0x100822584 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xaa0>
100821dcc:      add x9, x8, #0x1
100821dd0:      str x9, [x0]
100821dd4:      ldr x9, [x0, #0x18]
100821dd8:      cmp x21, x9
100821ddc:      b.hs    0x1008223ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x908>
100821de0:      ldr x9, [x0, #0x10]
100821de4:      mov w10, #0x18              ; =24
100821de8:      madd    x9, x21, x10, x9
100821dec:      ldr x10, [x9]
100821df0:      cmp x10, #0x1
100821df4:      b.ne    0x1008223f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x90c>
100821df8:      ldr x21, [x9, #0x8]
100821dfc:      str x8, [x0]
100821e00:      ldr x8, [x24]
100821e04:      cmn x8, #0x1
100821e08:      stp x25, x20, [sp, #0x8]
100821e0c:      b.eq    0x100821e74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x390>
100821e10:      mrs x9, TPIDRRO_EL0
100821e14:      and x9, x9, #0xfffffffffffffff8
100821e18:      ldr x8, [x9, x8, lsl #3]
100821e1c:      cbz x8, 0x100821e74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x390>
100821e20:      ldr x8, [x8, #0x19e8]
100821e24:      cbz x8, 0x100821e74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x390>
100821e28:      ldr x9, [x8]
100821e2c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100821e30:      cmp x9, x10
100821e34:      b.hs    0x1008224b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x9d4>
100821e38:      add x10, x9, #0x1
100821e3c:      str x10, [x8]
100821e40:      ldr x10, [x8, #0x18]
100821e44:      cmp x22, x10
100821e48:      b.hs    0x1008223ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x908>
100821e4c:      ldr x10, [x8, #0x10]
100821e50:      mov w11, #0x18              ; =24
100821e54:      madd    x10, x22, x11, x10
100821e58:      ldr x11, [x10]
100821e5c:      cmp x11, #0x1
100821e60:      b.ne    0x1008224c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x9e0>
100821e64:      mov x20, x24
100821e68:      ldr x22, [x10, #0x8]
100821e6c:      str x9, [x8]
100821e70:      b   0x100821ed4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x3f0>
100821e74:      adrp    x0, 0x10112a000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc8tenuring17NURSERY_CAP_SCALE0s_023___RUST_STD_INTERNAL_VAL+0x10>
100821e78:      add x0, x0, #0x668
100821e7c:      ldr x8, [x0]
100821e80:      blr x8
100821e84:      ldrb    w8, [x0, #0x20]
100821e88:      cbnz    w8, 0x10082254c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xa68>
100821e8c:      ldr x8, [x0]
100821e90:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100821e94:      cmp x8, x9
100821e98:      b.hs    0x100822584 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xaa0>
100821e9c:      add x9, x8, #0x1
100821ea0:      str x9, [x0]
100821ea4:      ldr x9, [x0, #0x18]
100821ea8:      cmp x22, x9
100821eac:      b.hs    0x1008223ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x908>
100821eb0:      ldr x9, [x0, #0x10]
100821eb4:      mov w10, #0x18              ; =24
100821eb8:      madd    x9, x22, x10, x9
100821ebc:      ldr x10, [x9]
100821ec0:      cmp x10, #0x1
100821ec4:      b.ne    0x1008223f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x90c>
100821ec8:      mov x20, x24
100821ecc:      ldr x22, [x9, #0x8]
100821ed0:      str x8, [x0]
100821ed4:      cbz x21, 0x1008220c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5e0>
100821ed8:      lsr x27, x23, #32
100821edc:      adrp    x8, 0x101125000 <__MergedGlobals+0xd8>
100821ee0:      add x8, x8, #0x95c
100821ee4:      ldr w25, [x8]
100821ee8:      cmp w25, #0x300
100821eec:      b.hs    0x1008220a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5bc>
100821ef0:      ldr x8, [x20]
100821ef4:      cmn x8, #0x1
100821ef8:      b.eq    0x100822090 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5ac>
100821efc:      mrs x9, TPIDRRO_EL0
100821f00:      and x9, x9, #0xfffffffffffffff8
100821f04:      ldr x0, [x9, x8, lsl #3]
100821f08:      cbz x0, 0x100822090 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5ac>
100821f0c:      add x8, x0, x25, lsl #3
100821f10:      ldr x0, [x8, #0x1e8]
100821f14:      cbz x0, 0x1008220a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5bc>
100821f18:      ldr x0, [x0]
100821f1c:      cbz x0, 0x1008220b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5d0>
100821f20:      mov w8, #-0x40000001        ; =-1073741825
100821f24:      cmp w27, w8
100821f28:      b.gt    0x1008220c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5e0>
100821f2c:      ldr x9, [x0, #0x5198]
100821f30:      ubfx    x8, x23, #47, #15
100821f34:      cmp x8, x9
100821f38:      b.hs    0x1008220c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5e0>
100821f3c:      ldr x9, [x0, #0x5190]
100821f40:      ldr x8, [x9, x8, lsl #3]
100821f44:      cbz x8, 0x1008220c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5e0>
100821f48:      ubfx    x9, x23, #37, #10
100821f4c:      ldr x8, [x8, x9, lsl #3]
100821f50:      cbz x8, 0x1008220c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5e0>
100821f54:      ubfx    x9, x23, #32, #5
100821f58:      add x23, x8, x9, lsl #5
100821f5c:      ldrb    w25, [x23, #0x1c]
100821f60:      tbz w25, #0x0, 0x1008220c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5e0>
100821f64:      str x19, [sp]
100821f68:      ldp w28, w26, [x23, #0x10]
100821f6c:      ldp x19, x24, [x23]
100821f70:      cbz x22, 0x100821f8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x4a8>
100821f74:      mov x0, x22
100821f78:      bl  0x1003a4340 <_keys_array_len_capped_to_capacity>
100821f7c:      cmp x19, x22
100821f80:      ldr x19, [sp]
100821f84:      b.eq    0x100821f9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x4b8>
100821f88:      b   0x1008220c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5e0>
100821f8c:      mov w0, #0x0                ; =0
100821f90:      cmp x19, x22
100821f94:      ldr x19, [sp]
100821f98:      b.ne    0x1008220c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5e0>
100821f9c:      tbnz    w25, #0x5, 0x1008220c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5e0>
100821fa0:      cbnz    x24, 0x1008220c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5e0>
100821fa4:      ldr x8, [sp, #0x10]
100821fa8:      cmp w26, w8
100821fac:      b.ne    0x1008220c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5e0>
100821fb0:      cmp w28, w0
100821fb4:      b.ne    0x1008220c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5e0>
100821fb8:      str w27, [x21, #0x4]
100821fbc:      mov x24, x20
100821fc0:      ldr x8, [x20]
100821fc4:      cmn x8, #0x1
100821fc8:      b.eq    0x1008220dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5f8>
100821fcc:      mrs x9, TPIDRRO_EL0
100821fd0:      and x9, x9, #0xfffffffffffffff8
100821fd4:      ldr x0, [x9, x8, lsl #3]
100821fd8:      cbz x0, 0x1008220dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5f8>
100821fdc:      lsr x1, x21, #20
100821fe0:      ldr x8, [x0, #0x10]
100821fe4:      ldrb    w9, [x8, #0x28]
100821fe8:      ldp x20, x25, [sp, #0x8]
100821fec:      tbz w9, #0x0, 0x1008220f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x610>
100821ff0:      ldr x9, [x8, #0x20]
100821ff4:      cmp x9, x1
100821ff8:      b.ne    0x1008220f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x610>
100821ffc:      ldr x9, [x8]
100822000:      cmp x9, x21
100822004:      b.hi    0x1008220f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x610>
100822008:      ldr x9, [x8, #0x8]
10082200c:      cmp x9, x21
100822010:      b.hi    0x100822124 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x640>
100822014:      b   0x1008220f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x610>
100822018:      adrp    x0, 0x10112a000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc8tenuring17NURSERY_CAP_SCALE0s_023___RUST_STD_INTERNAL_VAL+0x10>
10082201c:      add x0, x0, #0x668
100822020:      ldr x8, [x0]
100822024:      blr x8
100822028:      ldrb    w8, [x0, #0x20]
10082202c:      cbz w8, 0x100821b88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xa4>
100822030:      cmp w8, #0x2
100822034:      b.ne    0x1008225dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xaf8>
100822038:      b   0x100822600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xb1c>
10082203c:      adrp    x0, 0x10112a000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc8tenuring17NURSERY_CAP_SCALE0s_023___RUST_STD_INTERNAL_VAL+0x10>
100822040:      add x0, x0, #0x608
100822044:      ldr x8, [x0]
100822048:      blr x8
10082204c:      ldr x8, [x0]
100822050:      adrp    x0, 0x10112b000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json8raw_json12RAW_JSON_KEY0s_023___RUST_STD_INTERNAL_VAL+0x8>
100822054:      add x0, x0, #0xf58
100822058:      ldr x9, [x0]
10082205c:      blr x9
100822060:      ldr x9, [x0]
100822064:      cmp x9, x8
100822068:      b.ls    0x100822088 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x5a4>
10082206c:      str x8, [x0]
100822070:      adrp    x0, 0x10112c000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy29GC_SAFEPOINT_DEFER_ARENA_BASE0s_023___RUST_STD_INTERNAL_VAL>
100822074:      add x0, x0, #0x1e0
100822078:      ldr x8, [x0]
10082207c:      blr x8
100822080:      mov w8, #0x1                ; =1
100822084:      strb    w8, [x0]
100822088:      bl  0x100813240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
10082208c:      b   0x100821c10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x12c>
100822090:      bl  0x100cb5b38 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100822094:      add x8, x0, x25, lsl #3
100822098:      ldr x0, [x8, #0x1e8]
10082209c:      cbnz    x0, 0x100821f18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x434>
1008220a0:      adrp    x0, 0x1010d2000 <_anon.47db62074bb938d3ace44517495b1cd3.1235+0xa98>
1008220a4:      add x0, x0, #0x728
1008220a8:      bl  0x100cb5794 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1008220ac:      ldr x0, [x0]
1008220b0:      cbnz    x0, 0x100821f20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x43c>
1008220b4:      bl  0x100cc56c0 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
1008220b8:      mov w8, #-0x40000001        ; =-1073741825
1008220bc:      cmp w27, w8
1008220c0:      b.le    0x100821f2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x448>
1008220c4:      adrp    x0, 0x100e0b000 <_anon.c2a4b59a01bfaad05414bd5c213a645e.880+0x102>
1008220c8:      add x0, x0, #0x367
1008220cc:      adrp    x2, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
1008220d0:      add x2, x2, #0xfa8
1008220d4:      mov w1, #0x75               ; =117
1008220d8:      bl  0x100c8b4e8 <__RNvNtCsjgY6bXVaRmE_4core9panicking5panic>
1008220dc:      bl  0x100cb5b38 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1008220e0:      lsr x1, x21, #20
1008220e4:      ldr x8, [x0, #0x10]
1008220e8:      ldrb    w9, [x8, #0x28]
1008220ec:      ldp x20, x25, [sp, #0x8]
1008220f0:      tbnz    w9, #0x0, 0x100821ff0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x50c>
1008220f4:      ldrb    w9, [x8, #0x58]
1008220f8:      cbz w9, 0x10082213c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x658>
1008220fc:      ldr x9, [x8, #0x50]
100822100:      cmp x9, x1
100822104:      b.ne    0x10082213c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x658>
100822108:      ldr x9, [x8, #0x30]
10082210c:      cmp x9, x21
100822110:      b.hi    0x10082213c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x658>
100822114:      ldr x9, [x8, #0x38]
100822118:      cmp x9, x21
10082211c:      b.ls    0x10082213c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x658>
100822120:      add x8, x8, #0x30
100822124:      adrp    x26, 0x101125000 <__MergedGlobals+0xd8>
100822128:      add x26, x26, #0x95c
10082212c:      ldrb    w9, [x8, #0x19]
100822130:      cmp w9, #0xff
100822134:      b.ne    0x1008221c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x6dc>
100822138:      b   0x1008221d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x6f0>
10082213c:      ldrb    w9, [x8, #0x88]
100822140:      adrp    x26, 0x101125000 <__MergedGlobals+0xd8>
100822144:      add x26, x26, #0x95c
100822148:      cbz w9, 0x100822184 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x6a0>
10082214c:      ldr x9, [x8, #0x80]
100822150:      cmp x9, x1
100822154:      b.ne    0x100822184 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x6a0>
100822158:      ldr x9, [x8, #0x60]
10082215c:      cmp x9, x21
100822160:      b.hi    0x100822184 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x6a0>
100822164:      ldr x9, [x8, #0x68]
100822168:      cmp x9, x21
10082216c:      b.ls    0x100822184 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x6a0>
100822170:      add x8, x8, #0x60
100822174:      ldrb    w9, [x8, #0x19]
100822178:      cmp w9, #0xff
10082217c:      b.ne    0x1008221c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x6dc>
100822180:      b   0x1008221d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x6f0>
100822184:      ldrb    w9, [x8, #0xb8]
100822188:      cbz w9, 0x1008221d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x6f0>
10082218c:      ldr x9, [x8, #0xb0]
100822190:      cmp x9, x1
100822194:      b.ne    0x1008221d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x6f0>
100822198:      ldr x9, [x8, #0x90]
10082219c:      cmp x9, x21
1008221a0:      b.hi    0x1008221d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x6f0>
1008221a4:      ldr x9, [x8, #0x98]
1008221a8:      cmp x9, x21
1008221ac:      b.ls    0x1008221d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x6f0>
1008221b0:      add x8, x8, #0x90
1008221b4:      ldrb    w9, [x8, #0x19]
1008221b8:      cmp w9, #0xff
1008221bc:      b.eq    0x1008221d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x6f0>
1008221c0:      ldrb    w8, [x8, #0x18]
1008221c4:      sub w8, w8, #0x1
1008221c8:      cmp w8, #0x3
1008221cc:      b.hs    0x1008221f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x70c>
1008221d0:      b   0x1008221fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x718>
1008221d4:      add x8, sp, #0x18
1008221d8:      mov x0, x21
1008221dc:      bl  0x1008d247c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta37classify_heap_space_in_range_uncached>
1008221e0:      ldrb    w8, [sp, #0x20]
1008221e4:      sub w8, w8, #0x1
1008221e8:      cmp w8, #0x3
1008221ec:      b.lo    0x1008221fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x718>
1008221f0:      ldrb    w8, [x23, #0x1c]
1008221f4:      orr w8, w8, #0xc
1008221f8:      strb    w8, [x23, #0x1c]
1008221fc:      ldr w22, [x21, #0x4]
100822200:      ldr w23, [x26]
100822204:      cmp w23, #0x300
100822208:      b.hs    0x1008223c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x8e0>
10082220c:      ldr x8, [x24]
100822210:      cmn x8, #0x1
100822214:      b.eq    0x1008223b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x8d0>
100822218:      mrs x9, TPIDRRO_EL0
10082221c:      and x9, x9, #0xfffffffffffffff8
100822220:      ldr x0, [x9, x8, lsl #3]
100822224:      cbz x0, 0x1008223b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x8d0>
100822228:      add x8, x0, x23, lsl #3
10082222c:      ldr x0, [x8, #0x1e8]
100822230:      cbz x0, 0x1008223c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x8e0>
100822234:      ldr x0, [x0]
100822238:      cbz x0, 0x1008223d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x8f4>
10082223c:      mov w8, #-0x40000001        ; =-1073741825
100822240:      cmp w22, w8
100822244:      b.gt    0x10082229c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x7b8>
100822248:      ldr x9, [x0, #0x5198]
10082224c:      ubfx    x8, x22, #15, #15
100822250:      cmp x8, x9
100822254:      b.hs    0x10082229c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x7b8>
100822258:      ldr x9, [x0, #0x5190]
10082225c:      ldr x8, [x9, x8, lsl #3]
100822260:      cbz x8, 0x10082229c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x7b8>
100822264:      ubfx    x9, x22, #5, #10
100822268:      ldr x8, [x8, x9, lsl #3]
10082226c:      cbz x8, 0x10082229c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x7b8>
100822270:      and x9, x22, #0x1f
100822274:      add x8, x8, x9, lsl #5
100822278:      ldrb    w9, [x8, #0x1c]
10082227c:      tbz w9, #0x0, 0x10082229c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x7b8>
100822280:      ldr x0, [x8]
100822284:      cbz x0, 0x10082229c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x7b8>
100822288:      ldr w8, [x21, #0x4]
10082228c:      mov w9, #-0x40000001        ; =-1073741825
100822290:      cmp w8, w9
100822294:      b.gt    0x10082229c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x7b8>
100822298:      bl  0x1003a4340 <_keys_array_len_capped_to_capacity>
10082229c:      ldurh   w8, [x21, #-0x6]
1008222a0:      orr w8, w8, #0x200
1008222a4:      sturh   w8, [x21, #-0x6]
1008222a8:      cmp x25, #0x9
1008222ac:      b.hs    0x100822400 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x91c>
1008222b0:      cbz x25, 0x10082232c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x848>
1008222b4:      ldr x8, [x20, #0x8]
1008222b8:      str x8, [x21, #0x10]
1008222bc:      cmp x25, #0x1
1008222c0:      b.eq    0x10082232c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x848>
1008222c4:      ldr x8, [x20, #0x18]
1008222c8:      str x8, [x21, #0x18]
1008222cc:      cmp x25, #0x2
1008222d0:      b.eq    0x10082232c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x848>
1008222d4:      ldr x8, [x20, #0x28]
1008222d8:      str x8, [x21, #0x20]
1008222dc:      cmp x25, #0x3
1008222e0:      b.eq    0x10082232c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x848>
1008222e4:      ldr x8, [x20, #0x38]
1008222e8:      str x8, [x21, #0x28]
1008222ec:      cmp x25, #0x4
1008222f0:      b.eq    0x10082232c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x848>
1008222f4:      ldr x8, [x20, #0x48]
1008222f8:      str x8, [x21, #0x30]
1008222fc:      cmp x25, #0x5
100822300:      b.eq    0x10082232c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x848>
100822304:      ldr x8, [x20, #0x58]
100822308:      str x8, [x21, #0x38]
10082230c:      cmp x25, #0x6
100822310:      b.eq    0x10082232c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x848>
100822314:      ldr x8, [x20, #0x68]
100822318:      str x8, [x21, #0x40]
10082231c:      cmp x25, #0x7
100822320:      b.eq    0x10082232c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x848>
100822324:      ldr x8, [x20, #0x78]
100822328:      str x8, [x21, #0x48]
10082232c:      adrp    x0, 0x10112c000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy29GC_SAFEPOINT_DEFER_ARENA_BASE0s_023___RUST_STD_INTERNAL_VAL>
100822330:      add x0, x0, #0x1c8
100822334:      ldr x8, [x0]
100822338:      blr x8
10082233c:      ldrb    w8, [x0, #0x38]
100822340:      cmp w8, #0x1
100822344:      b.ne    0x100822420 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x93c>
100822348:      ldr x8, [x0]
10082234c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100822350:      cmp x8, x9
100822354:      b.hs    0x100822430 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x94c>
100822358:      ldr x8, [x0, #0x20]
10082235c:      cmp x8, #0x1, lsl #12       ; =0x1000
100822360:      b.hi    0x10082243c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x958>
100822364:      bl  0x100818820 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy49gc_schedule_parse_boundary_collection_if_pressure>
100822368:      mov x1, #0x7ffd000000000000 ; =9222527611924643840
10082236c:      bfxil   x1, x21, #0, #48
100822370:      ldr x8, [x24]
100822374:      cmn x8, #0x1
100822378:      b.eq    0x100822458 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x974>
10082237c:      mrs x9, TPIDRRO_EL0
100822380:      and x9, x9, #0xfffffffffffffff8
100822384:      ldr x8, [x9, x8, lsl #3]
100822388:      cbz x8, 0x100822458 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x974>
10082238c:      ldr x8, [x8, #0x19e8]
100822390:      cbz x8, 0x100822458 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x974>
100822394:      ldr x9, [x8]
100822398:      cbnz    x9, 0x10082260c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xb28>
10082239c:      ldr x9, [x8, #0x18]
1008223a0:      cmp x19, x9
1008223a4:      b.hi    0x1008223ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x8c8>
1008223a8:      str x19, [x8, #0x18]
1008223ac:      str xzr, [x8]
1008223b0:      b   0x100822488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x9a4>
1008223b4:      bl  0x100cb5b38 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1008223b8:      add x8, x0, x23, lsl #3
1008223bc:      ldr x0, [x8, #0x1e8]
1008223c0:      cbnz    x0, 0x100822234 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x750>
1008223c4:      adrp    x0, 0x1010d2000 <_anon.47db62074bb938d3ace44517495b1cd3.1235+0xa98>
1008223c8:      add x0, x0, #0x728
1008223cc:      bl  0x100cb5794 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1008223d0:      ldr x0, [x0]
1008223d4:      cbnz    x0, 0x10082223c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x758>
1008223d8:      bl  0x100cc56c0 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
1008223dc:      mov w8, #-0x40000001        ; =-1073741825
1008223e0:      cmp w22, w8
1008223e4:      b.le    0x100822248 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x764>
1008223e8:      b   0x10082229c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x7b8>
1008223ec:      bl  0x100c95990 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1008223f0:      adrp    x0, 0x100db8000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x27a0>
1008223f4:      add x0, x0, #0x6e0
1008223f8:      mov w1, #0xb                ; =11
1008223fc:      bl  0x100c95958 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
100822400:      adrp    x3, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
100822404:      add x3, x3, #0xfc0
100822408:      mov x0, #0x0                ; =0
10082240c:      mov x1, x25
100822410:      mov w2, #0x8                ; =8
100822414:      bl  0x100c8b54c <__RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail>
100822418:      tbnz    w23, #0x8, 0x100822590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xaac>
10082241c:      bl  0x100c964f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object6shapes24shape_id_exhausted_abort>
100822420:      mov x1, #0x0                ; =0
100822424:      bl  0x100cc18a4 <__RINvMs0_NtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native4lazyINtB6_7StorageINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBe_11collections4hash3map7HashMapNtNtCsctvjasLqLe9_5alloc6string6StringyEEuE16get_or_init_slowNvNvNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins7console16CONSOLE_COUNTERS27___rust_std_internal_init_fnEB3C_>
100822428:      cbnz    x0, 0x100822348 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x864>
10082242c:      b   0x100822600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xb1c>
100822430:      adrp    x0, 0x101097000 <_anon.438b28c8644b10f28676d307896bf03a.683>
100822434:      add x0, x0, #0x518
100822438:      bl  0x100c8b25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10082243c:      bl  0x100cbac24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json12parse_scalar15clear_key_cache>
100822440:      bl  0x100818820 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy49gc_schedule_parse_boundary_collection_if_pressure>
100822444:      mov x1, #0x7ffd000000000000 ; =9222527611924643840
100822448:      bfxil   x1, x21, #0, #48
10082244c:      ldr x8, [x24]
100822450:      cmn x8, #0x1
100822454:      b.ne    0x10082237c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x898>
100822458:      adrp    x0, 0x10112a000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc8tenuring17NURSERY_CAP_SCALE0s_023___RUST_STD_INTERNAL_VAL+0x10>
10082245c:      add x0, x0, #0x668
100822460:      ldr x8, [x0]
100822464:      blr x8
100822468:      ldrb    w8, [x0, #0x20]
10082246c:      cbnz    w8, 0x100822594 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xab0>
100822470:      ldr x8, [x0]
100822474:      cbnz    x8, 0x1008225c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xae4>
100822478:      ldr x8, [x0, #0x18]
10082247c:      cmp x19, x8
100822480:      b.hi    0x100822488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x9a4>
100822484:      str x19, [x0, #0x18]
100822488:      mov w0, #0x1                ; =1
10082248c:      ldp x29, x30, [sp, #0x80]
100822490:      ldp x20, x19, [sp, #0x70]
100822494:      ldp x22, x21, [sp, #0x60]
100822498:      ldp x24, x23, [sp, #0x50]
10082249c:      ldp x26, x25, [sp, #0x40]
1008224a0:      ldp x28, x27, [sp, #0x30]
1008224a4:      add sp, sp, #0x90
1008224a8:      ret
1008224ac:      adrp    x0, 0x101094000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1008224b0:      add x0, x0, #0x498
1008224b4:      bl  0x100c8b25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008224b8:      adrp    x0, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
1008224bc:      add x0, x0, #0x5f8
1008224c0:      bl  0x100c8b25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008224c4:      adrp    x0, 0x100e03000 <_anon.d13bca72c7c43155356dec6763133824.2879+0x6dd>
1008224c8:      add x0, x0, #0xcd2
1008224cc:      mov w1, #0xb                ; =11
1008224d0:      bl  0x100c95958 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1008224d4:      cmp w8, #0x2
1008224d8:      b.eq    0x100822600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xb1c>
1008224dc:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
1008224e0:      add x1, x1, #0x36c
1008224e4:      mov x23, x0
1008224e8:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008224ec:      mov x0, x23
1008224f0:      strb    wzr, [x23, #0x20]
1008224f4:      ldr x9, [x23]
1008224f8:      mov x8, #0x7fffffffffffffff ; =9223372036854775807
1008224fc:      cmp x9, x8
100822500:      b.lo    0x100821cd0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x1ec>
100822504:      b   0x100822584 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xaa0>
100822508:      cmp w8, #0x2
10082250c:      b.eq    0x100822600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xb1c>
100822510:      mov x26, x20
100822514:      mov x20, x24
100822518:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
10082251c:      add x1, x1, #0x36c
100822520:      mov x24, x0
100822524:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100822528:      mov x0, x24
10082252c:      strb    wzr, [x24, #0x20]
100822530:      mov x24, x20
100822534:      mov x20, x26
100822538:      ldr x8, [x0]
10082253c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100822540:      cmp x8, x9
100822544:      b.lo    0x100821dcc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x2e8>
100822548:      b   0x100822584 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xaa0>
10082254c:      cmp w8, #0x2
100822550:      b.eq    0x100822600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xb1c>
100822554:      mov x20, x24
100822558:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
10082255c:      add x1, x1, #0x36c
100822560:      mov x24, x0
100822564:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100822568:      mov x0, x24
10082256c:      strb    wzr, [x24, #0x20]
100822570:      mov x24, x20
100822574:      ldr x8, [x0]
100822578:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10082257c:      cmp x8, x9
100822580:      b.lo    0x100821e9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x3b8>
100822584:      adrp    x0, 0x101093000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
100822588:      add x0, x0, #0xf70
10082258c:      bl  0x100c8b25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100822590:      bl  0x100c96540 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object6shapes25invalid_shape_facts_abort>
100822594:      cmp w8, #0x2
100822598:      b.eq    0x100822600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xb1c>
10082259c:      mov x20, x1
1008225a0:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
1008225a4:      add x1, x1, #0x36c
1008225a8:      mov x19, x0
1008225ac:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008225b0:      mov x0, x19
1008225b4:      strb    wzr, [x19, #0x20]
1008225b8:      mov x1, x20
1008225bc:      ldr x19, [sp]
1008225c0:      ldr x8, [x0]
1008225c4:      cbz x8, 0x100822478 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0x994>
1008225c8:      adrp    x0, 0x101099000 <_anon.438b28c8644b10f28676d307896bf03a.1140>
1008225cc:      add x0, x0, #0x2b8
1008225d0:      bl  0x100c8b22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008225d4:      cmp w8, #0x1
1008225d8:      b.ne    0x100822600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xb1c>
1008225dc:      mov x19, x1
1008225e0:      adrp    x1, 0x10094b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtB1L_6string6StringEEECs5gMwpk3Cs4e_13perry_runtime+0x40>
1008225e4:      add x1, x1, #0x36c
1008225e8:      mov x20, x0
1008225ec:      bl  0x100b9959c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008225f0:      mov x0, x20
1008225f4:      strb    wzr, [x20, #0x20]
1008225f8:      mov x1, x19
1008225fc:      b   0x100821b88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate+0xa4>
100822600:      adrp    x0, 0x101093000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
100822604:      add x0, x0, #0xed8
100822608:      bl  0x100cd1edc <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10082260c:      adrp    x0, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
100822610:      add x0, x0, #0x750
100822614:      bl  0x100c8b22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
