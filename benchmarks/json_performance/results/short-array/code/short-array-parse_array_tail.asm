/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/short-array-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100899df4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>:
100899df4:      stp d9, d8, [sp, #-0x70]!
100899df8:      stp x28, x27, [sp, #0x10]
100899dfc:      stp x26, x25, [sp, #0x20]
100899e00:      stp x24, x23, [sp, #0x30]
100899e04:      stp x22, x21, [sp, #0x40]
100899e08:      stp x20, x19, [sp, #0x50]
100899e0c:      stp x29, x30, [sp, #0x60]
100899e10:      add x29, sp, #0x60
100899e14:      mov x19, x2
100899e18:      mov x21, x0
100899e1c:      adrp    x0, 0x101134000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json17PARSE_SHAPE_CACHE0023___RUST_STD_INTERNAL_VAL>
100899e20:      add x0, x0, #0x660
100899e24:      ldr x8, [x0]
100899e28:      blr x8
100899e2c:      mov x20, x0
100899e30:      ldrb    w8, [x0, #0x20]
100899e34:      cbnz    w8, 0x10089a2d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4dc>
100899e38:      ldr x8, [x20]
100899e3c:      cbnz    x8, 0x10089a300 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x50c>
100899e40:      mov x22, #0x7ffd000000000000 ; =9222527611924643840
100899e44:      bfxil   x22, x1, #0, #48
100899e48:      mov x8, #-0x1               ; =-1
100899e4c:      str x8, [x20]
100899e50:      mov x0, x20
100899e54:      ldr x8, [x0, #0x8]!
100899e58:      ldr x28, [x20, #0x18]
100899e5c:      cmp x28, x8
100899e60:      b.ne    0x100899e68 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x74>
100899e64:      bl  0x100ccc608 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
100899e68:      ldr x8, [x20, #0x10]
100899e6c:      str x22, [x8, x28, lsl #3]
100899e70:      add x8, x28, #0x1
100899e74:      str x8, [x20, #0x18]
100899e78:      ldr x8, [x20]
100899e7c:      add x8, x8, #0x1
100899e80:      str x8, [x20]
100899e84:      mov x0, x21
100899e88:      bl  0x10089987c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
100899e8c:      ldrb    w8, [x21, #0x90]
100899e90:      cbz w8, 0x10089a1ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3f8>
100899e94:      mov x24, x0
100899e98:      mov x22, #0x7fffffffffffffff ; =9223372036854775807
100899e9c:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
100899ea0:      fmov    d8, x8
100899ea4:      mov x27, #-0x1              ; =-1
100899ea8:      mov x23, #0x2600            ; =9728
100899eac:      movk    x23, #0x1, lsl #32
100899eb0:      ldrb    w8, [x20, #0x20]
100899eb4:      cbnz    w8, 0x10089a090 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x29c>
100899eb8:      ldr x8, [x20]
100899ebc:      cmp x8, x22
100899ec0:      b.hs    0x10089a344 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
100899ec4:      add x9, x8, #0x1
100899ec8:      str x9, [x20]
100899ecc:      ldr x9, [x20, #0x18]
100899ed0:      cmp x28, x9
100899ed4:      b.hs    0x100899f08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x114>
100899ed8:      ldr x9, [x20, #0x10]
100899edc:      ldr x9, [x9, x28, lsl #3]
100899ee0:      and x25, x9, #0xffffffffffff
100899ee4:      str x8, [x20]
100899ee8:      ldp w26, w8, [x25]
100899eec:      cmp w26, w8
100899ef0:      b.lo    0x100899f1c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x128>
100899ef4:      fmov    d0, x24
100899ef8:      mov x0, x25
100899efc:      bl  0x100848268 <_js_array_push_f64>
100899f00:      and x25, x0, #0xffffffffffff
100899f04:      b   0x10089a0f8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x304>
100899f08:      mov w25, #0x1               ; =1
100899f0c:      str x8, [x20]
100899f10:      ldp w26, w8, [x25]
100899f14:      cmp w26, w8
100899f18:      b.hs    0x100899ef4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x100>
100899f1c:      add x27, x25, x26, lsl #3
100899f20:      str x24, [x27, #0x8]!
100899f24:      mov x0, x25
100899f28:      bl  0x1008ddb44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
100899f2c:      lsr x22, x25, #3
100899f30:      tbz w0, #0x0, 0x100899f54 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x160>
100899f34:      mov w8, #0x7ffe             ; =32766
100899f38:      cmp x8, x24, lsr #48
100899f3c:      b.ne    0x100899f74 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x180>
100899f40:      mov x0, x24
100899f44:      bl  0x10024367c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
100899f48:      tbnz    w0, #0x0, 0x100899f90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
100899f4c:      scvtf   d0, w24
100899f50:      b   0x100899f8c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x198>
100899f54:      cmp x22, #0x201
100899f58:      b.lo    0x100899f90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
100899f5c:      ldurb   w8, [x25, #-0x8]
100899f60:      cmp w8, #0x1
100899f64:      b.ne    0x100899f90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
100899f68:      ldurh   w8, [x25, #-0x6]
100899f6c:      tbnz    w8, #0xc, 0x100899f34 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x140>
100899f70:      b   0x100899f90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
100899f74:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
100899f78:      cmp x24, x8
100899f7c:      b.gt    0x100899f90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
100899f80:      fmov    d0, x24
100899f84:      fcmp    d0, d0
100899f88:      fcsel   d0, d8, d0, vs
100899f8c:      fmov    x24, d0
100899f90:      str x24, [x27]
100899f94:      mov w8, #0x7ffe             ; =32766
100899f98:      cmp x8, x24, lsr #48
100899f9c:      b.ne    0x100899fbc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x1c8>
100899fa0:      mov x0, x24
100899fa4:      bl  0x10024367c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
100899fa8:      tbnz    w0, #0x0, 0x100899fc8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x1d4>
100899fac:      scvtf   d0, w24
100899fb0:      cmp x22, #0x201
100899fb4:      b.hs    0x10089a018 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x224>
100899fb8:      b   0x10089a03c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
100899fbc:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
100899fc0:      cmp x24, x8
100899fc4:      b.le    0x10089a004 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x210>
100899fc8:      cmp x22, #0x201
100899fcc:      b.lo    0x10089a03c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
100899fd0:      ldurb   w8, [x25, #-0x8]
100899fd4:      cmp w8, #0x1
100899fd8:      b.ne    0x10089a03c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
100899fdc:      ldurh   w8, [x25, #-0x6]
100899fe0:      mov w9, #0xef7f             ; =61311
100899fe4:      and w9, w8, w9
100899fe8:      sturh   w9, [x25, #-0x6]
100899fec:      mov w9, #0x1080             ; =4224
100899ff0:      tst w8, w9
100899ff4:      b.eq    0x10089a03c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
100899ff8:      mov x0, x25
100899ffc:      bl  0x1007bd668 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
10089a000:      b   0x10089a03c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10089a004:      fmov    d0, x24
10089a008:      fcmp    d0, d0
10089a00c:      fcsel   d0, d8, d0, vs
10089a010:      cmp x22, #0x201
10089a014:      b.lo    0x10089a03c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10089a018:      ldurb   w8, [x25, #-0x8]
10089a01c:      cmp w8, #0x1
10089a020:      b.ne    0x10089a03c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10089a024:      ldurh   w8, [x25, #-0x6]
10089a028:      tbz w8, #0x7, 0x10089a03c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10089a02c:      ldr w8, [x25]
10089a030:      cmp w26, w8
10089a034:      b.hs    0x10089a03c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10089a038:      str d0, [x27]
10089a03c:      mov x0, x25
10089a040:      mov x1, x26
10089a044:      mov x2, x24
10089a048:      bl  0x1007ea6c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
10089a04c:      mov x0, x25
10089a050:      bl  0x1008cc038 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
10089a054:      mov x22, #0x7fffffffffffffff ; =9223372036854775807
10089a058:      cbz w0, 0x10089a0ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
10089a05c:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
10089a060:      add x8, x8, #0x28
10089a064:      ldapr   x8, [x8]
10089a068:      cbnz    x8, 0x10089a0bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2c8>
10089a06c:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
10089a070:      ldrb    w8, [x8, #0x30]
10089a074:      tbz w8, #0x0, 0x10089a0d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2e0>
10089a078:      mov x0, x25
10089a07c:      mov x1, x27
10089a080:      mov x2, x24
10089a084:      mov w3, #0x0                ; =0
10089a088:      bl  0x100553db4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
10089a08c:      b   0x10089a0ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
10089a090:      cmp w8, #0x2
10089a094:      b.eq    0x10089a314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
10089a098:      mov x0, x20
10089a09c:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
10089a0a0:      add x1, x1, #0xd0
10089a0a4:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10089a0a8:      strb    wzr, [x20, #0x20]
10089a0ac:      ldr x8, [x20]
10089a0b0:      cmp x8, x22
10089a0b4:      b.lo    0x100899ec4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0xd0>
10089a0b8:      b   0x10089a344 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
10089a0bc:      adrp    x0, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
10089a0c0:      add x0, x0, #0x28
10089a0c4:      bl  0x100cc1e44 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
10089a0c8:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
10089a0cc:      ldrb    w8, [x8, #0x30]
10089a0d0:      tbnz    w8, #0x0, 0x10089a078 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x284>
10089a0d4:      adrp    x8, 0x1011f8000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f448>
10089a0d8:      add x8, x8, #0xfe0
10089a0dc:      ldr w8, [x8]
10089a0e0:      cbz w8, 0x10089a0ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
10089a0e4:      mov x0, x24
10089a0e8:      bl  0x100555294 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
10089a0ec:      add w8, w26, #0x1
10089a0f0:      str w8, [x25]
10089a0f4:      mov x27, #-0x1              ; =-1
10089a0f8:      ldrb    w8, [x20, #0x20]
10089a0fc:      cbnz    w8, 0x10089a1bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3c8>
10089a100:      ldr x8, [x20]
10089a104:      cbnz    x8, 0x10089a1e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3ec>
10089a108:      str x27, [x20]
10089a10c:      ldr x8, [x20, #0x18]
10089a110:      cmp x28, x8
10089a114:      b.hs    0x10089a144 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x350>
10089a118:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
10089a11c:      orr x8, x25, x8
10089a120:      ldr x9, [x20, #0x10]
10089a124:      str x8, [x9, x28, lsl #3]
10089a128:      ldr x8, [x20]
10089a12c:      add x8, x8, #0x1
10089a130:      str x8, [x20]
10089a134:      ldp x9, x8, [x21, #0x30]
10089a138:      cmp x8, x9
10089a13c:      b.lo    0x10089a158 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x364>
10089a140:      b   0x10089a184 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
10089a144:      mov x8, #0x0                ; =0
10089a148:      str x8, [x20]
10089a14c:      ldp x9, x8, [x21, #0x30]
10089a150:      cmp x8, x9
10089a154:      b.hs    0x10089a184 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
10089a158:      ldr x10, [x21, #0x28]
10089a15c:      ldrb    w11, [x10, x8]
10089a160:      cmp w11, #0x20
10089a164:      b.hi    0x10089a184 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
10089a168:      lsr x11, x23, x11
10089a16c:      tbz w11, #0x0, 0x10089a184 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
10089a170:      add x8, x8, #0x1
10089a174:      str x8, [x21, #0x38]
10089a178:      cmp x9, x8
10089a17c:      b.ne    0x10089a15c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x368>
10089a180:      b   0x10089a240 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
10089a184:      cmp x8, x9
10089a188:      b.hs    0x10089a1f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3fc>
10089a18c:      ldr x10, [x21, #0x28]
10089a190:      ldrb    w10, [x10, x8]
10089a194:      cmp w10, #0x2c
10089a198:      b.ne    0x10089a1f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3fc>
10089a19c:      add x8, x8, #0x1
10089a1a0:      str x8, [x21, #0x38]
10089a1a4:      mov x0, x21
10089a1a8:      bl  0x10089987c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10089a1ac:      mov x24, x0
10089a1b0:      ldrb    w8, [x21, #0x90]
10089a1b4:      tbnz    w8, #0x0, 0x100899eb0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0xbc>
10089a1b8:      b   0x10089a1ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3f8>
10089a1bc:      cmp w8, #0x2
10089a1c0:      b.eq    0x10089a314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
10089a1c4:      mov x0, x20
10089a1c8:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
10089a1cc:      add x1, x1, #0xd0
10089a1d0:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10089a1d4:      strb    wzr, [x20, #0x20]
10089a1d8:      ldr x8, [x20]
10089a1dc:      cbz x8, 0x10089a108 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x314>
10089a1e0:      adrp    x0, 0x10109c000 <_anon.438b28c8644b10f28676d307896bf03a.21>
10089a1e4:      add x0, x0, #0xde0
10089a1e8:      bl  0x100c99aac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10089a1ec:      ldp x9, x8, [x21, #0x30]
10089a1f0:      cmp x8, x9
10089a1f4:      b.hs    0x10089a240 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
10089a1f8:      ldr x10, [x21, #0x28]
10089a1fc:      mov x11, #0x2600            ; =9728
10089a200:      movk    x11, #0x1, lsl #32
10089a204:      ldrb    w12, [x10, x8]
10089a208:      cmp w12, #0x20
10089a20c:      b.hi    0x10089a22c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x438>
10089a210:      lsr x13, x11, x12
10089a214:      tbz w13, #0x0, 0x10089a22c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x438>
10089a218:      add x8, x8, #0x1
10089a21c:      str x8, [x21, #0x38]
10089a220:      cmp x9, x8
10089a224:      b.ne    0x10089a204 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x410>
10089a228:      b   0x10089a240 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
10089a22c:      cmp w12, #0x5d
10089a230:      b.ne    0x10089a240 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
10089a234:      add x8, x8, #0x1
10089a238:      str x8, [x21, #0x38]
10089a23c:      b   0x10089a244 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x450>
10089a240:      strb    wzr, [x21, #0x90]
10089a244:      ldrb    w8, [x20, #0x20]
10089a248:      cbnz    w8, 0x10089a30c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x518>
10089a24c:      ldr x8, [x20]
10089a250:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10089a254:      cmp x8, x9
10089a258:      b.hs    0x10089a344 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
10089a25c:      add x9, x8, #0x1
10089a260:      str x9, [x20]
10089a264:      ldr x9, [x20, #0x18]
10089a268:      cmp x28, x9
10089a26c:      b.hs    0x10089a284 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x490>
10089a270:      ldr x9, [x20, #0x10]
10089a274:      ldr x9, [x9, x28, lsl #3]
10089a278:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
10089a27c:      bfxil   x0, x9, #0, #48
10089a280:      b   0x10089a28c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x498>
10089a284:      mov x0, #0x1                ; =1
10089a288:      movk    x0, #0x7ffd, lsl #48
10089a28c:      str x8, [x20]
10089a290:      cbnz    x8, 0x10089a2c4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4d0>
10089a294:      ldr x8, [x20, #0x18]
10089a298:      cmp x19, x8
10089a29c:      b.hi    0x10089a2a4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4b0>
10089a2a0:      str x19, [x20, #0x18]
10089a2a4:      ldp x29, x30, [sp, #0x60]
10089a2a8:      ldp x20, x19, [sp, #0x50]
10089a2ac:      ldp x22, x21, [sp, #0x40]
10089a2b0:      ldp x24, x23, [sp, #0x30]
10089a2b4:      ldp x26, x25, [sp, #0x20]
10089a2b8:      ldp x28, x27, [sp, #0x10]
10089a2bc:      ldp d9, d8, [sp], #0x70
10089a2c0:      ret
10089a2c4:      adrp    x0, 0x10109c000 <_anon.438b28c8644b10f28676d307896bf03a.21>
10089a2c8:      add x0, x0, #0xe58
10089a2cc:      bl  0x100c99aac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10089a2d0:      cmp w8, #0x1
10089a2d4:      b.ne    0x10089a314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
10089a2d8:      adrp    x8, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
10089a2dc:      add x8, x8, #0xd0
10089a2e0:      mov x0, x20
10089a2e4:      mov x22, x1
10089a2e8:      mov x1, x8
10089a2ec:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10089a2f0:      mov x1, x22
10089a2f4:      strb    wzr, [x20, #0x20]
10089a2f8:      ldr x8, [x20]
10089a2fc:      cbz x8, 0x100899e40 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4c>
10089a300:      adrp    x0, 0x10109c000 <_anon.438b28c8644b10f28676d307896bf03a.21>
10089a304:      add x0, x0, #0xdf8
10089a308:      bl  0x100c99aac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10089a30c:      cmp w8, #0x2
10089a310:      b.ne    0x10089a320 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x52c>
10089a314:      adrp    x0, 0x10109b000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10089a318:      add x0, x0, #0xed8
10089a31c:      bl  0x100cdb71c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10089a320:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
10089a324:      add x1, x1, #0xd0
10089a328:      mov x0, x20
10089a32c:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10089a330:      strb    wzr, [x20, #0x20]
10089a334:      ldr x8, [x20]
10089a338:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10089a33c:      cmp x8, x9
10089a340:      b.lo    0x10089a25c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x468>
10089a344:      adrp    x0, 0x10109c000 <_anon.438b28c8644b10f28676d307896bf03a.21>
10089a348:      add x0, x0, #0xdc8
10089a34c:      bl  0x100c99adc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
