/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010028af18 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>:
10028af18:      stp d9, d8, [sp, #-0x70]!
10028af1c:      stp x28, x27, [sp, #0x10]
10028af20:      stp x26, x25, [sp, #0x20]
10028af24:      stp x24, x23, [sp, #0x30]
10028af28:      stp x22, x21, [sp, #0x40]
10028af2c:      stp x20, x19, [sp, #0x50]
10028af30:      stp x29, x30, [sp, #0x60]
10028af34:      add x29, sp, #0x60
10028af38:      mov x19, x2
10028af3c:      mov x21, x0
10028af40:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
10028af44:      add x0, x0, #0x590
10028af48:      ldr x8, [x0]
10028af4c:      blr x8
10028af50:      mov x20, x0
10028af54:      ldrb    w8, [x0, #0x20]
10028af58:      cbnz    w8, 0x10028b3f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4dc>
10028af5c:      ldr x8, [x20]
10028af60:      cbnz    x8, 0x10028b424 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x50c>
10028af64:      mov x22, #0x7ffd000000000000 ; =9222527611924643840
10028af68:      bfxil   x22, x1, #0, #48
10028af6c:      mov x8, #-0x1               ; =-1
10028af70:      str x8, [x20]
10028af74:      mov x0, x20
10028af78:      ldr x8, [x0, #0x8]!
10028af7c:      ldr x28, [x20, #0x18]
10028af80:      cmp x28, x8
10028af84:      b.ne    0x10028af8c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x74>
10028af88:      bl  0x100ccd37c <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
10028af8c:      ldr x8, [x20, #0x10]
10028af90:      str x22, [x8, x28, lsl #3]
10028af94:      add x8, x28, #0x1
10028af98:      str x8, [x20, #0x18]
10028af9c:      ldr x8, [x20]
10028afa0:      add x8, x8, #0x1
10028afa4:      str x8, [x20]
10028afa8:      mov x0, x21
10028afac:      bl  0x10028a9a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10028afb0:      ldrb    w8, [x21, #0x90]
10028afb4:      cbz w8, 0x10028b310 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3f8>
10028afb8:      mov x24, x0
10028afbc:      mov x22, #0x7fffffffffffffff ; =9223372036854775807
10028afc0:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
10028afc4:      fmov    d8, x8
10028afc8:      mov x27, #-0x1              ; =-1
10028afcc:      mov x23, #0x2600            ; =9728
10028afd0:      movk    x23, #0x1, lsl #32
10028afd4:      ldrb    w8, [x20, #0x20]
10028afd8:      cbnz    w8, 0x10028b1b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x29c>
10028afdc:      ldr x8, [x20]
10028afe0:      cmp x8, x22
10028afe4:      b.hs    0x10028b468 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
10028afe8:      add x9, x8, #0x1
10028afec:      str x9, [x20]
10028aff0:      ldr x9, [x20, #0x18]
10028aff4:      cmp x28, x9
10028aff8:      b.hs    0x10028b02c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x114>
10028affc:      ldr x9, [x20, #0x10]
10028b000:      ldr x9, [x9, x28, lsl #3]
10028b004:      and x25, x9, #0xffffffffffff
10028b008:      str x8, [x20]
10028b00c:      ldp w26, w8, [x25]
10028b010:      cmp w26, w8
10028b014:      b.lo    0x10028b040 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x128>
10028b018:      fmov    d0, x24
10028b01c:      mov x0, x25
10028b020:      bl  0x100493508 <_js_array_push_f64>
10028b024:      and x25, x0, #0xffffffffffff
10028b028:      b   0x10028b21c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x304>
10028b02c:      mov w25, #0x1               ; =1
10028b030:      str x8, [x20]
10028b034:      ldp w26, w8, [x25]
10028b038:      cmp w26, w8
10028b03c:      b.hs    0x10028b018 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x100>
10028b040:      add x27, x25, x26, lsl #3
10028b044:      str x24, [x27, #0x8]!
10028b048:      mov x0, x25
10028b04c:      bl  0x1002ce484 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
10028b050:      lsr x22, x25, #3
10028b054:      tbz w0, #0x0, 0x10028b078 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x160>
10028b058:      mov w8, #0x7ffe             ; =32766
10028b05c:      cmp x8, x24, lsr #48
10028b060:      b.ne    0x10028b098 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x180>
10028b064:      mov x0, x24
10028b068:      bl  0x100aa65f8 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10028b06c:      tbnz    w0, #0x0, 0x10028b0b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
10028b070:      scvtf   d0, w24
10028b074:      b   0x10028b0b0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x198>
10028b078:      cmp x22, #0x201
10028b07c:      b.lo    0x10028b0b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
10028b080:      ldurb   w8, [x25, #-0x8]
10028b084:      cmp w8, #0x1
10028b088:      b.ne    0x10028b0b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
10028b08c:      ldurh   w8, [x25, #-0x6]
10028b090:      tbnz    w8, #0xc, 0x10028b058 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x140>
10028b094:      b   0x10028b0b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
10028b098:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
10028b09c:      cmp x24, x8
10028b0a0:      b.gt    0x10028b0b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
10028b0a4:      fmov    d0, x24
10028b0a8:      fcmp    d0, d0
10028b0ac:      fcsel   d0, d8, d0, vs
10028b0b0:      fmov    x24, d0
10028b0b4:      str x24, [x27]
10028b0b8:      mov w8, #0x7ffe             ; =32766
10028b0bc:      cmp x8, x24, lsr #48
10028b0c0:      b.ne    0x10028b0e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x1c8>
10028b0c4:      mov x0, x24
10028b0c8:      bl  0x100aa65f8 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10028b0cc:      tbnz    w0, #0x0, 0x10028b0ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x1d4>
10028b0d0:      scvtf   d0, w24
10028b0d4:      cmp x22, #0x201
10028b0d8:      b.hs    0x10028b13c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x224>
10028b0dc:      b   0x10028b160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10028b0e0:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
10028b0e4:      cmp x24, x8
10028b0e8:      b.le    0x10028b128 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x210>
10028b0ec:      cmp x22, #0x201
10028b0f0:      b.lo    0x10028b160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10028b0f4:      ldurb   w8, [x25, #-0x8]
10028b0f8:      cmp w8, #0x1
10028b0fc:      b.ne    0x10028b160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10028b100:      ldurh   w8, [x25, #-0x6]
10028b104:      mov w9, #0xef7f             ; =61311
10028b108:      and w9, w8, w9
10028b10c:      sturh   w9, [x25, #-0x6]
10028b110:      mov w9, #0x1080             ; =4224
10028b114:      tst w8, w9
10028b118:      b.eq    0x10028b160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10028b11c:      mov x0, x25
10028b120:      bl  0x100422620 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
10028b124:      b   0x10028b160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10028b128:      fmov    d0, x24
10028b12c:      fcmp    d0, d0
10028b130:      fcsel   d0, d8, d0, vs
10028b134:      cmp x22, #0x201
10028b138:      b.lo    0x10028b160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10028b13c:      ldurb   w8, [x25, #-0x8]
10028b140:      cmp w8, #0x1
10028b144:      b.ne    0x10028b160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10028b148:      ldurh   w8, [x25, #-0x6]
10028b14c:      tbz w8, #0x7, 0x10028b160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10028b150:      ldr w8, [x25]
10028b154:      cmp w26, w8
10028b158:      b.hs    0x10028b160 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10028b15c:      str d0, [x27]
10028b160:      mov x0, x25
10028b164:      mov x1, x26
10028b168:      mov x2, x24
10028b16c:      bl  0x1001d7e00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
10028b170:      mov x0, x25
10028b174:      bl  0x1002c0a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
10028b178:      mov x22, #0x7fffffffffffffff ; =9223372036854775807
10028b17c:      cbz w0, 0x10028b210 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
10028b180:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
10028b184:      add x8, x8, #0x910
10028b188:      ldapr   x8, [x8]
10028b18c:      cbnz    x8, 0x10028b1e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2c8>
10028b190:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
10028b194:      ldrb    w8, [x8, #0x918]
10028b198:      tbz w8, #0x0, 0x10028b1f8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2e0>
10028b19c:      mov x0, x25
10028b1a0:      mov x1, x27
10028b1a4:      mov x2, x24
10028b1a8:      mov w3, #0x0                ; =0
10028b1ac:      bl  0x1004357e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
10028b1b0:      b   0x10028b210 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
10028b1b4:      cmp w8, #0x2
10028b1b8:      b.eq    0x10028b438 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
10028b1bc:      mov x0, x20
10028b1c0:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
10028b1c4:      add x1, x1, #0xf78
10028b1c8:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10028b1cc:      strb    wzr, [x20, #0x20]
10028b1d0:      ldr x8, [x20]
10028b1d4:      cmp x8, x22
10028b1d8:      b.lo    0x10028afe8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0xd0>
10028b1dc:      b   0x10028b468 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
10028b1e0:      adrp    x0, 0x101130000 <_perry_global_baseline_worker_ts__1>
10028b1e4:      add x0, x0, #0x910
10028b1e8:      bl  0x100cc5d84 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
10028b1ec:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
10028b1f0:      ldrb    w8, [x8, #0x918]
10028b1f4:      tbnz    w8, #0x0, 0x10028b19c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x284>
10028b1f8:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7e0>
10028b1fc:      add x8, x8, #0xb70
10028b200:      ldr w8, [x8]
10028b204:      cbz w8, 0x10028b210 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
10028b208:      mov x0, x24
10028b20c:      bl  0x1004369e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
10028b210:      add w8, w26, #0x1
10028b214:      str w8, [x25]
10028b218:      mov x27, #-0x1              ; =-1
10028b21c:      ldrb    w8, [x20, #0x20]
10028b220:      cbnz    w8, 0x10028b2e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3c8>
10028b224:      ldr x8, [x20]
10028b228:      cbnz    x8, 0x10028b304 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3ec>
10028b22c:      str x27, [x20]
10028b230:      ldr x8, [x20, #0x18]
10028b234:      cmp x28, x8
10028b238:      b.hs    0x10028b268 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x350>
10028b23c:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
10028b240:      orr x8, x25, x8
10028b244:      ldr x9, [x20, #0x10]
10028b248:      str x8, [x9, x28, lsl #3]
10028b24c:      ldr x8, [x20]
10028b250:      add x8, x8, #0x1
10028b254:      str x8, [x20]
10028b258:      ldp x9, x8, [x21, #0x30]
10028b25c:      cmp x8, x9
10028b260:      b.lo    0x10028b27c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x364>
10028b264:      b   0x10028b2a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
10028b268:      mov x8, #0x0                ; =0
10028b26c:      str x8, [x20]
10028b270:      ldp x9, x8, [x21, #0x30]
10028b274:      cmp x8, x9
10028b278:      b.hs    0x10028b2a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
10028b27c:      ldr x10, [x21, #0x28]
10028b280:      ldrb    w11, [x10, x8]
10028b284:      cmp w11, #0x20
10028b288:      b.hi    0x10028b2a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
10028b28c:      lsr x11, x23, x11
10028b290:      tbz w11, #0x0, 0x10028b2a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
10028b294:      add x8, x8, #0x1
10028b298:      str x8, [x21, #0x38]
10028b29c:      cmp x9, x8
10028b2a0:      b.ne    0x10028b280 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x368>
10028b2a4:      b   0x10028b364 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
10028b2a8:      cmp x8, x9
10028b2ac:      b.hs    0x10028b314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3fc>
10028b2b0:      ldr x10, [x21, #0x28]
10028b2b4:      ldrb    w10, [x10, x8]
10028b2b8:      cmp w10, #0x2c
10028b2bc:      b.ne    0x10028b314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3fc>
10028b2c0:      add x8, x8, #0x1
10028b2c4:      str x8, [x21, #0x38]
10028b2c8:      mov x0, x21
10028b2cc:      bl  0x10028a9a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10028b2d0:      mov x24, x0
10028b2d4:      ldrb    w8, [x21, #0x90]
10028b2d8:      tbnz    w8, #0x0, 0x10028afd4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0xbc>
10028b2dc:      b   0x10028b310 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3f8>
10028b2e0:      cmp w8, #0x2
10028b2e4:      b.eq    0x10028b438 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
10028b2e8:      mov x0, x20
10028b2ec:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
10028b2f0:      add x1, x1, #0xf78
10028b2f4:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10028b2f8:      strb    wzr, [x20, #0x20]
10028b2fc:      ldr x8, [x20]
10028b300:      cbz x8, 0x10028b22c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x314>
10028b304:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10028b308:      add x0, x0, #0xde0
10028b30c:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10028b310:      ldp x9, x8, [x21, #0x30]
10028b314:      cmp x8, x9
10028b318:      b.hs    0x10028b364 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
10028b31c:      ldr x10, [x21, #0x28]
10028b320:      mov x11, #0x2600            ; =9728
10028b324:      movk    x11, #0x1, lsl #32
10028b328:      ldrb    w12, [x10, x8]
10028b32c:      cmp w12, #0x20
10028b330:      b.hi    0x10028b350 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x438>
10028b334:      lsr x13, x11, x12
10028b338:      tbz w13, #0x0, 0x10028b350 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x438>
10028b33c:      add x8, x8, #0x1
10028b340:      str x8, [x21, #0x38]
10028b344:      cmp x9, x8
10028b348:      b.ne    0x10028b328 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x410>
10028b34c:      b   0x10028b364 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
10028b350:      cmp w12, #0x5d
10028b354:      b.ne    0x10028b364 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
10028b358:      add x8, x8, #0x1
10028b35c:      str x8, [x21, #0x38]
10028b360:      b   0x10028b368 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x450>
10028b364:      strb    wzr, [x21, #0x90]
10028b368:      ldrb    w8, [x20, #0x20]
10028b36c:      cbnz    w8, 0x10028b430 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x518>
10028b370:      ldr x8, [x20]
10028b374:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10028b378:      cmp x8, x9
10028b37c:      b.hs    0x10028b468 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
10028b380:      add x9, x8, #0x1
10028b384:      str x9, [x20]
10028b388:      ldr x9, [x20, #0x18]
10028b38c:      cmp x28, x9
10028b390:      b.hs    0x10028b3a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x490>
10028b394:      ldr x9, [x20, #0x10]
10028b398:      ldr x9, [x9, x28, lsl #3]
10028b39c:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
10028b3a0:      bfxil   x0, x9, #0, #48
10028b3a4:      b   0x10028b3b0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x498>
10028b3a8:      mov x0, #0x1                ; =1
10028b3ac:      movk    x0, #0x7ffd, lsl #48
10028b3b0:      str x8, [x20]
10028b3b4:      cbnz    x8, 0x10028b3e8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4d0>
10028b3b8:      ldr x8, [x20, #0x18]
10028b3bc:      cmp x19, x8
10028b3c0:      b.hi    0x10028b3c8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4b0>
10028b3c4:      str x19, [x20, #0x18]
10028b3c8:      ldp x29, x30, [sp, #0x60]
10028b3cc:      ldp x20, x19, [sp, #0x50]
10028b3d0:      ldp x22, x21, [sp, #0x40]
10028b3d4:      ldp x24, x23, [sp, #0x30]
10028b3d8:      ldp x26, x25, [sp, #0x20]
10028b3dc:      ldp x28, x27, [sp, #0x10]
10028b3e0:      ldp d9, d8, [sp], #0x70
10028b3e4:      ret
10028b3e8:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10028b3ec:      add x0, x0, #0xe58
10028b3f0:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10028b3f4:      cmp w8, #0x1
10028b3f8:      b.ne    0x10028b438 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
10028b3fc:      adrp    x8, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
10028b400:      add x8, x8, #0xf78
10028b404:      mov x0, x20
10028b408:      mov x22, x1
10028b40c:      mov x1, x8
10028b410:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10028b414:      mov x1, x22
10028b418:      strb    wzr, [x20, #0x20]
10028b41c:      ldr x8, [x20]
10028b420:      cbz x8, 0x10028af64 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4c>
10028b424:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10028b428:      add x0, x0, #0xdf8
10028b42c:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10028b430:      cmp w8, #0x2
10028b434:      b.ne    0x10028b444 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x52c>
10028b438:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10028b43c:      add x0, x0, #0xed8
10028b440:      bl  0x100cdab9c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10028b444:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
10028b448:      add x1, x1, #0xf78
10028b44c:      mov x0, x20
10028b450:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10028b454:      strb    wzr, [x20, #0x20]
10028b458:      ldr x8, [x20]
10028b45c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10028b460:      cmp x8, x9
10028b464:      b.lo    0x10028b380 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x468>
10028b468:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10028b46c:      add x0, x0, #0xdc8
10028b470:      bl  0x100c9855c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
