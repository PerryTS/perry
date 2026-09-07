/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001006d6100 <_js_array_grow>:
1006d6100:      sub sp, sp, #0xb0
1006d6104:      stp x28, x27, [sp, #0x50]
1006d6108:      stp x26, x25, [sp, #0x60]
1006d610c:      stp x24, x23, [sp, #0x70]
1006d6110:      stp x22, x21, [sp, #0x80]
1006d6114:      stp x20, x19, [sp, #0x90]
1006d6118:      stp x29, x30, [sp, #0xa0]
1006d611c:      add x29, sp, #0xa0
1006d6120:      cmp x0, #0xfff
1006d6124:      b.ls    0x1006d6474 <_js_array_grow+0x374>
1006d6128:      mov x19, x0
1006d612c:      lsr x8, x0, #51
1006d6130:      cmp x8, #0xfff
1006d6134:      b.lo    0x1006d614c <_js_array_grow+0x4c>
1006d6138:      mov w8, #0x7ffc             ; =32764
1006d613c:      cmp x8, x19, lsr #48
1006d6140:      b.eq    0x1006d6474 <_js_array_grow+0x374>
1006d6144:      ands    x19, x19, #0xffffffffffff
1006d6148:      b.eq    0x1006d6474 <_js_array_grow+0x374>
1006d614c:      and x8, x19, #0xfffffffffff00000
1006d6150:      lsr x9, x19, #47
1006d6154:      cmp x9, #0x0
1006d6158:      ccmp    x8, #0x0, #0x4, eq
1006d615c:      b.eq    0x1006d6474 <_js_array_grow+0x374>
1006d6160:      tst x19, #0x3
1006d6164:      ccmp    x19, #0x7, #0x0, eq
1006d6168:      mov x22, x1
1006d616c:      b.ls    0x1006d6274 <_js_array_grow+0x174>
1006d6170:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
1006d6174:      add x8, x8, #0x8f0
1006d6178:      ldr x8, [x8]
1006d617c:      cmn x8, #0x1
1006d6180:      b.eq    0x1006d6658 <_js_array_grow+0x558>
1006d6184:      mrs x9, TPIDRRO_EL0
1006d6188:      and x9, x9, #0xfffffffffffffff8
1006d618c:      ldr x0, [x9, x8, lsl #3]
1006d6190:      cbz x0, 0x1006d6658 <_js_array_grow+0x558>
1006d6194:      lsr x1, x19, #20
1006d6198:      ldr x8, [x0, #0x10]
1006d619c:      ldrb    w9, [x8, #0x28]
1006d61a0:      tbz w9, #0x0, 0x1006d61c0 <_js_array_grow+0xc0>
1006d61a4:      ldr x9, [x8, #0x20]
1006d61a8:      cmp x9, x1
1006d61ac:      b.ne    0x1006d61c0 <_js_array_grow+0xc0>
1006d61b0:      ldp x9, x10, [x8]
1006d61b4:      cmp x9, x19
1006d61b8:      ccmp    x10, x19, #0x0, ls
1006d61bc:      b.hi    0x1006d623c <_js_array_grow+0x13c>
1006d61c0:      ldrb    w9, [x8, #0x58]
1006d61c4:      cbz w9, 0x1006d61e4 <_js_array_grow+0xe4>
1006d61c8:      ldr x9, [x8, #0x50]
1006d61cc:      cmp x9, x1
1006d61d0:      b.ne    0x1006d61e4 <_js_array_grow+0xe4>
1006d61d4:      ldp x9, x10, [x8, #0x30]
1006d61d8:      cmp x9, x19
1006d61dc:      ccmp    x10, x19, #0x0, ls
1006d61e0:      b.hi    0x1006d6230 <_js_array_grow+0x130>
1006d61e4:      ldrb    w9, [x8, #0x88]
1006d61e8:      cbz w9, 0x1006d6208 <_js_array_grow+0x108>
1006d61ec:      ldr x9, [x8, #0x80]
1006d61f0:      cmp x9, x1
1006d61f4:      b.ne    0x1006d6208 <_js_array_grow+0x108>
1006d61f8:      ldp x9, x10, [x8, #0x60]
1006d61fc:      cmp x9, x19
1006d6200:      ccmp    x10, x19, #0x0, ls
1006d6204:      b.hi    0x1006d6238 <_js_array_grow+0x138>
1006d6208:      ldrb    w9, [x8, #0xb8]
1006d620c:      cbz w9, 0x1006d6248 <_js_array_grow+0x148>
1006d6210:      ldr x9, [x8, #0xb0]
1006d6214:      cmp x9, x1
1006d6218:      b.ne    0x1006d6248 <_js_array_grow+0x148>
1006d621c:      ldp x9, x10, [x8, #0x90]!
1006d6220:      cmp x9, x19
1006d6224:      ccmp    x10, x19, #0x0, ls
1006d6228:      b.hi    0x1006d623c <_js_array_grow+0x13c>
1006d622c:      b   0x1006d6248 <_js_array_grow+0x148>
1006d6230:      add x8, x8, #0x30
1006d6234:      b   0x1006d623c <_js_array_grow+0x13c>
1006d6238:      add x8, x8, #0x60
1006d623c:      ldrb    w8, [x8, #0x19]
1006d6240:      cmp w8, #0xff
1006d6244:      b.ne    0x1006d6254 <_js_array_grow+0x154>
1006d6248:      mov x0, x19
1006d624c:      bl  0x10045cfb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
1006d6250:      and w8, w0, #0xff
1006d6254:      cbz w8, 0x1006d6274 <_js_array_grow+0x174>
1006d6258:      ldurb   w8, [x19, #-0x8]
1006d625c:      ldurb   w9, [x19, #-0x7]
1006d6260:      mov w10, #0x82              ; =130
1006d6264:      and w9, w9, w10
1006d6268:      cmp w9, #0x2
1006d626c:      ccmp    w8, #0x1, #0x0, eq
1006d6270:      b.eq    0x1006d6498 <_js_array_grow+0x398>
1006d6274:      mov x0, x19
1006d6278:      bl  0x100693e04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1006d627c:      mov x8, x0
1006d6280:      cbz x0, 0x1006d631c <_js_array_grow+0x21c>
1006d6284:      ldrb    w9, [x8]
1006d6288:      cmp w9, #0x1
1006d628c:      b.ne    0x1006d63ac <_js_array_grow+0x2ac>
1006d6290:      ldrsb   w9, [x8, #0x1]
1006d6294:      mov x0, x8
1006d6298:      tbz w9, #0x1f, 0x1006d63f0 <_js_array_grow+0x2f0>
1006d629c:      mov x20, x8
1006d62a0:      ldr x19, [x8, #0x8]
1006d62a4:      mov x0, x19
1006d62a8:      bl  0x100693e04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1006d62ac:      mov x1, x22
1006d62b0:      cbz x0, 0x1006d6474 <_js_array_grow+0x374>
1006d62b4:      ldrb    w8, [x0]
1006d62b8:      cmp w8, #0x1
1006d62bc:      b.ne    0x1006d6474 <_js_array_grow+0x374>
1006d62c0:      ldrsb   w8, [x0, #0x1]
1006d62c4:      tbz w8, #0x1f, 0x1006d63a4 <_js_array_grow+0x2a4>
1006d62c8:      mov w21, #0x1               ; =1
1006d62cc:      ldr x19, [x0, #0x8]
1006d62d0:      mov x0, x19
1006d62d4:      bl  0x100693e04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1006d62d8:      mov x1, x22
1006d62dc:      cbz x0, 0x1006d6474 <_js_array_grow+0x374>
1006d62e0:      ldrb    w8, [x0]
1006d62e4:      cmp w8, #0x1
1006d62e8:      b.ne    0x1006d6474 <_js_array_grow+0x374>
1006d62ec:      cmp w21, #0x3f
1006d62f0:      b.hi    0x1006d6474 <_js_array_grow+0x374>
1006d62f4:      add w21, w21, #0x1
1006d62f8:      ldrsb   w8, [x0, #0x1]
1006d62fc:      tbnz    w8, #0x1f, 0x1006d62cc <_js_array_grow+0x1cc>
1006d6300:      mov x8, x20
1006d6304:      str x19, [x20, #0x8]
1006d6308:      ldrb    w9, [x20, #0x1]
1006d630c:      orr w9, w9, #0x80
1006d6310:      strb    w9, [x20, #0x1]
1006d6314:      ldrb    w9, [x0]
1006d6318:      b   0x1006d63b4 <_js_array_grow+0x2b4>
1006d631c:      mov x20, x8
1006d6320:      adrp    x8, 0x101200000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7a8>
1006d6324:      add x8, x8, #0xf2a
1006d6328:      ldaprb  w8, [x8]
1006d632c:      cbz w8, 0x1006d635c <_js_array_grow+0x25c>
1006d6330:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
1006d6334:      add x8, x8, #0xbb0
1006d6338:      ldapr   x9, [x8]
1006d633c:      cmp x9, x19
1006d6340:      b.hi    0x1006d635c <_js_array_grow+0x25c>
1006d6344:      ldapur  x8, [x8, #0x8]
1006d6348:      cmp x8, x19
1006d634c:      b.lo    0x1006d635c <_js_array_grow+0x25c>
1006d6350:      mov x0, x19
1006d6354:      bl  0x1004c8a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
1006d6358:      tbnz    w0, #0x0, 0x1006d63a0 <_js_array_grow+0x2a0>
1006d635c:      adrp    x8, 0x101211000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfc20>
1006d6360:      add x8, x8, #0xad0
1006d6364:      ldaprb  w8, [x8]
1006d6368:      mov x1, x22
1006d636c:      cbz w8, 0x1006d6474 <_js_array_grow+0x374>
1006d6370:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
1006d6374:      add x8, x8, #0x8c0
1006d6378:      ldapr   x9, [x8]
1006d637c:      cmp x9, x19
1006d6380:      b.hi    0x1006d6474 <_js_array_grow+0x374>
1006d6384:      ldapur  x8, [x8, #0x8]
1006d6388:      cmp x8, x19
1006d638c:      b.lo    0x1006d6474 <_js_array_grow+0x374>
1006d6390:      mov x0, x19
1006d6394:      bl  0x1008bdee8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
1006d6398:      mov x1, x22
1006d639c:      tbz w0, #0x0, 0x1006d6474 <_js_array_grow+0x374>
1006d63a0:      mov x0, #0x0                ; =0
1006d63a4:      mov x8, x20
1006d63a8:      b   0x1006d63f0 <_js_array_grow+0x2f0>
1006d63ac:      mov x0, x8
1006d63b0:      mov x1, x22
1006d63b4:      cmp w9, #0x1
1006d63b8:      b.eq    0x1006d63f0 <_js_array_grow+0x2f0>
1006d63bc:      cmp w9, #0x9
1006d63c0:      b.ne    0x1006d6474 <_js_array_grow+0x374>
1006d63c4:      ldr w8, [x19, #0x4]
1006d63c8:      mov w9, #0x5841             ; =22593
1006d63cc:      movk    w9, #0x4c5a, lsl #16
1006d63d0:      cmp w8, w9
1006d63d4:      b.ne    0x1006d6474 <_js_array_grow+0x374>
1006d63d8:      mov x0, x19
1006d63dc:      bl  0x10035a198 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape22force_materialize_lazy>
1006d63e0:      mov x1, x22
1006d63e4:      cbz x0, 0x1006d6474 <_js_array_grow+0x374>
1006d63e8:      mov x19, x0
1006d63ec:      b   0x1006d64bc <_js_array_grow+0x3bc>
1006d63f0:      ldp w10, w9, [x19]
1006d63f4:      cmp w10, w9
1006d63f8:      b.ls    0x1006d6418 <_js_array_grow+0x318>
1006d63fc:      cbz x8, 0x1006d6428 <_js_array_grow+0x328>
1006d6400:      ldr w8, [x0, #0x4]
1006d6404:      lsl x9, x9, #3
1006d6408:      add x9, x9, #0x10
1006d640c:      cmp x9, x8
1006d6410:      b.ne    0x1006d6428 <_js_array_grow+0x328>
1006d6414:      b   0x1006d64bc <_js_array_grow+0x3bc>
1006d6418:      mov w8, #0xe100             ; =57600
1006d641c:      movk    w8, #0x5f5, lsl #16
1006d6420:      cmp w10, w8
1006d6424:      b.ls    0x1006d64bc <_js_array_grow+0x3bc>
1006d6428:      adrp    x8, 0x101200000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7a8>
1006d642c:      add x8, x8, #0xf2a
1006d6430:      ldaprb  w8, [x8]
1006d6434:      cbz w8, 0x1006d6464 <_js_array_grow+0x364>
1006d6438:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
1006d643c:      add x8, x8, #0xbb0
1006d6440:      ldapr   x9, [x8]
1006d6444:      cmp x9, x19
1006d6448:      b.hi    0x1006d6464 <_js_array_grow+0x364>
1006d644c:      ldapur  x8, [x8, #0x8]
1006d6450:      cmp x8, x19
1006d6454:      b.lo    0x1006d6464 <_js_array_grow+0x364>
1006d6458:      mov x0, x19
1006d645c:      bl  0x1004c8a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
1006d6460:      tbnz    w0, #0x0, 0x1006d64bc <_js_array_grow+0x3bc>
1006d6464:      mov x0, x19
1006d6468:      bl  0x100668d5c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray23lookup_typed_array_kind>
1006d646c:      mov x1, x22
1006d6470:      tbnz    w0, #0x0, 0x1006d64bc <_js_array_grow+0x3bc>
1006d6474:      mov x0, x1
1006d6478:      ldp x29, x30, [sp, #0xa0]
1006d647c:      ldp x20, x19, [sp, #0x90]
1006d6480:      ldp x22, x21, [sp, #0x80]
1006d6484:      ldp x24, x23, [sp, #0x70]
1006d6488:      ldp x26, x25, [sp, #0x60]
1006d648c:      ldp x28, x27, [sp, #0x50]
1006d6490:      add sp, sp, #0xb0
1006d6494:      b   0x1003a92e8 <_js_array_alloc>
1006d6498:      ldr w8, [x19]
1006d649c:      mov w9, #0xe100             ; =57600
1006d64a0:      movk    w9, #0x5f5, lsl #16
1006d64a4:      orr w9, w9, #0x1
1006d64a8:      cmp w8, w9
1006d64ac:      b.hs    0x1006d6274 <_js_array_grow+0x174>
1006d64b0:      ldr w9, [x19, #0x4]
1006d64b4:      cmp w8, w9
1006d64b8:      b.hi    0x1006d6274 <_js_array_grow+0x174>
1006d64bc:      mov x0, x19
1006d64c0:      bl  0x10068714c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header18array_object_flags>
1006d64c4:      tst w0, #0x6
1006d64c8:      b.ne    0x1006d6cc0 <_js_array_grow+0xbc0>
1006d64cc:      mov x0, x19
1006d64d0:      bl  0x10068714c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header18array_object_flags>
1006d64d4:      tbnz    w0, #0x0, 0x1006d6cc0 <_js_array_grow+0xbc0>
1006d64d8:      adrp    x26, 0x101134000 <_perry_global_baseline_worker_ts__1>
1006d64dc:      add x26, x26, #0x8f0
1006d64e0:      ldr x8, [x26]
1006d64e4:      cmn x8, #0x1
1006d64e8:      b.eq    0x1006d651c <_js_array_grow+0x41c>
1006d64ec:      mrs x9, TPIDRRO_EL0
1006d64f0:      and x9, x9, #0xfffffffffffffff8
1006d64f4:      ldr x8, [x9, x8, lsl #3]
1006d64f8:      cbz x8, 0x1006d651c <_js_array_grow+0x41c>
1006d64fc:      ldr x8, [x8, #0x19e8]
1006d6500:      cbz x8, 0x1006d651c <_js_array_grow+0x41c>
1006d6504:      ldr x9, [x8]
1006d6508:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1006d650c:      cmp x9, x10
1006d6510:      b.hs    0x1006d6e9c <_js_array_grow+0xd9c>
1006d6514:      ldr x20, [x8, #0x18]
1006d6518:      b   0x1006d652c <_js_array_grow+0x42c>
1006d651c:      adrp    x0, 0x1010ca000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7promise10then_probe17VERDICT_IN_FLIGHT+0x2228>
1006d6520:      add x0, x0, #0xd70
1006d6524:      bl  0x100135d6c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
1006d6528:      mov x20, x0
1006d652c:      str x20, [sp, #0x20]
1006d6530:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1006d6534:      stp x19, x8, [sp, #0x40]
1006d6538:      mov w8, #0x1                ; =1
1006d653c:      str x8, [sp, #0x38]
1006d6540:      add x0, sp, #0x38
1006d6544:      bl  0x100668884 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1006d6548:      str x0, [sp, #0x28]
1006d654c:      ldr w23, [x19, #0x4]
1006d6550:      cmp w22, w23
1006d6554:      b.ls    0x1006d6614 <_js_array_grow+0x514>
1006d6558:      mov x21, x0
1006d655c:      lsl w8, w23, #1
1006d6560:      cmp w22, w8
1006d6564:      csel    w24, w22, w8, hi
1006d6568:      ubfiz   x22, x24, #3, #32
1006d656c:      ldurb   w8, [x19, #-0x7]
1006d6570:      tbnz    w8, #0x5, 0x1006d6830 <_js_array_grow+0x730>
1006d6574:      ldr x8, [x26]
1006d6578:      cmn x8, #0x1
1006d657c:      b.eq    0x1006d6808 <_js_array_grow+0x708>
1006d6580:      mrs x9, TPIDRRO_EL0
1006d6584:      and x9, x9, #0xfffffffffffffff8
1006d6588:      ldr x0, [x9, x8, lsl #3]
1006d658c:      cbz x0, 0x1006d6808 <_js_array_grow+0x708>
1006d6590:      lsr x1, x19, #20
1006d6594:      ldr x8, [x0, #0x10]
1006d6598:      ldrb    w9, [x8, #0x28]
1006d659c:      tbz w9, #0x0, 0x1006d65bc <_js_array_grow+0x4bc>
1006d65a0:      ldr x9, [x8, #0x20]
1006d65a4:      cmp x9, x1
1006d65a8:      b.ne    0x1006d65bc <_js_array_grow+0x4bc>
1006d65ac:      ldp x9, x10, [x8]
1006d65b0:      cmp x9, x19
1006d65b4:      ccmp    x10, x19, #0x0, ls
1006d65b8:      b.hi    0x1006d66a8 <_js_array_grow+0x5a8>
1006d65bc:      ldrb    w9, [x8, #0x58]
1006d65c0:      cbz w9, 0x1006d65e0 <_js_array_grow+0x4e0>
1006d65c4:      ldr x9, [x8, #0x50]
1006d65c8:      cmp x9, x1
1006d65cc:      b.ne    0x1006d65e0 <_js_array_grow+0x4e0>
1006d65d0:      ldp x9, x10, [x8, #0x30]
1006d65d4:      cmp x9, x19
1006d65d8:      ccmp    x10, x19, #0x0, ls
1006d65dc:      b.hi    0x1006d66a4 <_js_array_grow+0x5a4>
1006d65e0:      ldrb    w9, [x8, #0x88]
1006d65e4:      cbz w9, 0x1006d6670 <_js_array_grow+0x570>
1006d65e8:      ldr x9, [x8, #0x80]
1006d65ec:      cmp x9, x1
1006d65f0:      b.ne    0x1006d6670 <_js_array_grow+0x570>
1006d65f4:      ldr x9, [x8, #0x60]
1006d65f8:      cmp x9, x19
1006d65fc:      b.hi    0x1006d6670 <_js_array_grow+0x570>
1006d6600:      ldr x9, [x8, #0x68]
1006d6604:      cmp x9, x19
1006d6608:      b.ls    0x1006d6670 <_js_array_grow+0x570>
1006d660c:      add x8, x8, #0x60
1006d6610:      b   0x1006d66a8 <_js_array_grow+0x5a8>
1006d6614:      ldr x8, [x26]
1006d6618:      cmn x8, #0x1
1006d661c:      b.eq    0x1006d6cb0 <_js_array_grow+0xbb0>
1006d6620:      mrs x9, TPIDRRO_EL0
1006d6624:      and x9, x9, #0xfffffffffffffff8
1006d6628:      ldr x8, [x9, x8, lsl #3]
1006d662c:      cbz x8, 0x1006d6cb0 <_js_array_grow+0xbb0>
1006d6630:      ldr x8, [x8, #0x19e8]
1006d6634:      cbz x8, 0x1006d6cb0 <_js_array_grow+0xbb0>
1006d6638:      ldr x9, [x8]
1006d663c:      cbnz    x9, 0x1006d6ca4 <_js_array_grow+0xba4>
1006d6640:      ldr x9, [x8, #0x18]
1006d6644:      cmp x20, x9
1006d6648:      b.hi    0x1006d6650 <_js_array_grow+0x550>
1006d664c:      str x20, [x8, #0x18]
1006d6650:      str xzr, [x8]
1006d6654:      b   0x1006d6cc0 <_js_array_grow+0xbc0>
1006d6658:      bl  0x100cd2ac8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006d665c:      lsr x1, x19, #20
1006d6660:      ldr x8, [x0, #0x10]
1006d6664:      ldrb    w9, [x8, #0x28]
1006d6668:      tbnz    w9, #0x0, 0x1006d61a4 <_js_array_grow+0xa4>
1006d666c:      b   0x1006d61c0 <_js_array_grow+0xc0>
1006d6670:      ldrb    w9, [x8, #0xb8]
1006d6674:      cbz w9, 0x1006d66b4 <_js_array_grow+0x5b4>
1006d6678:      ldr x9, [x8, #0xb0]
1006d667c:      cmp x9, x1
1006d6680:      b.ne    0x1006d66b4 <_js_array_grow+0x5b4>
1006d6684:      ldr x9, [x8, #0x90]
1006d6688:      cmp x9, x19
1006d668c:      b.hi    0x1006d66b4 <_js_array_grow+0x5b4>
1006d6690:      ldr x9, [x8, #0x98]
1006d6694:      cmp x9, x19
1006d6698:      b.ls    0x1006d66b4 <_js_array_grow+0x5b4>
1006d669c:      add x8, x8, #0x90
1006d66a0:      b   0x1006d66a8 <_js_array_grow+0x5a8>
1006d66a4:      add x8, x8, #0x30
1006d66a8:      ldrb    w8, [x8, #0x19]
1006d66ac:      cmp w8, #0xff
1006d66b0:      b.ne    0x1006d66c0 <_js_array_grow+0x5c0>
1006d66b4:      mov x0, x19
1006d66b8:      bl  0x10045cfb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
1006d66bc:      and w8, w0, #0xff
1006d66c0:      cmp w8, #0x1
1006d66c4:      b.ne    0x1006d6830 <_js_array_grow+0x730>
1006d66c8:      mov w8, #0x3ffe             ; =16382
1006d66cc:      cmp w24, w8
1006d66d0:      b.hi    0x1006d6830 <_js_array_grow+0x730>
1006d66d4:      ldr x8, [x26]
1006d66d8:      cmn x8, #0x1
1006d66dc:      b.eq    0x1006d6820 <_js_array_grow+0x720>
1006d66e0:      mrs x9, TPIDRRO_EL0
1006d66e4:      and x9, x9, #0xfffffffffffffff8
1006d66e8:      ldr x0, [x9, x8, lsl #3]
1006d66ec:      cbz x0, 0x1006d6820 <_js_array_grow+0x720>
1006d66f0:      ldr x8, [x0, #0x28]
1006d66f4:      ldrb    w8, [x8]
1006d66f8:      tbnz    w8, #0x0, 0x1006d6830 <_js_array_grow+0x730>
1006d66fc:      ldr x8, [x26]
1006d6700:      cmn x8, #0x1
1006d6704:      b.eq    0x1006d6a0c <_js_array_grow+0x90c>
1006d6708:      mrs x9, TPIDRRO_EL0
1006d670c:      and x9, x9, #0xfffffffffffffff8
1006d6710:      ldr x0, [x9, x8, lsl #3]
1006d6714:      cbz x0, 0x1006d6a0c <_js_array_grow+0x90c>
1006d6718:      ldr x25, [x0, #0x8]
1006d671c:      ldr x8, [x26]
1006d6720:      cmn x8, #0x1
1006d6724:      b.eq    0x1006d6a20 <_js_array_grow+0x920>
1006d6728:      mrs x9, TPIDRRO_EL0
1006d672c:      and x9, x9, #0xfffffffffffffff8
1006d6730:      ldr x0, [x9, x8, lsl #3]
1006d6734:      cbz x0, 0x1006d6a20 <_js_array_grow+0x920>
1006d6738:      ldr x19, [x0]
1006d673c:      ldr x8, [x25]
1006d6740:      cbz x8, 0x1006d6764 <_js_array_grow+0x664>
1006d6744:      ldp x1, x0, [x19, #0x10]
1006d6748:      cmp x0, x1
1006d674c:      b.hs    0x1006d6f00 <_js_array_grow+0xe00>
1006d6750:      ldr x8, [x19, #0x8]
1006d6754:      ldr x9, [x25, #0x8]
1006d6758:      mov w10, #0x30              ; =48
1006d675c:      madd    x8, x0, x10, x8
1006d6760:      str x9, [x8, #0x20]
1006d6764:      add x27, x22, #0x10
1006d6768:      ldr x1, [x19, #0x18]
1006d676c:      add x2, x22, #0x10
1006d6770:      mov x0, x19
1006d6774:      bl  0x1006640a8 <__RNvMs1_NtNtCs5gMwpk3Cs4e_13perry_runtime5arena5blockNtB5_5Arena15try_block_alloc>
1006d6778:      cmp x0, #0x1
1006d677c:      b.ne    0x1006d6830 <_js_array_grow+0x730>
1006d6780:      ldr x8, [x25]
1006d6784:      cbz x8, 0x1006d67b4 <_js_array_grow+0x6b4>
1006d6788:      ldp x8, x0, [x19, #0x10]
1006d678c:      cmp x0, x8
1006d6790:      b.hs    0x1006d6f0c <_js_array_grow+0xe0c>
1006d6794:      ldr x8, [x19, #0x8]
1006d6798:      mov w9, #0x30               ; =48
1006d679c:      madd    x8, x0, x9, x8
1006d67a0:      ldr x9, [x8, #0x10]
1006d67a4:      ldur    q0, [x8, #0x18]
1006d67a8:      str x9, [x25]
1006d67ac:      ext.16b v0, v0, v0, #0x8
1006d67b0:      stur    q0, [x25, #0x8]
1006d67b4:      cbz x1, 0x1006d6830 <_js_array_grow+0x730>
1006d67b8:      mov w8, #0x1                ; =1
1006d67bc:      strb    w8, [x1]
1006d67c0:      ldr x8, [x26]
1006d67c4:      cmn x8, #0x1
1006d67c8:      b.eq    0x1006d6e8c <_js_array_grow+0xd8c>
1006d67cc:      mrs x9, TPIDRRO_EL0
1006d67d0:      and x9, x9, #0xfffffffffffffff8
1006d67d4:      ldr x0, [x9, x8, lsl #3]
1006d67d8:      cbz x0, 0x1006d6e8c <_js_array_grow+0xd8c>
1006d67dc:      ldr x8, [x0, #0x30]
1006d67e0:      ldrb    w8, [x8]
1006d67e4:      orr w8, w8, #0x2
1006d67e8:      strb    w8, [x1, #0x1]
1006d67ec:      mov x0, x1
1006d67f0:      mov x19, x1
1006d67f4:      bl  0x1006802d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier19gc_note_black_birth>
1006d67f8:      strh    wzr, [x19, #0x2]
1006d67fc:      str w27, [x19, #0x4]
1006d6800:      add x19, x19, #0x8
1006d6804:      b   0x1006d6850 <_js_array_grow+0x750>
1006d6808:      bl  0x100cd2ac8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006d680c:      lsr x1, x19, #20
1006d6810:      ldr x8, [x0, #0x10]
1006d6814:      ldrb    w9, [x8, #0x28]
1006d6818:      tbnz    w9, #0x0, 0x1006d65a0 <_js_array_grow+0x4a0>
1006d681c:      b   0x1006d65bc <_js_array_grow+0x4bc>
1006d6820:      bl  0x100cd2ac8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006d6824:      ldr x8, [x0, #0x28]
1006d6828:      ldrb    w8, [x8]
1006d682c:      tbz w8, #0x0, 0x1006d66fc <_js_array_grow+0x5fc>
1006d6830:      add x0, x22, #0x8
1006d6834:      mov w1, #0x8                ; =8
1006d6838:      mov w2, #0x1                ; =1
1006d683c:      bl  0x10036bc64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena10allocators18arena_alloc_gc_old>
1006d6840:      mov x19, x0
1006d6844:      ldurb   w8, [x0, #-0x7]
1006d6848:      orr w8, w8, #0x20
1006d684c:      sturb   w8, [x0, #-0x7]
1006d6850:      ldr x8, [x26]
1006d6854:      cmn x8, #0x1
1006d6858:      b.eq    0x1006d68bc <_js_array_grow+0x7bc>
1006d685c:      mrs x9, TPIDRRO_EL0
1006d6860:      and x9, x9, #0xfffffffffffffff8
1006d6864:      ldr x8, [x9, x8, lsl #3]
1006d6868:      cbz x8, 0x1006d68bc <_js_array_grow+0x7bc>
1006d686c:      ldr x8, [x8, #0x19e8]
1006d6870:      cbz x8, 0x1006d68bc <_js_array_grow+0x7bc>
1006d6874:      ldr x9, [x8]
1006d6878:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1006d687c:      cmp x9, x10
1006d6880:      b.hs    0x1006d6ec4 <_js_array_grow+0xdc4>
1006d6884:      add x10, x9, #0x1
1006d6888:      str x10, [x8]
1006d688c:      ldr x10, [x8, #0x18]
1006d6890:      cmp x21, x10
1006d6894:      b.hs    0x1006d6ed0 <_js_array_grow+0xdd0>
1006d6898:      ldr x10, [x8, #0x10]
1006d689c:      mov w11, #0x18              ; =24
1006d68a0:      madd    x10, x21, x11, x10
1006d68a4:      ldr x11, [x10]
1006d68a8:      cmp x11, #0x1
1006d68ac:      b.ne    0x1006d6ed4 <_js_array_grow+0xdd4>
1006d68b0:      ldr x21, [x10, #0x8]
1006d68b4:      str x9, [x8]
1006d68b8:      b   0x1006d68d0 <_js_array_grow+0x7d0>
1006d68bc:      adrp    x0, 0x1010ca000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7promise10then_probe17VERDICT_IN_FLIGHT+0x2228>
1006d68c0:      add x0, x0, #0xd70
1006d68c4:      add x1, sp, #0x28
1006d68c8:      bl  0x100135b90 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
1006d68cc:      mov x21, x0
1006d68d0:      lsl x8, x23, #3
1006d68d4:      add x22, x8, #0x8
1006d68d8:      mov x0, x19
1006d68dc:      mov x1, x21
1006d68e0:      mov x2, x22
1006d68e4:      bl  0x100ce9f6c <_writev+0x100ce9f6c>
1006d68e8:      str w24, [x19, #0x4]
1006d68ec:      sub x8, x24, x23
1006d68f0:      lsl x2, x8, #3
1006d68f4:      adrp    x1, 0x100dd7000 <_anon.f895325a8a8e91adc7c73ff5482c6caa.1688+0x188>
1006d68f8:      add x1, x1, #0xa70
1006d68fc:      add x0, x19, x22
1006d6900:      bl  0x100ce9f90 <_writev+0x100ce9f90>
1006d6904:      ldurh   w8, [x21, #-0x6]
1006d6908:      sturh   w8, [x19, #-0x6]
1006d690c:      mov x0, x21
1006d6910:      mov x1, x19
1006d6914:      bl  0x100241940 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout15layout_transfer>
1006d6918:      mov x0, x21
1006d691c:      mov x1, x19
1006d6920:      bl  0x10037ec6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header35transfer_array_named_property_owner>
1006d6924:      mov x0, x21
1006d6928:      mov x1, x19
1006d692c:      bl  0x1005f30b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object16descriptor_state25transfer_descriptor_owner>
1006d6930:      adrp    x8, 0x101201000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc11instruments24INCREMENTAL_CYCLE_STARTS>
1006d6934:      ldr w8, [x8, #0x184]
1006d6938:      cbz w8, 0x1006d6948 <_js_array_grow+0x848>
1006d693c:      mov x0, x19
1006d6940:      bl  0x100683cc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array15header_gc_slots34replay_array_growth_write_barriers>
1006d6944:      b   0x1006d6c40 <_js_array_grow+0xb40>
1006d6948:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
1006d694c:      add x8, x8, #0x1b8
1006d6950:      ldapr   x8, [x8]
1006d6954:      cbnz    x8, 0x1006d6ea8 <_js_array_grow+0xda8>
1006d6958:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
1006d695c:      ldrb    w8, [x8, #0x1c0]
1006d6960:      cbz w8, 0x1006d6c40 <_js_array_grow+0xb40>
1006d6964:      adrp    x8, 0x101200000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7a8>
1006d6968:      ldrb    w8, [x8, #0xea0]
1006d696c:      cbz w8, 0x1006d69d4 <_js_array_grow+0x8d4>
1006d6970:      tst x19, #0xffff800000000007
1006d6974:      mov w8, #0x100000           ; =1048576
1006d6978:      ccmp    x19, x8, #0x0, eq
1006d697c:      b.lo    0x1006d6c40 <_js_array_grow+0xb40>
1006d6980:      ldr x8, [x26]
1006d6984:      cmn x8, #0x1
1006d6988:      b.eq    0x1006d6a34 <_js_array_grow+0x934>
1006d698c:      mrs x9, TPIDRRO_EL0
1006d6990:      and x9, x9, #0xfffffffffffffff8
1006d6994:      ldr x0, [x9, x8, lsl #3]
1006d6998:      cbz x0, 0x1006d6a34 <_js_array_grow+0x934>
1006d699c:      lsr x1, x19, #20
1006d69a0:      ldr x8, [x0, #0x10]
1006d69a4:      ldrb    w9, [x8, #0x28]
1006d69a8:      tbz w9, #0x0, 0x1006d6a48 <_js_array_grow+0x948>
1006d69ac:      ldr x9, [x8, #0x20]
1006d69b0:      cmp x9, x1
1006d69b4:      b.ne    0x1006d6a48 <_js_array_grow+0x948>
1006d69b8:      ldr x9, [x8]
1006d69bc:      cmp x9, x19
1006d69c0:      b.hi    0x1006d6a48 <_js_array_grow+0x948>
1006d69c4:      ldr x9, [x8, #0x8]
1006d69c8:      cmp x9, x19
1006d69cc:      b.hi    0x1006d6ae0 <_js_array_grow+0x9e0>
1006d69d0:      b   0x1006d6a48 <_js_array_grow+0x948>
1006d69d4:      mov w8, #0xc                ; =12
1006d69d8:      strb    w8, [sp, #0x38]
1006d69dc:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
1006d69e0:      add x8, x8, #0x7f8
1006d69e4:      ldapr   x8, [x8]
1006d69e8:      cbnz    x8, 0x1006d6ee4 <_js_array_grow+0xde4>
1006d69ec:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
1006d69f0:      ldrb    w8, [x8, #0x800]
1006d69f4:      cbz w8, 0x1006d6c40 <_js_array_grow+0xb40>
1006d69f8:      adrp    x0, 0x1010cb000 <_anon.b0a7a82b26242b618f19288e3549d1f3.65+0x108>
1006d69fc:      add x0, x0, #0x588
1006d6a00:      add x1, sp, #0x38
1006d6a04:      bl  0x10012d574 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc9telemetry20BarrierTraceCountersEE4withNCNvNtB1w_7barrier32bump_write_barrier_trace_counter0uEB1y_>
1006d6a08:      b   0x1006d6c40 <_js_array_grow+0xb40>
1006d6a0c:      bl  0x100cd2ac8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006d6a10:      ldr x25, [x0, #0x8]
1006d6a14:      ldr x8, [x26]
1006d6a18:      cmn x8, #0x1
1006d6a1c:      b.ne    0x1006d6728 <_js_array_grow+0x628>
1006d6a20:      bl  0x100cd2ac8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006d6a24:      ldr x19, [x0]
1006d6a28:      ldr x8, [x25]
1006d6a2c:      cbnz    x8, 0x1006d6744 <_js_array_grow+0x644>
1006d6a30:      b   0x1006d6764 <_js_array_grow+0x664>
1006d6a34:      bl  0x100cd2ac8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006d6a38:      lsr x1, x19, #20
1006d6a3c:      ldr x8, [x0, #0x10]
1006d6a40:      ldrb    w9, [x8, #0x28]
1006d6a44:      tbnz    w9, #0x0, 0x1006d69ac <_js_array_grow+0x8ac>
1006d6a48:      ldrb    w9, [x8, #0x58]
1006d6a4c:      cbz w9, 0x1006d6a7c <_js_array_grow+0x97c>
1006d6a50:      ldr x9, [x8, #0x50]
1006d6a54:      cmp x9, x1
1006d6a58:      b.ne    0x1006d6a7c <_js_array_grow+0x97c>
1006d6a5c:      ldr x9, [x8, #0x30]
1006d6a60:      cmp x9, x19
1006d6a64:      b.hi    0x1006d6a7c <_js_array_grow+0x97c>
1006d6a68:      ldr x9, [x8, #0x38]
1006d6a6c:      cmp x9, x19
1006d6a70:      b.ls    0x1006d6a7c <_js_array_grow+0x97c>
1006d6a74:      add x8, x8, #0x30
1006d6a78:      b   0x1006d6ae0 <_js_array_grow+0x9e0>
1006d6a7c:      ldrb    w9, [x8, #0x88]
1006d6a80:      cbz w9, 0x1006d6ab0 <_js_array_grow+0x9b0>
1006d6a84:      ldr x9, [x8, #0x80]
1006d6a88:      cmp x9, x1
1006d6a8c:      b.ne    0x1006d6ab0 <_js_array_grow+0x9b0>
1006d6a90:      ldr x9, [x8, #0x60]
1006d6a94:      cmp x9, x19
1006d6a98:      b.hi    0x1006d6ab0 <_js_array_grow+0x9b0>
1006d6a9c:      ldr x9, [x8, #0x68]
1006d6aa0:      cmp x9, x19
1006d6aa4:      b.ls    0x1006d6ab0 <_js_array_grow+0x9b0>
1006d6aa8:      add x8, x8, #0x60
1006d6aac:      b   0x1006d6ae0 <_js_array_grow+0x9e0>
1006d6ab0:      ldrb    w9, [x8, #0xb8]
1006d6ab4:      cbz w9, 0x1006d6aec <_js_array_grow+0x9ec>
1006d6ab8:      ldr x9, [x8, #0xb0]
1006d6abc:      cmp x9, x1
1006d6ac0:      b.ne    0x1006d6aec <_js_array_grow+0x9ec>
1006d6ac4:      ldr x9, [x8, #0x90]
1006d6ac8:      cmp x9, x19
1006d6acc:      b.hi    0x1006d6aec <_js_array_grow+0x9ec>
1006d6ad0:      ldr x9, [x8, #0x98]
1006d6ad4:      cmp x9, x19
1006d6ad8:      b.ls    0x1006d6aec <_js_array_grow+0x9ec>
1006d6adc:      add x8, x8, #0x90
1006d6ae0:      ldrb    w8, [x8, #0x19]
1006d6ae4:      cmp w8, #0xff
1006d6ae8:      b.ne    0x1006d6af8 <_js_array_grow+0x9f8>
1006d6aec:      mov x0, x19
1006d6af0:      bl  0x10045cfb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
1006d6af4:      and w8, w0, #0xff
1006d6af8:      cmp w8, #0x3
1006d6afc:      b.ne    0x1006d6c40 <_js_array_grow+0xb40>
1006d6b00:      ldr x8, [x26]
1006d6b04:      cmn x8, #0x1
1006d6b08:      b.eq    0x1006d6b54 <_js_array_grow+0xa54>
1006d6b0c:      mrs x9, TPIDRRO_EL0
1006d6b10:      and x9, x9, #0xfffffffffffffff8
1006d6b14:      ldr x0, [x9, x8, lsl #3]
1006d6b18:      cbz x0, 0x1006d6b54 <_js_array_grow+0xa54>
1006d6b1c:      lsr x1, x21, #20
1006d6b20:      ldr x8, [x0, #0x10]
1006d6b24:      ldrb    w9, [x8, #0x28]
1006d6b28:      tbz w9, #0x0, 0x1006d6b68 <_js_array_grow+0xa68>
1006d6b2c:      ldr x9, [x8, #0x20]
1006d6b30:      cmp x9, x1
1006d6b34:      b.ne    0x1006d6b68 <_js_array_grow+0xa68>
1006d6b38:      ldr x9, [x8]
1006d6b3c:      cmp x9, x21
1006d6b40:      b.hi    0x1006d6b68 <_js_array_grow+0xa68>
1006d6b44:      ldr x9, [x8, #0x8]
1006d6b48:      cmp x9, x21
1006d6b4c:      b.hi    0x1006d6c00 <_js_array_grow+0xb00>
1006d6b50:      b   0x1006d6b68 <_js_array_grow+0xa68>
1006d6b54:      bl  0x100cd2ac8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006d6b58:      lsr x1, x21, #20
1006d6b5c:      ldr x8, [x0, #0x10]
1006d6b60:      ldrb    w9, [x8, #0x28]
1006d6b64:      tbnz    w9, #0x0, 0x1006d6b2c <_js_array_grow+0xa2c>
1006d6b68:      ldrb    w9, [x8, #0x58]
1006d6b6c:      cbz w9, 0x1006d6b9c <_js_array_grow+0xa9c>
1006d6b70:      ldr x9, [x8, #0x50]
1006d6b74:      cmp x9, x1
1006d6b78:      b.ne    0x1006d6b9c <_js_array_grow+0xa9c>
1006d6b7c:      ldr x9, [x8, #0x30]
1006d6b80:      cmp x9, x21
1006d6b84:      b.hi    0x1006d6b9c <_js_array_grow+0xa9c>
1006d6b88:      ldr x9, [x8, #0x38]
1006d6b8c:      cmp x9, x21
1006d6b90:      b.ls    0x1006d6b9c <_js_array_grow+0xa9c>
1006d6b94:      add x8, x8, #0x30
1006d6b98:      b   0x1006d6c00 <_js_array_grow+0xb00>
1006d6b9c:      ldrb    w9, [x8, #0x88]
1006d6ba0:      cbz w9, 0x1006d6bd0 <_js_array_grow+0xad0>
1006d6ba4:      ldr x9, [x8, #0x80]
1006d6ba8:      cmp x9, x1
1006d6bac:      b.ne    0x1006d6bd0 <_js_array_grow+0xad0>
1006d6bb0:      ldr x9, [x8, #0x60]
1006d6bb4:      cmp x9, x21
1006d6bb8:      b.hi    0x1006d6bd0 <_js_array_grow+0xad0>
1006d6bbc:      ldr x9, [x8, #0x68]
1006d6bc0:      cmp x9, x21
1006d6bc4:      b.ls    0x1006d6bd0 <_js_array_grow+0xad0>
1006d6bc8:      add x8, x8, #0x60
1006d6bcc:      b   0x1006d6c00 <_js_array_grow+0xb00>
1006d6bd0:      ldrb    w9, [x8, #0xb8]
1006d6bd4:      cbz w9, 0x1006d6c0c <_js_array_grow+0xb0c>
1006d6bd8:      ldr x9, [x8, #0xb0]
1006d6bdc:      cmp x9, x1
1006d6be0:      b.ne    0x1006d6c0c <_js_array_grow+0xb0c>
1006d6be4:      ldr x9, [x8, #0x90]
1006d6be8:      cmp x9, x21
1006d6bec:      b.hi    0x1006d6c0c <_js_array_grow+0xb0c>
1006d6bf0:      ldr x9, [x8, #0x98]
1006d6bf4:      cmp x9, x21
1006d6bf8:      b.ls    0x1006d6c0c <_js_array_grow+0xb0c>
1006d6bfc:      add x8, x8, #0x90
1006d6c00:      ldrb    w8, [x8, #0x19]
1006d6c04:      cmp w8, #0xff
1006d6c08:      b.ne    0x1006d6c18 <_js_array_grow+0xb18>
1006d6c0c:      mov x0, x21
1006d6c10:      bl  0x10045cfb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
1006d6c14:      and w8, w0, #0xff
1006d6c18:      cmp w8, #0x3
1006d6c1c:      b.ne    0x1006d693c <_js_array_grow+0x83c>
1006d6c20:      lsr x28, x21, #12
1006d6c24:      add x8, x22, x21
1006d6c28:      str x8, [sp, #0x10]
1006d6c2c:      sub x8, x8, #0x1
1006d6c30:      lsr x8, x8, #12
1006d6c34:      str x8, [sp, #0x18]
1006d6c38:      cmp x28, x8
1006d6c3c:      b.ls    0x1006d6ce4 <_js_array_grow+0xbe4>
1006d6c40:      mov x0, x21
1006d6c44:      bl  0x100693e04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1006d6c48:      cbz x0, 0x1006d6e60 <_js_array_grow+0xd60>
1006d6c4c:      ldrb    w8, [x0]
1006d6c50:      cmp w8, #0x1
1006d6c54:      b.ne    0x1006d6e60 <_js_array_grow+0xd60>
1006d6c58:      ldrb    w8, [x0, #0x1]
1006d6c5c:      tbz w8, #0x1, 0x1006d6e60 <_js_array_grow+0xd60>
1006d6c60:      str x19, [x0, #0x8]
1006d6c64:      orr w8, w8, #0x80
1006d6c68:      strb    w8, [x0, #0x1]
1006d6c6c:      ldrh    w8, [x0, #0x2]
1006d6c70:      orr w8, w8, #0xc000
1006d6c74:      strh    w8, [x0, #0x2]
1006d6c78:      ldr x8, [x26]
1006d6c7c:      cmn x8, #0x1
1006d6c80:      b.eq    0x1006d6cb0 <_js_array_grow+0xbb0>
1006d6c84:      mrs x9, TPIDRRO_EL0
1006d6c88:      and x9, x9, #0xfffffffffffffff8
1006d6c8c:      ldr x8, [x9, x8, lsl #3]
1006d6c90:      cbz x8, 0x1006d6cb0 <_js_array_grow+0xbb0>
1006d6c94:      ldr x8, [x8, #0x19e8]
1006d6c98:      cbz x8, 0x1006d6cb0 <_js_array_grow+0xbb0>
1006d6c9c:      ldr x9, [x8]
1006d6ca0:      cbz x9, 0x1006d6640 <_js_array_grow+0x540>
1006d6ca4:      adrp    x0, 0x1010cb000 <_anon.b0a7a82b26242b618f19288e3549d1f3.65+0x108>
1006d6ca8:      add x0, x0, #0x150
1006d6cac:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1006d6cb0:      adrp    x0, 0x1010ca000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7promise10then_probe17VERDICT_IN_FLIGHT+0x2228>
1006d6cb4:      add x0, x0, #0xd70
1006d6cb8:      add x1, sp, #0x20
1006d6cbc:      bl  0x100136148 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
1006d6cc0:      mov x0, x19
1006d6cc4:      ldp x29, x30, [sp, #0xa0]
1006d6cc8:      ldp x20, x19, [sp, #0x90]
1006d6ccc:      ldp x22, x21, [sp, #0x80]
1006d6cd0:      ldp x24, x23, [sp, #0x70]
1006d6cd4:      ldp x26, x25, [sp, #0x60]
1006d6cd8:      ldp x28, x27, [sp, #0x50]
1006d6cdc:      add sp, sp, #0xb0
1006d6ce0:      ret
1006d6ce4:      mvn x8, x21
1006d6ce8:      add x8, x8, x19
1006d6cec:      str x8, [sp, #0x8]
1006d6cf0:      adrp    x23, 0x101134000 <_perry_global_baseline_worker_ts__1>
1006d6cf4:      add x23, x23, #0x7f8
1006d6cf8:      mov w27, #0x6               ; =6
1006d6cfc:      adrp    x22, 0x101134000 <_perry_global_baseline_worker_ts__1>
1006d6d00:      b   0x1006d6d14 <_js_array_grow+0xc14>
1006d6d04:      ldr x8, [sp, #0x18]
1006d6d08:      cmp x28, x8
1006d6d0c:      add x28, x28, #0x1
1006d6d10:      b.eq    0x1006d6c40 <_js_array_grow+0xb40>
1006d6d14:      str x28, [sp, #0x38]
1006d6d18:      add x1, sp, #0x38
1006d6d1c:      adrp    x0, 0x1010cb000 <_anon.b0a7a82b26242b618f19288e3549d1f3.65+0x108>
1006d6d20:      add x0, x0, #0x648
1006d6d24:      bl  0x100160c24 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3set7HashSetjNtNtCs5gMwpk3Cs4e_13perry_runtime9fast_hash9PtrHasherEEE4withNCNvNtNtB2g_2gc7barrier24dirty_old_page_is_marked0bEB2g_>
1006d6d28:      cbz w0, 0x1006d6d04 <_js_array_grow+0xc04>
1006d6d2c:      lsl x9, x28, #12
1006d6d30:      cmp x21, x9
1006d6d34:      csel    x8, x21, x9, hi
1006d6d38:      add x9, x9, #0x1, lsl #12   ; =0x1000
1006d6d3c:      ldr x10, [sp, #0x10]
1006d6d40:      cmp x10, x9
1006d6d44:      csel    x9, x10, x9, lo
1006d6d48:      cmp x8, x9
1006d6d4c:      b.hs    0x1006d6d04 <_js_array_grow+0xc04>
1006d6d50:      sub x10, x19, x21
1006d6d54:      add x8, x10, x8
1006d6d58:      lsr x25, x8, #12
1006d6d5c:      ldr x8, [sp, #0x8]
1006d6d60:      add x8, x8, x9
1006d6d64:      lsr x24, x8, #12
1006d6d68:      cmp x25, x24
1006d6d6c:      b.ls    0x1006d6d80 <_js_array_grow+0xc80>
1006d6d70:      b   0x1006d6d04 <_js_array_grow+0xc04>
1006d6d74:      cmp x25, x24
1006d6d78:      add x25, x25, #0x1
1006d6d7c:      b.hs    0x1006d6d04 <_js_array_grow+0xc04>
1006d6d80:      strb    w27, [sp, #0x38]
1006d6d84:      ldapr   x8, [x23]
1006d6d88:      cbnz    x8, 0x1006d6db4 <_js_array_grow+0xcb4>
1006d6d8c:      ldrb    w8, [x22, #0x800]
1006d6d90:      cbz w8, 0x1006d6dc4 <_js_array_grow+0xcc4>
1006d6d94:      add x1, sp, #0x38
1006d6d98:      adrp    x0, 0x1010cb000 <_anon.b0a7a82b26242b618f19288e3549d1f3.65+0x108>
1006d6d9c:      add x0, x0, #0x588
1006d6da0:      bl  0x10012d574 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc9telemetry20BarrierTraceCountersEE4withNCNvNtB1w_7barrier32bump_write_barrier_trace_counter0uEB1y_>
1006d6da4:      ldr x8, [x26]
1006d6da8:      cmn x8, #0x1
1006d6dac:      b.ne    0x1006d6dd0 <_js_array_grow+0xcd0>
1006d6db0:      b   0x1006d6dfc <_js_array_grow+0xcfc>
1006d6db4:      mov x0, x23
1006d6db8:      bl  0x100cc424c <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_trace_enabled0E0zEB1A_>
1006d6dbc:      ldrb    w8, [x22, #0x800]
1006d6dc0:      cbnz    w8, 0x1006d6d94 <_js_array_grow+0xc94>
1006d6dc4:      ldr x8, [x26]
1006d6dc8:      cmn x8, #0x1
1006d6dcc:      b.eq    0x1006d6dfc <_js_array_grow+0xcfc>
1006d6dd0:      mrs x9, TPIDRRO_EL0
1006d6dd4:      and x9, x9, #0xfffffffffffffff8
1006d6dd8:      ldr x0, [x9, x8, lsl #3]
1006d6ddc:      cbz x0, 0x1006d6dfc <_js_array_grow+0xcfc>
1006d6de0:      and x8, x25, #0xf
1006d6de4:      add x8, x0, x8, lsl #3
1006d6de8:      ldr x8, [x8, #0x88]
1006d6dec:      cmp x25, x8
1006d6df0:      b.ne    0x1006d6e14 <_js_array_grow+0xd14>
1006d6df4:      mov w8, #0x9                ; =9
1006d6df8:      b   0x1006d6e24 <_js_array_grow+0xd24>
1006d6dfc:      bl  0x100cd2ac8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006d6e00:      and x8, x25, #0xf
1006d6e04:      add x8, x0, x8, lsl #3
1006d6e08:      ldr x8, [x8, #0x88]
1006d6e0c:      cmp x25, x8
1006d6e10:      b.eq    0x1006d6df4 <_js_array_grow+0xcf4>
1006d6e14:      mov x0, x25
1006d6e18:      bl  0x100681124 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier28mark_dirty_old_page_uncached>
1006d6e1c:      tbz w0, #0x0, 0x1006d6d74 <_js_array_grow+0xc74>
1006d6e20:      mov w8, #0x7                ; =7
1006d6e24:      strb    w8, [sp, #0x38]
1006d6e28:      ldapr   x8, [x23]
1006d6e2c:      cbnz    x8, 0x1006d6e4c <_js_array_grow+0xd4c>
1006d6e30:      ldrb    w8, [x22, #0x800]
1006d6e34:      cbz w8, 0x1006d6d74 <_js_array_grow+0xc74>
1006d6e38:      add x1, sp, #0x38
1006d6e3c:      adrp    x0, 0x1010cb000 <_anon.b0a7a82b26242b618f19288e3549d1f3.65+0x108>
1006d6e40:      add x0, x0, #0x588
1006d6e44:      bl  0x10012d574 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc9telemetry20BarrierTraceCountersEE4withNCNvNtB1w_7barrier32bump_write_barrier_trace_counter0uEB1y_>
1006d6e48:      b   0x1006d6d74 <_js_array_grow+0xc74>
1006d6e4c:      mov x0, x23
1006d6e50:      bl  0x100cc424c <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_trace_enabled0E0zEB1A_>
1006d6e54:      ldrb    w8, [x22, #0x800]
1006d6e58:      cbnz    w8, 0x1006d6e38 <_js_array_grow+0xd38>
1006d6e5c:      b   0x1006d6d74 <_js_array_grow+0xc74>
1006d6e60:      str x21, [sp, #0x30]
1006d6e64:      add x8, sp, #0x30
1006d6e68:      adrp    x9, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
1006d6e6c:      add x9, x9, #0x288
1006d6e70:      stp x8, x9, [sp, #0x38]
1006d6e74:      adrp    x0, 0x100dfd000 <_anon.b0a7a82b26242b618f19288e3549d1f3.1285+0x47>
1006d6e78:      add x0, x0, #0x1e
1006d6e7c:      adrp    x2, 0x1010ce000 <_anon.b0a7a82b26242b618f19288e3549d1f3.1024+0x240>
1006d6e80:      add x2, x2, #0x2f8
1006d6e84:      add x1, sp, #0x38
1006d6e88:      bl  0x100c9e13c <__RNvNtCsjgY6bXVaRmE_4core9panicking9panic_fmt>
1006d6e8c:      mov x19, x1
1006d6e90:      bl  0x100cd2ac8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006d6e94:      mov x1, x19
1006d6e98:      b   0x1006d67dc <_js_array_grow+0x6dc>
1006d6e9c:      adrp    x0, 0x1010ca000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7promise10then_probe17VERDICT_IN_FLIGHT+0x2228>
1006d6ea0:      add x0, x0, #0xe80
1006d6ea4:      bl  0x100c9de9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1006d6ea8:      adrp    x0, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
1006d6eac:      add x0, x0, #0x1b8
1006d6eb0:      bl  0x100cc433c <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
1006d6eb4:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
1006d6eb8:      ldrb    w8, [x8, #0x1c0]
1006d6ebc:      cbnz    w8, 0x1006d6964 <_js_array_grow+0x864>
1006d6ec0:      b   0x1006d6c40 <_js_array_grow+0xb40>
1006d6ec4:      adrp    x0, 0x1010ca000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7promise10then_probe17VERDICT_IN_FLIGHT+0x2228>
1006d6ec8:      add x0, x0, #0xdf0
1006d6ecc:      bl  0x100c9de9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1006d6ed0:      bl  0x100cd3b3c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1006d6ed4:      adrp    x0, 0x100df9000 <_anon.b83c9ebac74d21cc1819cc60175a5dbc.2038+0x23b>
1006d6ed8:      add x0, x0, #0x51f
1006d6edc:      mov w1, #0xb                ; =11
1006d6ee0:      bl  0x100cd3b04 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1006d6ee4:      adrp    x0, 0x101134000 <_perry_global_baseline_worker_ts__1>
1006d6ee8:      add x0, x0, #0x7f8
1006d6eec:      bl  0x100cc424c <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_trace_enabled0E0zEB1A_>
1006d6ef0:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
1006d6ef4:      ldrb    w8, [x8, #0x800]
1006d6ef8:      cbnz    w8, 0x1006d69f8 <_js_array_grow+0x8f8>
1006d6efc:      b   0x1006d6c40 <_js_array_grow+0xb40>
1006d6f00:      adrp    x2, 0x1010cb000 <_anon.b0a7a82b26242b618f19288e3549d1f3.65+0x108>
1006d6f04:      add x2, x2, #0x6a8
1006d6f08:      bl  0x100c9dfcc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1006d6f0c:      adrp    x2, 0x1010cb000 <_anon.b0a7a82b26242b618f19288e3549d1f3.65+0x108>
1006d6f10:      add x2, x2, #0x6c0
1006d6f14:      mov x1, x8
1006d6f18:      bl  0x100c9dfcc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
