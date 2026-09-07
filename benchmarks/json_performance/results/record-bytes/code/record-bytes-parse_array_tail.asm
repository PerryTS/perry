/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010033b018 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>:
10033b018:      stp d9, d8, [sp, #-0x70]!
10033b01c:      stp x28, x27, [sp, #0x10]
10033b020:      stp x26, x25, [sp, #0x20]
10033b024:      stp x24, x23, [sp, #0x30]
10033b028:      stp x22, x21, [sp, #0x40]
10033b02c:      stp x20, x19, [sp, #0x50]
10033b030:      stp x29, x30, [sp, #0x60]
10033b034:      add x29, sp, #0x60
10033b038:      mov x19, x2
10033b03c:      mov x21, x0
10033b040:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
10033b044:      add x0, x0, #0x398
10033b048:      ldr x8, [x0]
10033b04c:      blr x8
10033b050:      mov x20, x0
10033b054:      ldrb    w8, [x0, #0x20]
10033b058:      cbnz    w8, 0x10033b4f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4dc>
10033b05c:      ldr x8, [x20]
10033b060:      cbnz    x8, 0x10033b524 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x50c>
10033b064:      mov x22, #0x7ffd000000000000 ; =9222527611924643840
10033b068:      bfxil   x22, x1, #0, #48
10033b06c:      mov x8, #-0x1               ; =-1
10033b070:      str x8, [x20]
10033b074:      mov x0, x20
10033b078:      ldr x8, [x0, #0x8]!
10033b07c:      ldr x28, [x20, #0x18]
10033b080:      cmp x28, x8
10033b084:      b.ne    0x10033b08c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x74>
10033b088:      bl  0x100cd4250 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
10033b08c:      ldr x8, [x20, #0x10]
10033b090:      str x22, [x8, x28, lsl #3]
10033b094:      add x8, x28, #0x1
10033b098:      str x8, [x20, #0x18]
10033b09c:      ldr x8, [x20]
10033b0a0:      add x8, x8, #0x1
10033b0a4:      str x8, [x20]
10033b0a8:      mov x0, x21
10033b0ac:      bl  0x10033aaa0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10033b0b0:      ldrb    w8, [x21, #0x90]
10033b0b4:      cbz w8, 0x10033b410 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3f8>
10033b0b8:      mov x24, x0
10033b0bc:      mov x22, #0x7fffffffffffffff ; =9223372036854775807
10033b0c0:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
10033b0c4:      fmov    d8, x8
10033b0c8:      mov x27, #-0x1              ; =-1
10033b0cc:      mov x23, #0x2600            ; =9728
10033b0d0:      movk    x23, #0x1, lsl #32
10033b0d4:      ldrb    w8, [x20, #0x20]
10033b0d8:      cbnz    w8, 0x10033b2b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x29c>
10033b0dc:      ldr x8, [x20]
10033b0e0:      cmp x8, x22
10033b0e4:      b.hs    0x10033b568 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
10033b0e8:      add x9, x8, #0x1
10033b0ec:      str x9, [x20]
10033b0f0:      ldr x9, [x20, #0x18]
10033b0f4:      cmp x28, x9
10033b0f8:      b.hs    0x10033b12c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x114>
10033b0fc:      ldr x9, [x20, #0x10]
10033b100:      ldr x9, [x9, x28, lsl #3]
10033b104:      and x25, x9, #0xffffffffffff
10033b108:      str x8, [x20]
10033b10c:      ldp w26, w8, [x25]
10033b110:      cmp w26, w8
10033b114:      b.lo    0x10033b140 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x128>
10033b118:      fmov    d0, x24
10033b11c:      mov x0, x25
10033b120:      bl  0x1006d92c8 <_js_array_push_f64>
10033b124:      and x25, x0, #0xffffffffffff
10033b128:      b   0x10033b31c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x304>
10033b12c:      mov w25, #0x1               ; =1
10033b130:      str x8, [x20]
10033b134:      ldp w26, w8, [x25]
10033b138:      cmp w26, w8
10033b13c:      b.hs    0x10033b118 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x100>
10033b140:      add x27, x25, x26, lsl #3
10033b144:      str x24, [x27, #0x8]!
10033b148:      mov x0, x25
10033b14c:      bl  0x10037afc4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
10033b150:      lsr x22, x25, #3
10033b154:      tbz w0, #0x0, 0x10033b178 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x160>
10033b158:      mov w8, #0x7ffe             ; =32766
10033b15c:      cmp x8, x24, lsr #48
10033b160:      b.ne    0x10033b198 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x180>
10033b164:      mov x0, x24
10033b168:      bl  0x1008033fc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10033b16c:      tbnz    w0, #0x0, 0x10033b1b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
10033b170:      scvtf   d0, w24
10033b174:      b   0x10033b1b0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x198>
10033b178:      cmp x22, #0x201
10033b17c:      b.lo    0x10033b1b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
10033b180:      ldurb   w8, [x25, #-0x8]
10033b184:      cmp w8, #0x1
10033b188:      b.ne    0x10033b1b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
10033b18c:      ldurh   w8, [x25, #-0x6]
10033b190:      tbnz    w8, #0xc, 0x10033b158 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x140>
10033b194:      b   0x10033b1b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
10033b198:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
10033b19c:      cmp x24, x8
10033b1a0:      b.gt    0x10033b1b4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x19c>
10033b1a4:      fmov    d0, x24
10033b1a8:      fcmp    d0, d0
10033b1ac:      fcsel   d0, d8, d0, vs
10033b1b0:      fmov    x24, d0
10033b1b4:      str x24, [x27]
10033b1b8:      mov w8, #0x7ffe             ; =32766
10033b1bc:      cmp x8, x24, lsr #48
10033b1c0:      b.ne    0x10033b1e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x1c8>
10033b1c4:      mov x0, x24
10033b1c8:      bl  0x1008033fc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10033b1cc:      tbnz    w0, #0x0, 0x10033b1ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x1d4>
10033b1d0:      scvtf   d0, w24
10033b1d4:      cmp x22, #0x201
10033b1d8:      b.hs    0x10033b23c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x224>
10033b1dc:      b   0x10033b260 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10033b1e0:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
10033b1e4:      cmp x24, x8
10033b1e8:      b.le    0x10033b228 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x210>
10033b1ec:      cmp x22, #0x201
10033b1f0:      b.lo    0x10033b260 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10033b1f4:      ldurb   w8, [x25, #-0x8]
10033b1f8:      cmp w8, #0x1
10033b1fc:      b.ne    0x10033b260 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10033b200:      ldurh   w8, [x25, #-0x6]
10033b204:      mov w9, #0xef7f             ; =61311
10033b208:      and w9, w8, w9
10033b20c:      sturh   w9, [x25, #-0x6]
10033b210:      mov w9, #0x1080             ; =4224
10033b214:      tst w8, w9
10033b218:      b.eq    0x10033b260 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10033b21c:      mov x0, x25
10033b220:      bl  0x10066de20 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
10033b224:      b   0x10033b260 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10033b228:      fmov    d0, x24
10033b22c:      fcmp    d0, d0
10033b230:      fcsel   d0, d8, d0, vs
10033b234:      cmp x22, #0x201
10033b238:      b.lo    0x10033b260 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10033b23c:      ldurb   w8, [x25, #-0x8]
10033b240:      cmp w8, #0x1
10033b244:      b.ne    0x10033b260 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10033b248:      ldurh   w8, [x25, #-0x6]
10033b24c:      tbz w8, #0x7, 0x10033b260 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10033b250:      ldr w8, [x25]
10033b254:      cmp w26, w8
10033b258:      b.hs    0x10033b260 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x248>
10033b25c:      str d0, [x27]
10033b260:      mov x0, x25
10033b264:      mov x1, x26
10033b268:      mov x2, x24
10033b26c:      bl  0x100242640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
10033b270:      mov x0, x25
10033b274:      bl  0x10036cee4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
10033b278:      mov x22, #0x7fffffffffffffff ; =9223372036854775807
10033b27c:      cbz w0, 0x10033b310 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
10033b280:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
10033b284:      add x8, x8, #0x1b8
10033b288:      ldapr   x8, [x8]
10033b28c:      cbnz    x8, 0x10033b2e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2c8>
10033b290:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
10033b294:      ldrb    w8, [x8, #0x1c0]
10033b298:      tbz w8, #0x0, 0x10033b2f8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2e0>
10033b29c:      mov x0, x25
10033b2a0:      mov x1, x27
10033b2a4:      mov x2, x24
10033b2a8:      mov w3, #0x0                ; =0
10033b2ac:      bl  0x100680e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
10033b2b0:      b   0x10033b310 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
10033b2b4:      cmp w8, #0x2
10033b2b8:      b.eq    0x10033b538 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
10033b2bc:      mov x0, x20
10033b2c0:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10033b2c4:      add x1, x1, #0x87c
10033b2c8:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10033b2cc:      strb    wzr, [x20, #0x20]
10033b2d0:      ldr x8, [x20]
10033b2d4:      cmp x8, x22
10033b2d8:      b.lo    0x10033b0e8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0xd0>
10033b2dc:      b   0x10033b568 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
10033b2e0:      adrp    x0, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
10033b2e4:      add x0, x0, #0x1b8
10033b2e8:      bl  0x100cc433c <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
10033b2ec:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
10033b2f0:      ldrb    w8, [x8, #0x1c0]
10033b2f4:      tbnz    w8, #0x0, 0x10033b29c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x284>
10033b2f8:      adrp    x8, 0x101201000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc11instruments24INCREMENTAL_CYCLE_STARTS>
10033b2fc:      add x8, x8, #0x184
10033b300:      ldr w8, [x8]
10033b304:      cbz w8, 0x10033b310 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x2f8>
10033b308:      mov x0, x24
10033b30c:      bl  0x100682074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
10033b310:      add w8, w26, #0x1
10033b314:      str w8, [x25]
10033b318:      mov x27, #-0x1              ; =-1
10033b31c:      ldrb    w8, [x20, #0x20]
10033b320:      cbnz    w8, 0x10033b3e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3c8>
10033b324:      ldr x8, [x20]
10033b328:      cbnz    x8, 0x10033b404 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3ec>
10033b32c:      str x27, [x20]
10033b330:      ldr x8, [x20, #0x18]
10033b334:      cmp x28, x8
10033b338:      b.hs    0x10033b368 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x350>
10033b33c:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
10033b340:      orr x8, x25, x8
10033b344:      ldr x9, [x20, #0x10]
10033b348:      str x8, [x9, x28, lsl #3]
10033b34c:      ldr x8, [x20]
10033b350:      add x8, x8, #0x1
10033b354:      str x8, [x20]
10033b358:      ldp x9, x8, [x21, #0x30]
10033b35c:      cmp x8, x9
10033b360:      b.lo    0x10033b37c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x364>
10033b364:      b   0x10033b3a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
10033b368:      mov x8, #0x0                ; =0
10033b36c:      str x8, [x20]
10033b370:      ldp x9, x8, [x21, #0x30]
10033b374:      cmp x8, x9
10033b378:      b.hs    0x10033b3a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
10033b37c:      ldr x10, [x21, #0x28]
10033b380:      ldrb    w11, [x10, x8]
10033b384:      cmp w11, #0x20
10033b388:      b.hi    0x10033b3a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
10033b38c:      lsr x11, x23, x11
10033b390:      tbz w11, #0x0, 0x10033b3a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x390>
10033b394:      add x8, x8, #0x1
10033b398:      str x8, [x21, #0x38]
10033b39c:      cmp x9, x8
10033b3a0:      b.ne    0x10033b380 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x368>
10033b3a4:      b   0x10033b464 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
10033b3a8:      cmp x8, x9
10033b3ac:      b.hs    0x10033b414 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3fc>
10033b3b0:      ldr x10, [x21, #0x28]
10033b3b4:      ldrb    w10, [x10, x8]
10033b3b8:      cmp w10, #0x2c
10033b3bc:      b.ne    0x10033b414 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3fc>
10033b3c0:      add x8, x8, #0x1
10033b3c4:      str x8, [x21, #0x38]
10033b3c8:      mov x0, x21
10033b3cc:      bl  0x10033aaa0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10033b3d0:      mov x24, x0
10033b3d4:      ldrb    w8, [x21, #0x90]
10033b3d8:      tbnz    w8, #0x0, 0x10033b0d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0xbc>
10033b3dc:      b   0x10033b410 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x3f8>
10033b3e0:      cmp w8, #0x2
10033b3e4:      b.eq    0x10033b538 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
10033b3e8:      mov x0, x20
10033b3ec:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10033b3f0:      add x1, x1, #0x87c
10033b3f4:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10033b3f8:      strb    wzr, [x20, #0x20]
10033b3fc:      ldr x8, [x20]
10033b400:      cbz x8, 0x10033b32c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x314>
10033b404:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10033b408:      add x0, x0, #0xde0
10033b40c:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10033b410:      ldp x9, x8, [x21, #0x30]
10033b414:      cmp x8, x9
10033b418:      b.hs    0x10033b464 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
10033b41c:      ldr x10, [x21, #0x28]
10033b420:      mov x11, #0x2600            ; =9728
10033b424:      movk    x11, #0x1, lsl #32
10033b428:      ldrb    w12, [x10, x8]
10033b42c:      cmp w12, #0x20
10033b430:      b.hi    0x10033b450 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x438>
10033b434:      lsr x13, x11, x12
10033b438:      tbz w13, #0x0, 0x10033b450 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x438>
10033b43c:      add x8, x8, #0x1
10033b440:      str x8, [x21, #0x38]
10033b444:      cmp x9, x8
10033b448:      b.ne    0x10033b428 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x410>
10033b44c:      b   0x10033b464 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
10033b450:      cmp w12, #0x5d
10033b454:      b.ne    0x10033b464 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x44c>
10033b458:      add x8, x8, #0x1
10033b45c:      str x8, [x21, #0x38]
10033b460:      b   0x10033b468 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x450>
10033b464:      strb    wzr, [x21, #0x90]
10033b468:      ldrb    w8, [x20, #0x20]
10033b46c:      cbnz    w8, 0x10033b530 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x518>
10033b470:      ldr x8, [x20]
10033b474:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10033b478:      cmp x8, x9
10033b47c:      b.hs    0x10033b568 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x550>
10033b480:      add x9, x8, #0x1
10033b484:      str x9, [x20]
10033b488:      ldr x9, [x20, #0x18]
10033b48c:      cmp x28, x9
10033b490:      b.hs    0x10033b4a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x490>
10033b494:      ldr x9, [x20, #0x10]
10033b498:      ldr x9, [x9, x28, lsl #3]
10033b49c:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
10033b4a0:      bfxil   x0, x9, #0, #48
10033b4a4:      b   0x10033b4b0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x498>
10033b4a8:      mov x0, #0x1                ; =1
10033b4ac:      movk    x0, #0x7ffd, lsl #48
10033b4b0:      str x8, [x20]
10033b4b4:      cbnz    x8, 0x10033b4e8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4d0>
10033b4b8:      ldr x8, [x20, #0x18]
10033b4bc:      cmp x19, x8
10033b4c0:      b.hi    0x10033b4c8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4b0>
10033b4c4:      str x19, [x20, #0x18]
10033b4c8:      ldp x29, x30, [sp, #0x60]
10033b4cc:      ldp x20, x19, [sp, #0x50]
10033b4d0:      ldp x22, x21, [sp, #0x40]
10033b4d4:      ldp x24, x23, [sp, #0x30]
10033b4d8:      ldp x26, x25, [sp, #0x20]
10033b4dc:      ldp x28, x27, [sp, #0x10]
10033b4e0:      ldp d9, d8, [sp], #0x70
10033b4e4:      ret
10033b4e8:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10033b4ec:      add x0, x0, #0xe58
10033b4f0:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10033b4f4:      cmp w8, #0x1
10033b4f8:      b.ne    0x10033b538 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x520>
10033b4fc:      adrp    x8, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10033b500:      add x8, x8, #0x87c
10033b504:      mov x0, x20
10033b508:      mov x22, x1
10033b50c:      mov x1, x8
10033b510:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10033b514:      mov x1, x22
10033b518:      strb    wzr, [x20, #0x20]
10033b51c:      ldr x8, [x20]
10033b520:      cbz x8, 0x10033b064 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x4c>
10033b524:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10033b528:      add x0, x0, #0xdf8
10033b52c:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10033b530:      cmp w8, #0x2
10033b534:      b.ne    0x10033b544 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x52c>
10033b538:      adrp    x0, 0x1010a3000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10033b53c:      add x0, x0, #0xed8
10033b540:      bl  0x100ce071c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10033b544:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10033b548:      add x1, x1, #0x87c
10033b54c:      mov x0, x20
10033b550:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10033b554:      strb    wzr, [x20, #0x20]
10033b558:      ldr x8, [x20]
10033b55c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10033b560:      cmp x8, x9
10033b564:      b.lo    0x10033b480 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail+0x468>
10033b568:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10033b56c:      add x0, x0, #0xdc8
10033b570:      bl  0x100c9de9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
