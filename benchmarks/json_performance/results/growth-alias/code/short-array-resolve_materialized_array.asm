/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/short-array-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010090afb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array>:
10090afb4:      stp x26, x25, [sp, #-0x50]!
10090afb8:      stp x24, x23, [sp, #0x10]
10090afbc:      stp x22, x21, [sp, #0x20]
10090afc0:      stp x20, x19, [sp, #0x30]
10090afc4:      stp x29, x30, [sp, #0x40]
10090afc8:      add x29, sp, #0x40
10090afcc:      mov x19, x0
10090afd0:      ldr x23, [x19, #0x20]!
10090afd4:      cbz x23, 0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090afd8:      lsr x8, x23, #51
10090afdc:      mov x21, x23
10090afe0:      cmp x8, #0xfff
10090afe4:      b.lo    0x10090affc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x48>
10090afe8:      mov w8, #0x7ffc             ; =32764
10090afec:      cmp x8, x23, lsr #48
10090aff0:      b.eq    0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090aff4:      ands    x21, x23, #0xffffffffffff
10090aff8:      b.eq    0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090affc:      and x8, x21, #0xfffffffffff00000
10090b000:      lsr x9, x21, #47
10090b004:      cmp x9, #0x0
10090b008:      ccmp    x8, #0x0, #0x4, eq
10090b00c:      b.eq    0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090b010:      tst x21, #0x3
10090b014:      ccmp    x21, #0x7, #0x0, eq
10090b018:      mov x20, x0
10090b01c:      b.ls    0x10090b12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
10090b020:      adrp    x8, 0x10112c000 <_perry_global_baseline_worker_ts__1>
10090b024:      add x8, x8, #0x2c8
10090b028:      ldr x8, [x8]
10090b02c:      cmn x8, #0x1
10090b030:      b.eq    0x10090b430 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x47c>
10090b034:      mrs x9, TPIDRRO_EL0
10090b038:      and x9, x9, #0xfffffffffffffff8
10090b03c:      ldr x8, [x9, x8, lsl #3]
10090b040:      cbz x8, 0x10090b430 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x47c>
10090b044:      lsr x1, x21, #20
10090b048:      ldr x8, [x8, #0x10]
10090b04c:      ldrb    w9, [x8, #0x28]
10090b050:      tbz w9, #0x0, 0x10090b070 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
10090b054:      ldr x9, [x8, #0x20]
10090b058:      cmp x9, x1
10090b05c:      b.ne    0x10090b070 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
10090b060:      ldp x9, x10, [x8]
10090b064:      cmp x9, x21
10090b068:      ccmp    x10, x21, #0x0, ls
10090b06c:      b.hi    0x10090b0ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
10090b070:      ldrb    w9, [x8, #0x58]
10090b074:      cbz w9, 0x10090b094 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xe0>
10090b078:      ldr x9, [x8, #0x50]
10090b07c:      cmp x9, x1
10090b080:      b.ne    0x10090b094 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xe0>
10090b084:      ldp x9, x10, [x8, #0x30]
10090b088:      cmp x9, x21
10090b08c:      ccmp    x10, x21, #0x0, ls
10090b090:      b.hi    0x10090b0e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x12c>
10090b094:      ldrb    w9, [x8, #0x88]
10090b098:      cbz w9, 0x10090b0b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x104>
10090b09c:      ldr x9, [x8, #0x80]
10090b0a0:      cmp x9, x1
10090b0a4:      b.ne    0x10090b0b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x104>
10090b0a8:      ldp x9, x10, [x8, #0x60]
10090b0ac:      cmp x9, x21
10090b0b0:      ccmp    x10, x21, #0x0, ls
10090b0b4:      b.hi    0x10090b0e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x134>
10090b0b8:      ldrb    w9, [x8, #0xb8]
10090b0bc:      cbz w9, 0x10090b0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
10090b0c0:      ldr x9, [x8, #0xb0]
10090b0c4:      cmp x9, x1
10090b0c8:      b.ne    0x10090b0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
10090b0cc:      ldp x9, x10, [x8, #0x90]!
10090b0d0:      cmp x9, x21
10090b0d4:      ccmp    x10, x21, #0x0, ls
10090b0d8:      b.hi    0x10090b0ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
10090b0dc:      b   0x10090b0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
10090b0e0:      add x8, x8, #0x30
10090b0e4:      b   0x10090b0ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
10090b0e8:      add x8, x8, #0x60
10090b0ec:      ldrb    w8, [x8, #0x19]
10090b0f0:      cmp w8, #0xff
10090b0f4:      b.ne    0x10090b10c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x158>
10090b0f8:      mov x0, x21
10090b0fc:      bl  0x100559228 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
10090b100:      mov x8, x0
10090b104:      mov x0, x20
10090b108:      and w8, w8, #0xff
10090b10c:      cbz w8, 0x10090b12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
10090b110:      ldurb   w8, [x21, #-0x8]
10090b114:      ldurb   w9, [x21, #-0x7]
10090b118:      mov w10, #0x82              ; =130
10090b11c:      and w9, w9, w10
10090b120:      cmp w9, #0x2
10090b124:      ccmp    w8, #0x1, #0x0, eq
10090b128:      b.eq    0x10090b24c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x298>
10090b12c:      mov x0, x21
10090b130:      bl  0x1008e3210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
10090b134:      cbz x0, 0x10090b164 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x1b0>
10090b138:      ldrb    w9, [x0]
10090b13c:      cmp w9, #0x1
10090b140:      b.ne    0x10090b208 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x254>
10090b144:      ldrsb   w8, [x0, #0x1]
10090b148:      tbnz    w8, #0x1f, 0x10090b278 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x2c4>
10090b14c:      mov x8, x0
10090b150:      mov x0, x20
10090b154:      ldp w10, w9, [x21]
10090b158:      cmp w10, w9
10090b15c:      b.hi    0x10090b310 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x35c>
10090b160:      b   0x10090b32c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x378>
10090b164:      adrp    x8, 0x101178000 <_out_buf+0x3f08>
10090b168:      add x8, x8, #0x71b
10090b16c:      ldaprb  w8, [x8]
10090b170:      cbz w8, 0x10090b1b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
10090b174:      adrp    x8, 0x10112c000 <_perry_global_baseline_worker_ts__1>
10090b178:      add x8, x8, #0x2d0
10090b17c:      ldapr   x9, [x8]
10090b180:      cmp x9, x21
10090b184:      b.hi    0x10090b1b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
10090b188:      ldapur  x8, [x8, #0x8]
10090b18c:      cmp x8, x21
10090b190:      b.lo    0x10090b1b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
10090b194:      mov x24, x0
10090b198:      mov x0, x21
10090b19c:      bl  0x10019d27c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
10090b1a0:      mov x8, x0
10090b1a4:      mov x0, x24
10090b1a8:      tbz w8, #0x0, 0x10090b1b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
10090b1ac:      mov x8, #0x0                ; =0
10090b1b0:      b   0x10090b2fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x348>
10090b1b4:      adrp    x8, 0x1011f9000 <_PERRY_TA_KIND_CACHE+0x10>
10090b1b8:      add x8, x8, #0x478
10090b1bc:      ldaprb  w8, [x8]
10090b1c0:      cbz w8, 0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090b1c4:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
10090b1c8:      add x8, x8, #0x4b8
10090b1cc:      ldapr   x9, [x8]
10090b1d0:      cmp x9, x21
10090b1d4:      b.hi    0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090b1d8:      ldapur  x8, [x8, #0x8]
10090b1dc:      cmp x8, x21
10090b1e0:      b.lo    0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090b1e4:      mov x24, x0
10090b1e8:      mov x0, x21
10090b1ec:      bl  0x10063ed2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
10090b1f0:      mov x9, x0
10090b1f4:      mov x8, #0x0                ; =0
10090b1f8:      mov x22, #0x0               ; =0
10090b1fc:      mov x0, x20
10090b200:      tbnz    w9, #0x0, 0x10090b300 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x34c>
10090b204:      b   0x10090b414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
10090b208:      mov x24, x0
10090b20c:      mov x8, x0
10090b210:      cmp w9, #0x1
10090b214:      b.eq    0x10090b2fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x348>
10090b218:      cmp w9, #0x9
10090b21c:      b.ne    0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090b220:      ldr w8, [x21, #0x4]
10090b224:      mov w9, #0x5841             ; =22593
10090b228:      movk    w9, #0x4c5a, lsl #16
10090b22c:      cmp w8, w9
10090b230:      b.ne    0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090b234:      mov x0, x21
10090b238:      bl  0x1008b9a34 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape22force_materialize_lazy>
10090b23c:      mov x22, x0
10090b240:      mov x0, x20
10090b244:      cbnz    x22, 0x10090b3d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
10090b248:      b   0x10090b414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
10090b24c:      ldr w8, [x21]
10090b250:      mov w9, #0xe100             ; =57600
10090b254:      movk    w9, #0x5f5, lsl #16
10090b258:      orr w9, w9, #0x1
10090b25c:      cmp w8, w9
10090b260:      b.hs    0x10090b12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
10090b264:      ldr w9, [x21, #0x4]
10090b268:      cmp w8, w9
10090b26c:      b.hi    0x10090b12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
10090b270:      mov x22, x21
10090b274:      b   0x10090b3d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
10090b278:      mov x24, x0
10090b27c:      ldr x21, [x0, #0x8]
10090b280:      mov x0, x21
10090b284:      bl  0x1008e3210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
10090b288:      cbz x0, 0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090b28c:      mov x8, x0
10090b290:      ldrb    w9, [x0]
10090b294:      cmp w9, #0x1
10090b298:      b.ne    0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090b29c:      ldrsb   w9, [x8, #0x1]
10090b2a0:      tbz w9, #0x1f, 0x10090b150 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x19c>
10090b2a4:      mov w25, #0x1               ; =1
10090b2a8:      ldr x21, [x8, #0x8]
10090b2ac:      mov x0, x21
10090b2b0:      bl  0x1008e3210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
10090b2b4:      cbz x0, 0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090b2b8:      mov x8, x0
10090b2bc:      mov x22, #0x0               ; =0
10090b2c0:      ldrb    w9, [x0]
10090b2c4:      cmp w9, #0x1
10090b2c8:      b.ne    0x10090b414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
10090b2cc:      cmp w25, #0x3f
10090b2d0:      b.hi    0x10090b414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
10090b2d4:      add w25, w25, #0x1
10090b2d8:      ldrsb   w9, [x8, #0x1]
10090b2dc:      tbnz    w9, #0x1f, 0x10090b2a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x2f4>
10090b2e0:      str x21, [x24, #0x8]
10090b2e4:      ldrb    w10, [x24, #0x1]
10090b2e8:      orr w10, w10, #0x80
10090b2ec:      strb    w10, [x24, #0x1]
10090b2f0:      ldrb    w9, [x8]
10090b2f4:      cmp w9, #0x1
10090b2f8:      b.ne    0x10090b218 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x264>
10090b2fc:      mov x0, x20
10090b300:      ldp w10, w9, [x21]
10090b304:      cmp w10, w9
10090b308:      b.ls    0x10090b32c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x378>
10090b30c:      cbz x24, 0x10090b340 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x38c>
10090b310:      ldr w8, [x8, #0x4]
10090b314:      ubfiz   x9, x9, #3, #32
10090b318:      add x9, x9, #0x10
10090b31c:      mov x22, x21
10090b320:      cmp x9, x8
10090b324:      b.eq    0x10090b3d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
10090b328:      b   0x10090b340 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x38c>
10090b32c:      mov w8, #0xe100             ; =57600
10090b330:      movk    w8, #0x5f5, lsl #16
10090b334:      mov x22, x21
10090b338:      cmp w10, w8
10090b33c:      b.ls    0x10090b3d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
10090b340:      adrp    x8, 0x101178000 <_out_buf+0x3f08>
10090b344:      add x8, x8, #0x71b
10090b348:      ldaprb  w8, [x8]
10090b34c:      cbz w8, 0x10090b388 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
10090b350:      adrp    x8, 0x10112c000 <_perry_global_baseline_worker_ts__1>
10090b354:      add x8, x8, #0x2d0
10090b358:      ldapr   x9, [x8]
10090b35c:      cmp x9, x21
10090b360:      b.hi    0x10090b388 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
10090b364:      ldapur  x8, [x8, #0x8]
10090b368:      cmp x8, x21
10090b36c:      b.lo    0x10090b388 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
10090b370:      mov x0, x21
10090b374:      bl  0x10019d27c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
10090b378:      tbz w0, #0x0, 0x10090b388 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
10090b37c:      mov x0, x20
10090b380:      mov x22, x21
10090b384:      b   0x10090b3d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
10090b388:      adrp    x8, 0x1011f9000 <_PERRY_TA_KIND_CACHE+0x10>
10090b38c:      add x8, x8, #0x478
10090b390:      ldaprb  w8, [x8]
10090b394:      cbz w8, 0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090b398:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
10090b39c:      add x8, x8, #0x4b8
10090b3a0:      ldapr   x9, [x8]
10090b3a4:      cmp x21, x9
10090b3a8:      b.lo    0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090b3ac:      ldapur  x8, [x8, #0x8]
10090b3b0:      cmp x21, x8
10090b3b4:      b.hi    0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090b3b8:      mov x0, x21
10090b3bc:      bl  0x10063ed2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
10090b3c0:      mov x8, x0
10090b3c4:      mov x0, x20
10090b3c8:      mov x22, x21
10090b3cc:      tbz w8, #0x0, 0x10090b410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10090b3d0:      cmp x22, x23
10090b3d4:      b.eq    0x10090b4ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
10090b3d8:      str x22, [x0, #0x20]
10090b3dc:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
10090b3e0:      add x8, x8, #0x28
10090b3e4:      ldapr   x8, [x8]
10090b3e8:      cbnz    x8, 0x10090b450 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x49c>
10090b3ec:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
10090b3f0:      ldrb    w8, [x8, #0x30]
10090b3f4:      tbz w8, #0x0, 0x10090b46c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4b8>
10090b3f8:      mov x0, x20
10090b3fc:      mov x1, x19
10090b400:      mov x2, x22
10090b404:      mov w3, #0x0                ; =0
10090b408:      bl  0x100553db4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
10090b40c:      b   0x10090b484 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4d0>
10090b410:      mov x22, #0x0               ; =0
10090b414:      mov x0, x22
10090b418:      ldp x29, x30, [sp, #0x40]
10090b41c:      ldp x20, x19, [sp, #0x30]
10090b420:      ldp x22, x21, [sp, #0x20]
10090b424:      ldp x24, x23, [sp, #0x10]
10090b428:      ldp x26, x25, [sp], #0x50
10090b42c:      ret
10090b430:      bl  0x100cb9d4c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
10090b434:      mov x8, x0
10090b438:      mov x0, x20
10090b43c:      lsr x1, x21, #20
10090b440:      ldr x8, [x8, #0x10]
10090b444:      ldrb    w9, [x8, #0x28]
10090b448:      tbnz    w9, #0x0, 0x10090b054 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xa0>
10090b44c:      b   0x10090b070 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
10090b450:      adrp    x0, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
10090b454:      add x0, x0, #0x28
10090b458:      bl  0x100cc1e44 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
10090b45c:      mov x0, x20
10090b460:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
10090b464:      ldrb    w8, [x8, #0x30]
10090b468:      tbnz    w8, #0x0, 0x10090b3f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x444>
10090b46c:      adrp    x8, 0x1011f8000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f448>
10090b470:      add x8, x8, #0xfe0
10090b474:      ldr w8, [x8]
10090b478:      cbz w8, 0x10090b488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4d4>
10090b47c:      mov x0, x22
10090b480:      bl  0x100555294 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
10090b484:      mov x0, x20
10090b488:      ldr x8, [x19]
10090b48c:      cbz x8, 0x10090b4ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
10090b490:      ldr x8, [x0, #0x10]
10090b494:      cbz x8, 0x10090b4ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
10090b498:      mov x0, x20
10090b49c:      bl  0x1007c4a7c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime15json_tape_store7release>
10090b4a0:      mov x0, x20
10090b4a4:      str xzr, [x20, #0x10]
10090b4a8:      str wzr, [x20, #0xc]
10090b4ac:      ldr w8, [x22]
10090b4b0:      str w8, [x0]
10090b4b4:      b   0x10090b414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
