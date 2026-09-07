/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/wide39-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010081c100 <_js_object_alloc_class_dynamic_parent>:
10081c100:      sub sp, sp, #0xa0
10081c104:      stp x28, x27, [sp, #0x40]
10081c108:      stp x26, x25, [sp, #0x50]
10081c10c:      stp x24, x23, [sp, #0x60]
10081c110:      stp x22, x21, [sp, #0x70]
10081c114:      stp x20, x19, [sp, #0x80]
10081c118:      stp x29, x30, [sp, #0x90]
10081c11c:      add x29, sp, #0x90
10081c120:      mov x27, x3
10081c124:      mov x22, x2
10081c128:      mov x21, x1
10081c12c:      mov x19, x0
10081c130:      bl  0x10057e260 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object19class_meta_registry19get_parent_class_id>
10081c134:      mov x20, x1
10081c138:      mov w1, #0x0                ; =0
10081c13c:      cmp w0, #0x1
10081c140:      b.ne    0x10081c1e8 <_js_object_alloc_class_dynamic_parent+0xe8>
10081c144:      cbz w20, 0x10081c1e8 <_js_object_alloc_class_dynamic_parent+0xe8>
10081c148:      add x0, sp, #0x10
10081c14c:      mov x1, x20
10081c150:      bl  0x1005925d8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object5alloc27registered_class_keys_array>
10081c154:      ldr w8, [sp, #0x10]
10081c158:      tbz w8, #0x0, 0x10081c1e4 <_js_object_alloc_class_dynamic_parent+0xe4>
10081c15c:      ldr x26, [sp, #0x18]
10081c160:      ldr w24, [x26]
10081c164:      mov w21, #0x2717            ; =10007
10081c168:      mov w23, #0x8480            ; =33920
10081c16c:      movk    w23, #0x1e, lsl #16
10081c170:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081c174:      ldr w25, [x8, #0x634]
10081c178:      adrp    x28, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081c17c:      cmp w25, #0x300
10081c180:      b.hs    0x10081c228 <_js_object_alloc_class_dynamic_parent+0x128>
10081c184:      ldr x8, [x28, #0x48]
10081c188:      cmn x8, #0x1
10081c18c:      b.eq    0x10081c218 <_js_object_alloc_class_dynamic_parent+0x118>
10081c190:      mrs x9, TPIDRRO_EL0
10081c194:      and x9, x9, #0xfffffffffffffff8
10081c198:      ldr x0, [x9, x8, lsl #3]
10081c19c:      cbz x0, 0x10081c218 <_js_object_alloc_class_dynamic_parent+0x118>
10081c1a0:      add x8, x0, x25, lsl #3
10081c1a4:      ldr x0, [x8, #0x1e8]
10081c1a8:      cbz x0, 0x10081c228 <_js_object_alloc_class_dynamic_parent+0x128>
10081c1ac:      madd    w23, w19, w21, w23
10081c1b0:      ldr x0, [x0]
10081c1b4:      cbz x0, 0x10081c240 <_js_object_alloc_class_dynamic_parent+0x140>
10081c1b8:      and w8, w23, #0xff
10081c1bc:      add x8, x0, w8, uxtw #4
10081c1c0:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081c1c4:      ldr w9, [x8]
10081c1c8:      cmp w9, w23
10081c1cc:      b.ne    0x10081c25c <_js_object_alloc_class_dynamic_parent+0x15c>
10081c1d0:      ldr x21, [x8, #0x8]
10081c1d4:      ldr w25, [x8, #0x4]
10081c1d8:      cbz x21, 0x10081c338 <_js_object_alloc_class_dynamic_parent+0x238>
10081c1dc:      ldr w22, [x21]
10081c1e0:      b   0x10081c688 <_js_object_alloc_class_dynamic_parent+0x588>
10081c1e4:      mov x1, x20
10081c1e8:      mov x0, x19
10081c1ec:      mov x2, x21
10081c1f0:      mov x3, x22
10081c1f4:      mov x4, x27
10081c1f8:      ldp x29, x30, [sp, #0x90]
10081c1fc:      ldp x20, x19, [sp, #0x80]
10081c200:      ldp x22, x21, [sp, #0x70]
10081c204:      ldp x24, x23, [sp, #0x60]
10081c208:      ldp x26, x25, [sp, #0x50]
10081c20c:      ldp x28, x27, [sp, #0x40]
10081c210:      add sp, sp, #0xa0
10081c214:      b   0x10081ccc0 <_js_object_alloc_class_with_keys>
10081c218:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081c21c:      add x8, x0, x25, lsl #3
10081c220:      ldr x0, [x8, #0x1e8]
10081c224:      cbnz    x0, 0x10081c1ac <_js_object_alloc_class_dynamic_parent+0xac>
10081c228:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1b0>
10081c22c:      add x0, x0, #0x360
10081c230:      bl  0x100ad935c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081c234:      madd    w23, w19, w21, w23
10081c238:      ldr x0, [x0]
10081c23c:      cbnz    x0, 0x10081c1b8 <_js_object_alloc_class_dynamic_parent+0xb8>
10081c240:      bl  0x100adb02c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081c244:      and w8, w23, #0xff
10081c248:      add x8, x0, w8, uxtw #4
10081c24c:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081c250:      ldr w9, [x8]
10081c254:      cmp w9, w23
10081c258:      b.eq    0x10081c1d0 <_js_object_alloc_class_dynamic_parent+0xd0>
10081c25c:      ldr x8, [x0, #0x5088]
10081c260:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081c264:      cmp x8, x9
10081c268:      b.hs    0x10081cbe4 <_js_object_alloc_class_dynamic_parent+0xae4>
10081c26c:      add x9, x8, #0x1
10081c270:      str x9, [x0, #0x5088]
10081c274:      ldr x9, [x0, #0x50a8]
10081c278:      cbz x9, 0x10081c328 <_js_object_alloc_class_dynamic_parent+0x228>
10081c27c:      mov x9, #0x0                ; =0
10081c280:      mov x10, #0x7c15            ; =31765
10081c284:      movk    x10, #0x7f4a, lsl #16
10081c288:      movk    x10, #0x79b9, lsl #32
10081c28c:      movk    x10, #0x9e37, lsl #48
10081c290:      mul x10, x23, x10
10081c294:      eor x13, x10, x10, lsr #32
10081c298:      lsr x12, x10, #57
10081c29c:      ldr x10, [x0, #0x5098]
10081c2a0:      ldr x11, [x0, #0x5090]
10081c2a4:      dup.8b  v0, w12
10081c2a8:      movi.2d v1, #0xffffffffffffffff
10081c2ac:      mov w12, #0x18              ; =24
10081c2b0:      and x13, x13, x10
10081c2b4:      ldr d2, [x11, x13]
10081c2b8:      cmeq.8b v3, v2, v0
10081c2bc:      fmov    x14, d3
10081c2c0:      ands    x14, x14, #0x8080808080808080
10081c2c4:      b.eq    0x10081c2f8 <_js_object_alloc_class_dynamic_parent+0x1f8>
10081c2c8:      rbit    x15, x14
10081c2cc:      clz x15, x15
10081c2d0:      add x15, x13, x15, lsr #3
10081c2d4:      and x15, x15, x10
10081c2d8:      mneg    x15, x15, x12
10081c2dc:      add x15, x11, x15
10081c2e0:      ldur    w16, [x15, #-0x18]
10081c2e4:      cmp w23, w16
10081c2e8:      b.eq    0x10081c3c8 <_js_object_alloc_class_dynamic_parent+0x2c8>
10081c2ec:      sub x15, x14, #0x2
10081c2f0:      ands    x14, x15, x14
10081c2f4:      b.ne    0x10081c2c8 <_js_object_alloc_class_dynamic_parent+0x1c8>
10081c2f8:      cmeq.8b v2, v2, v1
10081c2fc:      fmov    x14, d2
10081c300:      cbnz    x14, 0x10081c328 <_js_object_alloc_class_dynamic_parent+0x228>
10081c304:      add x9, x9, #0x8
10081c308:      add x13, x13, x9
10081c30c:      and x13, x13, x10
10081c310:      ldr d2, [x11, x13]
10081c314:      cmeq.8b v3, v2, v0
10081c318:      fmov    x14, d3
10081c31c:      ands    x14, x14, #0x8080808080808080
10081c320:      b.ne    0x10081c2c8 <_js_object_alloc_class_dynamic_parent+0x1c8>
10081c324:      b   0x10081c2f8 <_js_object_alloc_class_dynamic_parent+0x1f8>
10081c328:      mov w25, #0x0               ; =0
10081c32c:      mov x21, #0x0               ; =0
10081c330:      str x8, [x0, #0x5088]
10081c334:      cbnz    x21, 0x10081c1dc <_js_object_alloc_class_dynamic_parent+0xdc>
10081c338:      mov x25, #0x0               ; =0
10081c33c:      mov w9, #0x1                ; =1
10081c340:      mov w8, #0x8                ; =8
10081c344:      str x8, [sp, #0x8]
10081c348:      cbz x22, 0x10081c4d4 <_js_object_alloc_class_dynamic_parent+0x3d4>
10081c34c:      cbz w27, 0x10081c4d4 <_js_object_alloc_class_dynamic_parent+0x3d4>
10081c350:      str x26, [sp]
10081c354:      mov w26, #0x0               ; =0
10081c358:      mov w8, #0x0                ; =0
10081c35c:      mov w9, #0x8                ; =8
10081c360:      str x9, [sp, #0x8]
10081c364:      mov w21, w27
10081c368:      b   0x10081c37c <_js_object_alloc_class_dynamic_parent+0x27c>
10081c36c:      mov w26, #0x1               ; =1
10081c370:      mov x22, x25
10081c374:      mov w8, #0x1                ; =1
10081c378:      cbnz    x27, 0x10081c3e8 <_js_object_alloc_class_dynamic_parent+0x2e8>
10081c37c:      tbnz    w8, #0x0, 0x10081c3dc <_js_object_alloc_class_dynamic_parent+0x2dc>
10081c380:      mov x25, x22
10081c384:      mov x27, #0x0               ; =0
10081c388:      cbz x21, 0x10081c36c <_js_object_alloc_class_dynamic_parent+0x26c>
10081c38c:      ldrb    w8, [x25, x27]
10081c390:      cbz w8, 0x10081c3b4 <_js_object_alloc_class_dynamic_parent+0x2b4>
10081c394:      add x27, x27, #0x1
10081c398:      cmp x21, x27
10081c39c:      b.ne    0x10081c38c <_js_object_alloc_class_dynamic_parent+0x28c>
10081c3a0:      mov w26, #0x1               ; =1
10081c3a4:      mov x22, x25
10081c3a8:      mov w8, #0x1                ; =1
10081c3ac:      mov x27, x21
10081c3b0:      b   0x10081c378 <_js_object_alloc_class_dynamic_parent+0x278>
10081c3b4:      mvn x9, x27
10081c3b8:      add x21, x9, x21
10081c3bc:      add x9, x25, x27
10081c3c0:      add x22, x9, #0x1
10081c3c4:      b   0x10081c378 <_js_object_alloc_class_dynamic_parent+0x278>
10081c3c8:      ldur    x21, [x15, #-0x10]
10081c3cc:      ldur    w25, [x15, #-0x8]
10081c3d0:      str x8, [x0, #0x5088]
10081c3d4:      cbnz    x21, 0x10081c1dc <_js_object_alloc_class_dynamic_parent+0xdc>
10081c3d8:      b   0x10081c338 <_js_object_alloc_class_dynamic_parent+0x238>
10081c3dc:      mov x25, #0x0               ; =0
10081c3e0:      mov w9, #0x1                ; =1
10081c3e4:      b   0x10081c4d0 <_js_object_alloc_class_dynamic_parent+0x3d0>
10081c3e8:      mov w0, #0x40               ; =64
10081c3ec:      mov w1, #0x8                ; =8
10081c3f0:      bl  0x100af6548 <_mi_malloc_aligned>
10081c3f4:      cbz x0, 0x10081cc80 <_js_object_alloc_class_dynamic_parent+0xb80>
10081c3f8:      stp x25, x27, [x0]
10081c3fc:      mov w8, #0x4                ; =4
10081c400:      stp x8, x0, [sp, #0x28]
10081c404:      mov w25, #0x1               ; =1
10081c408:      str x25, [sp, #0x38]
10081c40c:      mov x8, x21
10081c410:      mov x10, x22
10081c414:      mov x9, x26
10081c418:      b   0x10081c42c <_js_object_alloc_class_dynamic_parent+0x32c>
10081c41c:      mov w26, #0x1               ; =1
10081c420:      mov x10, x27
10081c424:      mov w9, #0x1                ; =1
10081c428:      cbnz    x28, 0x10081c480 <_js_object_alloc_class_dynamic_parent+0x380>
10081c42c:      tbnz    w9, #0x0, 0x10081c4bc <_js_object_alloc_class_dynamic_parent+0x3bc>
10081c430:      mov x27, x10
10081c434:      mov x28, #0x0               ; =0
10081c438:      cbz x8, 0x10081c41c <_js_object_alloc_class_dynamic_parent+0x31c>
10081c43c:      ldrb    w9, [x27, x28]
10081c440:      cbz w9, 0x10081c464 <_js_object_alloc_class_dynamic_parent+0x364>
10081c444:      add x28, x28, #0x1
10081c448:      cmp x8, x28
10081c44c:      b.ne    0x10081c43c <_js_object_alloc_class_dynamic_parent+0x33c>
10081c450:      mov w26, #0x1               ; =1
10081c454:      mov x10, x27
10081c458:      mov w9, #0x1                ; =1
10081c45c:      mov x28, x8
10081c460:      b   0x10081c428 <_js_object_alloc_class_dynamic_parent+0x328>
10081c464:      mvn x10, x28
10081c468:      add x21, x10, x8
10081c46c:      add x8, x27, x28
10081c470:      add x22, x8, #0x1
10081c474:      mov x8, x21
10081c478:      mov x10, x22
10081c47c:      b   0x10081c428 <_js_object_alloc_class_dynamic_parent+0x328>
10081c480:      ldr x8, [sp, #0x28]
10081c484:      cmp x25, x8
10081c488:      b.eq    0x10081c49c <_js_object_alloc_class_dynamic_parent+0x39c>
10081c48c:      add x8, x0, x25, lsl #4
10081c490:      stp x27, x28, [x8]
10081c494:      add x25, x25, #0x1
10081c498:      b   0x10081c408 <_js_object_alloc_class_dynamic_parent+0x308>
10081c49c:      add x0, sp, #0x28
10081c4a0:      mov x1, x25
10081c4a4:      mov w2, #0x1                ; =1
10081c4a8:      mov w3, #0x8                ; =8
10081c4ac:      mov w4, #0x10               ; =16
10081c4b0:      bl  0x100ad5bd0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs7wa3bwJW6LN_13perry_runtime>
10081c4b4:      ldr x0, [sp, #0x30]
10081c4b8:      b   0x10081c48c <_js_object_alloc_class_dynamic_parent+0x38c>
10081c4bc:      ldp x8, x9, [sp, #0x28]
10081c4c0:      str x9, [sp, #0x8]
10081c4c4:      cmp x8, #0x0
10081c4c8:      cset    w9, eq
10081c4cc:      adrp    x28, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081c4d0:      ldr x26, [sp]
10081c4d4:      str w9, [sp]
10081c4d8:      add w22, w24, w25
10081c4dc:      ubfiz   x8, x22, #3, #32
10081c4e0:      add x0, x8, #0x8
10081c4e4:      mov w1, #0x8                ; =8
10081c4e8:      mov w2, #0x1                ; =1
10081c4ec:      bl  0x1004db2e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators24arena_alloc_gc_longlived>
10081c4f0:      mov x21, x0
10081c4f4:      stp w22, w22, [x0]
10081c4f8:      lsr x8, x0, #3
10081c4fc:      cmp x8, #0x201
10081c500:      b.lo    0x10081c590 <_js_object_alloc_class_dynamic_parent+0x490>
10081c504:      ldurb   w8, [x21, #-0x8]
10081c508:      cmp w8, #0x1
10081c50c:      b.ne    0x10081c538 <_js_object_alloc_class_dynamic_parent+0x438>
10081c510:      ldurh   w8, [x21, #-0x6]
10081c514:      mov w9, #0x1080             ; =4224
10081c518:      mov w10, #0xef7f            ; =61311
10081c51c:      and w10, w8, w10
10081c520:      sturh   w10, [x21, #-0x6]
10081c524:      tst w8, w9
10081c528:      b.eq    0x10081c548 <_js_object_alloc_class_dynamic_parent+0x448>
10081c52c:      mov x0, x21
10081c530:      bl  0x1002b6f40 <__RNvNtCs7wa3bwJW6LN_13perry_runtime14typed_feedback32invalidate_representation_change>
10081c534:      ldurb   w8, [x21, #-0x8]
10081c538:      sub w9, w8, #0x15
10081c53c:      cmn w9, #0x14
10081c540:      b.hs    0x10081c54c <_js_object_alloc_class_dynamic_parent+0x44c>
10081c544:      b   0x10081c590 <_js_object_alloc_class_dynamic_parent+0x490>
10081c548:      mov w8, #0x1                ; =1
10081c54c:      mov w8, w8
10081c550:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081c554:      add x9, x9, #0xb40
10081c558:      add x8, x9, x8, lsl #5
10081c55c:      ldrb    w8, [x8, #0x14]
10081c560:      mov w9, #0x16               ; =22
10081c564:      lsr w8, w9, w8
10081c568:      tbz w8, #0x0, 0x10081c590 <_js_object_alloc_class_dynamic_parent+0x490>
10081c56c:      ldurh   w8, [x21, #-0x6]
10081c570:      mov w9, #0x4000             ; =16384
10081c574:      bfxil   w9, w8, #0, #13
10081c578:      sturh   w9, [x21, #-0x6]
10081c57c:      mov x0, x21
10081c580:      bl  0x100413d38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081c584:      ldurh   w8, [x21, #-0x6]
10081c588:      and w8, w8, #0xffffefff
10081c58c:      sturh   w8, [x21, #-0x6]
10081c590:      cbz w24, 0x10081c5c8 <_js_object_alloc_class_dynamic_parent+0x4c8>
10081c594:      mov x1, #0x0                ; =0
10081c598:      add x26, x26, #0x8
10081c59c:      add x27, x1, #0x1
10081c5a0:      lsl x8, x1, #3
10081c5a4:      ldr d0, [x26, x8]
10081c5a8:      add x8, x21, x8
10081c5ac:      str d0, [x8, #0x8]
10081c5b0:      fmov    x2, d0
10081c5b4:      mov x0, x21
10081c5b8:      bl  0x1004f10dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array15header_gc_slots27note_array_slot_layout_only>
10081c5bc:      mov x1, x27
10081c5c0:      cmp x24, x27
10081c5c4:      b.ne    0x10081c59c <_js_object_alloc_class_dynamic_parent+0x49c>
10081c5c8:      ldr x27, [sp, #0x8]
10081c5cc:      cbz x25, 0x10081c610 <_js_object_alloc_class_dynamic_parent+0x510>
10081c5d0:      add x25, x27, x25, lsl #4
10081c5d4:      mov x26, x27
10081c5d8:      ldr x0, [x26]
10081c5dc:      ldr w1, [x26, #0x8]
10081c5e0:      bl  0x100885194 <_js_string_from_bytes_longlived>
10081c5e4:      mov x2, #0x7fff000000000000 ; =9223090561878065152
10081c5e8:      bfxil   x2, x0, #0, #48
10081c5ec:      add x8, x21, x24, lsl #3
10081c5f0:      str x2, [x8, #0x8]
10081c5f4:      mov x0, x21
10081c5f8:      mov x1, x24
10081c5fc:      bl  0x1004f10dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array15header_gc_slots27note_array_slot_layout_only>
10081c600:      add x24, x24, #0x1
10081c604:      add x26, x26, #0x10
10081c608:      cmp x26, x25
10081c60c:      b.ne    0x10081c5d8 <_js_object_alloc_class_dynamic_parent+0x4d8>
10081c610:      mov x0, x23
10081c614:      mov x1, x21
10081c618:      bl  0x10032223c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object18shape_cache_insert>
10081c61c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
10081c620:      ldr w24, [x8, #0x634]
10081c624:      cmp w24, #0x300
10081c628:      and w10, w23, #0xff
10081c62c:      b.hs    0x10081ca7c <_js_object_alloc_class_dynamic_parent+0x97c>
10081c630:      ldr x8, [x28, #0x48]
10081c634:      cmn x8, #0x1
10081c638:      b.eq    0x10081ca68 <_js_object_alloc_class_dynamic_parent+0x968>
10081c63c:      mrs x9, TPIDRRO_EL0
10081c640:      and x9, x9, #0xfffffffffffffff8
10081c644:      ldr x0, [x9, x8, lsl #3]
10081c648:      cbz x0, 0x10081ca68 <_js_object_alloc_class_dynamic_parent+0x968>
10081c64c:      add x8, x0, x24, lsl #3
10081c650:      ldr x0, [x8, #0x1e8]
10081c654:      cbz x0, 0x10081ca7c <_js_object_alloc_class_dynamic_parent+0x97c>
10081c658:      ldr x0, [x0]
10081c65c:      cbz x0, 0x10081ca94 <_js_object_alloc_class_dynamic_parent+0x994>
10081c660:      add x8, x0, x10, lsl #4
10081c664:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081c668:      ldr w9, [x8]
10081c66c:      cmp w9, w23
10081c670:      b.ne    0x10081cab0 <_js_object_alloc_class_dynamic_parent+0x9b0>
10081c674:      ldr w25, [x8, #0x4]
10081c678:      ldr w8, [sp]
10081c67c:      tbnz    w8, #0x0, 0x10081c688 <_js_object_alloc_class_dynamic_parent+0x588>
10081c680:      mov x0, x27
10081c684:      bl  0x100af62c0 <_mi_free>
10081c688:      mov w8, #0x2                ; =2
10081c68c:      cmp w22, #0x2
10081c690:      csel    w27, w22, w8, hi
10081c694:      ubfiz   x8, x27, #3, #32
10081c698:      mov w9, #0x3ffd             ; =16381
10081c69c:      cmp w22, w9
10081c6a0:      b.ls    0x10081c6c8 <_js_object_alloc_class_dynamic_parent+0x5c8>
10081c6a4:      add x0, x8, #0x10
10081c6a8:      mov w1, #0x8                ; =8
10081c6ac:      mov w2, #0x2                ; =2
10081c6b0:      bl  0x1004da858 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators18arena_alloc_gc_old>
10081c6b4:      mov x24, x0
10081c6b8:      ldurb   w8, [x0, #-0x7]
10081c6bc:      orr w8, w8, #0x20
10081c6c0:      sturb   w8, [x0, #-0x7]
10081c6c4:      b   0x10081c834 <_js_object_alloc_class_dynamic_parent+0x734>
10081c6c8:      add x23, x8, #0x18
10081c6cc:      ldr x8, [x28, #0x48]
10081c6d0:      cmn x8, #0x1
10081c6d4:      b.eq    0x10081ca20 <_js_object_alloc_class_dynamic_parent+0x920>
10081c6d8:      mrs x9, TPIDRRO_EL0
10081c6dc:      and x9, x9, #0xfffffffffffffff8
10081c6e0:      ldr x0, [x9, x8, lsl #3]
10081c6e4:      cbz x0, 0x10081ca20 <_js_object_alloc_class_dynamic_parent+0x920>
10081c6e8:      ldr x8, [x0, #0x28]
10081c6ec:      ldrb    w8, [x8]
10081c6f0:      tbz w8, #0x0, 0x10081c75c <_js_object_alloc_class_dynamic_parent+0x65c>
10081c6f4:      ldr x8, [x28, #0x48]
10081c6f8:      cmn x8, #0x1
10081c6fc:      b.eq    0x10081cba4 <_js_object_alloc_class_dynamic_parent+0xaa4>
10081c700:      mrs x9, TPIDRRO_EL0
10081c704:      and x9, x9, #0xfffffffffffffff8
10081c708:      ldr x0, [x9, x8, lsl #3]
10081c70c:      cbz x0, 0x10081cba4 <_js_object_alloc_class_dynamic_parent+0xaa4>
10081c710:      ldr x26, [x0, #0x20]
10081c714:      ldr x8, [x26]
10081c718:      cbnz    x8, 0x10081cbb4 <_js_object_alloc_class_dynamic_parent+0xab4>
10081c71c:      mov x8, #-0x1               ; =-1
10081c720:      str x8, [x26]
10081c724:      ldr x1, [x26, #0x18]
10081c728:      cbz x1, 0x10081c758 <_js_object_alloc_class_dynamic_parent+0x658>
10081c72c:      mov x0, #0x0                ; =0
10081c730:      ldr x8, [x26, #0x10]
10081c734:      lsl x10, x1, #4
10081c738:      mov x9, x8
10081c73c:      ldr x11, [x9, #0x8]
10081c740:      cmp x11, x23
10081c744:      b.eq    0x10081c900 <_js_object_alloc_class_dynamic_parent+0x800>
10081c748:      add x0, x0, #0x1
10081c74c:      add x9, x9, #0x10
10081c750:      subs    x10, x10, #0x10
10081c754:      b.ne    0x10081c73c <_js_object_alloc_class_dynamic_parent+0x63c>
10081c758:      str xzr, [x26]
10081c75c:      mov x0, x23
10081c760:      bl  0x1004da49c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena10allocators11arena_alloc>
10081c764:      mov w8, #0x2                ; =2
10081c768:      strb    w8, [x0]
10081c76c:      ldr x8, [x28, #0x48]
10081c770:      cmn x8, #0x1
10081c774:      b.eq    0x10081ca34 <_js_object_alloc_class_dynamic_parent+0x934>
10081c778:      mrs x9, TPIDRRO_EL0
10081c77c:      and x9, x9, #0xfffffffffffffff8
10081c780:      ldr x8, [x9, x8, lsl #3]
10081c784:      cbz x8, 0x10081ca34 <_js_object_alloc_class_dynamic_parent+0x934>
10081c788:      ldr x8, [x8, #0x30]
10081c78c:      ldrb    w8, [x8]
10081c790:      orr w8, w8, #0x2
10081c794:      strb    w8, [x0, #0x1]
10081c798:      ldr x8, [x28, #0x48]
10081c79c:      cmn x8, #0x1
10081c7a0:      b.eq    0x10081ca48 <_js_object_alloc_class_dynamic_parent+0x948>
10081c7a4:      mrs x9, TPIDRRO_EL0
10081c7a8:      and x9, x9, #0xfffffffffffffff8
10081c7ac:      ldr x8, [x9, x8, lsl #3]
10081c7b0:      cbz x8, 0x10081ca48 <_js_object_alloc_class_dynamic_parent+0x948>
10081c7b4:      ldr x8, [x8, #0x30]
10081c7b8:      ldrb    w8, [x8]
10081c7bc:      tbz w8, #0x0, 0x10081c828 <_js_object_alloc_class_dynamic_parent+0x728>
10081c7c0:      ldrb    w8, [x0]
10081c7c4:      sub w9, w8, #0x15
10081c7c8:      cmn w9, #0x14
10081c7cc:      b.lo    0x10081c828 <_js_object_alloc_class_dynamic_parent+0x728>
10081c7d0:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081c7d4:      add x9, x9, #0xb40
10081c7d8:      add x8, x9, x8, lsl #5
10081c7dc:      ldrb    w8, [x8, #0x1b]
10081c7e0:      tbnz    w8, #0x0, 0x10081c828 <_js_object_alloc_class_dynamic_parent+0x728>
10081c7e4:      mov x26, x0
10081c7e8:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081c7ec:      add x0, x0, #0x658
10081c7f0:      ldr x8, [x0]
10081c7f4:      blr x8
10081c7f8:      mov x24, x0
10081c7fc:      ldrb    w8, [x0, #0x18]
10081c800:      cbnz    w8, 0x10081cbf4 <_js_object_alloc_class_dynamic_parent+0xaf4>
10081c804:      ldr x28, [x24, #0x10]
10081c808:      ldr x8, [x24]
10081c80c:      cmp x28, x8
10081c810:      mov x0, x26
10081c814:      b.eq    0x10081cc24 <_js_object_alloc_class_dynamic_parent+0xb24>
10081c818:      ldr x8, [x24, #0x8]
10081c81c:      str x0, [x8, x28, lsl #3]
10081c820:      add x8, x28, #0x1
10081c824:      str x8, [x24, #0x10]
10081c828:      strh    wzr, [x0, #0x2]
10081c82c:      str w23, [x0, #0x4]
10081c830:      add x24, x0, #0x8
10081c834:      stp w19, w20, [x24]
10081c838:      lsl x2, x27, #3
10081c83c:      str xzr, [x24, #0x8]
10081c840:      adrp    x1, 0x100c2b000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x58a8>
10081c844:      add x1, x1, #0xcf0
10081c848:      add x0, x24, #0x10
10081c84c:      bl  0x100af9190 <_writev+0x100af9190>
10081c850:      mov x0, x24
10081c854:      mov x1, x21
10081c858:      mov x2, x22
10081c85c:      bl  0x100324800 <__RNvNtCs7wa3bwJW6LN_13perry_runtime6object31set_object_keys_array_with_live>
10081c860:      lsr x8, x24, #3
10081c864:      cmp x8, #0x201
10081c868:      b.lo    0x10081c8bc <_js_object_alloc_class_dynamic_parent+0x7bc>
10081c86c:      ldurb   w8, [x24, #-0x8]
10081c870:      sub w9, w8, #0x15
10081c874:      cmn w9, #0x14
10081c878:      b.lo    0x10081c8bc <_js_object_alloc_class_dynamic_parent+0x7bc>
10081c87c:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081c880:      add x9, x9, #0xb40
10081c884:      add x8, x9, x8, lsl #5
10081c888:      ldrb    w8, [x8, #0x14]
10081c88c:      mov w9, #0x16               ; =22
10081c890:      lsr w8, w9, w8
10081c894:      tbz w8, #0x0, 0x10081c8bc <_js_object_alloc_class_dynamic_parent+0x7bc>
10081c898:      ldurh   w8, [x24, #-0x6]
10081c89c:      mov w9, #0x4000             ; =16384
10081c8a0:      bfxil   w9, w8, #0, #13
10081c8a4:      sturh   w9, [x24, #-0x6]
10081c8a8:      mov x0, x24
10081c8ac:      bl  0x100413d38 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc13layout_tables20layout_forget_object>
10081c8b0:      ldurh   w8, [x24, #-0x6]
10081c8b4:      and w8, w8, #0xffffefff
10081c8b8:      sturh   w8, [x24, #-0x6]
10081c8bc:      mov x0, x24
10081c8c0:      mov x1, x25
10081c8c4:      mov x2, x22
10081c8c8:      bl  0x1005981c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes24birth_stamp_object_shape>
10081c8cc:      mov x0, x19
10081c8d0:      mov x1, x22
10081c8d4:      mov x2, x21
10081c8d8:      bl  0x100591e40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object5alloc25remember_class_keys_array>
10081c8dc:      mov x0, x24
10081c8e0:      ldp x29, x30, [sp, #0x90]
10081c8e4:      ldp x20, x19, [sp, #0x80]
10081c8e8:      ldp x22, x21, [sp, #0x70]
10081c8ec:      ldp x24, x23, [sp, #0x60]
10081c8f0:      ldp x26, x25, [sp, #0x50]
10081c8f4:      ldp x28, x27, [sp, #0x40]
10081c8f8:      add sp, sp, #0xa0
10081c8fc:      ret
10081c900:      cmp x0, x1
10081c904:      b.hs    0x10081cbf0 <_js_object_alloc_class_dynamic_parent+0xaf0>
10081c908:      ldr x24, [x9]
10081c90c:      subs    x10, x1, #0x1
10081c910:      ldr q0, [x8, x10, lsl #4]
10081c914:      str q0, [x9]
10081c918:      str x10, [x26, #0x18]
10081c91c:      b.ne    0x10081c944 <_js_object_alloc_class_dynamic_parent+0x844>
10081c920:      ldr x8, [x28, #0x48]
10081c924:      cmn x8, #0x1
10081c928:      b.eq    0x10081cbdc <_js_object_alloc_class_dynamic_parent+0xadc>
10081c92c:      mrs x9, TPIDRRO_EL0
10081c930:      and x9, x9, #0xfffffffffffffff8
10081c934:      ldr x0, [x9, x8, lsl #3]
10081c938:      cbz x0, 0x10081cbdc <_js_object_alloc_class_dynamic_parent+0xadc>
10081c93c:      ldr x8, [x0, #0x28]
10081c940:      strb    wzr, [x8]
10081c944:      ldr x8, [x26]
10081c948:      add x8, x8, #0x1
10081c94c:      str x8, [x26]
10081c950:      mov w8, #0x2                ; =2
10081c954:      mov x9, x28
10081c958:      mov x28, x24
10081c95c:      strb    w8, [x28, #-0x8]!
10081c960:      mov x26, x9
10081c964:      ldr x8, [x9, #0x48]
10081c968:      cmn x8, #0x1
10081c96c:      b.eq    0x10081cbc0 <_js_object_alloc_class_dynamic_parent+0xac0>
10081c970:      mrs x9, TPIDRRO_EL0
10081c974:      and x9, x9, #0xfffffffffffffff8
10081c978:      ldr x0, [x9, x8, lsl #3]
10081c97c:      cbz x0, 0x10081cbc0 <_js_object_alloc_class_dynamic_parent+0xac0>
10081c980:      ldr x8, [x0, #0x30]
10081c984:      ldrb    w8, [x8]
10081c988:      orr w8, w8, #0x2
10081c98c:      sturb   w8, [x24, #-0x7]
10081c990:      ldr x8, [x26, #0x48]
10081c994:      cmn x8, #0x1
10081c998:      b.eq    0x10081cbc8 <_js_object_alloc_class_dynamic_parent+0xac8>
10081c99c:      mrs x9, TPIDRRO_EL0
10081c9a0:      and x9, x9, #0xfffffffffffffff8
10081c9a4:      ldr x0, [x9, x8, lsl #3]
10081c9a8:      cbz x0, 0x10081cbc8 <_js_object_alloc_class_dynamic_parent+0xac8>
10081c9ac:      ldr x8, [x0, #0x30]
10081c9b0:      ldrb    w8, [x8]
10081c9b4:      tbz w8, #0x0, 0x10081ca14 <_js_object_alloc_class_dynamic_parent+0x914>
10081c9b8:      ldrb    w8, [x28]
10081c9bc:      sub w9, w8, #0x15
10081c9c0:      cmn w9, #0x14
10081c9c4:      b.lo    0x10081ca14 <_js_object_alloc_class_dynamic_parent+0x914>
10081c9c8:      adrp    x9, 0x100e8a000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime9messaging21GC_SCANNER_REGISTERED+0xa8>
10081c9cc:      add x9, x9, #0xb40
10081c9d0:      add x8, x9, x8, lsl #5
10081c9d4:      ldrb    w8, [x8, #0x1b]
10081c9d8:      tbnz    w8, #0x0, 0x10081ca14 <_js_object_alloc_class_dynamic_parent+0x914>
10081c9dc:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
10081c9e0:      add x0, x0, #0x658
10081c9e4:      ldr x8, [x0]
10081c9e8:      blr x8
10081c9ec:      ldrb    w8, [x0, #0x18]
10081c9f0:      cbnz    w8, 0x10081cc34 <_js_object_alloc_class_dynamic_parent+0xb34>
10081c9f4:      ldr x26, [x0, #0x10]
10081c9f8:      ldr x8, [x0]
10081c9fc:      cmp x26, x8
10081ca00:      b.eq    0x10081cc70 <_js_object_alloc_class_dynamic_parent+0xb70>
10081ca04:      ldr x8, [x0, #0x8]
10081ca08:      str x28, [x8, x26, lsl #3]
10081ca0c:      add x8, x26, #0x1
10081ca10:      str x8, [x0, #0x10]
10081ca14:      sturh   wzr, [x24, #-0x6]
10081ca18:      stur    w23, [x24, #-0x4]
10081ca1c:      b   0x10081c834 <_js_object_alloc_class_dynamic_parent+0x734>
10081ca20:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081ca24:      ldr x8, [x0, #0x28]
10081ca28:      ldrb    w8, [x8]
10081ca2c:      tbnz    w8, #0x0, 0x10081c6f4 <_js_object_alloc_class_dynamic_parent+0x5f4>
10081ca30:      b   0x10081c75c <_js_object_alloc_class_dynamic_parent+0x65c>
10081ca34:      mov x24, x0
10081ca38:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081ca3c:      mov x8, x0
10081ca40:      mov x0, x24
10081ca44:      b   0x10081c788 <_js_object_alloc_class_dynamic_parent+0x688>
10081ca48:      mov x24, x0
10081ca4c:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081ca50:      mov x8, x0
10081ca54:      mov x0, x24
10081ca58:      ldr x8, [x8, #0x30]
10081ca5c:      ldrb    w8, [x8]
10081ca60:      tbnz    w8, #0x0, 0x10081c7c0 <_js_object_alloc_class_dynamic_parent+0x6c0>
10081ca64:      b   0x10081c828 <_js_object_alloc_class_dynamic_parent+0x728>
10081ca68:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081ca6c:      and w10, w23, #0xff
10081ca70:      add x8, x0, x24, lsl #3
10081ca74:      ldr x0, [x8, #0x1e8]
10081ca78:      cbnz    x0, 0x10081c658 <_js_object_alloc_class_dynamic_parent+0x558>
10081ca7c:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1b0>
10081ca80:      add x0, x0, #0x360
10081ca84:      bl  0x100ad935c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10081ca88:      and w10, w23, #0xff
10081ca8c:      ldr x0, [x0]
10081ca90:      cbnz    x0, 0x10081c660 <_js_object_alloc_class_dynamic_parent+0x560>
10081ca94:      bl  0x100adb02c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
10081ca98:      and w10, w23, #0xff
10081ca9c:      add x8, x0, x10, lsl #4
10081caa0:      add x8, x8, #0x4, lsl #12   ; =0x4000
10081caa4:      ldr w9, [x8]
10081caa8:      cmp w9, w23
10081caac:      b.eq    0x10081c674 <_js_object_alloc_class_dynamic_parent+0x574>
10081cab0:      ldr x8, [x0, #0x5088]
10081cab4:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10081cab8:      cmp x8, x9
10081cabc:      b.hs    0x10081cbe4 <_js_object_alloc_class_dynamic_parent+0xae4>
10081cac0:      add x9, x8, #0x1
10081cac4:      str x9, [x0, #0x5088]
10081cac8:      ldr x9, [x0, #0x50a8]
10081cacc:      cbz x9, 0x10081cb7c <_js_object_alloc_class_dynamic_parent+0xa7c>
10081cad0:      mov x9, #0x0                ; =0
10081cad4:      mov x10, #0x7c15            ; =31765
10081cad8:      movk    x10, #0x7f4a, lsl #16
10081cadc:      movk    x10, #0x79b9, lsl #32
10081cae0:      movk    x10, #0x9e37, lsl #48
10081cae4:      mul x10, x23, x10
10081cae8:      eor x13, x10, x10, lsr #32
10081caec:      lsr x12, x10, #57
10081caf0:      ldr x10, [x0, #0x5098]
10081caf4:      ldr x11, [x0, #0x5090]
10081caf8:      dup.8b  v0, w12
10081cafc:      movi.2d v1, #0xffffffffffffffff
10081cb00:      mov w12, #0x18              ; =24
10081cb04:      and x13, x13, x10
10081cb08:      ldr d2, [x11, x13]
10081cb0c:      cmeq.8b v3, v2, v0
10081cb10:      fmov    x14, d3
10081cb14:      ands    x14, x14, #0x8080808080808080
10081cb18:      b.eq    0x10081cb4c <_js_object_alloc_class_dynamic_parent+0xa4c>
10081cb1c:      rbit    x15, x14
10081cb20:      clz x15, x15
10081cb24:      add x15, x13, x15, lsr #3
10081cb28:      and x15, x15, x10
10081cb2c:      mneg    x15, x15, x12
10081cb30:      add x15, x11, x15
10081cb34:      ldur    w16, [x15, #-0x18]
10081cb38:      cmp w23, w16
10081cb3c:      b.eq    0x10081cb90 <_js_object_alloc_class_dynamic_parent+0xa90>
10081cb40:      sub x15, x14, #0x2
10081cb44:      ands    x14, x15, x14
10081cb48:      b.ne    0x10081cb1c <_js_object_alloc_class_dynamic_parent+0xa1c>
10081cb4c:      cmeq.8b v2, v2, v1
10081cb50:      fmov    x14, d2
10081cb54:      cbnz    x14, 0x10081cb7c <_js_object_alloc_class_dynamic_parent+0xa7c>
10081cb58:      add x9, x9, #0x8
10081cb5c:      add x13, x13, x9
10081cb60:      and x13, x13, x10
10081cb64:      ldr d2, [x11, x13]
10081cb68:      cmeq.8b v3, v2, v0
10081cb6c:      fmov    x14, d3
10081cb70:      ands    x14, x14, #0x8080808080808080
10081cb74:      b.ne    0x10081cb1c <_js_object_alloc_class_dynamic_parent+0xa1c>
10081cb78:      b   0x10081cb4c <_js_object_alloc_class_dynamic_parent+0xa4c>
10081cb7c:      mov w25, #0x0               ; =0
10081cb80:      str x8, [x0, #0x5088]
10081cb84:      ldr w8, [sp]
10081cb88:      tbz w8, #0x0, 0x10081c680 <_js_object_alloc_class_dynamic_parent+0x580>
10081cb8c:      b   0x10081c688 <_js_object_alloc_class_dynamic_parent+0x588>
10081cb90:      ldur    w25, [x15, #-0x8]
10081cb94:      str x8, [x0, #0x5088]
10081cb98:      ldr w8, [sp]
10081cb9c:      tbz w8, #0x0, 0x10081c680 <_js_object_alloc_class_dynamic_parent+0x580>
10081cba0:      b   0x10081c688 <_js_object_alloc_class_dynamic_parent+0x588>
10081cba4:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081cba8:      ldr x26, [x0, #0x20]
10081cbac:      ldr x8, [x26]
10081cbb0:      cbz x8, 0x10081c71c <_js_object_alloc_class_dynamic_parent+0x61c>
10081cbb4:      adrp    x0, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ec0>
10081cbb8:      add x0, x0, #0x150
10081cbbc:      bl  0x100ab14ec <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10081cbc0:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081cbc4:      b   0x10081c980 <_js_object_alloc_class_dynamic_parent+0x880>
10081cbc8:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081cbcc:      ldr x8, [x0, #0x30]
10081cbd0:      ldrb    w8, [x8]
10081cbd4:      tbnz    w8, #0x0, 0x10081c9b8 <_js_object_alloc_class_dynamic_parent+0x8b8>
10081cbd8:      b   0x10081ca14 <_js_object_alloc_class_dynamic_parent+0x914>
10081cbdc:      bl  0x100adbef0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
10081cbe0:      b   0x10081c93c <_js_object_alloc_class_dynamic_parent+0x83c>
10081cbe4:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1b0>
10081cbe8:      add x0, x0, #0x7c8
10081cbec:      bl  0x100ab151c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10081cbf0:      bl  0x100ab11bc <__RNvNvMs_NtCsctvjasLqLe9_5alloc3vecINtB6_3VecppE11swap_remove13assert_failed>
10081cbf4:      cmp w8, #0x1
10081cbf8:      b.ne    0x10081cc3c <_js_object_alloc_class_dynamic_parent+0xb3c>
10081cbfc:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081cc00:      add x1, x1, #0xbb8
10081cc04:      mov x0, x24
10081cc08:      bl  0x1009bf25c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081cc0c:      strb    wzr, [x24, #0x18]
10081cc10:      ldr x28, [x24, #0x10]
10081cc14:      ldr x8, [x24]
10081cc18:      cmp x28, x8
10081cc1c:      mov x0, x26
10081cc20:      b.ne    0x10081c818 <_js_object_alloc_class_dynamic_parent+0x718>
10081cc24:      mov x0, x24
10081cc28:      bl  0x100ad8464 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081cc2c:      mov x0, x26
10081cc30:      b   0x10081c818 <_js_object_alloc_class_dynamic_parent+0x718>
10081cc34:      cmp w8, #0x2
10081cc38:      b.ne    0x10081cc48 <_js_object_alloc_class_dynamic_parent+0xb48>
10081cc3c:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
10081cc40:      add x0, x0, #0x850
10081cc44:      bl  0x100aef91c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10081cc48:      adrp    x1, 0x10018a000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs7wa3bwJW6LN_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionIB2v_NtNtNtNtB1a_2fs14dir_glob_watch13watch_backend14SharedInstanceEEEKj1_EEB1a_+0x8>
10081cc4c:      add x1, x1, #0xbb8
10081cc50:      mov x26, x0
10081cc54:      bl  0x1009bf25c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10081cc58:      mov x0, x26
10081cc5c:      strb    wzr, [x26, #0x18]
10081cc60:      ldr x26, [x26, #0x10]
10081cc64:      ldr x8, [x0]
10081cc68:      cmp x26, x8
10081cc6c:      b.ne    0x10081ca04 <_js_object_alloc_class_dynamic_parent+0x904>
10081cc70:      str x0, [sp, #0x8]
10081cc74:      bl  0x100ad8464 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs7wa3bwJW6LN_13perry_runtime>
10081cc78:      ldr x0, [sp, #0x8]
10081cc7c:      b   0x10081ca04 <_js_object_alloc_class_dynamic_parent+0x904>
10081cc80:      mov w0, #0x8                ; =8
10081cc84:      mov w1, #0x40               ; =64
10081cc88:      bl  0x100ab1164 <__RNvNtCsctvjasLqLe9_5alloc7raw_vec12handle_error>
        ...
