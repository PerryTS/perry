/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/growth-alias-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004282a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array>:
1004282a4:      stp x26, x25, [sp, #-0x50]!
1004282a8:      stp x24, x23, [sp, #0x10]
1004282ac:      stp x22, x21, [sp, #0x20]
1004282b0:      stp x20, x19, [sp, #0x30]
1004282b4:      stp x29, x30, [sp, #0x40]
1004282b8:      add x29, sp, #0x40
1004282bc:      mov x19, x0
1004282c0:      ldr x23, [x19, #0x20]!
1004282c4:      cbz x23, 0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1004282c8:      lsr x8, x23, #51
1004282cc:      mov x21, x23
1004282d0:      cmp x8, #0xfff
1004282d4:      b.lo    0x1004282ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x48>
1004282d8:      mov w8, #0x7ffc             ; =32764
1004282dc:      cmp x8, x23, lsr #48
1004282e0:      b.eq    0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1004282e4:      ands    x21, x23, #0xffffffffffff
1004282e8:      b.eq    0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1004282ec:      and x8, x21, #0xfffffffffff00000
1004282f0:      lsr x9, x21, #47
1004282f4:      cmp x9, #0x0
1004282f8:      ccmp    x8, #0x0, #0x4, eq
1004282fc:      b.eq    0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100428300:      tst x21, #0x3
100428304:      ccmp    x21, #0x7, #0x0, eq
100428308:      mov x20, x0
10042830c:      b.ls    0x10042841c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
100428310:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
100428314:      add x8, x8, #0xfe0
100428318:      ldr x8, [x8]
10042831c:      cmn x8, #0x1
100428320:      b.eq    0x100428720 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x47c>
100428324:      mrs x9, TPIDRRO_EL0
100428328:      and x9, x9, #0xfffffffffffffff8
10042832c:      ldr x8, [x9, x8, lsl #3]
100428330:      cbz x8, 0x100428720 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x47c>
100428334:      lsr x1, x21, #20
100428338:      ldr x8, [x8, #0x10]
10042833c:      ldrb    w9, [x8, #0x28]
100428340:      tbz w9, #0x0, 0x100428360 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
100428344:      ldr x9, [x8, #0x20]
100428348:      cmp x9, x1
10042834c:      b.ne    0x100428360 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
100428350:      ldp x9, x10, [x8]
100428354:      cmp x9, x21
100428358:      ccmp    x10, x21, #0x0, ls
10042835c:      b.hi    0x1004283dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
100428360:      ldrb    w9, [x8, #0x58]
100428364:      cbz w9, 0x100428384 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xe0>
100428368:      ldr x9, [x8, #0x50]
10042836c:      cmp x9, x1
100428370:      b.ne    0x100428384 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xe0>
100428374:      ldp x9, x10, [x8, #0x30]
100428378:      cmp x9, x21
10042837c:      ccmp    x10, x21, #0x0, ls
100428380:      b.hi    0x1004283d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x12c>
100428384:      ldrb    w9, [x8, #0x88]
100428388:      cbz w9, 0x1004283a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x104>
10042838c:      ldr x9, [x8, #0x80]
100428390:      cmp x9, x1
100428394:      b.ne    0x1004283a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x104>
100428398:      ldp x9, x10, [x8, #0x60]
10042839c:      cmp x9, x21
1004283a0:      ccmp    x10, x21, #0x0, ls
1004283a4:      b.hi    0x1004283d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x134>
1004283a8:      ldrb    w9, [x8, #0xb8]
1004283ac:      cbz w9, 0x1004283e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
1004283b0:      ldr x9, [x8, #0xb0]
1004283b4:      cmp x9, x1
1004283b8:      b.ne    0x1004283e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
1004283bc:      ldp x9, x10, [x8, #0x90]!
1004283c0:      cmp x9, x21
1004283c4:      ccmp    x10, x21, #0x0, ls
1004283c8:      b.hi    0x1004283dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
1004283cc:      b   0x1004283e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
1004283d0:      add x8, x8, #0x30
1004283d4:      b   0x1004283dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
1004283d8:      add x8, x8, #0x60
1004283dc:      ldrb    w8, [x8, #0x19]
1004283e0:      cmp w8, #0xff
1004283e4:      b.ne    0x1004283fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x158>
1004283e8:      mov x0, x21
1004283ec:      bl  0x1009960b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
1004283f0:      mov x8, x0
1004283f4:      mov x0, x20
1004283f8:      and w8, w8, #0xff
1004283fc:      cbz w8, 0x10042841c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
100428400:      ldurb   w8, [x21, #-0x8]
100428404:      ldurb   w9, [x21, #-0x7]
100428408:      mov w10, #0x82              ; =130
10042840c:      and w9, w9, w10
100428410:      cmp w9, #0x2
100428414:      ccmp    w8, #0x1, #0x0, eq
100428418:      b.eq    0x10042853c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x298>
10042841c:      mov x0, x21
100428420:      bl  0x100401a90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100428424:      cbz x0, 0x100428454 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x1b0>
100428428:      ldrb    w9, [x0]
10042842c:      cmp w9, #0x1
100428430:      b.ne    0x1004284f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x254>
100428434:      ldrsb   w8, [x0, #0x1]
100428438:      tbnz    w8, #0x1f, 0x100428568 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x2c4>
10042843c:      mov x8, x0
100428440:      mov x0, x20
100428444:      ldp w10, w9, [x21]
100428448:      cmp w10, w9
10042844c:      b.hi    0x100428600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x35c>
100428450:      b   0x10042861c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x378>
100428454:      adrp    x8, 0x10116c000 <_out_buf+0x3f08>
100428458:      add x8, x8, #0x710
10042845c:      ldaprb  w8, [x8]
100428460:      cbz w8, 0x1004284a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
100428464:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
100428468:      add x8, x8, #0x258
10042846c:      ldapr   x9, [x8]
100428470:      cmp x9, x21
100428474:      b.hi    0x1004284a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
100428478:      ldapur  x8, [x8, #0x8]
10042847c:      cmp x8, x21
100428480:      b.lo    0x1004284a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
100428484:      mov x24, x0
100428488:      mov x0, x21
10042848c:      bl  0x10019dd98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100428490:      mov x8, x0
100428494:      mov x0, x24
100428498:      tbz w8, #0x0, 0x1004284a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
10042849c:      mov x8, #0x0                ; =0
1004284a0:      b   0x1004285ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x348>
1004284a4:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x110>
1004284a8:      add x8, x8, #0xa61
1004284ac:      ldaprb  w8, [x8]
1004284b0:      cbz w8, 0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1004284b4:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
1004284b8:      add x8, x8, #0xae8
1004284bc:      ldapr   x9, [x8]
1004284c0:      cmp x9, x21
1004284c4:      b.hi    0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1004284c8:      ldapur  x8, [x8, #0x8]
1004284cc:      cmp x8, x21
1004284d0:      b.lo    0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1004284d4:      mov x24, x0
1004284d8:      mov x0, x21
1004284dc:      bl  0x100895b28 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
1004284e0:      mov x9, x0
1004284e4:      mov x8, #0x0                ; =0
1004284e8:      mov x22, #0x0               ; =0
1004284ec:      mov x0, x20
1004284f0:      tbnz    w9, #0x0, 0x1004285f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x34c>
1004284f4:      b   0x100428704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
1004284f8:      mov x24, x0
1004284fc:      mov x8, x0
100428500:      cmp w9, #0x1
100428504:      b.eq    0x1004285ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x348>
100428508:      cmp w9, #0x9
10042850c:      b.ne    0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100428510:      ldr w8, [x21, #0x4]
100428514:      mov w9, #0x5841             ; =22593
100428518:      movk    w9, #0x4c5a, lsl #16
10042851c:      cmp w8, w9
100428520:      b.ne    0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100428524:      mov x0, x21
100428528:      bl  0x1003db41c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape22force_materialize_lazy>
10042852c:      mov x22, x0
100428530:      mov x0, x20
100428534:      cbnz    x22, 0x1004286c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
100428538:      b   0x100428704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
10042853c:      ldr w8, [x21]
100428540:      mov w9, #0xe100             ; =57600
100428544:      movk    w9, #0x5f5, lsl #16
100428548:      orr w9, w9, #0x1
10042854c:      cmp w8, w9
100428550:      b.hs    0x10042841c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
100428554:      ldr w9, [x21, #0x4]
100428558:      cmp w8, w9
10042855c:      b.hi    0x10042841c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
100428560:      mov x22, x21
100428564:      b   0x1004286c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
100428568:      mov x24, x0
10042856c:      ldr x21, [x0, #0x8]
100428570:      mov x0, x21
100428574:      bl  0x100401a90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100428578:      cbz x0, 0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10042857c:      mov x8, x0
100428580:      ldrb    w9, [x0]
100428584:      cmp w9, #0x1
100428588:      b.ne    0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10042858c:      ldrsb   w9, [x8, #0x1]
100428590:      tbz w9, #0x1f, 0x100428440 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x19c>
100428594:      mov w25, #0x1               ; =1
100428598:      ldr x21, [x8, #0x8]
10042859c:      mov x0, x21
1004285a0:      bl  0x100401a90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1004285a4:      cbz x0, 0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1004285a8:      mov x8, x0
1004285ac:      mov x22, #0x0               ; =0
1004285b0:      ldrb    w9, [x0]
1004285b4:      cmp w9, #0x1
1004285b8:      b.ne    0x100428704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
1004285bc:      cmp w25, #0x3f
1004285c0:      b.hi    0x100428704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
1004285c4:      add w25, w25, #0x1
1004285c8:      ldrsb   w9, [x8, #0x1]
1004285cc:      tbnz    w9, #0x1f, 0x100428598 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x2f4>
1004285d0:      str x21, [x24, #0x8]
1004285d4:      ldrb    w10, [x24, #0x1]
1004285d8:      orr w10, w10, #0x80
1004285dc:      strb    w10, [x24, #0x1]
1004285e0:      ldrb    w9, [x8]
1004285e4:      cmp w9, #0x1
1004285e8:      b.ne    0x100428508 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x264>
1004285ec:      mov x0, x20
1004285f0:      ldp w10, w9, [x21]
1004285f4:      cmp w10, w9
1004285f8:      b.ls    0x10042861c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x378>
1004285fc:      cbz x24, 0x100428630 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x38c>
100428600:      ldr w8, [x8, #0x4]
100428604:      ubfiz   x9, x9, #3, #32
100428608:      add x9, x9, #0x10
10042860c:      mov x22, x21
100428610:      cmp x9, x8
100428614:      b.eq    0x1004286c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
100428618:      b   0x100428630 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x38c>
10042861c:      mov w8, #0xe100             ; =57600
100428620:      movk    w8, #0x5f5, lsl #16
100428624:      mov x22, x21
100428628:      cmp w10, w8
10042862c:      b.ls    0x1004286c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
100428630:      adrp    x8, 0x10116c000 <_out_buf+0x3f08>
100428634:      add x8, x8, #0x710
100428638:      ldaprb  w8, [x8]
10042863c:      cbz w8, 0x100428678 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
100428640:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
100428644:      add x8, x8, #0x258
100428648:      ldapr   x9, [x8]
10042864c:      cmp x9, x21
100428650:      b.hi    0x100428678 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
100428654:      ldapur  x8, [x8, #0x8]
100428658:      cmp x8, x21
10042865c:      b.lo    0x100428678 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
100428660:      mov x0, x21
100428664:      bl  0x10019dd98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100428668:      tbz w0, #0x0, 0x100428678 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
10042866c:      mov x0, x20
100428670:      mov x22, x21
100428674:      b   0x1004286c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
100428678:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x110>
10042867c:      add x8, x8, #0xa61
100428680:      ldaprb  w8, [x8]
100428684:      cbz w8, 0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100428688:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
10042868c:      add x8, x8, #0xae8
100428690:      ldapr   x9, [x8]
100428694:      cmp x21, x9
100428698:      b.lo    0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
10042869c:      ldapur  x8, [x8, #0x8]
1004286a0:      cmp x21, x8
1004286a4:      b.hi    0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1004286a8:      mov x0, x21
1004286ac:      bl  0x100895b28 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
1004286b0:      mov x8, x0
1004286b4:      mov x0, x20
1004286b8:      mov x22, x21
1004286bc:      tbz w8, #0x0, 0x100428700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1004286c0:      cmp x22, x23
1004286c4:      b.eq    0x10042879c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
1004286c8:      str x22, [x0, #0x20]
1004286cc:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
1004286d0:      add x8, x8, #0xd50
1004286d4:      ldapr   x8, [x8]
1004286d8:      cbnz    x8, 0x100428740 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x49c>
1004286dc:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
1004286e0:      ldrb    w8, [x8, #0xd58]
1004286e4:      tbz w8, #0x0, 0x10042875c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4b8>
1004286e8:      mov x0, x20
1004286ec:      mov x1, x19
1004286f0:      mov x2, x22
1004286f4:      mov w3, #0x0                ; =0
1004286f8:      bl  0x10098f020 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
1004286fc:      b   0x100428774 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4d0>
100428700:      mov x22, #0x0               ; =0
100428704:      mov x0, x22
100428708:      ldp x29, x30, [sp, #0x40]
10042870c:      ldp x20, x19, [sp, #0x30]
100428710:      ldp x22, x21, [sp, #0x20]
100428714:      ldp x24, x23, [sp, #0x10]
100428718:      ldp x26, x25, [sp], #0x50
10042871c:      ret
100428720:      bl  0x100cb1624 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100428724:      mov x8, x0
100428728:      mov x0, x20
10042872c:      lsr x1, x21, #20
100428730:      ldr x8, [x8, #0x10]
100428734:      ldrb    w9, [x8, #0x28]
100428738:      tbnz    w9, #0x0, 0x100428344 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xa0>
10042873c:      b   0x100428360 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
100428740:      adrp    x0, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
100428744:      add x0, x0, #0xd50
100428748:      bl  0x100cb63bc <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
10042874c:      mov x0, x20
100428750:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
100428754:      ldrb    w8, [x8, #0xd58]
100428758:      tbnz    w8, #0x0, 0x1004286e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x444>
10042875c:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x110>
100428760:      add x8, x8, #0xaa8
100428764:      ldr w8, [x8]
100428768:      cbz w8, 0x100428778 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4d4>
10042876c:      mov x0, x22
100428770:      bl  0x1009904f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
100428774:      mov x0, x20
100428778:      ldr x8, [x19]
10042877c:      cbz x8, 0x10042879c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
100428780:      ldr x8, [x0, #0x10]
100428784:      cbz x8, 0x10042879c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
100428788:      mov x0, x20
10042878c:      bl  0x1008a0cc0 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime15json_tape_store7release>
100428790:      mov x0, x20
100428794:      str xzr, [x20, #0x10]
100428798:      str wzr, [x20, #0xc]
10042879c:      ldr w8, [x22]
1004287a0:      str w8, [x0]
1004287a4:      b   0x100428704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
