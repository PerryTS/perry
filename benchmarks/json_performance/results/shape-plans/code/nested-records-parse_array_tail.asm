/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/nested-records-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100890f18 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>:
100890f18:      stp d9, d8, [sp, #-0x70]!
100890f1c:      stp x28, x27, [sp, #0x10]
100890f20:      stp x26, x25, [sp, #0x20]
100890f24:      stp x24, x23, [sp, #0x30]
100890f28:      stp x22, x21, [sp, #0x40]
100890f2c:      stp x20, x19, [sp, #0x50]
100890f30:      stp x29, x30, [sp, #0x60]
100890f34:      add x29, sp, #0x60
100890f38:      mov x19, x2
100890f3c:      mov x21, x0
100890f40:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
100890f44:      add x0, x0, #0x2a0
100890f48:      ldr x8, [x0]
100890f4c:      blr x8
100890f50:      mov x20, x0
100890f54:      ldrb    w8, [x0, #0x20]
100890f58:      cbnz    w8, 0x1008913f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4dc>
100890f5c:      ldr x8, [x20]
100890f60:      cbnz    x8, 0x100891424 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x50c>
100890f64:      mov x22, #0x7ffd000000000000 ; =9222527611924643840
100890f68:      bfxil   x22, x1, #0, #48
100890f6c:      mov x8, #-0x1               ; =-1
100890f70:      str x8, [x20]
100890f74:      mov x0, x20
100890f78:      ldr x8, [x0, #0x8]!
100890f7c:      ldr x28, [x20, #0x18]
100890f80:      cmp x28, x8
100890f84:      b.ne    0x100890f8c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x74>
100890f88:      bl  0x100cad9e0 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
100890f8c:      ldr x8, [x20, #0x10]
100890f90:      str x22, [x8, x28, lsl #3]
100890f94:      add x8, x28, #0x1
100890f98:      str x8, [x20, #0x18]
100890f9c:      ldr x8, [x20]
100890fa0:      add x8, x8, #0x1
100890fa4:      str x8, [x20]
100890fa8:      mov x0, x21
100890fac:      bl  0x1008909a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
100890fb0:      ldrb    w8, [x21, #0x90]
100890fb4:      cbz w8, 0x100891310 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3f8>
100890fb8:      mov x24, x0
100890fbc:      mov x22, #0x7fffffffffffffff ; =9223372036854775807
100890fc0:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
100890fc4:      fmov    d8, x8
100890fc8:      mov x27, #-0x1              ; =-1
100890fcc:      mov x23, #0x2600            ; =9728
100890fd0:      movk    x23, #0x1, lsl #32
100890fd4:      ldrb    w8, [x20, #0x20]
100890fd8:      cbnz    w8, 0x1008911b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x29c>
100890fdc:      ldr x8, [x20]
100890fe0:      cmp x8, x22
100890fe4:      b.hs    0x100891468 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
100890fe8:      add x9, x8, #0x1
100890fec:      str x9, [x20]
100890ff0:      ldr x9, [x20, #0x18]
100890ff4:      cmp x28, x9
100890ff8:      b.hs    0x10089102c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x114>
100890ffc:      ldr x9, [x20, #0x10]
100891000:      ldr x9, [x9, x28, lsl #3]
100891004:      and x25, x9, #0xffffffffffff
100891008:      str x8, [x20]
10089100c:      ldp w26, w8, [x25]
100891010:      cmp w26, w8
100891014:      b.lo    0x100891040 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x128>
100891018:      fmov    d0, x24
10089101c:      mov x0, x25
100891020:      bl  0x100615c88 <_js_array_push_f64>
100891024:      and x25, x0, #0xffffffffffff
100891028:      b   0x10089121c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x304>
10089102c:      mov w25, #0x1               ; =1
100891030:      str x8, [x20]
100891034:      ldp w26, w8, [x25]
100891038:      cmp w26, w8
10089103c:      b.hs    0x100891018 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x100>
100891040:      add x27, x25, x26, lsl #3
100891044:      str x24, [x27, #0x8]!
100891048:      mov x0, x25
10089104c:      bl  0x1008d05c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
100891050:      lsr x22, x25, #3
100891054:      tbz w0, #0x0, 0x100891078 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x160>
100891058:      mov w8, #0x7ffe             ; =32766
10089105c:      cmp x8, x24, lsr #48
100891060:      b.ne    0x100891098 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x180>
100891064:      mov x0, x24
100891068:      bl  0x100845448 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10089106c:      tbnz    w0, #0x0, 0x1008910b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
100891070:      scvtf   d0, w24
100891074:      b   0x1008910b0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x198>
100891078:      cmp x22, #0x201
10089107c:      b.lo    0x1008910b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
100891080:      ldurb   w8, [x25, #-0x8]
100891084:      cmp w8, #0x1
100891088:      b.ne    0x1008910b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
10089108c:      ldurh   w8, [x25, #-0x6]
100891090:      tbnz    w8, #0xc, 0x100891058 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x140>
100891094:      b   0x1008910b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
100891098:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
10089109c:      cmp x24, x8
1008910a0:      b.gt    0x1008910b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
1008910a4:      fmov    d0, x24
1008910a8:      fcmp    d0, d0
1008910ac:      fcsel   d0, d8, d0, vs
1008910b0:      fmov    x24, d0
1008910b4:      str x24, [x27]
1008910b8:      mov w8, #0x7ffe             ; =32766
1008910bc:      cmp x8, x24, lsr #48
1008910c0:      b.ne    0x1008910e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x1c8>
1008910c4:      mov x0, x24
1008910c8:      bl  0x100845448 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
1008910cc:      tbnz    w0, #0x0, 0x1008910ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x1d4>
1008910d0:      scvtf   d0, w24
1008910d4:      cmp x22, #0x201
1008910d8:      b.hs    0x10089113c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x224>
1008910dc:      b   0x100891160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
1008910e0:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
1008910e4:      cmp x24, x8
1008910e8:      b.le    0x100891128 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x210>
1008910ec:      cmp x22, #0x201
1008910f0:      b.lo    0x100891160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
1008910f4:      ldurb   w8, [x25, #-0x8]
1008910f8:      cmp w8, #0x1
1008910fc:      b.ne    0x100891160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
100891100:      ldurh   w8, [x25, #-0x6]
100891104:      mov w9, #0xef7f             ; =61311
100891108:      and w9, w8, w9
10089110c:      sturh   w9, [x25, #-0x6]
100891110:      mov w9, #0x1080             ; =4224
100891114:      tst w8, w9
100891118:      b.eq    0x100891160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10089111c:      mov x0, x25
100891120:      bl  0x1005aaee0 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
100891124:      b   0x100891160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
100891128:      fmov    d0, x24
10089112c:      fcmp    d0, d0
100891130:      fcsel   d0, d8, d0, vs
100891134:      cmp x22, #0x201
100891138:      b.lo    0x100891160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10089113c:      ldurb   w8, [x25, #-0x8]
100891140:      cmp w8, #0x1
100891144:      b.ne    0x100891160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
100891148:      ldurh   w8, [x25, #-0x6]
10089114c:      tbz w8, #0x7, 0x100891160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
100891150:      ldr w8, [x25]
100891154:      cmp w26, w8
100891158:      b.hs    0x100891160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10089115c:      str d0, [x27]
100891160:      mov x0, x25
100891164:      mov x1, x26
100891168:      mov x2, x24
10089116c:      bl  0x100319c40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
100891170:      mov x0, x25
100891174:      bl  0x1008c2b8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
100891178:      mov x22, #0x7fffffffffffffff ; =9223372036854775807
10089117c:      cbz w0, 0x100891210 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
100891180:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
100891184:      add x8, x8, #0x58
100891188:      ldapr   x8, [x8]
10089118c:      cbnz    x8, 0x1008911e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2c8>
100891190:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
100891194:      ldrb    w8, [x8, #0x60]
100891198:      tbz w8, #0x0, 0x1008911f8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2e0>
10089119c:      mov x0, x25
1008911a0:      mov x1, x27
1008911a4:      mov x2, x24
1008911a8:      mov w3, #0x0                ; =0
1008911ac:      bl  0x1005b943c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
1008911b0:      b   0x100891210 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
1008911b4:      cmp w8, #0x2
1008911b8:      b.eq    0x100891438 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
1008911bc:      mov x0, x20
1008911c0:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1008911c4:      add x1, x1, #0xeec
1008911c8:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008911cc:      strb    wzr, [x20, #0x20]
1008911d0:      ldr x8, [x20]
1008911d4:      cmp x8, x22
1008911d8:      b.lo    0x100890fe8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0xd0>
1008911dc:      b   0x100891468 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
1008911e0:      adrp    x0, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
1008911e4:      add x0, x0, #0x58
1008911e8:      bl  0x100cd0144 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
1008911ec:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
1008911f0:      ldrb    w8, [x8, #0x60]
1008911f4:      tbnz    w8, #0x0, 0x10089119c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x284>
1008911f8:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7fb58>
1008911fc:      add x8, x8, #0x9c4
100891200:      ldr w8, [x8]
100891204:      cbz w8, 0x100891210 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
100891208:      mov x0, x24
10089120c:      bl  0x1005ba638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
100891210:      add w8, w26, #0x1
100891214:      str w8, [x25]
100891218:      mov x27, #-0x1              ; =-1
10089121c:      ldrb    w8, [x20, #0x20]
100891220:      cbnz    w8, 0x1008912e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3c8>
100891224:      ldr x8, [x20]
100891228:      cbnz    x8, 0x100891304 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3ec>
10089122c:      str x27, [x20]
100891230:      ldr x8, [x20, #0x18]
100891234:      cmp x28, x8
100891238:      b.hs    0x100891268 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x350>
10089123c:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100891240:      orr x8, x25, x8
100891244:      ldr x9, [x20, #0x10]
100891248:      str x8, [x9, x28, lsl #3]
10089124c:      ldr x8, [x20]
100891250:      add x8, x8, #0x1
100891254:      str x8, [x20]
100891258:      ldp x9, x8, [x21, #0x30]
10089125c:      cmp x8, x9
100891260:      b.lo    0x10089127c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x364>
100891264:      b   0x1008912a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
100891268:      mov x8, #0x0                ; =0
10089126c:      str x8, [x20]
100891270:      ldp x9, x8, [x21, #0x30]
100891274:      cmp x8, x9
100891278:      b.hs    0x1008912a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
10089127c:      ldr x10, [x21, #0x28]
100891280:      ldrb    w11, [x10, x8]
100891284:      cmp w11, #0x20
100891288:      b.hi    0x1008912a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
10089128c:      lsr x11, x23, x11
100891290:      tbz w11, #0x0, 0x1008912a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
100891294:      add x8, x8, #0x1
100891298:      str x8, [x21, #0x38]
10089129c:      cmp x9, x8
1008912a0:      b.ne    0x100891280 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x368>
1008912a4:      b   0x100891364 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
1008912a8:      cmp x8, x9
1008912ac:      b.hs    0x100891314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3fc>
1008912b0:      ldr x10, [x21, #0x28]
1008912b4:      ldrb    w10, [x10, x8]
1008912b8:      cmp w10, #0x2c
1008912bc:      b.ne    0x100891314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3fc>
1008912c0:      add x8, x8, #0x1
1008912c4:      str x8, [x21, #0x38]
1008912c8:      mov x0, x21
1008912cc:      bl  0x1008909a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1008912d0:      mov x24, x0
1008912d4:      ldrb    w8, [x21, #0x90]
1008912d8:      tbnz    w8, #0x0, 0x100890fd4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0xbc>
1008912dc:      b   0x100891310 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3f8>
1008912e0:      cmp w8, #0x2
1008912e4:      b.eq    0x100891438 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
1008912e8:      mov x0, x20
1008912ec:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1008912f0:      add x1, x1, #0xeec
1008912f4:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008912f8:      strb    wzr, [x20, #0x20]
1008912fc:      ldr x8, [x20]
100891300:      cbz x8, 0x10089122c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x314>
100891304:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
100891308:      add x0, x0, #0xde0
10089130c:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100891310:      ldp x9, x8, [x21, #0x30]
100891314:      cmp x8, x9
100891318:      b.hs    0x100891364 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
10089131c:      ldr x10, [x21, #0x28]
100891320:      mov x11, #0x2600            ; =9728
100891324:      movk    x11, #0x1, lsl #32
100891328:      ldrb    w12, [x10, x8]
10089132c:      cmp w12, #0x20
100891330:      b.hi    0x100891350 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x438>
100891334:      lsr x13, x11, x12
100891338:      tbz w13, #0x0, 0x100891350 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x438>
10089133c:      add x8, x8, #0x1
100891340:      str x8, [x21, #0x38]
100891344:      cmp x9, x8
100891348:      b.ne    0x100891328 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x410>
10089134c:      b   0x100891364 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
100891350:      cmp w12, #0x5d
100891354:      b.ne    0x100891364 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
100891358:      add x8, x8, #0x1
10089135c:      str x8, [x21, #0x38]
100891360:      b   0x100891368 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x450>
100891364:      strb    wzr, [x21, #0x90]
100891368:      ldrb    w8, [x20, #0x20]
10089136c:      cbnz    w8, 0x100891430 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x518>
100891370:      ldr x8, [x20]
100891374:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100891378:      cmp x8, x9
10089137c:      b.hs    0x100891468 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
100891380:      add x9, x8, #0x1
100891384:      str x9, [x20]
100891388:      ldr x9, [x20, #0x18]
10089138c:      cmp x28, x9
100891390:      b.hs    0x1008913a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x490>
100891394:      ldr x9, [x20, #0x10]
100891398:      ldr x9, [x9, x28, lsl #3]
10089139c:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
1008913a0:      bfxil   x0, x9, #0, #48
1008913a4:      b   0x1008913b0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x498>
1008913a8:      mov x0, #0x1                ; =1
1008913ac:      movk    x0, #0x7ffd, lsl #48
1008913b0:      str x8, [x20]
1008913b4:      cbnz    x8, 0x1008913e8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4d0>
1008913b8:      ldr x8, [x20, #0x18]
1008913bc:      cmp x19, x8
1008913c0:      b.hi    0x1008913c8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4b0>
1008913c4:      str x19, [x20, #0x18]
1008913c8:      ldp x29, x30, [sp, #0x60]
1008913cc:      ldp x20, x19, [sp, #0x50]
1008913d0:      ldp x22, x21, [sp, #0x40]
1008913d4:      ldp x24, x23, [sp, #0x30]
1008913d8:      ldp x26, x25, [sp, #0x20]
1008913dc:      ldp x28, x27, [sp, #0x10]
1008913e0:      ldp d9, d8, [sp], #0x70
1008913e4:      ret
1008913e8:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1008913ec:      add x0, x0, #0xe58
1008913f0:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008913f4:      cmp w8, #0x1
1008913f8:      b.ne    0x100891438 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
1008913fc:      adrp    x8, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
100891400:      add x8, x8, #0xeec
100891404:      mov x0, x20
100891408:      mov x22, x1
10089140c:      mov x1, x8
100891410:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100891414:      mov x1, x22
100891418:      strb    wzr, [x20, #0x20]
10089141c:      ldr x8, [x20]
100891420:      cbz x8, 0x100890f64 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4c>
100891424:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
100891428:      add x0, x0, #0xdf8
10089142c:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100891430:      cmp w8, #0x2
100891434:      b.ne    0x100891444 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x52c>
100891438:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10089143c:      add x0, x0, #0xed8
100891440:      bl  0x100cdc11c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
100891444:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
100891448:      add x1, x1, #0xeec
10089144c:      mov x0, x20
100891450:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100891454:      strb    wzr, [x20, #0x20]
100891458:      ldr x8, [x20]
10089145c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100891460:      cmp x8, x9
100891464:      b.lo    0x100891380 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x468>
100891468:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10089146c:      add x0, x0, #0xdc8
100891470:      bl  0x100c99c5c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
