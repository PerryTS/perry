/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/short-array-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008ca158 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow>:
1008ca158:      sub sp, sp, #0x1a0
1008ca15c:      stp x28, x27, [sp, #0x140]
1008ca160:      stp x26, x25, [sp, #0x150]
1008ca164:      stp x24, x23, [sp, #0x160]
1008ca168:      stp x22, x21, [sp, #0x170]
1008ca16c:      stp x20, x19, [sp, #0x180]
1008ca170:      stp x29, x30, [sp, #0x190]
1008ca174:      add x29, sp, #0x190
1008ca178:      mov x21, x2
1008ca17c:      mov x22, x1
1008ca180:      mov x19, x0
1008ca184:      add x25, sp, #0x90
1008ca188:      add x20, x1, #0x14
1008ca18c:      cmp x2, #0x2
1008ca190:      b.ne    0x1008ca1a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x50>
1008ca194:      ldrh    w8, [x20]
1008ca198:      mov w9, #0x7d7b             ; =32123
1008ca19c:      cmp w8, w9
1008ca1a0:      b.eq    0x1008ca1dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x84>
1008ca1a4:      b   0x1008ca21c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xc4>
1008ca1a8:      cmp x21, #0x3
1008ca1ac:      b.lo    0x1008ca21c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xc4>
1008ca1b0:      ldrb    w8, [x20]
1008ca1b4:      cmp w8, #0x20
1008ca1b8:      b.hi    0x1008ca1e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x90>
1008ca1bc:      mov x9, #0x2600             ; =9728
1008ca1c0:      movk    x9, #0x1, lsl #32
1008ca1c4:      lsr x9, x9, x8
1008ca1c8:      tbz w9, #0x0, 0x1008ca1e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x90>
1008ca1cc:      add x0, x22, #0x14
1008ca1d0:      mov x1, x21
1008ca1d4:      bl  0x1008c102c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty19padded_empty_object>
1008ca1d8:      tbz w0, #0x0, 0x1008ca214 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xbc>
1008ca1dc:      bl  0x1008c10f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty21allocate_empty_object>
1008ca1e0:      stp xzr, x0, [x19]
1008ca1e4:      b   0x1008ca5fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a4>
1008ca1e8:      cmp w8, #0x7b
1008ca1ec:      b.ne    0x1008ca214 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xbc>
1008ca1f0:      ldrb    w8, [x22, #0x15]
1008ca1f4:      cmp w8, #0x20
1008ca1f8:      b.hi    0x1008ca20c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xb4>
1008ca1fc:      mov x9, #0x2600             ; =9728
1008ca200:      movk    x9, #0x1, lsl #32
1008ca204:      lsr x9, x9, x8
1008ca208:      tbnz    w9, #0x0, 0x1008ca1cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x74>
1008ca20c:      cmp w8, #0x7d
1008ca210:      b.eq    0x1008ca1cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x74>
1008ca214:      cmp x21, #0x41
1008ca218:      b.hs    0x1008ca280 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x128>
1008ca21c:      add x0, sp, #0x90
1008ca220:      add x1, x22, #0x14
1008ca224:      mov x2, x21
1008ca228:      bl  0x1008c1658 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode>
1008ca22c:      ldr x8, [sp, #0x90]
1008ca230:      cbz x8, 0x1008ca32c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1008ca234:      ldr x8, [sp, #0x118]
1008ca238:      str x8, [sp, #0x80]
1008ca23c:      ldur    q0, [x25, #0x48]
1008ca240:      ldur    q1, [x25, #0x58]
1008ca244:      stp q0, q1, [sp, #0x40]
1008ca248:      ldur    q0, [x25, #0x68]
1008ca24c:      ldur    q1, [x25, #0x78]
1008ca250:      stp q0, q1, [sp, #0x60]
1008ca254:      ldur    q0, [x25, #0x8]
1008ca258:      ldur    q1, [x25, #0x18]
1008ca25c:      stp q0, q1, [sp]
1008ca260:      ldur    q0, [x25, #0x28]
1008ca264:      ldur    q1, [x25, #0x38]
1008ca268:      stp q0, q1, [sp, #0x20]
1008ca26c:      mov x0, sp
1008ca270:      bl  0x1008c20b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate>
1008ca274:      tbz w0, #0x0, 0x1008ca32c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1008ca278:      stp xzr, x1, [x19]
1008ca27c:      b   0x1008ca5fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a4>
1008ca280:      cmp x21, #0x3e9
1008ca284:      b.lo    0x1008ca32c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1008ca288:      add x0, x22, #0x14
1008ca28c:      mov x1, x21
1008ca290:      mov w2, #0x3e8              ; =1000
1008ca294:      bl  0x1008c2c5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1008ca298:      tbz w0, #0x0, 0x1008ca32c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1008ca29c:      add x0, x22, #0x14
1008ca2a0:      mov x1, x21
1008ca2a4:      mov w2, #0xa120             ; =41248
1008ca2a8:      movk    w2, #0x7, lsl #16
1008ca2ac:      bl  0x1008c2c5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1008ca2b0:      tbz w0, #0x0, 0x1008ca6a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x54c>
1008ca2b4:      adrp    x8, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
1008ca2b8:      add x8, x8, #0xf80
1008ca2bc:      adrp    x9, 0x100e1c000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime10bun_compat12width_tables13EAW_AMBIGUOUS+0x580>
1008ca2c0:      add x9, x9, #0x168
1008ca2c4:      stp x9, x8, [sp]
1008ca2c8:      adrp    x0, 0x100efc000 <_anon.b8734461ce4fd0c908478712a5ac704e.489+0x1a>
1008ca2cc:      add x0, x0, #0x173
1008ca2d0:      add x8, sp, #0x90
1008ca2d4:      mov x1, sp
1008ca2d8:      bl  0x100023808 <__RNvNvNtCsctvjasLqLe9_5alloc3fmt6format12format_inner>
1008ca2dc:      ldr x20, [sp, #0x98]
1008ca2e0:      ldr w1, [sp, #0xa0]
1008ca2e4:      mov x0, x20
1008ca2e8:      mov x2, x1
1008ca2ec:      bl  0x10095ad00 <_js_string_from_bytes_with_capacity>
1008ca2f0:      mov x3, x0
1008ca2f4:      adrp    x1, 0x100ddd000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text15SB_WINDOWS_1254+0x98>
1008ca2f8:      add x1, x1, #0xcb5
1008ca2fc:      mov w21, #0x1               ; =1
1008ca300:      mov w0, #0x2                ; =2
1008ca304:      mov w2, #0xa                ; =10
1008ca308:      mov w4, #0x1                ; =1
1008ca30c:      bl  0x100294dac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
1008ca310:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1008ca314:      bfxil   x8, x0, #0, #48
1008ca318:      stp x21, x8, [x19]
1008ca31c:      ldr x8, [sp, #0x90]
1008ca320:      cbz x8, 0x1008ca5fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a4>
1008ca324:      mov x0, x20
1008ca328:      b   0x1008ca5f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a0>
1008ca32c:      adrp    x0, 0x101134000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json17PARSE_SHAPE_CACHE0023___RUST_STD_INTERNAL_VAL>
1008ca330:      add x0, x0, #0x660
1008ca334:      ldr x8, [x0]
1008ca338:      blr x8
1008ca33c:      mov x20, x0
1008ca340:      ldrb    w8, [x0, #0x20]
1008ca344:      cbnz    w8, 0x1008ca788 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x630>
1008ca348:      ldr x8, [x20]
1008ca34c:      cbnz    x8, 0x1008ca7d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x67c>
1008ca350:      mov x26, #0x7fff000000000000 ; =9223090561878065152
1008ca354:      bfxil   x26, x22, #0, #48
1008ca358:      mov x8, #-0x1               ; =-1
1008ca35c:      str x8, [x20]
1008ca360:      mov x23, x20
1008ca364:      ldr x8, [x23, #0x8]!
1008ca368:      ldr x24, [x20, #0x18]
1008ca36c:      cmp x24, x8
1008ca370:      b.ne    0x1008ca37c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x224>
1008ca374:      mov x0, x23
1008ca378:      bl  0x100ccc608 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1008ca37c:      ldr x8, [x20, #0x10]
1008ca380:      str x26, [x8, x24, lsl #3]
1008ca384:      add x8, x24, #0x1
1008ca388:      str x8, [x20, #0x18]
1008ca38c:      ldr x8, [x20]
1008ca390:      add x8, x8, #0x1
1008ca394:      str x8, [x20]
1008ca398:      mov x0, #0x0                ; =0
1008ca39c:      bl  0x10039e7b0 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy43GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1J_6option6OptionQIB2o_INtNtB1J_4cell4CellbEEEEE9call_onceBc_>
1008ca3a0:      mov x22, x0
1008ca3a4:      ldrb    w8, [x0]
1008ca3a8:      strb    wzr, [x0]
1008ca3ac:      cbz w8, 0x1008ca3e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x288>
1008ca3b0:      mov x0, #0x0                ; =0
1008ca3b4:      bl  0x10039e7d0 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1008ca3b8:      ldrb    w8, [x0]
1008ca3bc:      tst w8, #0x3
1008ca3c0:      b.ne    0x1008ca3d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x280>
1008ca3c4:      adrp    x8, 0x101178000 <_out_buf+0x3f08>
1008ca3c8:      add x8, x8, #0xb38
1008ca3cc:      ldapr   w8, [x8]
1008ca3d0:      cmp w8, #0x0
1008ca3d4:      b.le    0x1008ca61c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4c4>
1008ca3d8:      mov w8, #0x1                ; =1
1008ca3dc:      strb    w8, [x22]
1008ca3e0:      bl  0x100347d00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1008ca3e4:      bl  0x100347bec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
1008ca3e8:      ldrb    w8, [x20, #0x20]
1008ca3ec:      cbnz    w8, 0x1008ca66c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x514>
1008ca3f0:      ldr x8, [x20]
1008ca3f4:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008ca3f8:      cmp x8, x9
1008ca3fc:      b.hs    0x1008ca698 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x540>
1008ca400:      add x9, x8, #0x1
1008ca404:      str x9, [x20]
1008ca408:      ldr x10, [x20, #0x18]
1008ca40c:      mov w9, #0x1                ; =1
1008ca410:      cmp x24, x10
1008ca414:      b.hs    0x1008ca428 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2d0>
1008ca418:      ldr x10, [x20, #0x10]
1008ca41c:      ldr x10, [x10, x24, lsl #3]
1008ca420:      and x10, x10, #0xffffffffffff
1008ca424:      b   0x1008ca42c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2d4>
1008ca428:      mov w10, #0x1               ; =1
1008ca42c:      str x8, [x20]
1008ca430:      add x8, x10, #0x14
1008ca434:      movi.2d v0, #0000000000000000
1008ca438:      stur    q0, [x25, #0x78]
1008ca43c:      stur    q0, [x25, #0x68]
1008ca440:      stur    q0, [x25, #0x58]
1008ca444:      stur    q0, [x25, #0x48]
1008ca448:      strb    w9, [sp, #0x120]
1008ca44c:      mov x9, #-0x1               ; =-1
1008ca450:      stp x8, x21, [sp, #0xb8]
1008ca454:      str x9, [sp, #0x90]
1008ca458:      stp xzr, xzr, [sp, #0xc8]
1008ca45c:      str xzr, [sp, #0x118]
1008ca460:      add x0, sp, #0x90
1008ca464:      bl  0x10089987c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1008ca468:      mov x21, x0
1008ca46c:      ldp x8, x9, [sp, #0xc0]
1008ca470:      cmp x9, x8
1008ca474:      b.hs    0x1008ca4a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
1008ca478:      ldr x10, [sp, #0xb8]
1008ca47c:      mov x11, #0x2600            ; =9728
1008ca480:      movk    x11, #0x1, lsl #32
1008ca484:      ldrb    w12, [x10, x9]
1008ca488:      cmp w12, #0x20
1008ca48c:      b.hi    0x1008ca4a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
1008ca490:      lsr x12, x11, x12
1008ca494:      tbz w12, #0x0, 0x1008ca4a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
1008ca498:      add x9, x9, #0x1
1008ca49c:      cmp x8, x9
1008ca4a0:      b.ne    0x1008ca484 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x32c>
1008ca4a4:      mov x9, x8
1008ca4a8:      ldrb    w25, [sp, #0x120]
1008ca4ac:      cmp x9, x8
1008ca4b0:      cset    w26, eq
1008ca4b4:      ldrb    w8, [x20, #0x20]
1008ca4b8:      cbnz    w8, 0x1008ca7b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x658>
1008ca4bc:      ldr x8, [x20]
1008ca4c0:      cbnz    x8, 0x1008ca7d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x67c>
1008ca4c4:      mov x8, #-0x1               ; =-1
1008ca4c8:      str x8, [x20]
1008ca4cc:      ldr x27, [x20, #0x18]
1008ca4d0:      ldr x8, [x20, #0x8]
1008ca4d4:      cmp x27, x8
1008ca4d8:      b.ne    0x1008ca4e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x38c>
1008ca4dc:      mov x0, x23
1008ca4e0:      bl  0x100ccc608 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1008ca4e4:      ldr x8, [x20, #0x10]
1008ca4e8:      str x21, [x8, x27, lsl #3]
1008ca4ec:      add x8, x27, #0x1
1008ca4f0:      str x8, [x20, #0x18]
1008ca4f4:      ldr x8, [x20]
1008ca4f8:      add x8, x8, #0x1
1008ca4fc:      str x8, [x20]
1008ca500:      mov x0, #0x0                ; =0
1008ca504:      bl  0x10039e7d0 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1008ca508:      ldrb    w8, [x0]
1008ca50c:      and w8, w8, #0xfffffffd
1008ca510:      strb    w8, [x0]
1008ca514:      bl  0x100348798 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy22gc_bump_malloc_trigger>
1008ca518:      adrp    x23, 0x10112c000 <_perry_global_baseline_worker_ts__1>
1008ca51c:      add x23, x23, #0x5c0
1008ca520:      ldapr   x8, [x23]
1008ca524:      cbnz    x8, 0x1008ca770 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x618>
1008ca528:      ldrb    w8, [x23, #0x8]
1008ca52c:      cbz w8, 0x1008ca55c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x404>
1008ca530:      bl  0x100195d64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena4walk18arena_in_use_bytes>
1008ca534:      adrp    x8, 0x10112c000 <_perry_global_baseline_worker_ts__1>
1008ca538:      add x8, x8, #0x658
1008ca53c:      ldapr   x8, [x8]
1008ca540:      cbnz    x8, 0x1008ca7f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x69c>
1008ca544:      adrp    x8, 0x10112c000 <_perry_global_baseline_worker_ts__1>
1008ca548:      ldr x8, [x8, #0x660]
1008ca54c:      cmp x0, x8
1008ca550:      b.lo    0x1008ca55c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x404>
1008ca554:      mov w8, #0x1                ; =1
1008ca558:      strb    w8, [x22]
1008ca55c:      ldrb    w8, [x20, #0x20]
1008ca560:      cbnz    w8, 0x1008ca7e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x688>
1008ca564:      ldr x8, [x20]
1008ca568:      cbnz    x8, 0x1008ca838 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x6e0>
1008ca56c:      and w22, w26, w25
1008ca570:      ldr x8, [x20, #0x18]
1008ca574:      cmp x24, x8
1008ca578:      b.hi    0x1008ca580 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x428>
1008ca57c:      str x24, [x20, #0x18]
1008ca580:      adrp    x0, 0x1010ce000 <_anon.b8734461ce4fd0c908478712a5ac704e.197+0x20>
1008ca584:      add x0, x0, #0xb48
1008ca588:      bl  0x100139b5c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api17parse_result_slows_0uEB2P_>
1008ca58c:      tbz w22, #0x0, 0x1008ca5a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x44c>
1008ca590:      stp xzr, x21, [x19]
1008ca594:      ldr x8, [sp, #0x90]
1008ca598:      cmn x8, #0x1
1008ca59c:      b.ne    0x1008ca5f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x498>
1008ca5a0:      b   0x1008ca5fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a4>
1008ca5a4:      adrp    x0, 0x100e18000 <_anon.b8734461ce4fd0c908478712a5ac704e.318+0x138>
1008ca5a8:      add x0, x0, #0x639
1008ca5ac:      mov w1, #0x21               ; =33
1008ca5b0:      mov w2, #0x21               ; =33
1008ca5b4:      bl  0x10095ad00 <_js_string_from_bytes_with_capacity>
1008ca5b8:      mov x3, x0
1008ca5bc:      adrp    x1, 0x100ddd000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text15SB_WINDOWS_1254+0x98>
1008ca5c0:      add x1, x1, #0xccd
1008ca5c4:      mov w20, #0x1               ; =1
1008ca5c8:      mov w0, #0x4                ; =4
1008ca5cc:      mov w2, #0xb                ; =11
1008ca5d0:      mov w4, #0x1                ; =1
1008ca5d4:      bl  0x100294dac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
1008ca5d8:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1008ca5dc:      bfxil   x8, x0, #0, #48
1008ca5e0:      stp x20, x8, [x19]
1008ca5e4:      ldr x8, [sp, #0x90]
1008ca5e8:      cmn x8, #0x1
1008ca5ec:      b.eq    0x1008ca5fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a4>
1008ca5f0:      cbz x8, 0x1008ca5fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a4>
1008ca5f4:      ldr x0, [sp, #0x98]
1008ca5f8:      bl  0x100ce20c0 <_mi_free>
1008ca5fc:      ldp x29, x30, [sp, #0x190]
1008ca600:      ldp x20, x19, [sp, #0x180]
1008ca604:      ldp x22, x21, [sp, #0x170]
1008ca608:      ldp x24, x23, [sp, #0x160]
1008ca60c:      ldp x26, x25, [sp, #0x150]
1008ca610:      ldp x28, x27, [sp, #0x140]
1008ca614:      add sp, sp, #0x1a0
1008ca618:      ret
1008ca61c:      mov x0, #0x0                ; =0
1008ca620:      bl  0x10039e968 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5block17ARENA_TOTAL_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1l_6option6OptionQIB20_INtNtB1l_4cell4CelljEEEEE9call_onceBc_>
1008ca624:      ldr x26, [x0]
1008ca628:      mov x0, #0x0                ; =0
1008ca62c:      bl  0x10039e630 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy21GC_NEXT_TRIGGER_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1n_6option6OptionQIB22_INtNtB1n_4cell4CelljEEEEE9call_onceBc_>
1008ca630:      ldr x8, [x0]
1008ca634:      cmp x8, x26
1008ca638:      b.ls    0x1008ca658 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x500>
1008ca63c:      str x26, [x0]
1008ca640:      adrp    x0, 0x101132000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime8dyn_eval3env13ENV_KEY_CACHE0023___RUST_STD_INTERNAL_VAL+0x10>
1008ca644:      add x0, x0, #0x9c8
1008ca648:      ldr x8, [x0]
1008ca64c:      blr x8
1008ca650:      mov w8, #0x1                ; =1
1008ca654:      strb    w8, [x0]
1008ca658:      bl  0x100347d00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1008ca65c:      bl  0x100347d00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1008ca660:      bl  0x100347bec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
1008ca664:      ldrb    w8, [x20, #0x20]
1008ca668:      cbz w8, 0x1008ca3f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x298>
1008ca66c:      cmp w8, #0x2
1008ca670:      b.eq    0x1008ca7e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x690>
1008ca674:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
1008ca678:      add x1, x1, #0xd0
1008ca67c:      mov x0, x20
1008ca680:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008ca684:      strb    wzr, [x20, #0x20]
1008ca688:      ldr x8, [x20]
1008ca68c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008ca690:      cmp x8, x9
1008ca694:      b.lo    0x1008ca400 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2a8>
1008ca698:      adrp    x0, 0x10109c000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1008ca69c:      add x0, x0, #0xdc8
1008ca6a0:      bl  0x100c99adc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008ca6a4:      stur    x21, [x29, #-0x68]
1008ca6a8:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1008ca6ac:      bfxil   x8, x22, #0, #48
1008ca6b0:      str x8, [sp, #0x90]
1008ca6b4:      adrp    x22, 0x1010ce000 <_anon.b8734461ce4fd0c908478712a5ac704e.197+0x20>
1008ca6b8:      add x22, x22, #0xb40
1008ca6bc:      add x1, sp, #0x90
1008ca6c0:      mov x0, x22
1008ca6c4:      bl  0x100137410 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json15parse_root_push0jEB2h_>
1008ca6c8:      mov x23, x0
1008ca6cc:      stp x0, x20, [x29, #-0x60]
1008ca6d0:      str x21, [sp]
1008ca6d4:      sub x8, x29, #0x58
1008ca6d8:      mov x9, sp
1008ca6dc:      stp x8, x9, [sp, #0x90]
1008ca6e0:      sub x8, x29, #0x60
1008ca6e4:      sub x9, x29, #0x68
1008ca6e8:      stp x8, x9, [sp, #0xa0]
1008ca6ec:      adrp    x0, 0x1010cd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5types18GC_TYPE_INFO_BY_ID+0x4c8>
1008ca6f0:      add x0, x0, #0x400
1008ca6f4:      add x1, sp, #0x90
1008ca6f8:      bl  0x10012c430 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawIB1t_NtNtNtB1S_5value7jsvalue7JSValueENCINvB1Q_19with_built_tape_rawB3o_NCNvNtNtB1S_4json9parse_api24try_parse_deep_iterative0E0E0IB1t_B3o_EEB1S_>
1008ca6fc:      mov x21, x0
1008ca700:      mov x20, x1
1008ca704:      str x23, [sp, #0x90]
1008ca708:      add x1, sp, #0x90
1008ca70c:      mov x0, x22
1008ca710:      bl  0x1001374a8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json18parse_root_restore0uEB2h_>
1008ca714:      adrp    x0, 0x1010ce000 <_anon.b8734461ce4fd0c908478712a5ac704e.197+0x20>
1008ca718:      add x0, x0, #0xb48
1008ca71c:      bl  0x100139cc4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api24try_parse_deep_iteratives_0uEB2P_>
1008ca720:      tbnz    w21, #0x0, 0x1008ca764 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x60c>
1008ca724:      adrp    x0, 0x100e17000 <_anon.78a33a9fe279ced61d81da3c9b3c7fad.1076+0xdb>
1008ca728:      add x0, x0, #0xd82
1008ca72c:      mov w1, #0x29               ; =41
1008ca730:      mov w2, #0x29               ; =41
1008ca734:      bl  0x10095ad00 <_js_string_from_bytes_with_capacity>
1008ca738:      mov x3, x0
1008ca73c:      adrp    x1, 0x100ddd000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text15SB_WINDOWS_1254+0x98>
1008ca740:      add x1, x1, #0xccd
1008ca744:      mov w21, #0x1               ; =1
1008ca748:      mov w0, #0x4                ; =4
1008ca74c:      mov w2, #0xb                ; =11
1008ca750:      mov w4, #0x1                ; =1
1008ca754:      bl  0x100294dac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
1008ca758:      mov x20, #0x7ffd000000000000 ; =9222527611924643840
1008ca75c:      bfxil   x20, x0, #0, #48
1008ca760:      b   0x1008ca768 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x610>
1008ca764:      mov x21, #0x0               ; =0
1008ca768:      stp x21, x20, [x19]
1008ca76c:      b   0x1008ca5fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4a4>
1008ca770:      adrp    x0, 0x10112c000 <_perry_global_baseline_worker_ts__1>
1008ca774:      add x0, x0, #0x5c0
1008ca778:      bl  0x100cc1a54 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtCs5gMwpk3Cs4e_13perry_runtime2gc14gen_gc_enabled0E0zEB1y_>
1008ca77c:      ldrb    w8, [x23, #0x8]
1008ca780:      cbnz    w8, 0x1008ca530 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x3d8>
1008ca784:      b   0x1008ca55c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x404>
1008ca788:      cmp w8, #0x1
1008ca78c:      b.ne    0x1008ca7e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x690>
1008ca790:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
1008ca794:      add x1, x1, #0xd0
1008ca798:      mov x0, x20
1008ca79c:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008ca7a0:      strb    wzr, [x20, #0x20]
1008ca7a4:      ldr x8, [x20]
1008ca7a8:      cbz x8, 0x1008ca350 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1f8>
1008ca7ac:      b   0x1008ca7d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x67c>
1008ca7b0:      cmp w8, #0x2
1008ca7b4:      b.eq    0x1008ca7e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x690>
1008ca7b8:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
1008ca7bc:      add x1, x1, #0xd0
1008ca7c0:      mov x0, x20
1008ca7c4:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008ca7c8:      strb    wzr, [x20, #0x20]
1008ca7cc:      ldr x8, [x20]
1008ca7d0:      cbz x8, 0x1008ca4c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x36c>
1008ca7d4:      adrp    x0, 0x10109c000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1008ca7d8:      add x0, x0, #0xdf8
1008ca7dc:      bl  0x100c99aac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008ca7e0:      cmp w8, #0x2
1008ca7e4:      b.ne    0x1008ca81c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x6c4>
1008ca7e8:      adrp    x0, 0x10109b000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1008ca7ec:      add x0, x0, #0xed8
1008ca7f0:      bl  0x100cdb71c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1008ca7f4:      adrp    x8, 0x10112c000 <_perry_global_baseline_worker_ts__1>
1008ca7f8:      add x8, x8, #0x658
1008ca7fc:      mov x23, x0
1008ca800:      mov x0, x8
1008ca804:      bl  0x100cc2384 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockjE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc11heap_budget38gc_tiny_parse_in_use_trigger_dyn_bytes0E0zEB1A_>
1008ca808:      adrp    x8, 0x10112c000 <_perry_global_baseline_worker_ts__1>
1008ca80c:      ldr x8, [x8, #0x660]
1008ca810:      cmp x23, x8
1008ca814:      b.hs    0x1008ca554 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x3fc>
1008ca818:      b   0x1008ca55c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x404>
1008ca81c:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
1008ca820:      add x1, x1, #0xd0
1008ca824:      mov x0, x20
1008ca828:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008ca82c:      strb    wzr, [x20, #0x20]
1008ca830:      ldr x8, [x20]
1008ca834:      cbz x8, 0x1008ca56c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x414>
1008ca838:      adrp    x0, 0x10109c000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1008ca83c:      add x0, x0, #0xe58
1008ca840:      bl  0x100c99aac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
