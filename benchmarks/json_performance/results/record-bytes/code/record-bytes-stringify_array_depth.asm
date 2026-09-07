/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008fc048 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth>:
1008fc048:      sub sp, sp, #0xf0
1008fc04c:      stp d9, d8, [sp, #0x80]
1008fc050:      stp x28, x27, [sp, #0x90]
1008fc054:      stp x26, x25, [sp, #0xa0]
1008fc058:      stp x24, x23, [sp, #0xb0]
1008fc05c:      stp x22, x21, [sp, #0xc0]
1008fc060:      stp x20, x19, [sp, #0xd0]
1008fc064:      stp x29, x30, [sp, #0xe0]
1008fc068:      add x29, sp, #0xe0
1008fc06c:      cmp w2, #0x3e9
1008fc070:      b.hs    0x1008fdf94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f4c>
1008fc074:      mov x20, x2
1008fc078:      mov x19, x1
1008fc07c:      mov x22, x0
1008fc080:      lsr x8, x0, #51
1008fc084:      cmp x8, #0xfff
1008fc088:      b.lo    0x1008fc0a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x58>
1008fc08c:      mov w8, #0x7ffc             ; =32764
1008fc090:      cmp x8, x22, lsr #48
1008fc094:      b.eq    0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc098:      ands    x22, x22, #0xffffffffffff
1008fc09c:      b.eq    0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc0a0:      and x8, x22, #0xfffffffffff00000
1008fc0a4:      lsr x9, x22, #47
1008fc0a8:      cmp x9, #0x0
1008fc0ac:      ccmp    x8, #0x0, #0x4, eq
1008fc0b0:      b.eq    0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc0b4:      tst x22, #0x3
1008fc0b8:      ccmp    x22, #0x7, #0x0, eq
1008fc0bc:      b.ls    0x1008fc1c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
1008fc0c0:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
1008fc0c4:      add x8, x8, #0x8f0
1008fc0c8:      ldr x8, [x8]
1008fc0cc:      cmn x8, #0x1
1008fc0d0:      b.eq    0x1008fce60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe18>
1008fc0d4:      mrs x9, TPIDRRO_EL0
1008fc0d8:      and x9, x9, #0xfffffffffffffff8
1008fc0dc:      ldr x0, [x9, x8, lsl #3]
1008fc0e0:      cbz x0, 0x1008fce60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe18>
1008fc0e4:      lsr x1, x22, #20
1008fc0e8:      ldr x8, [x0, #0x10]
1008fc0ec:      ldrb    w9, [x8, #0x28]
1008fc0f0:      tbz w9, #0x0, 0x1008fc110 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc8>
1008fc0f4:      ldr x9, [x8, #0x20]
1008fc0f8:      cmp x9, x1
1008fc0fc:      b.ne    0x1008fc110 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc8>
1008fc100:      ldp x9, x10, [x8]
1008fc104:      cmp x9, x22
1008fc108:      ccmp    x10, x22, #0x0, ls
1008fc10c:      b.hi    0x1008fc18c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x144>
1008fc110:      ldrb    w9, [x8, #0x58]
1008fc114:      cbz w9, 0x1008fc134 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xec>
1008fc118:      ldr x9, [x8, #0x50]
1008fc11c:      cmp x9, x1
1008fc120:      b.ne    0x1008fc134 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xec>
1008fc124:      ldp x9, x10, [x8, #0x30]
1008fc128:      cmp x9, x22
1008fc12c:      ccmp    x10, x22, #0x0, ls
1008fc130:      b.hi    0x1008fc180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x138>
1008fc134:      ldrb    w9, [x8, #0x88]
1008fc138:      cbz w9, 0x1008fc158 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x110>
1008fc13c:      ldr x9, [x8, #0x80]
1008fc140:      cmp x9, x1
1008fc144:      b.ne    0x1008fc158 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x110>
1008fc148:      ldp x9, x10, [x8, #0x60]
1008fc14c:      cmp x9, x22
1008fc150:      ccmp    x10, x22, #0x0, ls
1008fc154:      b.hi    0x1008fc188 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x140>
1008fc158:      ldrb    w9, [x8, #0xb8]
1008fc15c:      cbz w9, 0x1008fc198 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x150>
1008fc160:      ldr x9, [x8, #0xb0]
1008fc164:      cmp x9, x1
1008fc168:      b.ne    0x1008fc198 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x150>
1008fc16c:      ldp x9, x10, [x8, #0x90]!
1008fc170:      cmp x9, x22
1008fc174:      ccmp    x10, x22, #0x0, ls
1008fc178:      b.hi    0x1008fc18c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x144>
1008fc17c:      b   0x1008fc198 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x150>
1008fc180:      add x8, x8, #0x30
1008fc184:      b   0x1008fc18c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x144>
1008fc188:      add x8, x8, #0x60
1008fc18c:      ldrb    w8, [x8, #0x19]
1008fc190:      cmp w8, #0xff
1008fc194:      b.ne    0x1008fc1a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x15c>
1008fc198:      mov x0, x22
1008fc19c:      bl  0x10045cfb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
1008fc1a0:      and w8, w0, #0xff
1008fc1a4:      cbz w8, 0x1008fc1c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
1008fc1a8:      ldurb   w8, [x22, #-0x8]
1008fc1ac:      ldurb   w9, [x22, #-0x7]
1008fc1b0:      mov w10, #0x82              ; =130
1008fc1b4:      and w9, w9, w10
1008fc1b8:      cmp w9, #0x2
1008fc1bc:      ccmp    w8, #0x1, #0x0, eq
1008fc1c0:      b.eq    0x1008fc43c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3f4>
1008fc1c4:      mov x0, x22
1008fc1c8:      bl  0x100902d58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008fc1cc:      mov x8, x0
1008fc1d0:      cbz x0, 0x1008fc264 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x21c>
1008fc1d4:      ldrb    w9, [x8]
1008fc1d8:      cmp w9, #0x1
1008fc1dc:      b.ne    0x1008fc2f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2ac>
1008fc1e0:      ldrsb   w9, [x8, #0x1]
1008fc1e4:      mov x0, x8
1008fc1e8:      tbz w9, #0x1f, 0x1008fc334 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2ec>
1008fc1ec:      mov x21, x8
1008fc1f0:      ldr x22, [x8, #0x8]
1008fc1f4:      mov x0, x22
1008fc1f8:      bl  0x100902d58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008fc1fc:      cbz x0, 0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc200:      ldrb    w8, [x0]
1008fc204:      cmp w8, #0x1
1008fc208:      b.ne    0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc20c:      ldrsb   w8, [x0, #0x1]
1008fc210:      tbz w8, #0x1f, 0x1008fc2ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2a4>
1008fc214:      mov w23, #0x1               ; =1
1008fc218:      ldr x22, [x0, #0x8]
1008fc21c:      mov x0, x22
1008fc220:      bl  0x100902d58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008fc224:      cbz x0, 0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc228:      ldrb    w8, [x0]
1008fc22c:      cmp w8, #0x1
1008fc230:      b.ne    0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc234:      cmp w23, #0x3f
1008fc238:      b.hi    0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc23c:      add w23, w23, #0x1
1008fc240:      ldrsb   w8, [x0, #0x1]
1008fc244:      tbnz    w8, #0x1f, 0x1008fc218 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d0>
1008fc248:      mov x8, x21
1008fc24c:      str x22, [x21, #0x8]
1008fc250:      ldrb    w9, [x21, #0x1]
1008fc254:      orr w9, w9, #0x80
1008fc258:      strb    w9, [x21, #0x1]
1008fc25c:      ldrb    w9, [x0]
1008fc260:      b   0x1008fc2f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2b0>
1008fc264:      mov x21, x8
1008fc268:      adrp    x8, 0x101200000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7a8>
1008fc26c:      add x8, x8, #0xf2a
1008fc270:      ldaprb  w8, [x8]
1008fc274:      cbz w8, 0x1008fc2a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x25c>
1008fc278:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
1008fc27c:      add x8, x8, #0xbb0
1008fc280:      ldapr   x9, [x8]
1008fc284:      cmp x9, x22
1008fc288:      b.hi    0x1008fc2a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x25c>
1008fc28c:      ldapur  x8, [x8, #0x8]
1008fc290:      cmp x8, x22
1008fc294:      b.lo    0x1008fc2a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x25c>
1008fc298:      mov x0, x22
1008fc29c:      bl  0x1004c8a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
1008fc2a0:      tbnz    w0, #0x0, 0x1008fc2e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2a0>
1008fc2a4:      adrp    x8, 0x101211000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfc20>
1008fc2a8:      add x8, x8, #0xad0
1008fc2ac:      ldaprb  w8, [x8]
1008fc2b0:      cbz w8, 0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc2b4:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
1008fc2b8:      add x8, x8, #0x8c0
1008fc2bc:      ldapr   x8, [x8]
1008fc2c0:      cmp x8, x22
1008fc2c4:      b.hi    0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc2c8:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
1008fc2cc:      add x8, x8, #0x8c8
1008fc2d0:      ldapr   x8, [x8]
1008fc2d4:      cmp x8, x22
1008fc2d8:      b.lo    0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc2dc:      mov x0, x22
1008fc2e0:      bl  0x1008bdee8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
1008fc2e4:      tbz w0, #0x0, 0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc2e8:      mov x0, #0x0                ; =0
1008fc2ec:      mov x8, x21
1008fc2f0:      b   0x1008fc334 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2ec>
1008fc2f4:      mov x0, x8
1008fc2f8:      cmp w9, #0x1
1008fc2fc:      b.eq    0x1008fc334 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2ec>
1008fc300:      cmp w9, #0x9
1008fc304:      b.ne    0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc308:      ldr w8, [x22, #0x4]
1008fc30c:      mov w9, #0x5841             ; =22593
1008fc310:      movk    w9, #0x4c5a, lsl #16
1008fc314:      cmp w8, w9
1008fc318:      b.ne    0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc31c:      mov x0, x22
1008fc320:      bl  0x10035a198 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape22force_materialize_lazy>
1008fc324:      mov x22, x0
1008fc328:      str x0, [sp, #0x18]
1008fc32c:      cbnz    x0, 0x1008fc464 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x41c>
1008fc330:      b   0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc334:      ldp w10, w9, [x22]
1008fc338:      cmp w10, w9
1008fc33c:      b.ls    0x1008fc35c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x314>
1008fc340:      cbz x8, 0x1008fc36c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x324>
1008fc344:      ldr w8, [x0, #0x4]
1008fc348:      lsl x9, x9, #3
1008fc34c:      add x9, x9, #0x10
1008fc350:      cmp x9, x8
1008fc354:      b.ne    0x1008fc36c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x324>
1008fc358:      b   0x1008fc460 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x418>
1008fc35c:      mov w8, #0xe100             ; =57600
1008fc360:      movk    w8, #0x5f5, lsl #16
1008fc364:      cmp w10, w8
1008fc368:      b.ls    0x1008fc460 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x418>
1008fc36c:      adrp    x8, 0x101200000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7a8>
1008fc370:      add x8, x8, #0xf2a
1008fc374:      ldaprb  w8, [x8]
1008fc378:      cbz w8, 0x1008fc3a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x360>
1008fc37c:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
1008fc380:      add x8, x8, #0xbb0
1008fc384:      ldapr   x9, [x8]
1008fc388:      cmp x9, x22
1008fc38c:      b.hi    0x1008fc3a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x360>
1008fc390:      ldapur  x8, [x8, #0x8]
1008fc394:      cmp x8, x22
1008fc398:      b.lo    0x1008fc3a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x360>
1008fc39c:      mov x0, x22
1008fc3a0:      bl  0x1004c8a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
1008fc3a4:      tbnz    w0, #0x0, 0x1008fc460 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x418>
1008fc3a8:      adrp    x8, 0x101211000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfc20>
1008fc3ac:      add x8, x8, #0xad0
1008fc3b0:      ldaprb  w8, [x8]
1008fc3b4:      cbz w8, 0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc3b8:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
1008fc3bc:      add x8, x8, #0x8c0
1008fc3c0:      ldapr   x8, [x8]
1008fc3c4:      cmp x8, x22
1008fc3c8:      b.hi    0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc3cc:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
1008fc3d0:      add x8, x8, #0x8c8
1008fc3d4:      ldapr   x8, [x8]
1008fc3d8:      cmp x8, x22
1008fc3dc:      b.lo    0x1008fc3ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1008fc3e0:      mov x0, x22
1008fc3e4:      bl  0x1008bdee8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
1008fc3e8:      tbnz    w0, #0x0, 0x1008fc460 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x418>
1008fc3ec:      ldr x1, [x19, #0x10]
1008fc3f0:      ldr x8, [x19]
1008fc3f4:      sub x8, x8, x1
1008fc3f8:      cmp x8, #0x1
1008fc3fc:      b.ls    0x1008fd154 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x110c>
1008fc400:      ldr x8, [x19, #0x8]
1008fc404:      mov w9, #0x5d5b             ; =23899
1008fc408:      strh    w9, [x8, x1]
1008fc40c:      ldr x8, [x19, #0x10]
1008fc410:      add x8, x8, #0x2
1008fc414:      str x8, [x19, #0x10]
1008fc418:      ldp x29, x30, [sp, #0xe0]
1008fc41c:      ldp x20, x19, [sp, #0xd0]
1008fc420:      ldp x22, x21, [sp, #0xc0]
1008fc424:      ldp x24, x23, [sp, #0xb0]
1008fc428:      ldp x26, x25, [sp, #0xa0]
1008fc42c:      ldp x28, x27, [sp, #0x90]
1008fc430:      ldp d9, d8, [sp, #0x80]
1008fc434:      add sp, sp, #0xf0
1008fc438:      ret
1008fc43c:      ldr w8, [x22]
1008fc440:      mov w9, #0xe100             ; =57600
1008fc444:      movk    w9, #0x5f5, lsl #16
1008fc448:      orr w9, w9, #0x1
1008fc44c:      cmp w8, w9
1008fc450:      b.hs    0x1008fc1c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
1008fc454:      ldr w9, [x22, #0x4]
1008fc458:      cmp w8, w9
1008fc45c:      b.hi    0x1008fc1c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
1008fc460:      str x22, [sp, #0x18]
1008fc464:      mov x0, x22
1008fc468:      bl  0x1008faea4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify17array_get_to_json>
1008fc46c:      tbz w0, #0x0, 0x1008fc504 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4bc>
1008fc470:      fmov    x21, d0
1008fc474:      mov w8, #0x7ffd             ; =32765
1008fc478:      cmp x8, x21, lsr #48
1008fc47c:      b.ne    0x1008fce14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdcc>
1008fc480:      and x21, x21, #0xffffffffffff
1008fc484:      sub x8, x21, #0x100, lsl #12 ; =0x100000
1008fc488:      mov x9, #0x7ffffff00000     ; =140737487306752
1008fc48c:      cmp x8, x9
1008fc490:      b.hs    0x1008fce3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
1008fc494:      ldurb   w8, [x21, #-0x8]
1008fc498:      sub w8, w8, #0x1
1008fc49c:      cmp w8, #0x1
1008fc4a0:      b.hi    0x1008fce3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
1008fc4a4:      adrp    x8, 0x101200000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7a8>
1008fc4a8:      add x8, x8, #0xf2a
1008fc4ac:      ldaprb  w8, [x8]
1008fc4b0:      cbz w8, 0x1008fc4e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4a0>
1008fc4b4:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
1008fc4b8:      add x8, x8, #0xbb0
1008fc4bc:      ldapr   x9, [x8]
1008fc4c0:      cmp x21, x9
1008fc4c4:      b.lo    0x1008fc4e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4a0>
1008fc4c8:      ldapur  x8, [x8, #0x8]
1008fc4cc:      cmp x21, x8
1008fc4d0:      b.hi    0x1008fc4e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4a0>
1008fc4d4:      mov x0, x21
1008fc4d8:      mov.16b v8, v0
1008fc4dc:      bl  0x1004c8a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
1008fc4e0:      mov.16b v0, v8
1008fc4e4:      tbnz    w0, #0x0, 0x1008fce3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
1008fc4e8:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1008fc4ec:      add x0, x0, #0x4d0
1008fc4f0:      ldr x8, [x0]
1008fc4f4:      blr x8
1008fc4f8:      mov w8, #0x1                ; =1
1008fc4fc:      strb    w8, [x0]
1008fc500:      b   0x1008fce3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
1008fc504:      mov x0, x22
1008fc508:      bl  0x100902d58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008fc50c:      cbz x0, 0x1008fc53c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
1008fc510:      ldrb    w8, [x0]
1008fc514:      cmp w8, #0x1
1008fc518:      b.ne    0x1008fc53c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
1008fc51c:      ldrh    w21, [x0, #0x2]
1008fc520:      tbnz    w21, #0xa, 0x1008fc53c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
1008fc524:      ldp w8, w9, [x22]
1008fc528:      cmp w8, w9
1008fc52c:      b.hi    0x1008fc53c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
1008fc530:      mov x0, x22
1008fc534:      bl  0x100901f64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header35array_has_named_properties_resolved>
1008fc538:      tbz w0, #0x0, 0x1008fce78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe30>
1008fc53c:      adrp    x0, 0x1010dd000 <_anon.3c709ec65efe22d27798c2815252f2a2.778+0x188>
1008fc540:      add x0, x0, #0x20
1008fc544:      add x1, sp, #0x18
1008fc548:      bl  0x1001386c8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths_0bEB2j_>
1008fc54c:      tbnz    w0, #0x0, 0x1008fe06c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2024>
1008fc550:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1008fc554:      add x0, x0, #0x4b8
1008fc558:      ldr x8, [x0]
1008fc55c:      blr x8
1008fc560:      mov x24, x0
1008fc564:      ldrb    w8, [x0, #0x20]
1008fc568:      cbnz    w8, 0x1008fdd2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ce4>
1008fc56c:      ldr x8, [x24]
1008fc570:      cbnz    x8, 0x1008fdd50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d08>
1008fc574:      mov x8, #-0x1               ; =-1
1008fc578:      str x8, [x24]
1008fc57c:      mov x0, x24
1008fc580:      ldr x8, [x0, #0x8]!
1008fc584:      ldr x21, [x24, #0x18]
1008fc588:      cmp x21, x8
1008fc58c:      b.ne    0x1008fc594 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x54c>
1008fc590:      bl  0x100ce1000 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecjE8grow_oneCs3HfcutmYuk_10swc_common>
1008fc594:      ldr x8, [x24, #0x10]
1008fc598:      str x22, [x8, x21, lsl #3]
1008fc59c:      add x8, x21, #0x1
1008fc5a0:      str x8, [x24, #0x18]
1008fc5a4:      ldr x8, [x24]
1008fc5a8:      add x8, x8, #0x1
1008fc5ac:      str x8, [x24]
1008fc5b0:      ldr w8, [x22]
1008fc5b4:      str x8, [sp, #0x10]
1008fc5b8:      mov x0, x22
1008fc5bc:      bl  0x100902d58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008fc5c0:      cbz x0, 0x1008fc5cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x584>
1008fc5c4:      ldrh    w8, [x0, #0x2]
1008fc5c8:      tbnz    w8, #0xa, 0x1008fc600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5b8>
1008fc5cc:      adrp    x8, 0x101201000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc11instruments24INCREMENTAL_CYCLE_STARTS>
1008fc5d0:      ldrb    w8, [x8, #0x180]
1008fc5d4:      cbnz    w8, 0x1008fc600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5b8>
1008fc5d8:      adrp    x8, 0x101201000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc11instruments24INCREMENTAL_CYCLE_STARTS>
1008fc5dc:      ldrb    w8, [x8, #0x182]
1008fc5e0:      cbnz    w8, 0x1008fc600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5b8>
1008fc5e4:      ldp w8, w9, [x22]
1008fc5e8:      cmp w8, w9
1008fc5ec:      b.hi    0x1008fc600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5b8>
1008fc5f0:      mov x0, x22
1008fc5f4:      bl  0x1009ed870 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object15prototype_chain23object_static_prototype>
1008fc5f8:      cmp x0, #0x1
1008fc5fc:      b.ne    0x1008fcfc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf78>
1008fc600:      adrp    x27, 0x101134000 <_perry_global_baseline_worker_ts__1>
1008fc604:      add x27, x27, #0x8f0
1008fc608:      ldr x8, [x27]
1008fc60c:      cmn x8, #0x1
1008fc610:      b.eq    0x1008fc644 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5fc>
1008fc614:      mrs x9, TPIDRRO_EL0
1008fc618:      and x9, x9, #0xfffffffffffffff8
1008fc61c:      ldr x8, [x9, x8, lsl #3]
1008fc620:      cbz x8, 0x1008fc644 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5fc>
1008fc624:      ldr x8, [x8, #0x19e8]
1008fc628:      cbz x8, 0x1008fc644 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5fc>
1008fc62c:      ldr x9, [x8]
1008fc630:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1008fc634:      cmp x9, x10
1008fc638:      b.hs    0x1008fdca4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c5c>
1008fc63c:      ldr x21, [x8, #0x18]
1008fc640:      b   0x1008fc654 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x60c>
1008fc644:      adrp    x0, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fc648:      add x0, x0, #0x8a8
1008fc64c:      bl  0x100135d6c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
1008fc650:      mov x21, x0
1008fc654:      stur    x21, [x29, #-0x68]
1008fc658:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1008fc65c:      stp x22, x8, [sp, #0x38]
1008fc660:      mov w8, #0x1                ; =1
1008fc664:      str x8, [sp, #0x30]
1008fc668:      add x0, sp, #0x30
1008fc66c:      bl  0x1008bc1b0 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1008fc670:      mov x22, x0
1008fc674:      ldr x23, [x19, #0x10]
1008fc678:      ldr x8, [x19]
1008fc67c:      cmp x8, x23
1008fc680:      b.eq    0x1008fdcf4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cac>
1008fc684:      ldr x8, [x19, #0x8]
1008fc688:      mov w9, #0x5b               ; =91
1008fc68c:      strb    w9, [x8, x23]
1008fc690:      add x23, x23, #0x1
1008fc694:      str x23, [x19, #0x10]
1008fc698:      ldr x8, [sp, #0x10]
1008fc69c:      cbz w8, 0x1008fcd78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd30>
1008fc6a0:      stp x21, x24, [sp]
1008fc6a4:      adrp    x0, 0x10113c000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime6object32OS_CONSTANTS_PRIORITY_CACHE_SLOT7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
1008fc6a8:      add x0, x0, #0x7f8
1008fc6ac:      ldr x8, [x0]
1008fc6b0:      blr x8
1008fc6b4:      mov x21, x0
1008fc6b8:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1008fc6bc:      add x0, x0, #0x470
1008fc6c0:      ldr x8, [x0]
1008fc6c4:      blr x8
1008fc6c8:      mov x25, x0
1008fc6cc:      mov x26, #0x0               ; =0
1008fc6d0:      b   0x1008fc6e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x6a0>
1008fc6d4:      str xzr, [x8]
1008fc6d8:      add x26, x26, #0x1
1008fc6dc:      ldr x8, [sp, #0x10]
1008fc6e0:      cmp x8, x26
1008fc6e4:      b.eq    0x1008fcd70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd28>
1008fc6e8:      cbz x26, 0x1008fc710 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x6c8>
1008fc6ec:      ldr x23, [x19, #0x10]
1008fc6f0:      ldr x8, [x19]
1008fc6f4:      cmp x8, x23
1008fc6f8:      b.eq    0x1008fcc40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbf8>
1008fc6fc:      ldr x8, [x19, #0x8]
1008fc700:      mov w9, #0x2c               ; =44
1008fc704:      strb    w9, [x8, x23]
1008fc708:      add x8, x23, #0x1
1008fc70c:      str x8, [x19, #0x10]
1008fc710:      ldr x8, [x27]
1008fc714:      cmn x8, #0x1
1008fc718:      b.eq    0x1008fc77c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x734>
1008fc71c:      mrs x9, TPIDRRO_EL0
1008fc720:      and x9, x9, #0xfffffffffffffff8
1008fc724:      ldr x8, [x9, x8, lsl #3]
1008fc728:      cbz x8, 0x1008fc77c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x734>
1008fc72c:      ldr x8, [x8, #0x19e8]
1008fc730:      cbz x8, 0x1008fc77c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x734>
1008fc734:      ldr x9, [x8]
1008fc738:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1008fc73c:      cmp x9, x10
1008fc740:      b.hs    0x1008fdf0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
1008fc744:      add x10, x9, #0x1
1008fc748:      str x10, [x8]
1008fc74c:      ldr x10, [x8, #0x18]
1008fc750:      cmp x22, x10
1008fc754:      b.hs    0x1008fdf08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
1008fc758:      ldr x10, [x8, #0x10]
1008fc75c:      mov w11, #0x18              ; =24
1008fc760:      madd    x10, x22, x11, x10
1008fc764:      ldr x11, [x10]
1008fc768:      cmp x11, #0x1
1008fc76c:      b.ne    0x1008fdf18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ed0>
1008fc770:      ldr x0, [x10, #0x8]
1008fc774:      str x9, [x8]
1008fc778:      b   0x1008fc7c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x780>
1008fc77c:      ldrb    w8, [x21, #0x20]
1008fc780:      cbnz    w8, 0x1008fcc5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc14>
1008fc784:      ldr x8, [x21]
1008fc788:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008fc78c:      cmp x8, x9
1008fc790:      b.hs    0x1008fd9b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
1008fc794:      add x9, x8, #0x1
1008fc798:      str x9, [x21]
1008fc79c:      ldr x9, [x21, #0x18]
1008fc7a0:      cmp x22, x9
1008fc7a4:      b.hs    0x1008fdf08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
1008fc7a8:      ldr x9, [x21, #0x10]
1008fc7ac:      mov w10, #0x18              ; =24
1008fc7b0:      madd    x9, x22, x10, x9
1008fc7b4:      ldr x10, [x9]
1008fc7b8:      cmp x10, #0x1
1008fc7bc:      b.ne    0x1008fda04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19bc>
1008fc7c0:      ldr x0, [x9, #0x8]
1008fc7c4:      str x8, [x21]
1008fc7c8:      mov x1, x26
1008fc7cc:      bl  0x1006b1b90 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime5array8indexing11proto_chain14array_spec_get>
1008fc7d0:      mov.16b v8, v0
1008fc7d4:      fmov    x23, d8
1008fc7d8:      mov x8, #0x1                ; =1
1008fc7dc:      movk    x8, #0x7ffc, lsl #48
1008fc7e0:      cmp x23, x8
1008fc7e4:      mov x8, #0x10               ; =16
1008fc7e8:      movk    x8, #0x7ffc, lsl #48
1008fc7ec:      ccmp    x23, x8, #0x4, ne
1008fc7f0:      b.ne    0x1008fc828 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7e0>
1008fc7f4:      ldr x1, [x19, #0x10]
1008fc7f8:      ldr x8, [x19]
1008fc7fc:      sub x8, x8, x1
1008fc800:      cmp x8, #0x3
1008fc804:      b.ls    0x1008fcc24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbdc>
1008fc808:      ldr x8, [x19, #0x8]
1008fc80c:      mov w9, #0x756e             ; =30062
1008fc810:      movk    w9, #0x6c6c, lsl #16
1008fc814:      str w9, [x8, x1]
1008fc818:      ldr x8, [x19, #0x10]
1008fc81c:      add x8, x8, #0x4
1008fc820:      str x8, [x19, #0x10]
1008fc824:      b   0x1008fc6d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x690>
1008fc828:      and x24, x23, #0xffff000000000000
1008fc82c:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1008fc830:      cmp x24, x8
1008fc834:      b.ne    0x1008fc86c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x824>
1008fc838:      and x8, x23, #0xffffffffffff
1008fc83c:      cmp x8, #0x100, lsl #12     ; =0x100000
1008fc840:      b.lo    0x1008fc858 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x810>
1008fc844:      ldr w8, [x8, #0xc]
1008fc848:      mov w9, #0x4f53             ; =20307
1008fc84c:      movk    w9, #0x434c, lsl #16
1008fc850:      cmp w8, w9
1008fc854:      b.eq    0x1008fc7f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7ac>
1008fc858:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1008fc85c:      cmp x24, x8
1008fc860:      b.ne    0x1008fc898 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x850>
1008fc864:      and x0, x23, #0xffffffffffff
1008fc868:      b   0x1008fc8c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x878>
1008fc86c:      lsr x8, x23, #52
1008fc870:      cbnz    x8, 0x1008fc950 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
1008fc874:      and x8, x23, #0x7
1008fc878:      cbz x23, 0x1008fc8a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x85c>
1008fc87c:      cbnz    x8, 0x1008fc8a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x85c>
1008fc880:      mov x0, x23
1008fc884:      bl  0x100900d8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
1008fc888:      mov x8, x23
1008fc88c:      tbnz    w0, #0x0, 0x1008fc83c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7f4>
1008fc890:      mov x8, #0x0                ; =0
1008fc894:      b   0x1008fc8a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x85c>
1008fc898:      lsr x8, x23, #52
1008fc89c:      cbnz    x8, 0x1008fc950 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
1008fc8a0:      and x8, x23, #0x7
1008fc8a4:      cbz x23, 0x1008fc950 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
1008fc8a8:      cbnz    x8, 0x1008fc950 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
1008fc8ac:      mov x0, x23
1008fc8b0:      bl  0x100900d8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
1008fc8b4:      mov x8, x0
1008fc8b8:      mov x0, x23
1008fc8bc:      tbz w8, #0x0, 0x1008fc950 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
1008fc8c0:      cmp x0, #0x100, lsl #12     ; =0x100000
1008fc8c4:      b.lo    0x1008fc950 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
1008fc8c8:      adrp    x8, 0x101200000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7a8>
1008fc8cc:      add x8, x8, #0xea3
1008fc8d0:      ldaprb  w8, [x8]
1008fc8d4:      cbz w8, 0x1008fc950 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
1008fc8d8:      lsr x8, x0, #3
1008fc8dc:      mov x9, #0x7c15             ; =31765
1008fc8e0:      movk    x9, #0x7f4a, lsl #16
1008fc8e4:      movk    x9, #0x79b9, lsl #32
1008fc8e8:      movk    x9, #0x9e37, lsl #48
1008fc8ec:      mul x8, x8, x9
1008fc8f0:      lsr x9, x8, #54
1008fc8f4:      lsr x10, x8, #60
1008fc8f8:      adrp    x11, 0x101200000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7a8>
1008fc8fc:      add x11, x11, #0xea8
1008fc900:      add x10, x11, x10, lsl #3
1008fc904:      ldapr   x10, [x10]
1008fc908:      lsr x9, x10, x9
1008fc90c:      tbz w9, #0x0, 0x1008fc950 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
1008fc910:      lsr x9, x8, #44
1008fc914:      ubfx    x10, x8, #50, #4
1008fc918:      add x10, x11, x10, lsl #3
1008fc91c:      ldapr   x10, [x10]
1008fc920:      lsr x9, x10, x9
1008fc924:      tbz w9, #0x0, 0x1008fc950 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
1008fc928:      lsr x9, x8, #34
1008fc92c:      ubfx    x8, x8, #40, #4
1008fc930:      adrp    x10, 0x101200000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7a8>
1008fc934:      add x10, x10, #0xea8
1008fc938:      add x8, x10, x8, lsl #3
1008fc93c:      ldapr   x8, [x8]
1008fc940:      lsr x8, x8, x9
1008fc944:      tbz w8, #0x0, 0x1008fc950 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
1008fc948:      bl  0x1004adad0 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6symbol25is_registered_symbol_slow>
1008fc94c:      tbnz    w0, #0x0, 0x1008fc7f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7ac>
1008fc950:      ldr x8, [x27]
1008fc954:      cmn x8, #0x1
1008fc958:      b.eq    0x1008fc988 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x940>
1008fc95c:      mrs x9, TPIDRRO_EL0
1008fc960:      and x9, x9, #0xfffffffffffffff8
1008fc964:      ldr x8, [x9, x8, lsl #3]
1008fc968:      cbz x8, 0x1008fc988 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x940>
1008fc96c:      ldr x8, [x8, #0x19e8]
1008fc970:      cbz x8, 0x1008fc988 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x940>
1008fc974:      ldr x9, [x8], #0x18
1008fc978:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1008fc97c:      cmp x9, x10
1008fc980:      b.lo    0x1008fc9a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x95c>
1008fc984:      b   0x1008fdca4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c5c>
1008fc988:      ldrb    w8, [x21, #0x20]
1008fc98c:      cbnz    w8, 0x1008fccb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc6c>
1008fc990:      ldr x9, [x21]
1008fc994:      add x8, x21, #0x18
1008fc998:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1008fc99c:      cmp x9, x10
1008fc9a0:      b.hs    0x1008fda20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19d8>
1008fc9a4:      ldr x28, [x8]
1008fc9a8:      adrp    x8, 0x101201000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc11instruments24INCREMENTAL_CYCLE_STARTS>
1008fc9ac:      add x8, x8, #0x184
1008fc9b0:      ldr w8, [x8]
1008fc9b4:      cbz w8, 0x1008fc9c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x978>
1008fc9b8:      mov x0, x23
1008fc9bc:      bl  0x100682074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
1008fc9c0:      ldr x8, [x27]
1008fc9c4:      cmn x8, #0x1
1008fc9c8:      b.eq    0x1008fca2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9e4>
1008fc9cc:      mrs x9, TPIDRRO_EL0
1008fc9d0:      and x9, x9, #0xfffffffffffffff8
1008fc9d4:      ldr x8, [x9, x8, lsl #3]
1008fc9d8:      cbz x8, 0x1008fca2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9e4>
1008fc9dc:      ldr x24, [x8, #0x19e8]
1008fc9e0:      cbz x24, 0x1008fca2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9e4>
1008fc9e4:      ldr x8, [x24]
1008fc9e8:      cbnz    x8, 0x1008fdcd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c90>
1008fc9ec:      mov x8, #-0x1               ; =-1
1008fc9f0:      str x8, [x24]
1008fc9f4:      mov x0, x24
1008fc9f8:      ldr x8, [x0, #0x8]!
1008fc9fc:      ldr x23, [x24, #0x18]
1008fca00:      cmp x23, x8
1008fca04:      b.ne    0x1008fca0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9c4>
1008fca08:      bl  0x100cd42b4 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecTyyNtNtCseUPtmYZaE8V_5gimli6common13EhFrameOffsetEE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1008fca0c:      ldr x8, [x24, #0x10]
1008fca10:      mov w9, #0x18               ; =24
1008fca14:      madd    x8, x23, x9, x8
1008fca18:      str xzr, [x8]
1008fca1c:      str d8, [x8, #0x8]
1008fca20:      add x8, x23, #0x1
1008fca24:      str x8, [x24, #0x18]
1008fca28:      b   0x1008fca7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xa34>
1008fca2c:      ldrb    w8, [x21, #0x20]
1008fca30:      cbnz    w8, 0x1008fcce8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xca0>
1008fca34:      ldr x8, [x21]
1008fca38:      cbnz    x8, 0x1008fda2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19e4>
1008fca3c:      mov x8, #-0x1               ; =-1
1008fca40:      str x8, [x21]
1008fca44:      ldr x23, [x21, #0x18]
1008fca48:      ldr x8, [x21, #0x8]
1008fca4c:      cmp x23, x8
1008fca50:      b.ne    0x1008fca5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xa14>
1008fca54:      add x0, x21, #0x8
1008fca58:      bl  0x100cd42b4 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecTyyNtNtCseUPtmYZaE8V_5gimli6common13EhFrameOffsetEE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1008fca5c:      ldr x8, [x21, #0x10]
1008fca60:      mov w9, #0x18               ; =24
1008fca64:      madd    x8, x23, x9, x8
1008fca68:      str xzr, [x8]
1008fca6c:      str d8, [x8, #0x8]
1008fca70:      add x8, x23, #0x1
1008fca74:      str x8, [x21, #0x18]
1008fca78:      mov x24, x21
1008fca7c:      ldr x8, [x24]
1008fca80:      add x8, x8, #0x1
1008fca84:      str x8, [x24]
1008fca88:      str x26, [sp, #0x60]
1008fca8c:      ldrb    w8, [x25, #0x20]
1008fca90:      cbnz    w8, 0x1008fcc8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc44>
1008fca94:      ldr x8, [x25]
1008fca98:      cbnz    x8, 0x1008fda14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19cc>
1008fca9c:      mov x8, #-0x1               ; =-1
1008fcaa0:      str x8, [x25]
1008fcaa4:      str xzr, [x25, #0x18]
1008fcaa8:      add x8, sp, #0x60
1008fcaac:      str x8, [sp, #0x30]
1008fcab0:      adrp    x8, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
1008fcab4:      add x8, x8, #0xf80
1008fcab8:      str x8, [sp, #0x38]
1008fcabc:      add x0, x25, #0x8
1008fcac0:      add x3, sp, #0x30
1008fcac4:      adrp    x1, 0x1010a7000 <_anon.58120679d426c7dccd15bda76f596bde.683>
1008fcac8:      add x1, x1, #0x590
1008fcacc:      adrp    x2, 0x100ef4000 <_anon.58120679d426c7dccd15bda76f596bde.575+0x3>
1008fcad0:      add x2, x2, #0x2d4
1008fcad4:      bl  0x10002cf10 <__RNvNtCsjgY6bXVaRmE_4core3fmt5write>
1008fcad8:      ldr x8, [x25]
1008fcadc:      add x8, x8, #0x1
1008fcae0:      str x8, [x25]
1008fcae4:      ldr x8, [x27]
1008fcae8:      cmn x8, #0x1
1008fcaec:      b.eq    0x1008fcb64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb1c>
1008fcaf0:      mrs x9, TPIDRRO_EL0
1008fcaf4:      and x9, x9, #0xfffffffffffffff8
1008fcaf8:      ldr x8, [x9, x8, lsl #3]
1008fcafc:      cbz x8, 0x1008fcb64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb1c>
1008fcb00:      ldr x8, [x8, #0x19e8]
1008fcb04:      cbz x8, 0x1008fcb64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb1c>
1008fcb08:      ldr x9, [x8]
1008fcb0c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1008fcb10:      cmp x9, x10
1008fcb14:      b.hs    0x1008fdf0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
1008fcb18:      add x10, x9, #0x1
1008fcb1c:      str x10, [x8]
1008fcb20:      ldr x10, [x8, #0x18]
1008fcb24:      cmp x23, x10
1008fcb28:      b.hs    0x1008fdf08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
1008fcb2c:      ldr x10, [x8, #0x10]
1008fcb30:      mov w11, #0x18              ; =24
1008fcb34:      madd    x10, x23, x11, x10
1008fcb38:      ldr x11, [x10]
1008fcb3c:      cbnz    x11, 0x1008fdce4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c9c>
1008fcb40:      ldr d0, [x10, #0x8]
1008fcb44:      str x9, [x8]
1008fcb48:      add w1, w20, #0x1
1008fcb4c:      mov x0, x19
1008fcb50:      bl  0x1008fe0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
1008fcb54:      ldr x8, [x27]
1008fcb58:      cmn x8, #0x1
1008fcb5c:      b.ne    0x1008fcbc4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb7c>
1008fcb60:      b   0x1008fcbf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbb0>
1008fcb64:      ldrb    w8, [x21, #0x20]
1008fcb68:      cbnz    w8, 0x1008fcd10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xcc8>
1008fcb6c:      ldr x8, [x21]
1008fcb70:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008fcb74:      cmp x8, x9
1008fcb78:      b.hs    0x1008fd9b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
1008fcb7c:      add x9, x8, #0x1
1008fcb80:      str x9, [x21]
1008fcb84:      ldr x9, [x21, #0x18]
1008fcb88:      cmp x23, x9
1008fcb8c:      b.hs    0x1008fdf08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
1008fcb90:      ldr x9, [x21, #0x10]
1008fcb94:      mov w10, #0x18              ; =24
1008fcb98:      madd    x9, x23, x10, x9
1008fcb9c:      ldr x10, [x9]
1008fcba0:      cbnz    x10, 0x1008fda38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19f0>
1008fcba4:      ldr d0, [x9, #0x8]
1008fcba8:      str x8, [x21]
1008fcbac:      add w1, w20, #0x1
1008fcbb0:      mov x0, x19
1008fcbb4:      bl  0x1008fe0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
1008fcbb8:      ldr x8, [x27]
1008fcbbc:      cmn x8, #0x1
1008fcbc0:      b.eq    0x1008fcbf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbb0>
1008fcbc4:      mrs x9, TPIDRRO_EL0
1008fcbc8:      and x9, x9, #0xfffffffffffffff8
1008fcbcc:      ldr x8, [x9, x8, lsl #3]
1008fcbd0:      cbz x8, 0x1008fcbf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbb0>
1008fcbd4:      ldr x8, [x8, #0x19e8]
1008fcbd8:      cbz x8, 0x1008fcbf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbb0>
1008fcbdc:      ldr x9, [x8]
1008fcbe0:      cbnz    x9, 0x1008fdcb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c68>
1008fcbe4:      ldr x9, [x8, #0x18]
1008fcbe8:      cmp x28, x9
1008fcbec:      b.hi    0x1008fc6d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x68c>
1008fcbf0:      str x28, [x8, #0x18]
1008fcbf4:      b   0x1008fc6d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x68c>
1008fcbf8:      ldrb    w8, [x21, #0x20]
1008fcbfc:      cbnz    w8, 0x1008fcd40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xcf8>
1008fcc00:      ldr x8, [x21]
1008fcc04:      cbnz    x8, 0x1008fcd64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd1c>
1008fcc08:      add x8, x21, #0x18
1008fcc0c:      ldr x8, [x8]
1008fcc10:      cmp x28, x8
1008fcc14:      b.hi    0x1008fc6d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x690>
1008fcc18:      add x8, x21, #0x18
1008fcc1c:      str x28, [x8]
1008fcc20:      b   0x1008fc6d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x690>
1008fcc24:      mov x0, x19
1008fcc28:      mov w2, #0x4                ; =4
1008fcc2c:      mov w3, #0x1                ; =1
1008fcc30:      mov w4, #0x1                ; =1
1008fcc34:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fcc38:      ldr x1, [x19, #0x10]
1008fcc3c:      b   0x1008fc808 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7c0>
1008fcc40:      mov x0, x19
1008fcc44:      mov x1, x23
1008fcc48:      mov w2, #0x1                ; =1
1008fcc4c:      mov w3, #0x1                ; =1
1008fcc50:      mov w4, #0x1                ; =1
1008fcc54:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fcc58:      b   0x1008fc6fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x6b4>
1008fcc5c:      cmp w8, #0x2
1008fcc60:      b.eq    0x1008fdd64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
1008fcc64:      mov x0, x21
1008fcc68:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
1008fcc6c:      add x1, x1, #0x87c
1008fcc70:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008fcc74:      strb    wzr, [x21, #0x20]
1008fcc78:      ldr x8, [x21]
1008fcc7c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008fcc80:      cmp x8, x9
1008fcc84:      b.lo    0x1008fc794 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x74c>
1008fcc88:      b   0x1008fd9b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
1008fcc8c:      cmp w8, #0x2
1008fcc90:      b.eq    0x1008fdd64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
1008fcc94:      mov x0, x25
1008fcc98:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
1008fcc9c:      add x1, x1, #0x87c
1008fcca0:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008fcca4:      strb    wzr, [x25, #0x20]
1008fcca8:      ldr x8, [x25]
1008fccac:      cbz x8, 0x1008fca9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xa54>
1008fccb0:      b   0x1008fda14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19cc>
1008fccb4:      cmp w8, #0x2
1008fccb8:      b.eq    0x1008fdd64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
1008fccbc:      mov x0, x21
1008fccc0:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
1008fccc4:      add x1, x1, #0x87c
1008fccc8:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008fcccc:      strb    wzr, [x21, #0x20]
1008fccd0:      ldr x9, [x21]
1008fccd4:      add x8, x21, #0x18
1008fccd8:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1008fccdc:      cmp x9, x10
1008fcce0:      b.lo    0x1008fc9a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x95c>
1008fcce4:      b   0x1008fda20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19d8>
1008fcce8:      cmp w8, #0x2
1008fccec:      b.eq    0x1008fdd64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
1008fccf0:      mov x0, x21
1008fccf4:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
1008fccf8:      add x1, x1, #0x87c
1008fccfc:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008fcd00:      strb    wzr, [x21, #0x20]
1008fcd04:      ldr x8, [x21]
1008fcd08:      cbz x8, 0x1008fca3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9f4>
1008fcd0c:      b   0x1008fda2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19e4>
1008fcd10:      cmp w8, #0x2
1008fcd14:      b.eq    0x1008fdd64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
1008fcd18:      mov x0, x21
1008fcd1c:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
1008fcd20:      add x1, x1, #0x87c
1008fcd24:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008fcd28:      strb    wzr, [x21, #0x20]
1008fcd2c:      ldr x8, [x21]
1008fcd30:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008fcd34:      cmp x8, x9
1008fcd38:      b.lo    0x1008fcb7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb34>
1008fcd3c:      b   0x1008fd9b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
1008fcd40:      cmp w8, #0x2
1008fcd44:      b.eq    0x1008fdd64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
1008fcd48:      mov x0, x21
1008fcd4c:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
1008fcd50:      add x1, x1, #0x87c
1008fcd54:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008fcd58:      strb    wzr, [x21, #0x20]
1008fcd5c:      ldr x8, [x21]
1008fcd60:      cbz x8, 0x1008fcc08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbc0>
1008fcd64:      adrp    x0, 0x1010a9000 <_anon.58120679d426c7dccd15bda76f596bde.1139>
1008fcd68:      add x0, x0, #0x2d0
1008fcd6c:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008fcd70:      ldr x23, [x19, #0x10]
1008fcd74:      ldp x21, x24, [sp]
1008fcd78:      ldr x8, [x19]
1008fcd7c:      cmp x8, x23
1008fcd80:      b.eq    0x1008fdd10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cc8>
1008fcd84:      ldr x8, [x19, #0x8]
1008fcd88:      mov w9, #0x5d               ; =93
1008fcd8c:      strb    w9, [x8, x23]
1008fcd90:      add x8, x23, #0x1
1008fcd94:      str x8, [x19, #0x10]
1008fcd98:      ldr x8, [x27]
1008fcd9c:      cmn x8, #0x1
1008fcda0:      b.eq    0x1008fcddc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd94>
1008fcda4:      mrs x9, TPIDRRO_EL0
1008fcda8:      and x9, x9, #0xfffffffffffffff8
1008fcdac:      ldr x8, [x9, x8, lsl #3]
1008fcdb0:      cbz x8, 0x1008fcddc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd94>
1008fcdb4:      ldr x8, [x8, #0x19e8]
1008fcdb8:      cbz x8, 0x1008fcddc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd94>
1008fcdbc:      ldr x9, [x8]
1008fcdc0:      cbnz    x9, 0x1008fdcb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c68>
1008fcdc4:      ldr x9, [x8, #0x18]
1008fcdc8:      cmp x21, x9
1008fcdcc:      b.hi    0x1008fcdd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd8c>
1008fcdd0:      str x21, [x8, #0x18]
1008fcdd4:      str xzr, [x8]
1008fcdd8:      b   0x1008fcdec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xda4>
1008fcddc:      adrp    x0, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fcde0:      add x0, x0, #0x8a8
1008fcde4:      sub x1, x29, #0x68
1008fcde8:      bl  0x100136148 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
1008fcdec:      ldrb    w8, [x24, #0x20]
1008fcdf0:      cbnz    w8, 0x1008fdd5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d14>
1008fcdf4:      ldr x8, [x24]
1008fcdf8:      cbnz    x8, 0x1008fdd8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d44>
1008fcdfc:      ldr x8, [x24, #0x18]
1008fce00:      cbz x8, 0x1008fce0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdc4>
1008fce04:      sub x8, x8, #0x1
1008fce08:      str x8, [x24, #0x18]
1008fce0c:      str xzr, [x24]
1008fce10:      b   0x1008fc418 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3d0>
1008fce14:      lsr x8, x21, #52
1008fce18:      cbnz    x8, 0x1008fce3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
1008fce1c:      cbz x21, 0x1008fce3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
1008fce20:      and x8, x21, #0x7
1008fce24:      cbnz    x8, 0x1008fce3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
1008fce28:      mov x0, x21
1008fce2c:      mov.16b v8, v0
1008fce30:      bl  0x100900d8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
1008fce34:      mov.16b v0, v8
1008fce38:      tbnz    w0, #0x0, 0x1008fc484 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x43c>
1008fce3c:      add w1, w20, #0x1
1008fce40:      mov x0, x19
1008fce44:      bl  0x1008fe0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
1008fce48:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1008fce4c:      add x0, x0, #0x4d0
1008fce50:      ldr x8, [x0]
1008fce54:      blr x8
1008fce58:      strb    wzr, [x0]
1008fce5c:      b   0x1008fc418 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3d0>
1008fce60:      bl  0x100cd2ac8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1008fce64:      lsr x1, x22, #20
1008fce68:      ldr x8, [x0, #0x10]
1008fce6c:      ldrb    w9, [x8, #0x28]
1008fce70:      tbnz    w9, #0x0, 0x1008fc0f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xac>
1008fce74:      b   0x1008fc110 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc8>
1008fce78:      tbnz    w21, #0x7, 0x1008fce98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe50>
1008fce7c:      mov x8, x22
1008fce80:      ldr w9, [x8], #0x8
1008fce84:      add x9, x8, x9, lsl #3
1008fce88:      stp x8, x9, [sp, #0x30]
1008fce8c:      add x0, sp, #0x30
1008fce90:      bl  0x1008af3fc <__RINvXs2J_NtNtCsjgY6bXVaRmE_4core5slice4iterINtB7_4IterdENtNtNtNtBb_4iter6traits8iterator8Iterator3allNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json25stringify_primitive_array8try_emit0EB1J_>
1008fce94:      cbz w0, 0x1008fc53c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
1008fce98:      ldurh   w24, [x22, #-0x6]
1008fce9c:      ldr w21, [x22]
1008fcea0:      ldr x20, [x19, #0x10]
1008fcea4:      ldr x8, [x19]
1008fcea8:      cmp x8, x20
1008fceac:      b.eq    0x1008fdf5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f14>
1008fceb0:      ldr x8, [x19, #0x8]
1008fceb4:      mov w9, #0x5b               ; =91
1008fceb8:      strb    w9, [x8, x20]
1008fcebc:      add x20, x20, #0x1
1008fcec0:      str x20, [x19, #0x10]
1008fcec4:      lsl x23, x21, #3
1008fcec8:      tbnz    w24, #0x7, 0x1008fcf40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xef8>
1008fcecc:      cbz w21, 0x1008fda8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a44>
1008fced0:      ldr d0, [x22, #0x8]
1008fced4:      fmov    x0, d0
1008fced8:      mov x8, #-0x7ffc000000000001 ; =-9222246136947933185
1008fcedc:      add x8, x0, x8
1008fcee0:      cmp x8, #0x2
1008fcee4:      b.lo    0x1008fda54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a0c>
1008fcee8:      mov x8, #0x4                ; =4
1008fceec:      movk    x8, #0x7ffc, lsl #48
1008fcef0:      cmp x0, x8
1008fcef4:      b.eq    0x1008fd170 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1128>
1008fcef8:      mov x8, #0x3                ; =3
1008fcefc:      movk    x8, #0x7ffc, lsl #48
1008fcf00:      cmp x0, x8
1008fcf04:      b.ne    0x1008fd190 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1148>
1008fcf08:      ldr x8, [x19]
1008fcf0c:      sub x8, x8, x20
1008fcf10:      cmp x8, #0x4
1008fcf14:      b.ls    0x1008fe02c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1fe4>
1008fcf18:      ldr x8, [x19, #0x8]
1008fcf1c:      add x8, x8, x20
1008fcf20:      mov w9, #0x65               ; =101
1008fcf24:      strb    w9, [x8, #0x4]
1008fcf28:      mov w9, #0x6166             ; =24934
1008fcf2c:      movk    w9, #0x736c, lsl #16
1008fcf30:      str w9, [x8]
1008fcf34:      ldr x8, [x19, #0x10]
1008fcf38:      add x8, x8, #0x5
1008fcf3c:      b   0x1008fda7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a34>
1008fcf40:      cbz w21, 0x1008fda8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a44>
1008fcf44:      ldr d0, [x22, #0x8]
1008fcf48:      mov x0, x19
1008fcf4c:      bl  0x1008ec808 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
1008fcf50:      cmp w21, #0x1
1008fcf54:      b.eq    0x1008fda88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a40>
1008fcf58:      add x21, x22, #0x10
1008fcf5c:      sub x22, x23, #0x8
1008fcf60:      mov w23, #0x2c              ; =44
1008fcf64:      ldr d0, [x21], #0x8
1008fcf68:      ldr x20, [x19, #0x10]
1008fcf6c:      ldr x8, [x19]
1008fcf70:      cmp x8, x20
1008fcf74:      b.eq    0x1008fcf9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf54>
1008fcf78:      ldr x8, [x19, #0x8]
1008fcf7c:      strb    w23, [x8, x20]
1008fcf80:      add x8, x20, #0x1
1008fcf84:      str x8, [x19, #0x10]
1008fcf88:      mov x0, x19
1008fcf8c:      bl  0x1008ec808 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
1008fcf90:      subs    x22, x22, #0x8
1008fcf94:      b.ne    0x1008fcf64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf1c>
1008fcf98:      b   0x1008fda88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a40>
1008fcf9c:      mov x0, x19
1008fcfa0:      mov x1, x20
1008fcfa4:      mov w2, #0x1                ; =1
1008fcfa8:      mov w3, #0x1                ; =1
1008fcfac:      mov w4, #0x1                ; =1
1008fcfb0:      mov.16b v8, v0
1008fcfb4:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fcfb8:      mov.16b v0, v8
1008fcfbc:      b   0x1008fcf78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf30>
1008fcfc0:      mov x0, x22
1008fcfc4:      mov x1, x19
1008fcfc8:      mov x2, x20
1008fcfcc:      bl  0x1008f00b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit>
1008fcfd0:      tbz w0, #0x0, 0x1008fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xfb8>
1008fcfd4:      adrp    x0, 0x1010dd000 <_anon.3c709ec65efe22d27798c2815252f2a2.778+0x188>
1008fcfd8:      add x0, x0, #0x20
1008fcfdc:      ldp x29, x30, [sp, #0xe0]
1008fcfe0:      ldp x20, x19, [sp, #0xd0]
1008fcfe4:      ldp x22, x21, [sp, #0xc0]
1008fcfe8:      ldp x24, x23, [sp, #0xb0]
1008fcfec:      ldp x26, x25, [sp, #0xa0]
1008fcff0:      ldp x28, x27, [sp, #0x90]
1008fcff4:      ldp d9, d8, [sp, #0x80]
1008fcff8:      add sp, sp, #0xf0
1008fcffc:      b   0x100138578 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths3_0INtNtBZ_6option6OptionjEEB2j_>
1008fd000:      bl  0x1008bc0a4 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
1008fd004:      str x0, [sp, #0x20]
1008fd008:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1008fd00c:      stp x22, x8, [sp, #0x38]
1008fd010:      mov w8, #0x1                ; =1
1008fd014:      str x8, [sp, #0x30]
1008fd018:      add x0, sp, #0x30
1008fd01c:      bl  0x1008bc1b0 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1008fd020:      str x0, [sp, #0x28]
1008fd024:      add x8, sp, #0x28
1008fd028:      stur    x8, [x29, #-0x68]
1008fd02c:      ldr x8, [sp, #0x10]
1008fd030:      cmp w8, #0x1
1008fd034:      b.ls    0x1008fd220 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1008fd038:      sub x0, x29, #0x68
1008fd03c:      bl  0x1008b1684 <__RNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths4_0B7_>
1008fd040:      fmov    x21, d0
1008fd044:      mov w8, #0x7ffd             ; =32765
1008fd048:      cmp x8, x21, lsr #48
1008fd04c:      b.ne    0x1008fd1fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11b4>
1008fd050:      and x22, x21, #0xffffffffffff
1008fd054:      cmp x22, #0x100, lsl #12    ; =0x100000
1008fd058:      b.lo    0x1008fd220 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1008fd05c:      and x0, x21, #0xffffffffffff
1008fd060:      bl  0x1008c5df4 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4date17is_date_cell_addr>
1008fd064:      tbnz    w0, #0x0, 0x1008fd220 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1008fd068:      ldr w8, [x22]
1008fd06c:      mov w9, #-0xff5f            ; =-65375
1008fd070:      cmp w8, w9
1008fd074:      b.eq    0x1008fd220 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1008fd078:      sub x0, x29, #0x68
1008fd07c:      bl  0x1008b1684 <__RNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths4_0B7_>
1008fd080:      bl  0x100925508 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins10formatting16boxed_primitives26boxed_primitive_json_value>
1008fd084:      cmp x0, #0x1
1008fd088:      b.eq    0x1008fd220 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1008fd08c:      add x0, sp, #0x30
1008fd090:      mov x1, x21
1008fd094:      bl  0x1008f1840 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template27build_shape_prefix_template>
1008fd098:      ldr x8, [sp, #0x30]
1008fd09c:      cmn x8, #0x1
1008fd0a0:      b.eq    0x1008fd228 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11e0>
1008fd0a4:      ldr x21, [x19, #0x10]
1008fd0a8:      ldr x8, [x19]
1008fd0ac:      cmp x8, x21
1008fd0b0:      b.eq    0x1008fe0ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2064>
1008fd0b4:      ldr x8, [x19, #0x8]
1008fd0b8:      mov w9, #0x5b               ; =91
1008fd0bc:      strb    w9, [x8, x21]
1008fd0c0:      add x8, x21, #0x1
1008fd0c4:      str x8, [x19, #0x10]
1008fd0c8:      str xzr, [sp, #0x60]
1008fd0cc:      adrp    x0, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fd0d0:      add x0, x0, #0xf58
1008fd0d4:      add x1, sp, #0x60
1008fd0d8:      bl  0x100164dac <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
1008fd0dc:      adrp    x24, 0x101134000 <_perry_global_baseline_worker_ts__1>
1008fd0e0:      add x24, x24, #0x8f0
1008fd0e4:      ldr x8, [x24]
1008fd0e8:      cmn x8, #0x1
1008fd0ec:      b.eq    0x1008fdd98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d50>
1008fd0f0:      mrs x9, TPIDRRO_EL0
1008fd0f4:      and x9, x9, #0xfffffffffffffff8
1008fd0f8:      ldr x8, [x9, x8, lsl #3]
1008fd0fc:      cbz x8, 0x1008fdd98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d50>
1008fd100:      ldr x8, [x8, #0x19e8]
1008fd104:      cbz x8, 0x1008fdd98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d50>
1008fd108:      ldr x9, [x8]
1008fd10c:      mov x10, #0x7ffffffffffffffe ; =9223372036854775806
1008fd110:      cmp x9, x10
1008fd114:      b.hi    0x1008fdf0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
1008fd118:      ldr x10, [sp, #0x28]
1008fd11c:      add x11, x9, #0x1
1008fd120:      str x11, [x8]
1008fd124:      ldr x11, [x8, #0x18]
1008fd128:      cmp x10, x11
1008fd12c:      b.hs    0x1008fdf08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
1008fd130:      ldr x11, [x8, #0x10]
1008fd134:      mov w12, #0x18              ; =24
1008fd138:      madd    x10, x10, x12, x11
1008fd13c:      ldr x11, [x10]
1008fd140:      cmp x11, #0x1
1008fd144:      b.ne    0x1008fdf18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ed0>
1008fd148:      ldr x0, [x10, #0x8]
1008fd14c:      str x9, [x8]
1008fd150:      b   0x1008fdda8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d60>
1008fd154:      mov x0, x19
1008fd158:      mov w2, #0x2                ; =2
1008fd15c:      mov w3, #0x1                ; =1
1008fd160:      mov w4, #0x1                ; =1
1008fd164:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fd168:      ldr x1, [x19, #0x10]
1008fd16c:      b   0x1008fc400 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3b8>
1008fd170:      ldr x8, [x19]
1008fd174:      sub x8, x8, x20
1008fd178:      cmp x8, #0x3
1008fd17c:      b.ls    0x1008fe04c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2004>
1008fd180:      ldr x8, [x19, #0x8]
1008fd184:      mov w9, #0x7274             ; =29300
1008fd188:      movk    w9, #0x6575, lsl #16
1008fd18c:      b   0x1008fda70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a28>
1008fd190:      and x8, x0, #0xffff000000000000
1008fd194:      mov x9, #0x7fff000000000000 ; =9223090561878065152
1008fd198:      cmp x8, x9
1008fd19c:      b.eq    0x1008fda48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a00>
1008fd1a0:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
1008fd1a4:      cmp x8, x9
1008fd1a8:      b.ne    0x1008fdc98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c50>
1008fd1ac:      strb    wzr, [sp, #0x64]
1008fd1b0:      str wzr, [sp, #0x60]
1008fd1b4:      add x1, sp, #0x60
1008fd1b8:      bl  0x1008b67b4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime5value7jsvalueNtB2_7JSValue19short_string_to_buf>
1008fd1bc:      mov x1, x0
1008fd1c0:      add x8, sp, #0x30
1008fd1c4:      add x0, sp, #0x60
1008fd1c8:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1008fd1cc:      ldr w8, [sp, #0x30]
1008fd1d0:      tbz w8, #0x0, 0x1008fdcbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c74>
1008fd1d4:      ldr x1, [x19, #0x10]
1008fd1d8:      ldr x8, [x19]
1008fd1dc:      sub x8, x8, x1
1008fd1e0:      cmp x8, #0x3
1008fd1e4:      b.ls    0x1008fe090 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2048>
1008fd1e8:      ldr x8, [x19, #0x8]
1008fd1ec:      mov w9, #0x756e             ; =30062
1008fd1f0:      movk    w9, #0x6c6c, lsl #16
1008fd1f4:      str w9, [x8, x1]
1008fd1f8:      b   0x1008fda74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a2c>
1008fd1fc:      lsr x8, x21, #52
1008fd200:      cbnz    x8, 0x1008fd220 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1008fd204:      cbz x21, 0x1008fd220 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1008fd208:      and x8, x21, #0x7
1008fd20c:      cbnz    x8, 0x1008fd220 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1008fd210:      mov x0, x21
1008fd214:      bl  0x100900d8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
1008fd218:      mov x22, x21
1008fd21c:      cbnz    w0, 0x1008fd054 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x100c>
1008fd220:      mov x8, #-0x1               ; =-1
1008fd224:      str x8, [sp, #0x30]
1008fd228:      ldr x21, [x19, #0x10]
1008fd22c:      ldr x8, [x19]
1008fd230:      cmp x8, x21
1008fd234:      b.eq    0x1008fdfd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f8c>
1008fd238:      ldr x8, [x19, #0x8]
1008fd23c:      mov w9, #0x5b               ; =91
1008fd240:      strb    w9, [x8, x21]
1008fd244:      add x21, x21, #0x1
1008fd248:      str x21, [x19, #0x10]
1008fd24c:      ldr x8, [sp, #0x10]
1008fd250:      cbz w8, 0x1008fd9c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x197c>
1008fd254:      mov x23, #0x1               ; =1
1008fd258:      movk    x23, #0x7ffc, lsl #48
1008fd25c:      mov w28, #0x756e            ; =30062
1008fd260:      movk    w28, #0x6c6c, lsl #16
1008fd264:      adrp    x0, 0x10113c000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime6object32OS_CONSTANTS_PRIORITY_CACHE_SLOT7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
1008fd268:      add x0, x0, #0x7f8
1008fd26c:      ldr x8, [x0]
1008fd270:      blr x8
1008fd274:      mov x21, x0
1008fd278:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1008fd27c:      add x0, x0, #0x4d0
1008fd280:      ldr x8, [x0]
1008fd284:      blr x8
1008fd288:      str x0, [sp, #0x8]
1008fd28c:      mov x22, #0x0               ; =0
1008fd290:      adrp    x25, 0x101134000 <_perry_global_baseline_worker_ts__1>
1008fd294:      add x25, x25, #0x8f0
1008fd298:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
1008fd29c:      mov w24, #0x18              ; =24
1008fd2a0:      b   0x1008fd324 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12dc>
1008fd2a4:      ldr x1, [x19, #0x10]
1008fd2a8:      ldr x8, [x19]
1008fd2ac:      sub x8, x8, x1
1008fd2b0:      cmp x8, #0x3
1008fd2b4:      b.ls    0x1008fd2e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1298>
1008fd2b8:      ldr x8, [x19, #0x8]
1008fd2bc:      str w28, [x8, x1]
1008fd2c0:      ldr x8, [x19, #0x10]
1008fd2c4:      add x8, x8, #0x4
1008fd2c8:      str x8, [x19, #0x10]
1008fd2cc:      add x22, x22, #0x1
1008fd2d0:      ldr x8, [sp, #0x10]
1008fd2d4:      cmp x8, x22
1008fd2d8:      b.ne    0x1008fd324 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12dc>
1008fd2dc:      b   0x1008fd9c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1978>
1008fd2e0:      mov x0, x19
1008fd2e4:      mov w2, #0x4                ; =4
1008fd2e8:      mov w3, #0x1                ; =1
1008fd2ec:      mov w4, #0x1                ; =1
1008fd2f0:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fd2f4:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
1008fd2f8:      ldr x1, [x19, #0x10]
1008fd2fc:      b   0x1008fd2b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1270>
1008fd300:      ldr w2, [x8, #0x4]
1008fd304:      add x1, x8, #0x14
1008fd308:      mov x0, x19
1008fd30c:      bl  0x1008edb14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1008fd310:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
1008fd314:      add x22, x22, #0x1
1008fd318:      ldr x8, [sp, #0x10]
1008fd31c:      cmp x8, x22
1008fd320:      b.eq    0x1008fd9c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1978>
1008fd324:      cbz x22, 0x1008fd34c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1304>
1008fd328:      ldr x26, [x19, #0x10]
1008fd32c:      ldr x8, [x19]
1008fd330:      cmp x8, x26
1008fd334:      b.eq    0x1008fd798 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1750>
1008fd338:      ldr x8, [x19, #0x8]
1008fd33c:      mov w9, #0x2c               ; =44
1008fd340:      strb    w9, [x8, x26]
1008fd344:      add x8, x26, #0x1
1008fd348:      str x8, [x19, #0x10]
1008fd34c:      ldr x8, [x25]
1008fd350:      cmn x8, #0x1
1008fd354:      b.eq    0x1008fd3c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1380>
1008fd358:      mrs x9, TPIDRRO_EL0
1008fd35c:      and x9, x9, #0xfffffffffffffff8
1008fd360:      ldr x8, [x9, x8, lsl #3]
1008fd364:      cbz x8, 0x1008fd3c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1380>
1008fd368:      ldr x8, [x8, #0x19e8]
1008fd36c:      cbz x8, 0x1008fd534 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x14ec>
1008fd370:      ldr x9, [x8]
1008fd374:      cmp x9, x12
1008fd378:      b.hs    0x1008fdf0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
1008fd37c:      ldr x10, [sp, #0x28]
1008fd380:      add x11, x9, #0x1
1008fd384:      str x11, [x8]
1008fd388:      ldr x11, [x8, #0x18]
1008fd38c:      cmp x10, x11
1008fd390:      b.hs    0x1008fdf08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
1008fd394:      ldr x11, [x8, #0x10]
1008fd398:      madd    x10, x10, x24, x11
1008fd39c:      ldr x11, [x10]
1008fd3a0:      cmp x11, #0x1
1008fd3a4:      b.ne    0x1008fdf18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ed0>
1008fd3a8:      ldr x0, [x10, #0x8]
1008fd3ac:      str x9, [x8]
1008fd3b0:      add x8, x0, x22, lsl #3
1008fd3b4:      ldr d8, [x8, #0x8]
1008fd3b8:      fmov    x27, d8
1008fd3bc:      cmp x27, x23
1008fd3c0:      b.ne    0x1008fd424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x13dc>
1008fd3c4:      b   0x1008fd2a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
1008fd3c8:      ldr x26, [sp, #0x28]
1008fd3cc:      ldrb    w8, [x21, #0x20]
1008fd3d0:      cbnz    w8, 0x1008fd7b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1770>
1008fd3d4:      ldr x8, [x21]
1008fd3d8:      cmp x8, x12
1008fd3dc:      b.hs    0x1008fd9b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
1008fd3e0:      add x9, x8, #0x1
1008fd3e4:      str x9, [x21]
1008fd3e8:      ldr x9, [x21, #0x18]
1008fd3ec:      cmp x26, x9
1008fd3f0:      b.hs    0x1008fdf08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
1008fd3f4:      ldr x9, [x21, #0x10]
1008fd3f8:      madd    x9, x26, x24, x9
1008fd3fc:      ldr x10, [x9]
1008fd400:      cmp x10, #0x1
1008fd404:      b.ne    0x1008fda04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19bc>
1008fd408:      ldr x0, [x9, #0x8]
1008fd40c:      str x8, [x21]
1008fd410:      add x8, x0, x22, lsl #3
1008fd414:      ldr d8, [x8, #0x8]
1008fd418:      fmov    x27, d8
1008fd41c:      cmp x27, x23
1008fd420:      b.eq    0x1008fd2a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
1008fd424:      and x8, x27, #0xffff000000000000
1008fd428:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
1008fd42c:      cmp x8, x9
1008fd430:      b.eq    0x1008fd450 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1408>
1008fd434:      mov x9, #0x7fff000000000000 ; =9223090561878065152
1008fd438:      cmp x8, x9
1008fd43c:      b.ne    0x1008fd4dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1494>
1008fd440:      and x8, x27, #0xffffffffffff
1008fd444:      cmp x8, #0x1, lsl #12       ; =0x1000
1008fd448:      b.hs    0x1008fd300 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12b8>
1008fd44c:      b   0x1008fd2a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
1008fd450:      strb    wzr, [sp, #0x5c]
1008fd454:      str wzr, [sp, #0x58]
1008fd458:      ubfx    x1, x27, #40, #8
1008fd45c:      cbz x1, 0x1008fd4ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
1008fd460:      strb    w27, [sp, #0x58]
1008fd464:      cmp x1, #0x1
1008fd468:      b.eq    0x1008fd4ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
1008fd46c:      lsr x8, x27, #8
1008fd470:      strb    w8, [sp, #0x59]
1008fd474:      cmp x1, #0x2
1008fd478:      b.eq    0x1008fd4ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
1008fd47c:      lsr x8, x27, #16
1008fd480:      strb    w8, [sp, #0x5a]
1008fd484:      cmp x1, #0x3
1008fd488:      b.eq    0x1008fd4ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
1008fd48c:      lsr x8, x27, #24
1008fd490:      strb    w8, [sp, #0x5b]
1008fd494:      cmp x1, #0x4
1008fd498:      b.eq    0x1008fd4ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
1008fd49c:      lsr x8, x27, #32
1008fd4a0:      strb    w8, [sp, #0x5c]
1008fd4a4:      cmp x1, #0x5
1008fd4a8:      b.ne    0x1008fe0e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x209c>
1008fd4ac:      add x8, sp, #0x60
1008fd4b0:      add x0, sp, #0x58
1008fd4b4:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1008fd4b8:      ldr x8, [sp, #0x60]
1008fd4bc:      cbz x8, 0x1008fd560 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1518>
1008fd4c0:      ldr x1, [x19, #0x10]
1008fd4c4:      ldr x8, [x19]
1008fd4c8:      sub x8, x8, x1
1008fd4cc:      cmp x8, #0x3
1008fd4d0:      b.ls    0x1008fd85c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1814>
1008fd4d4:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
1008fd4d8:      b   0x1008fd2b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1270>
1008fd4dc:      mov x9, #0x2                ; =2
1008fd4e0:      movk    x9, #0x7ffc, lsl #48
1008fd4e4:      cmp x27, x9
1008fd4e8:      b.eq    0x1008fd2a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
1008fd4ec:      mov x9, #0x3                ; =3
1008fd4f0:      movk    x9, #0x7ffc, lsl #48
1008fd4f4:      cmp x27, x9
1008fd4f8:      b.eq    0x1008fd568 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1520>
1008fd4fc:      mov x9, #0x4                ; =4
1008fd500:      movk    x9, #0x7ffc, lsl #48
1008fd504:      cmp x27, x9
1008fd508:      b.ne    0x1008fd5b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1570>
1008fd50c:      ldr x1, [x19, #0x10]
1008fd510:      ldr x8, [x19]
1008fd514:      sub x8, x8, x1
1008fd518:      cmp x8, #0x3
1008fd51c:      b.ls    0x1008fd898 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1850>
1008fd520:      ldr x8, [x19, #0x8]
1008fd524:      mov w9, #0x7274             ; =29300
1008fd528:      movk    w9, #0x6575, lsl #16
1008fd52c:      str w9, [x8, x1]
1008fd530:      b   0x1008fd2c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1278>
1008fd534:      add x1, sp, #0x28
1008fd538:      adrp    x0, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fd53c:      add x0, x0, #0x8a8
1008fd540:      bl  0x100135b90 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
1008fd544:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
1008fd548:      add x8, x0, x22, lsl #3
1008fd54c:      ldr d8, [x8, #0x8]
1008fd550:      fmov    x27, d8
1008fd554:      cmp x27, x23
1008fd558:      b.ne    0x1008fd424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x13dc>
1008fd55c:      b   0x1008fd2a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
1008fd560:      ldp x1, x2, [sp, #0x68]
1008fd564:      b   0x1008fd308 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c0>
1008fd568:      ldr x1, [x19, #0x10]
1008fd56c:      ldr x8, [x19]
1008fd570:      sub x8, x8, x1
1008fd574:      cmp x8, #0x4
1008fd578:      b.ls    0x1008fd878 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1830>
1008fd57c:      ldr x8, [x19, #0x8]
1008fd580:      add x8, x8, x1
1008fd584:      mov w9, #0x65               ; =101
1008fd588:      strb    w9, [x8, #0x4]
1008fd58c:      mov w9, #0x6166             ; =24934
1008fd590:      movk    w9, #0x736c, lsl #16
1008fd594:      str w9, [x8]
1008fd598:      ldr x8, [x19, #0x10]
1008fd59c:      add x8, x8, #0x5
1008fd5a0:      str x8, [x19, #0x10]
1008fd5a4:      add x22, x22, #0x1
1008fd5a8:      ldr x8, [sp, #0x10]
1008fd5ac:      cmp x8, x22
1008fd5b0:      b.ne    0x1008fd324 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12dc>
1008fd5b4:      b   0x1008fd9c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1978>
1008fd5b8:      mov x9, #0x7ffd000000000000 ; =9222527611924643840
1008fd5bc:      cmp x8, x9
1008fd5c0:      b.eq    0x1008fd5f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x15ac>
1008fd5c4:      mov x9, #0x7ffa000000000000 ; =9221683186994511872
1008fd5c8:      cmp x8, x9
1008fd5cc:      b.ne    0x1008fd670 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1628>
1008fd5d0:      str x22, [sp, #0x60]
1008fd5d4:      add x1, sp, #0x60
1008fd5d8:      adrp    x0, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fd5dc:      add x0, x0, #0xf58
1008fd5e0:      bl  0x100164dac <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
1008fd5e4:      mov.16b v0, v8
1008fd5e8:      mov x0, x19
1008fd5ec:      bl  0x1008ecd14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars16serialize_bigint>
1008fd5f0:      b   0x1008fd310 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c8>
1008fd5f4:      str x22, [sp, #0x60]
1008fd5f8:      add x1, sp, #0x60
1008fd5fc:      adrp    x0, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fd600:      add x0, x0, #0xf58
1008fd604:      bl  0x100164dac <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
1008fd608:      and x26, x27, #0xffffffffffff
1008fd60c:      cmp x26, #0x100, lsl #12    ; =0x100000
1008fd610:      b.lo    0x1008fd6b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1668>
1008fd614:      mov x0, x27
1008fd618:      bl  0x1008fae2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify16is_closure_value>
1008fd61c:      tbnz    w0, #0x0, 0x1008fd6b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1668>
1008fd620:      mov x0, x27
1008fd624:      bl  0x1008fa0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify15is_symbol_value>
1008fd628:      cbnz    w0, 0x1008fd6b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1668>
1008fd62c:      mov.16b v0, v8
1008fd630:      bl  0x100925508 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins10formatting16boxed_primitives26boxed_primitive_json_value>
1008fd634:      cmp x0, #0x1
1008fd638:      b.ne    0x1008fd6f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16ac>
1008fd63c:      mov.16b v9, v0
1008fd640:      mov x0, x26
1008fd644:      bl  0x1008fb934 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify18object_get_to_json>
1008fd648:      tbz w0, #0x0, 0x1008fd710 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16c8>
1008fd64c:      mov.16b v8, v0
1008fd650:      bl  0x1009007f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify24arm_to_json_result_guard>
1008fd654:      add w1, w20, #0x1
1008fd658:      mov.16b v0, v8
1008fd65c:      mov x0, x19
1008fd660:      bl  0x1008fe0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
1008fd664:      ldr x8, [sp, #0x8]
1008fd668:      strb    wzr, [x8]
1008fd66c:      b   0x1008fd954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
1008fd670:      lsr x8, x27, #52
1008fd674:      cbnz    x8, 0x1008fd6e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x169c>
1008fd678:      cbz x27, 0x1008fd6e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x169c>
1008fd67c:      and x8, x27, #0x7
1008fd680:      cbnz    x8, 0x1008fd6e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x169c>
1008fd684:      mov x0, x27
1008fd688:      bl  0x100900d8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
1008fd68c:      tbz w0, #0x0, 0x1008fd6e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x169c>
1008fd690:      str x22, [sp, #0x60]
1008fd694:      add x1, sp, #0x60
1008fd698:      adrp    x0, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fd69c:      add x0, x0, #0xf58
1008fd6a0:      bl  0x100164dac <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
1008fd6a4:      mov x26, x27
1008fd6a8:      cmp x27, #0x100, lsl #12    ; =0x100000
1008fd6ac:      b.hs    0x1008fd614 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x15cc>
1008fd6b0:      ldr x1, [x19, #0x10]
1008fd6b4:      ldr x8, [x19]
1008fd6b8:      sub x8, x8, x1
1008fd6bc:      cmp x8, #0x3
1008fd6c0:      b.ls    0x1008fd960 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1918>
1008fd6c4:      ldr x8, [x19, #0x8]
1008fd6c8:      mov w28, #0x756e            ; =30062
1008fd6cc:      movk    w28, #0x6c6c, lsl #16
1008fd6d0:      str w28, [x8, x1]
1008fd6d4:      ldr x8, [x19, #0x10]
1008fd6d8:      add x8, x8, #0x4
1008fd6dc:      str x8, [x19, #0x10]
1008fd6e0:      b   0x1008fd310 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c8>
1008fd6e4:      mov x0, x19
1008fd6e8:      mov.16b v0, v8
1008fd6ec:      bl  0x1008ec808 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
1008fd6f0:      b   0x1008fd310 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c8>
1008fd6f4:      mov x0, x26
1008fd6f8:      bl  0x1008c5df4 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4date17is_date_cell_addr>
1008fd6fc:      tbz w0, #0x0, 0x1008fd724 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16dc>
1008fd700:      mov.16b v0, v8
1008fd704:      bl  0x1003838a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object17date_proto_thunks18date_to_json_value>
1008fd708:      add w1, w20, #0x1
1008fd70c:      b   0x1008fd718 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16d0>
1008fd710:      add w1, w20, #0x1
1008fd714:      mov.16b v0, v9
1008fd718:      mov x0, x19
1008fd71c:      bl  0x1008fe0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
1008fd720:      b   0x1008fd954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
1008fd724:      mov x0, x26
1008fd728:      bl  0x1004599c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json8raw_json19raw_json_text_bytes>
1008fd72c:      cbz x0, 0x1008fd7f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17a8>
1008fd730:      add x8, sp, #0x60
1008fd734:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1008fd738:      ldr w27, [sp, #0x60]
1008fd73c:      ldp x28, x8, [sp, #0x68]
1008fd740:      cmp w27, #0x0
1008fd744:      mov w9, #0x4                ; =4
1008fd748:      csel    x26, x9, x8, ne
1008fd74c:      ldr x1, [x19, #0x10]
1008fd750:      ldr x8, [x19]
1008fd754:      sub x8, x8, x1
1008fd758:      cmp x26, x8
1008fd75c:      b.hi    0x1008fd97c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1934>
1008fd760:      cbz x26, 0x1008fd78c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1744>
1008fd764:      cmp w27, #0x0
1008fd768:      adrp    x8, 0x100e1d000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text15SB_WINDOWS_1253+0x6e>
1008fd76c:      add x8, x8, #0xdbb
1008fd770:      csel    x8, x8, x28, ne
1008fd774:      ldr x9, [x19, #0x8]
1008fd778:      add x0, x9, x1
1008fd77c:      mov x1, x8
1008fd780:      mov x2, x26
1008fd784:      bl  0x100ce9f6c <_writev+0x100ce9f6c>
1008fd788:      ldr x1, [x19, #0x10]
1008fd78c:      add x8, x1, x26
1008fd790:      str x8, [x19, #0x10]
1008fd794:      b   0x1008fd954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
1008fd798:      mov x0, x19
1008fd79c:      mov x1, x26
1008fd7a0:      mov w2, #0x1                ; =1
1008fd7a4:      mov w3, #0x1                ; =1
1008fd7a8:      mov w4, #0x1                ; =1
1008fd7ac:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fd7b0:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
1008fd7b4:      b   0x1008fd338 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12f0>
1008fd7b8:      cmp w8, #0x2
1008fd7bc:      b.eq    0x1008fdd64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
1008fd7c0:      mov x0, x21
1008fd7c4:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
1008fd7c8:      add x1, x1, #0x87c
1008fd7cc:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008fd7d0:      strb    wzr, [x21, #0x20]
1008fd7d4:      mov w28, #0x756e            ; =30062
1008fd7d8:      movk    w28, #0x6c6c, lsl #16
1008fd7dc:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
1008fd7e0:      ldr x8, [x21]
1008fd7e4:      cmp x8, x12
1008fd7e8:      b.lo    0x1008fd3e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1398>
1008fd7ec:      b   0x1008fd9b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
1008fd7f0:      mov x0, x26
1008fd7f4:      bl  0x100904390 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header20is_registered_buffer>
1008fd7f8:      tbz w0, #0x0, 0x1008fd80c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c4>
1008fd7fc:      mov x0, x26
1008fd800:      mov x1, x19
1008fd804:      bl  0x1008e9a0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json16stringify_buffer16stringify_buffer>
1008fd808:      b   0x1008fd954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
1008fd80c:      mov x0, x26
1008fd810:      bl  0x1008bd680 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray23lookup_typed_array_kind>
1008fd814:      tbz w0, #0x0, 0x1008fd828 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17e0>
1008fd818:      mov x0, x26
1008fd81c:      mov x1, x19
1008fd820:      bl  0x1008ea694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json16stringify_buffer21stringify_typed_array>
1008fd824:      b   0x1008fd954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
1008fd828:      lsr x8, x26, #47
1008fd82c:      cbnz    x8, 0x1008fd8fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18b4>
1008fd830:      ldurb   w8, [x26, #-0x8]
1008fd834:      cmp w8, #0x4
1008fd838:      b.gt    0x1008fd8b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1870>
1008fd83c:      cmp w8, #0x1
1008fd840:      b.eq    0x1008fd934 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18ec>
1008fd844:      cmp w8, #0x2
1008fd848:      b.eq    0x1008fd908 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18c0>
1008fd84c:      cmp w8, #0x3
1008fd850:      b.ne    0x1008fd8fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18b4>
1008fd854:      ldr w2, [x26, #0x4]
1008fd858:      b   0x1008fd948 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1900>
1008fd85c:      mov x0, x19
1008fd860:      mov w2, #0x4                ; =4
1008fd864:      mov w3, #0x1                ; =1
1008fd868:      mov w4, #0x1                ; =1
1008fd86c:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fd870:      ldr x1, [x19, #0x10]
1008fd874:      b   0x1008fd4d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x148c>
1008fd878:      mov x0, x19
1008fd87c:      mov w2, #0x5                ; =5
1008fd880:      mov w3, #0x1                ; =1
1008fd884:      mov w4, #0x1                ; =1
1008fd888:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fd88c:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
1008fd890:      ldr x1, [x19, #0x10]
1008fd894:      b   0x1008fd57c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1534>
1008fd898:      mov x0, x19
1008fd89c:      mov w2, #0x4                ; =4
1008fd8a0:      mov w3, #0x1                ; =1
1008fd8a4:      mov w4, #0x1                ; =1
1008fd8a8:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fd8ac:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
1008fd8b0:      ldr x1, [x19, #0x10]
1008fd8b4:      b   0x1008fd520 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x14d8>
1008fd8b8:      cmp w8, #0x5
1008fd8bc:      b.eq    0x1008fd8d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1888>
1008fd8c0:      cmp w8, #0x8
1008fd8c4:      b.eq    0x1008fd8d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1888>
1008fd8c8:      cmp w8, #0xc
1008fd8cc:      b.ne    0x1008fd8fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18b4>
1008fd8d0:      ldr x1, [x19, #0x10]
1008fd8d4:      ldr x8, [x19]
1008fd8d8:      sub x8, x8, x1
1008fd8dc:      cmp x8, #0x1
1008fd8e0:      b.ls    0x1008fd998 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1950>
1008fd8e4:      ldr x8, [x19, #0x8]
1008fd8e8:      mov w9, #0x7d7b             ; =32123
1008fd8ec:      strh    w9, [x8, x1]
1008fd8f0:      ldr x8, [x19, #0x10]
1008fd8f4:      add x8, x8, #0x2
1008fd8f8:      b   0x1008fd790 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1748>
1008fd8fc:      mov x0, x26
1008fd900:      bl  0x1008fb740 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify17is_object_pointer>
1008fd904:      tbz w0, #0x0, 0x1008fd91c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18d4>
1008fd908:      add w2, w20, #0x1
1008fd90c:      mov x0, x26
1008fd910:      mov x1, x19
1008fd914:      bl  0x1008fee00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify22stringify_object_inner>
1008fd918:      b   0x1008fd954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
1008fd91c:      ldp w8, w2, [x26]
1008fd920:      sub w9, w2, #0x1
1008fd924:      mov w10, #0x270f            ; =9999
1008fd928:      cmp w9, w10
1008fd92c:      ccmp    w8, w2, #0x2, lo
1008fd930:      b.hi    0x1008fd948 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1900>
1008fd934:      add w2, w20, #0x1
1008fd938:      mov x0, x26
1008fd93c:      mov x1, x19
1008fd940:      bl  0x1008fc048 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth>
1008fd944:      b   0x1008fd954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
1008fd948:      add x1, x26, #0x14
1008fd94c:      mov x0, x19
1008fd950:      bl  0x1008edb14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1008fd954:      mov w28, #0x756e            ; =30062
1008fd958:      movk    w28, #0x6c6c, lsl #16
1008fd95c:      b   0x1008fd310 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c8>
1008fd960:      mov x0, x19
1008fd964:      mov w2, #0x4                ; =4
1008fd968:      mov w3, #0x1                ; =1
1008fd96c:      mov w4, #0x1                ; =1
1008fd970:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fd974:      ldr x1, [x19, #0x10]
1008fd978:      b   0x1008fd6c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x167c>
1008fd97c:      mov x0, x19
1008fd980:      mov x2, x26
1008fd984:      mov w3, #0x1                ; =1
1008fd988:      mov w4, #0x1                ; =1
1008fd98c:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fd990:      ldr x1, [x19, #0x10]
1008fd994:      b   0x1008fd764 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x171c>
1008fd998:      mov x0, x19
1008fd99c:      mov w2, #0x2                ; =2
1008fd9a0:      mov w3, #0x1                ; =1
1008fd9a4:      mov w4, #0x1                ; =1
1008fd9a8:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fd9ac:      ldr x1, [x19, #0x10]
1008fd9b0:      b   0x1008fd8e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x189c>
1008fd9b4:      adrp    x0, 0x1010a3000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1008fd9b8:      add x0, x0, #0xf70
1008fd9bc:      bl  0x100c9de9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008fd9c0:      ldr x21, [x19, #0x10]
1008fd9c4:      ldr x8, [x19]
1008fd9c8:      cmp x8, x21
1008fd9cc:      b.eq    0x1008fdff0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1fa8>
1008fd9d0:      ldr x8, [x19, #0x8]
1008fd9d4:      mov w9, #0x5d               ; =93
1008fd9d8:      strb    w9, [x8, x21]
1008fd9dc:      add x8, x21, #0x1
1008fd9e0:      str x8, [x19, #0x10]
1008fd9e4:      adrp    x0, 0x1010dd000 <_anon.3c709ec65efe22d27798c2815252f2a2.778+0x188>
1008fd9e8:      add x0, x0, #0x20
1008fd9ec:      bl  0x100138658 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths7_0INtNtBZ_6option6OptionjEEB2j_>
1008fd9f0:      add x0, sp, #0x30
1008fd9f4:      bl  0x1008a14a0 <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueINtNtB4_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template13ShapeTemplateEEB13_>
1008fd9f8:      add x0, sp, #0x20
1008fd9fc:      bl  0x1008a16e0 <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1008fda00:      b   0x1008fc418 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3d0>
1008fda04:      adrp    x0, 0x100dc6000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x1f60>
1008fda08:      add x0, x0, #0xf30
1008fda0c:      mov w1, #0xb                ; =11
1008fda10:      bl  0x100cd3b04 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1008fda14:      adrp    x0, 0x1010a7000 <_anon.58120679d426c7dccd15bda76f596bde.683>
1008fda18:      add x0, x0, #0x5c0
1008fda1c:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008fda20:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1008fda24:      add x0, x0, #0x498
1008fda28:      bl  0x100c9de9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008fda2c:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1008fda30:      add x0, x0, #0x4b0
1008fda34:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008fda38:      adrp    x0, 0x100dc6000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x1f60>
1008fda3c:      add x0, x0, #0xfb9
1008fda40:      mov w1, #0xf                ; =15
1008fda44:      bl  0x100cd3b04 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1008fda48:      and x8, x0, #0xffffffffffff
1008fda4c:      cmp x8, #0x1, lsl #12       ; =0x1000
1008fda50:      b.hs    0x1008fdcc4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c7c>
1008fda54:      ldr x8, [x19]
1008fda58:      sub x8, x8, x20
1008fda5c:      cmp x8, #0x3
1008fda60:      b.ls    0x1008fe00c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1fc4>
1008fda64:      ldr x8, [x19, #0x8]
1008fda68:      mov w9, #0x756e             ; =30062
1008fda6c:      movk    w9, #0x6c6c, lsl #16
1008fda70:      str w9, [x8, x20]
1008fda74:      ldr x8, [x19, #0x10]
1008fda78:      add x8, x8, #0x4
1008fda7c:      str x8, [x19, #0x10]
1008fda80:      cmp w21, #0x1
1008fda84:      b.ne    0x1008fdaac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a64>
1008fda88:      ldr x20, [x19, #0x10]
1008fda8c:      ldr x8, [x19]
1008fda90:      cmp x8, x20
1008fda94:      b.eq    0x1008fdf78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f30>
1008fda98:      ldr x8, [x19, #0x8]
1008fda9c:      mov w9, #0x5d               ; =93
1008fdaa0:      strb    w9, [x8, x20]
1008fdaa4:      add x8, x20, #0x1
1008fdaa8:      b   0x1008fc414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3cc>
1008fdaac:      add x21, x22, #0x10
1008fdab0:      mov w22, #0x756e            ; =30062
1008fdab4:      movk    w22, #0x6c6c, lsl #16
1008fdab8:      sub x23, x23, #0x8
1008fdabc:      mov w25, #0x2c              ; =44
1008fdac0:      mov x26, #-0x7ffc000000000001 ; =-9222246136947933185
1008fdac4:      mov x27, #0x3               ; =3
1008fdac8:      movk    x27, #0x7ffc, lsl #48
1008fdacc:      mov x28, #0x4               ; =4
1008fdad0:      movk    x28, #0x7ffc, lsl #48
1008fdad4:      mov x24, #0x7ff9000000000000 ; =9221401712017801216
1008fdad8:      b   0x1008fdb0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ac4>
1008fdadc:      ldr x1, [x19, #0x10]
1008fdae0:      ldr x8, [x19]
1008fdae4:      sub x8, x8, x1
1008fdae8:      cmp x8, #0x3
1008fdaec:      b.ls    0x1008fdc44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bfc>
1008fdaf0:      ldr x8, [x19, #0x8]
1008fdaf4:      str w22, [x8, x1]
1008fdaf8:      ldr x8, [x19, #0x10]
1008fdafc:      add x8, x8, #0x4
1008fdb00:      str x8, [x19, #0x10]
1008fdb04:      subs    x23, x23, #0x8
1008fdb08:      b.eq    0x1008fda88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a40>
1008fdb0c:      ldr d0, [x21], #0x8
1008fdb10:      ldr x20, [x19, #0x10]
1008fdb14:      ldr x8, [x19]
1008fdb18:      cmp x8, x20
1008fdb1c:      b.eq    0x1008fdc20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bd8>
1008fdb20:      fmov    x0, d0
1008fdb24:      ldr x8, [x19, #0x8]
1008fdb28:      strb    w25, [x8, x20]
1008fdb2c:      add x1, x20, #0x1
1008fdb30:      str x1, [x19, #0x10]
1008fdb34:      add x8, x0, x26
1008fdb38:      cmp x8, #0x2
1008fdb3c:      b.lo    0x1008fdae0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a98>
1008fdb40:      cmp x0, x27
1008fdb44:      b.eq    0x1008fdb74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b2c>
1008fdb48:      cmp x0, x28
1008fdb4c:      b.ne    0x1008fdbac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b64>
1008fdb50:      ldr x8, [x19]
1008fdb54:      sub x8, x8, x1
1008fdb58:      cmp x8, #0x3
1008fdb5c:      b.ls    0x1008fdc60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c18>
1008fdb60:      ldr x8, [x19, #0x8]
1008fdb64:      mov w9, #0x7274             ; =29300
1008fdb68:      movk    w9, #0x6575, lsl #16
1008fdb6c:      str w9, [x8, x1]
1008fdb70:      b   0x1008fdaf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ab0>
1008fdb74:      ldr x8, [x19]
1008fdb78:      sub x8, x8, x1
1008fdb7c:      cmp x8, #0x4
1008fdb80:      b.ls    0x1008fdc7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c34>
1008fdb84:      ldr x8, [x19, #0x8]
1008fdb88:      add x8, x8, x1
1008fdb8c:      mov w9, #0x65               ; =101
1008fdb90:      strb    w9, [x8, #0x4]
1008fdb94:      mov w9, #0x6166             ; =24934
1008fdb98:      movk    w9, #0x736c, lsl #16
1008fdb9c:      str w9, [x8]
1008fdba0:      ldr x8, [x19, #0x10]
1008fdba4:      add x8, x8, #0x5
1008fdba8:      b   0x1008fdb00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ab8>
1008fdbac:      and x8, x0, #0xffff000000000000
1008fdbb0:      cmp x8, x24
1008fdbb4:      b.eq    0x1008fdbdc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b94>
1008fdbb8:      mov x9, #0x7fff000000000000 ; =9223090561878065152
1008fdbbc:      cmp x8, x9
1008fdbc0:      b.ne    0x1008fdc14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bcc>
1008fdbc4:      and x8, x0, #0xffffffffffff
1008fdbc8:      cmp x8, #0x1, lsl #12       ; =0x1000
1008fdbcc:      b.lo    0x1008fdae0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a98>
1008fdbd0:      ldr w2, [x8, #0x4]
1008fdbd4:      add x1, x8, #0x14
1008fdbd8:      b   0x1008fdc08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bc0>
1008fdbdc:      strb    wzr, [sp, #0x64]
1008fdbe0:      str wzr, [sp, #0x60]
1008fdbe4:      add x1, sp, #0x60
1008fdbe8:      bl  0x1008b67b4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime5value7jsvalueNtB2_7JSValue19short_string_to_buf>
1008fdbec:      mov x1, x0
1008fdbf0:      add x8, sp, #0x30
1008fdbf4:      add x0, sp, #0x60
1008fdbf8:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1008fdbfc:      ldr w8, [sp, #0x30]
1008fdc00:      tbnz    w8, #0x0, 0x1008fdadc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a94>
1008fdc04:      ldp x1, x2, [sp, #0x38]
1008fdc08:      mov x0, x19
1008fdc0c:      bl  0x1008edb14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1008fdc10:      b   0x1008fdb04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1abc>
1008fdc14:      mov x0, x19
1008fdc18:      bl  0x1008ec808 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
1008fdc1c:      b   0x1008fdb04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1abc>
1008fdc20:      mov x0, x19
1008fdc24:      mov x1, x20
1008fdc28:      mov w2, #0x1                ; =1
1008fdc2c:      mov w3, #0x1                ; =1
1008fdc30:      mov w4, #0x1                ; =1
1008fdc34:      mov.16b v8, v0
1008fdc38:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fdc3c:      mov.16b v0, v8
1008fdc40:      b   0x1008fdb20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ad8>
1008fdc44:      mov x0, x19
1008fdc48:      mov w2, #0x4                ; =4
1008fdc4c:      mov w3, #0x1                ; =1
1008fdc50:      mov w4, #0x1                ; =1
1008fdc54:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fdc58:      ldr x1, [x19, #0x10]
1008fdc5c:      b   0x1008fdaf0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1aa8>
1008fdc60:      mov x0, x19
1008fdc64:      mov w2, #0x4                ; =4
1008fdc68:      mov w3, #0x1                ; =1
1008fdc6c:      mov w4, #0x1                ; =1
1008fdc70:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fdc74:      ldr x1, [x19, #0x10]
1008fdc78:      b   0x1008fdb60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b18>
1008fdc7c:      mov x0, x19
1008fdc80:      mov w2, #0x5                ; =5
1008fdc84:      mov w3, #0x1                ; =1
1008fdc88:      mov w4, #0x1                ; =1
1008fdc8c:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fdc90:      ldr x1, [x19, #0x10]
1008fdc94:      b   0x1008fdb84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b3c>
1008fdc98:      mov x0, x19
1008fdc9c:      bl  0x1008ec808 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
1008fdca0:      b   0x1008fda80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a38>
1008fdca4:      adrp    x0, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fdca8:      add x0, x0, #0x988
1008fdcac:      bl  0x100c9de9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008fdcb0:      adrp    x0, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fdcb4:      add x0, x0, #0xb68
1008fdcb8:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008fdcbc:      ldp x1, x2, [sp, #0x38]
1008fdcc0:      b   0x1008fdccc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c84>
1008fdcc4:      ldr w2, [x8, #0x4]
1008fdcc8:      add x1, x8, #0x14
1008fdccc:      mov x0, x19
1008fdcd0:      bl  0x1008edb14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1008fdcd4:      b   0x1008fda80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a38>
1008fdcd8:      adrp    x0, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fdcdc:      add x0, x0, #0x9a0
1008fdce0:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008fdce4:      adrp    x0, 0x100e1b000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1432+0x9>
1008fdce8:      add x0, x0, #0x7b8
1008fdcec:      mov w1, #0xf                ; =15
1008fdcf0:      bl  0x100cd3b04 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1008fdcf4:      mov x0, x19
1008fdcf8:      mov x1, x23
1008fdcfc:      mov w2, #0x1                ; =1
1008fdd00:      mov w3, #0x1                ; =1
1008fdd04:      mov w4, #0x1                ; =1
1008fdd08:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fdd0c:      b   0x1008fc684 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x63c>
1008fdd10:      mov x0, x19
1008fdd14:      mov x1, x23
1008fdd18:      mov w2, #0x1                ; =1
1008fdd1c:      mov w3, #0x1                ; =1
1008fdd20:      mov w4, #0x1                ; =1
1008fdd24:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fdd28:      b   0x1008fcd84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd3c>
1008fdd2c:      cmp w8, #0x1
1008fdd30:      b.ne    0x1008fdd64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
1008fdd34:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
1008fdd38:      add x1, x1, #0x87c
1008fdd3c:      mov x0, x24
1008fdd40:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008fdd44:      strb    wzr, [x24, #0x20]
1008fdd48:      ldr x8, [x24]
1008fdd4c:      cbz x8, 0x1008fc574 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x52c>
1008fdd50:      adrp    x0, 0x1010a7000 <_anon.58120679d426c7dccd15bda76f596bde.683>
1008fdd54:      add x0, x0, #0x8c0
1008fdd58:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008fdd5c:      cmp w8, #0x2
1008fdd60:      b.ne    0x1008fdd70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d28>
1008fdd64:      adrp    x0, 0x1010a3000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1008fdd68:      add x0, x0, #0xed8
1008fdd6c:      bl  0x100ce071c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1008fdd70:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
1008fdd74:      add x1, x1, #0x87c
1008fdd78:      mov x0, x24
1008fdd7c:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008fdd80:      strb    wzr, [x24, #0x20]
1008fdd84:      ldr x8, [x24]
1008fdd88:      cbz x8, 0x1008fcdfc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdb4>
1008fdd8c:      adrp    x0, 0x1010a7000 <_anon.58120679d426c7dccd15bda76f596bde.683>
1008fdd90:      add x0, x0, #0x8d8
1008fdd94:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008fdd98:      adrp    x0, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fdd9c:      add x0, x0, #0x8a8
1008fdda0:      add x1, sp, #0x28
1008fdda4:      bl  0x100135b90 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
1008fdda8:      ldr d8, [x0, #0x8]
1008fddac:      fmov    x0, d8
1008fddb0:      add x1, sp, #0x30
1008fddb4:      add w3, w20, #0x1
1008fddb8:      mov x2, x19
1008fddbc:      bl  0x1008f01c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template22try_emit_shape_element>
1008fddc0:      cbnz    w0, 0x1008fddd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d8c>
1008fddc4:      add w1, w20, #0x1
1008fddc8:      mov.16b v0, v8
1008fddcc:      mov x0, x19
1008fddd0:      bl  0x1008fe0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
1008fddd4:      mov w8, #0x1                ; =1
1008fddd8:      ldr x9, [sp, #0x10]
1008fdddc:      sub x25, x8, x9
1008fdde0:      mov w26, #0x2               ; =2
1008fdde4:      mov w27, #0x2c              ; =44
1008fdde8:      adrp    x21, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fddec:      add x21, x21, #0xf58
1008fddf0:      mov w28, #0x18              ; =24
1008fddf4:      adrp    x22, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fddf8:      add x22, x22, #0x8a8
1008fddfc:      b   0x1008fde10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1dc8>
1008fde00:      add x26, x26, #0x1
1008fde04:      add x8, x25, x26
1008fde08:      cmp x8, #0x2
1008fde0c:      b.eq    0x1008fdf28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ee0>
1008fde10:      ldr x23, [x19, #0x10]
1008fde14:      ldr x8, [x19]
1008fde18:      cmp x8, x23
1008fde1c:      b.eq    0x1008fdeec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ea4>
1008fde20:      sub x8, x26, #0x1
1008fde24:      ldr x9, [x19, #0x8]
1008fde28:      strb    w27, [x9, x23]
1008fde2c:      add x9, x23, #0x1
1008fde30:      str x9, [x19, #0x10]
1008fde34:      str x8, [sp, #0x60]
1008fde38:      add x1, sp, #0x60
1008fde3c:      mov x0, x21
1008fde40:      bl  0x100164dac <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
1008fde44:      ldr x8, [x24]
1008fde48:      cmn x8, #0x1
1008fde4c:      b.eq    0x1008fdeb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e68>
1008fde50:      mrs x9, TPIDRRO_EL0
1008fde54:      and x9, x9, #0xfffffffffffffff8
1008fde58:      ldr x8, [x9, x8, lsl #3]
1008fde5c:      cbz x8, 0x1008fdeb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e68>
1008fde60:      ldr x8, [x8, #0x19e8]
1008fde64:      cbz x8, 0x1008fdeb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e68>
1008fde68:      ldr x9, [x8]
1008fde6c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1008fde70:      cmp x9, x10
1008fde74:      b.hs    0x1008fdf0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
1008fde78:      ldr x10, [sp, #0x28]
1008fde7c:      add x11, x9, #0x1
1008fde80:      str x11, [x8]
1008fde84:      ldr x11, [x8, #0x18]
1008fde88:      cmp x10, x11
1008fde8c:      b.hs    0x1008fdf08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
1008fde90:      ldr x11, [x8, #0x10]
1008fde94:      madd    x10, x10, x28, x11
1008fde98:      ldr x11, [x10]
1008fde9c:      cmp x11, #0x1
1008fdea0:      b.ne    0x1008fdf18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ed0>
1008fdea4:      ldr x0, [x10, #0x8]
1008fdea8:      str x9, [x8]
1008fdeac:      b   0x1008fdebc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e74>
1008fdeb0:      add x1, sp, #0x28
1008fdeb4:      mov x0, x22
1008fdeb8:      bl  0x100135b90 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
1008fdebc:      ldr d8, [x0, x26, lsl #3]
1008fdec0:      fmov    x0, d8
1008fdec4:      add x1, sp, #0x30
1008fdec8:      add w3, w20, #0x1
1008fdecc:      mov x2, x19
1008fded0:      bl  0x1008f01c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template22try_emit_shape_element>
1008fded4:      tbnz    w0, #0x0, 0x1008fde00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1db8>
1008fded8:      add w1, w20, #0x1
1008fdedc:      mov.16b v0, v8
1008fdee0:      mov x0, x19
1008fdee4:      bl  0x1008fe0f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
1008fdee8:      b   0x1008fde00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1db8>
1008fdeec:      mov x0, x19
1008fdef0:      mov x1, x23
1008fdef4:      mov w2, #0x1                ; =1
1008fdef8:      mov w3, #0x1                ; =1
1008fdefc:      mov w4, #0x1                ; =1
1008fdf00:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fdf04:      b   0x1008fde20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1dd8>
1008fdf08:      bl  0x100cd3b3c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1008fdf0c:      adrp    x0, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fdf10:      add x0, x0, #0x928
1008fdf14:      bl  0x100c9de9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008fdf18:      adrp    x0, 0x100e1b000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1432+0x9>
1008fdf1c:      add x0, x0, #0x744
1008fdf20:      mov w1, #0xb                ; =11
1008fdf24:      bl  0x100cd3b04 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1008fdf28:      ldr x20, [x19, #0x10]
1008fdf2c:      ldr x8, [x19]
1008fdf30:      cmp x8, x20
1008fdf34:      b.eq    0x1008fe0c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2080>
1008fdf38:      ldr x8, [x19, #0x8]
1008fdf3c:      mov w9, #0x5d               ; =93
1008fdf40:      strb    w9, [x8, x20]
1008fdf44:      add x8, x20, #0x1
1008fdf48:      str x8, [x19, #0x10]
1008fdf4c:      adrp    x0, 0x1010dd000 <_anon.3c709ec65efe22d27798c2815252f2a2.778+0x188>
1008fdf50:      add x0, x0, #0x20
1008fdf54:      bl  0x1001385e8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths5_0INtNtBZ_6option6OptionjEEB2j_>
1008fdf58:      b   0x1008fd9f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19a8>
1008fdf5c:      mov x0, x19
1008fdf60:      mov x1, x20
1008fdf64:      mov w2, #0x1                ; =1
1008fdf68:      mov w3, #0x1                ; =1
1008fdf6c:      mov w4, #0x1                ; =1
1008fdf70:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fdf74:      b   0x1008fceb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe68>
1008fdf78:      mov x0, x19
1008fdf7c:      mov x1, x20
1008fdf80:      mov w2, #0x1                ; =1
1008fdf84:      mov w3, #0x1                ; =1
1008fdf88:      mov w4, #0x1                ; =1
1008fdf8c:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fdf90:      b   0x1008fda98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a50>
1008fdf94:      adrp    x0, 0x100e1e000 <_anon.3c709ec65efe22d27798c2815252f2a2.574+0x4>
1008fdf98:      add x0, x0, #0xa6f
1008fdf9c:      mov w1, #0x34               ; =52
1008fdfa0:      bl  0x100957b0c <_js_string_from_bytes>
1008fdfa4:      bl  0x1003ceec4 <_js_rangeerror_new>
1008fdfa8:      mov x8, #0x1                ; =1
1008fdfac:      movk    x8, #0x7ffc, lsl #48
1008fdfb0:      lsr x9, x0, #52
1008fdfb4:      mov x10, #0x7ffd000000000000 ; =9222527611924643840
1008fdfb8:      bfxil   x10, x0, #0, #48
1008fdfbc:      cmp x9, #0x7fe
1008fdfc0:      csel    x9, x0, x10, hi
1008fdfc4:      cmp x0, #0x0
1008fdfc8:      csinc   x8, x9, x8, ne
1008fdfcc:      fmov    d0, x8
1008fdfd0:      bl  0x100493ef8 <_js_throw>
1008fdfd4:      mov x0, x19
1008fdfd8:      mov x1, x21
1008fdfdc:      mov w2, #0x1                ; =1
1008fdfe0:      mov w3, #0x1                ; =1
1008fdfe4:      mov w4, #0x1                ; =1
1008fdfe8:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fdfec:      b   0x1008fd238 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11f0>
1008fdff0:      mov x0, x19
1008fdff4:      mov x1, x21
1008fdff8:      mov w2, #0x1                ; =1
1008fdffc:      mov w3, #0x1                ; =1
1008fe000:      mov w4, #0x1                ; =1
1008fe004:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fe008:      b   0x1008fd9d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1988>
1008fe00c:      mov x0, x19
1008fe010:      mov x1, x20
1008fe014:      mov w2, #0x4                ; =4
1008fe018:      mov w3, #0x1                ; =1
1008fe01c:      mov w4, #0x1                ; =1
1008fe020:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fe024:      ldr x20, [x19, #0x10]
1008fe028:      b   0x1008fda64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a1c>
1008fe02c:      mov x0, x19
1008fe030:      mov x1, x20
1008fe034:      mov w2, #0x5                ; =5
1008fe038:      mov w3, #0x1                ; =1
1008fe03c:      mov w4, #0x1                ; =1
1008fe040:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fe044:      ldr x20, [x19, #0x10]
1008fe048:      b   0x1008fcf18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xed0>
1008fe04c:      mov x0, x19
1008fe050:      mov x1, x20
1008fe054:      mov w2, #0x4                ; =4
1008fe058:      mov w3, #0x1                ; =1
1008fe05c:      mov w4, #0x1                ; =1
1008fe060:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fe064:      ldr x20, [x19, #0x10]
1008fe068:      b   0x1008fd180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1138>
1008fe06c:      adrp    x0, 0x100e1e000 <_anon.3c709ec65efe22d27798c2815252f2a2.574+0x4>
1008fe070:      add x0, x0, #0xa49
1008fe074:      mov w1, #0x25               ; =37
1008fe078:      bl  0x100957b0c <_js_string_from_bytes>
1008fe07c:      bl  0x1003dc904 <_js_typeerror_new>
1008fe080:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1008fe084:      bfxil   x8, x0, #0, #48
1008fe088:      fmov    d0, x8
1008fe08c:      bl  0x100493ef8 <_js_throw>
1008fe090:      mov x0, x19
1008fe094:      mov w2, #0x4                ; =4
1008fe098:      mov w3, #0x1                ; =1
1008fe09c:      mov w4, #0x1                ; =1
1008fe0a0:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fe0a4:      ldr x1, [x19, #0x10]
1008fe0a8:      b   0x1008fd1e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11a0>
1008fe0ac:      mov x0, x19
1008fe0b0:      mov x1, x21
1008fe0b4:      mov w2, #0x1                ; =1
1008fe0b8:      mov w3, #0x1                ; =1
1008fe0bc:      mov w4, #0x1                ; =1
1008fe0c0:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fe0c4:      b   0x1008fd0b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x106c>
1008fe0c8:      mov x0, x19
1008fe0cc:      mov x1, x20
1008fe0d0:      mov w2, #0x1                ; =1
1008fe0d4:      mov w3, #0x1                ; =1
1008fe0d8:      mov w4, #0x1                ; =1
1008fe0dc:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008fe0e0:      b   0x1008fdf38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ef0>
1008fe0e4:      adrp    x2, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008fe0e8:      add x2, x2, #0xcf8
1008fe0ec:      mov w0, #0x5                ; =5
1008fe0f0:      mov w1, #0x5                ; =5
1008fe0f4:      bl  0x100c9dfcc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
