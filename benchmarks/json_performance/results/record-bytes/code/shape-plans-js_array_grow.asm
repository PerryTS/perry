/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100490340 <_js_array_grow>:
100490340:      sub sp, sp, #0xb0
100490344:      stp x28, x27, [sp, #0x50]
100490348:      stp x26, x25, [sp, #0x60]
10049034c:      stp x24, x23, [sp, #0x70]
100490350:      stp x22, x21, [sp, #0x80]
100490354:      stp x20, x19, [sp, #0x90]
100490358:      stp x29, x30, [sp, #0xa0]
10049035c:      add x29, sp, #0xa0
100490360:      cmp x0, #0xfff
100490364:      b.ls    0x1004906b4 <_js_array_grow+0x374>
100490368:      mov x19, x0
10049036c:      lsr x8, x0, #51
100490370:      cmp x8, #0xfff
100490374:      b.lo    0x10049038c <_js_array_grow+0x4c>
100490378:      mov w8, #0x7ffc             ; =32764
10049037c:      cmp x8, x19, lsr #48
100490380:      b.eq    0x1004906b4 <_js_array_grow+0x374>
100490384:      ands    x19, x19, #0xffffffffffff
100490388:      b.eq    0x1004906b4 <_js_array_grow+0x374>
10049038c:      and x8, x19, #0xfffffffffff00000
100490390:      lsr x9, x19, #47
100490394:      cmp x9, #0x0
100490398:      ccmp    x8, #0x0, #0x4, eq
10049039c:      b.eq    0x1004906b4 <_js_array_grow+0x374>
1004903a0:      tst x19, #0x3
1004903a4:      ccmp    x19, #0x7, #0x0, eq
1004903a8:      mov x22, x1
1004903ac:      b.ls    0x1004904b4 <_js_array_grow+0x174>
1004903b0:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1004903b4:      add x8, x8, #0x4e8
1004903b8:      ldr x8, [x8]
1004903bc:      cmn x8, #0x1
1004903c0:      b.eq    0x100490898 <_js_array_grow+0x558>
1004903c4:      mrs x9, TPIDRRO_EL0
1004903c8:      and x9, x9, #0xfffffffffffffff8
1004903cc:      ldr x0, [x9, x8, lsl #3]
1004903d0:      cbz x0, 0x100490898 <_js_array_grow+0x558>
1004903d4:      lsr x1, x19, #20
1004903d8:      ldr x8, [x0, #0x10]
1004903dc:      ldrb    w9, [x8, #0x28]
1004903e0:      tbz w9, #0x0, 0x100490400 <_js_array_grow+0xc0>
1004903e4:      ldr x9, [x8, #0x20]
1004903e8:      cmp x9, x1
1004903ec:      b.ne    0x100490400 <_js_array_grow+0xc0>
1004903f0:      ldp x9, x10, [x8]
1004903f4:      cmp x9, x19
1004903f8:      ccmp    x10, x19, #0x0, ls
1004903fc:      b.hi    0x10049047c <_js_array_grow+0x13c>
100490400:      ldrb    w9, [x8, #0x58]
100490404:      cbz w9, 0x100490424 <_js_array_grow+0xe4>
100490408:      ldr x9, [x8, #0x50]
10049040c:      cmp x9, x1
100490410:      b.ne    0x100490424 <_js_array_grow+0xe4>
100490414:      ldp x9, x10, [x8, #0x30]
100490418:      cmp x9, x19
10049041c:      ccmp    x10, x19, #0x0, ls
100490420:      b.hi    0x100490470 <_js_array_grow+0x130>
100490424:      ldrb    w9, [x8, #0x88]
100490428:      cbz w9, 0x100490448 <_js_array_grow+0x108>
10049042c:      ldr x9, [x8, #0x80]
100490430:      cmp x9, x1
100490434:      b.ne    0x100490448 <_js_array_grow+0x108>
100490438:      ldp x9, x10, [x8, #0x60]
10049043c:      cmp x9, x19
100490440:      ccmp    x10, x19, #0x0, ls
100490444:      b.hi    0x100490478 <_js_array_grow+0x138>
100490448:      ldrb    w9, [x8, #0xb8]
10049044c:      cbz w9, 0x100490488 <_js_array_grow+0x148>
100490450:      ldr x9, [x8, #0xb0]
100490454:      cmp x9, x1
100490458:      b.ne    0x100490488 <_js_array_grow+0x148>
10049045c:      ldp x9, x10, [x8, #0x90]!
100490460:      cmp x9, x19
100490464:      ccmp    x10, x19, #0x0, ls
100490468:      b.hi    0x10049047c <_js_array_grow+0x13c>
10049046c:      b   0x100490488 <_js_array_grow+0x148>
100490470:      add x8, x8, #0x30
100490474:      b   0x10049047c <_js_array_grow+0x13c>
100490478:      add x8, x8, #0x60
10049047c:      ldrb    w8, [x8, #0x19]
100490480:      cmp w8, #0xff
100490484:      b.ne    0x100490494 <_js_array_grow+0x154>
100490488:      mov x0, x19
10049048c:      bl  0x100889a20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
100490490:      and w8, w0, #0xff
100490494:      cbz w8, 0x1004904b4 <_js_array_grow+0x174>
100490498:      ldurb   w8, [x19, #-0x8]
10049049c:      ldurb   w9, [x19, #-0x7]
1004904a0:      mov w10, #0x82              ; =130
1004904a4:      and w9, w9, w10
1004904a8:      cmp w9, #0x2
1004904ac:      ccmp    w8, #0x1, #0x0, eq
1004904b0:      b.eq    0x1004906d8 <_js_array_grow+0x398>
1004904b4:      mov x0, x19
1004904b8:      bl  0x10044b604 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1004904bc:      mov x8, x0
1004904c0:      cbz x0, 0x10049055c <_js_array_grow+0x21c>
1004904c4:      ldrb    w9, [x8]
1004904c8:      cmp w9, #0x1
1004904cc:      b.ne    0x1004905ec <_js_array_grow+0x2ac>
1004904d0:      ldrsb   w9, [x8, #0x1]
1004904d4:      mov x0, x8
1004904d8:      tbz w9, #0x1f, 0x100490630 <_js_array_grow+0x2f0>
1004904dc:      mov x20, x8
1004904e0:      ldr x19, [x8, #0x8]
1004904e4:      mov x0, x19
1004904e8:      bl  0x10044b604 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1004904ec:      mov x1, x22
1004904f0:      cbz x0, 0x1004906b4 <_js_array_grow+0x374>
1004904f4:      ldrb    w8, [x0]
1004904f8:      cmp w8, #0x1
1004904fc:      b.ne    0x1004906b4 <_js_array_grow+0x374>
100490500:      ldrsb   w8, [x0, #0x1]
100490504:      tbz w8, #0x1f, 0x1004905e4 <_js_array_grow+0x2a4>
100490508:      mov w21, #0x1               ; =1
10049050c:      ldr x19, [x0, #0x8]
100490510:      mov x0, x19
100490514:      bl  0x10044b604 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100490518:      mov x1, x22
10049051c:      cbz x0, 0x1004906b4 <_js_array_grow+0x374>
100490520:      ldrb    w8, [x0]
100490524:      cmp w8, #0x1
100490528:      b.ne    0x1004906b4 <_js_array_grow+0x374>
10049052c:      cmp w21, #0x3f
100490530:      b.hi    0x1004906b4 <_js_array_grow+0x374>
100490534:      add w21, w21, #0x1
100490538:      ldrsb   w8, [x0, #0x1]
10049053c:      tbnz    w8, #0x1f, 0x10049050c <_js_array_grow+0x1cc>
100490540:      mov x8, x20
100490544:      str x19, [x20, #0x8]
100490548:      ldrb    w9, [x20, #0x1]
10049054c:      orr w9, w9, #0x80
100490550:      strb    w9, [x20, #0x1]
100490554:      ldrb    w9, [x0]
100490558:      b   0x1004905f4 <_js_array_grow+0x2b4>
10049055c:      mov x20, x8
100490560:      adrp    x8, 0x10117c000 <_out_buf+0x3dc8>
100490564:      add x8, x8, #0x814
100490568:      ldaprb  w8, [x8]
10049056c:      cbz w8, 0x10049059c <_js_array_grow+0x25c>
100490570:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
100490574:      add x8, x8, #0x238
100490578:      ldapr   x9, [x8]
10049057c:      cmp x9, x19
100490580:      b.hi    0x10049059c <_js_array_grow+0x25c>
100490584:      ldapur  x8, [x8, #0x8]
100490588:      cmp x8, x19
10049058c:      b.lo    0x10049059c <_js_array_grow+0x25c>
100490590:      mov x0, x19
100490594:      bl  0x100192b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100490598:      tbnz    w0, #0x0, 0x1004905e0 <_js_array_grow+0x2a0>
10049059c:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x138>
1004905a0:      add x8, x8, #0xb78
1004905a4:      ldaprb  w8, [x8]
1004905a8:      mov x1, x22
1004905ac:      cbz w8, 0x1004906b4 <_js_array_grow+0x374>
1004905b0:      adrp    x8, 0x101131000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5timer16TIMER_REF_STATES+0x28>
1004905b4:      add x8, x8, #0xd88
1004905b8:      ldapr   x9, [x8]
1004905bc:      cmp x9, x19
1004905c0:      b.hi    0x1004906b4 <_js_array_grow+0x374>
1004905c4:      ldapur  x8, [x8, #0x8]
1004905c8:      cmp x8, x19
1004905cc:      b.lo    0x1004906b4 <_js_array_grow+0x374>
1004905d0:      mov x0, x19
1004905d4:      bl  0x1008e35a8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
1004905d8:      mov x1, x22
1004905dc:      tbz w0, #0x0, 0x1004906b4 <_js_array_grow+0x374>
1004905e0:      mov x0, #0x0                ; =0
1004905e4:      mov x8, x20
1004905e8:      b   0x100490630 <_js_array_grow+0x2f0>
1004905ec:      mov x0, x8
1004905f0:      mov x1, x22
1004905f4:      cmp w9, #0x1
1004905f8:      b.eq    0x100490630 <_js_array_grow+0x2f0>
1004905fc:      cmp w9, #0x9
100490600:      b.ne    0x1004906b4 <_js_array_grow+0x374>
100490604:      ldr w8, [x19, #0x4]
100490608:      mov w9, #0x5841             ; =22593
10049060c:      movk    w9, #0x4c5a, lsl #16
100490610:      cmp w8, w9
100490614:      b.ne    0x1004906b4 <_js_array_grow+0x374>
100490618:      mov x0, x19
10049061c:      bl  0x1002ac118 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape22force_materialize_lazy>
100490620:      mov x1, x22
100490624:      cbz x0, 0x1004906b4 <_js_array_grow+0x374>
100490628:      mov x19, x0
10049062c:      b   0x1004906fc <_js_array_grow+0x3bc>
100490630:      ldp w10, w9, [x19]
100490634:      cmp w10, w9
100490638:      b.ls    0x100490658 <_js_array_grow+0x318>
10049063c:      cbz x8, 0x100490668 <_js_array_grow+0x328>
100490640:      ldr w8, [x0, #0x4]
100490644:      lsl x9, x9, #3
100490648:      add x9, x9, #0x10
10049064c:      cmp x9, x8
100490650:      b.ne    0x100490668 <_js_array_grow+0x328>
100490654:      b   0x1004906fc <_js_array_grow+0x3bc>
100490658:      mov w8, #0xe100             ; =57600
10049065c:      movk    w8, #0x5f5, lsl #16
100490660:      cmp w10, w8
100490664:      b.ls    0x1004906fc <_js_array_grow+0x3bc>
100490668:      adrp    x8, 0x10117c000 <_out_buf+0x3dc8>
10049066c:      add x8, x8, #0x814
100490670:      ldaprb  w8, [x8]
100490674:      cbz w8, 0x1004906a4 <_js_array_grow+0x364>
100490678:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
10049067c:      add x8, x8, #0x238
100490680:      ldapr   x9, [x8]
100490684:      cmp x9, x19
100490688:      b.hi    0x1004906a4 <_js_array_grow+0x364>
10049068c:      ldapur  x8, [x8, #0x8]
100490690:      cmp x8, x19
100490694:      b.lo    0x1004906a4 <_js_array_grow+0x364>
100490698:      mov x0, x19
10049069c:      bl  0x100192b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
1004906a0:      tbnz    w0, #0x0, 0x1004906fc <_js_array_grow+0x3bc>
1004906a4:      mov x0, x19
1004906a8:      bl  0x10041def0 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray23lookup_typed_array_kind>
1004906ac:      mov x1, x22
1004906b0:      tbnz    w0, #0x0, 0x1004906fc <_js_array_grow+0x3bc>
1004906b4:      mov x0, x1
1004906b8:      ldp x29, x30, [sp, #0xa0]
1004906bc:      ldp x20, x19, [sp, #0x90]
1004906c0:      ldp x22, x21, [sp, #0x80]
1004906c4:      ldp x24, x23, [sp, #0x70]
1004906c8:      ldp x26, x25, [sp, #0x60]
1004906cc:      ldp x28, x27, [sp, #0x50]
1004906d0:      add sp, sp, #0xb0
1004906d4:      b   0x1002f710c <_js_array_alloc>
1004906d8:      ldr w8, [x19]
1004906dc:      mov w9, #0xe100             ; =57600
1004906e0:      movk    w9, #0x5f5, lsl #16
1004906e4:      orr w9, w9, #0x1
1004906e8:      cmp w8, w9
1004906ec:      b.hs    0x1004904b4 <_js_array_grow+0x174>
1004906f0:      ldr w9, [x19, #0x4]
1004906f4:      cmp w8, w9
1004906f8:      b.hi    0x1004904b4 <_js_array_grow+0x174>
1004906fc:      mov x0, x19
100490700:      bl  0x10043e520 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header18array_object_flags>
100490704:      tst w0, #0x6
100490708:      b.ne    0x100490f00 <_js_array_grow+0xbc0>
10049070c:      mov x0, x19
100490710:      bl  0x10043e520 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header18array_object_flags>
100490714:      tbnz    w0, #0x0, 0x100490f00 <_js_array_grow+0xbc0>
100490718:      adrp    x26, 0x101130000 <_perry_global_baseline_worker_ts__1>
10049071c:      add x26, x26, #0x4e8
100490720:      ldr x8, [x26]
100490724:      cmn x8, #0x1
100490728:      b.eq    0x10049075c <_js_array_grow+0x41c>
10049072c:      mrs x9, TPIDRRO_EL0
100490730:      and x9, x9, #0xfffffffffffffff8
100490734:      ldr x8, [x9, x8, lsl #3]
100490738:      cbz x8, 0x10049075c <_js_array_grow+0x41c>
10049073c:      ldr x8, [x8, #0x19e8]
100490740:      cbz x8, 0x10049075c <_js_array_grow+0x41c>
100490744:      ldr x9, [x8]
100490748:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10049074c:      cmp x9, x10
100490750:      b.hs    0x1004910dc <_js_array_grow+0xd9c>
100490754:      ldr x20, [x8, #0x18]
100490758:      b   0x10049076c <_js_array_grow+0x42c>
10049075c:      adrp    x0, 0x1010b0000 <_anon.e80e0661ef5195a01080c4f807135b03.1285+0x98>
100490760:      add x0, x0, #0xfd0
100490764:      bl  0x10013596c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
100490768:      mov x20, x0
10049076c:      str x20, [sp, #0x20]
100490770:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100490774:      stp x19, x8, [sp, #0x40]
100490778:      mov w8, #0x1                ; =1
10049077c:      str x8, [sp, #0x38]
100490780:      add x0, sp, #0x38
100490784:      bl  0x10041da38 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
100490788:      str x0, [sp, #0x28]
10049078c:      ldr w23, [x19, #0x4]
100490790:      cmp w22, w23
100490794:      b.ls    0x100490854 <_js_array_grow+0x514>
100490798:      mov x21, x0
10049079c:      lsl w8, w23, #1
1004907a0:      cmp w22, w8
1004907a4:      csel    w24, w22, w8, hi
1004907a8:      ubfiz   x22, x24, #3, #32
1004907ac:      ldurb   w8, [x19, #-0x7]
1004907b0:      tbnz    w8, #0x5, 0x100490a70 <_js_array_grow+0x730>
1004907b4:      ldr x8, [x26]
1004907b8:      cmn x8, #0x1
1004907bc:      b.eq    0x100490a48 <_js_array_grow+0x708>
1004907c0:      mrs x9, TPIDRRO_EL0
1004907c4:      and x9, x9, #0xfffffffffffffff8
1004907c8:      ldr x0, [x9, x8, lsl #3]
1004907cc:      cbz x0, 0x100490a48 <_js_array_grow+0x708>
1004907d0:      lsr x1, x19, #20
1004907d4:      ldr x8, [x0, #0x10]
1004907d8:      ldrb    w9, [x8, #0x28]
1004907dc:      tbz w9, #0x0, 0x1004907fc <_js_array_grow+0x4bc>
1004907e0:      ldr x9, [x8, #0x20]
1004907e4:      cmp x9, x1
1004907e8:      b.ne    0x1004907fc <_js_array_grow+0x4bc>
1004907ec:      ldp x9, x10, [x8]
1004907f0:      cmp x9, x19
1004907f4:      ccmp    x10, x19, #0x0, ls
1004907f8:      b.hi    0x1004908e8 <_js_array_grow+0x5a8>
1004907fc:      ldrb    w9, [x8, #0x58]
100490800:      cbz w9, 0x100490820 <_js_array_grow+0x4e0>
100490804:      ldr x9, [x8, #0x50]
100490808:      cmp x9, x1
10049080c:      b.ne    0x100490820 <_js_array_grow+0x4e0>
100490810:      ldp x9, x10, [x8, #0x30]
100490814:      cmp x9, x19
100490818:      ccmp    x10, x19, #0x0, ls
10049081c:      b.hi    0x1004908e4 <_js_array_grow+0x5a4>
100490820:      ldrb    w9, [x8, #0x88]
100490824:      cbz w9, 0x1004908b0 <_js_array_grow+0x570>
100490828:      ldr x9, [x8, #0x80]
10049082c:      cmp x9, x1
100490830:      b.ne    0x1004908b0 <_js_array_grow+0x570>
100490834:      ldr x9, [x8, #0x60]
100490838:      cmp x9, x19
10049083c:      b.hi    0x1004908b0 <_js_array_grow+0x570>
100490840:      ldr x9, [x8, #0x68]
100490844:      cmp x9, x19
100490848:      b.ls    0x1004908b0 <_js_array_grow+0x570>
10049084c:      add x8, x8, #0x60
100490850:      b   0x1004908e8 <_js_array_grow+0x5a8>
100490854:      ldr x8, [x26]
100490858:      cmn x8, #0x1
10049085c:      b.eq    0x100490ef0 <_js_array_grow+0xbb0>
100490860:      mrs x9, TPIDRRO_EL0
100490864:      and x9, x9, #0xfffffffffffffff8
100490868:      ldr x8, [x9, x8, lsl #3]
10049086c:      cbz x8, 0x100490ef0 <_js_array_grow+0xbb0>
100490870:      ldr x8, [x8, #0x19e8]
100490874:      cbz x8, 0x100490ef0 <_js_array_grow+0xbb0>
100490878:      ldr x9, [x8]
10049087c:      cbnz    x9, 0x100490ee4 <_js_array_grow+0xba4>
100490880:      ldr x9, [x8, #0x18]
100490884:      cmp x20, x9
100490888:      b.hi    0x100490890 <_js_array_grow+0x550>
10049088c:      str x20, [x8, #0x18]
100490890:      str xzr, [x8]
100490894:      b   0x100490f00 <_js_array_grow+0xbc0>
100490898:      bl  0x100ccaa2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
10049089c:      lsr x1, x19, #20
1004908a0:      ldr x8, [x0, #0x10]
1004908a4:      ldrb    w9, [x8, #0x28]
1004908a8:      tbnz    w9, #0x0, 0x1004903e4 <_js_array_grow+0xa4>
1004908ac:      b   0x100490400 <_js_array_grow+0xc0>
1004908b0:      ldrb    w9, [x8, #0xb8]
1004908b4:      cbz w9, 0x1004908f4 <_js_array_grow+0x5b4>
1004908b8:      ldr x9, [x8, #0xb0]
1004908bc:      cmp x9, x1
1004908c0:      b.ne    0x1004908f4 <_js_array_grow+0x5b4>
1004908c4:      ldr x9, [x8, #0x90]
1004908c8:      cmp x9, x19
1004908cc:      b.hi    0x1004908f4 <_js_array_grow+0x5b4>
1004908d0:      ldr x9, [x8, #0x98]
1004908d4:      cmp x9, x19
1004908d8:      b.ls    0x1004908f4 <_js_array_grow+0x5b4>
1004908dc:      add x8, x8, #0x90
1004908e0:      b   0x1004908e8 <_js_array_grow+0x5a8>
1004908e4:      add x8, x8, #0x30
1004908e8:      ldrb    w8, [x8, #0x19]
1004908ec:      cmp w8, #0xff
1004908f0:      b.ne    0x100490900 <_js_array_grow+0x5c0>
1004908f4:      mov x0, x19
1004908f8:      bl  0x100889a20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
1004908fc:      and w8, w0, #0xff
100490900:      cmp w8, #0x1
100490904:      b.ne    0x100490a70 <_js_array_grow+0x730>
100490908:      mov w8, #0x3ffe             ; =16382
10049090c:      cmp w24, w8
100490910:      b.hi    0x100490a70 <_js_array_grow+0x730>
100490914:      ldr x8, [x26]
100490918:      cmn x8, #0x1
10049091c:      b.eq    0x100490a60 <_js_array_grow+0x720>
100490920:      mrs x9, TPIDRRO_EL0
100490924:      and x9, x9, #0xfffffffffffffff8
100490928:      ldr x0, [x9, x8, lsl #3]
10049092c:      cbz x0, 0x100490a60 <_js_array_grow+0x720>
100490930:      ldr x8, [x0, #0x28]
100490934:      ldrb    w8, [x8]
100490938:      tbnz    w8, #0x0, 0x100490a70 <_js_array_grow+0x730>
10049093c:      ldr x8, [x26]
100490940:      cmn x8, #0x1
100490944:      b.eq    0x100490c4c <_js_array_grow+0x90c>
100490948:      mrs x9, TPIDRRO_EL0
10049094c:      and x9, x9, #0xfffffffffffffff8
100490950:      ldr x0, [x9, x8, lsl #3]
100490954:      cbz x0, 0x100490c4c <_js_array_grow+0x90c>
100490958:      ldr x25, [x0, #0x8]
10049095c:      ldr x8, [x26]
100490960:      cmn x8, #0x1
100490964:      b.eq    0x100490c60 <_js_array_grow+0x920>
100490968:      mrs x9, TPIDRRO_EL0
10049096c:      and x9, x9, #0xfffffffffffffff8
100490970:      ldr x0, [x9, x8, lsl #3]
100490974:      cbz x0, 0x100490c60 <_js_array_grow+0x920>
100490978:      ldr x19, [x0]
10049097c:      ldr x8, [x25]
100490980:      cbz x8, 0x1004909a4 <_js_array_grow+0x664>
100490984:      ldp x1, x0, [x19, #0x10]
100490988:      cmp x0, x1
10049098c:      b.hs    0x100491140 <_js_array_grow+0xe00>
100490990:      ldr x8, [x19, #0x8]
100490994:      ldr x9, [x25, #0x8]
100490998:      mov w10, #0x30              ; =48
10049099c:      madd    x8, x0, x10, x8
1004909a0:      str x9, [x8, #0x20]
1004909a4:      add x27, x22, #0x10
1004909a8:      ldr x1, [x19, #0x18]
1004909ac:      add x2, x22, #0x10
1004909b0:      mov x0, x19
1004909b4:      bl  0x10041a86c <__RNvMs1_NtNtCs5gMwpk3Cs4e_13perry_runtime5arena5blockNtB5_5Arena15try_block_alloc>
1004909b8:      cmp x0, #0x1
1004909bc:      b.ne    0x100490a70 <_js_array_grow+0x730>
1004909c0:      ldr x8, [x25]
1004909c4:      cbz x8, 0x1004909f4 <_js_array_grow+0x6b4>
1004909c8:      ldp x8, x0, [x19, #0x10]
1004909cc:      cmp x0, x8
1004909d0:      b.hs    0x10049114c <_js_array_grow+0xe0c>
1004909d4:      ldr x8, [x19, #0x8]
1004909d8:      mov w9, #0x30               ; =48
1004909dc:      madd    x8, x0, x9, x8
1004909e0:      ldr x9, [x8, #0x10]
1004909e4:      ldur    q0, [x8, #0x18]
1004909e8:      str x9, [x25]
1004909ec:      ext.16b v0, v0, v0, #0x8
1004909f0:      stur    q0, [x25, #0x8]
1004909f4:      cbz x1, 0x100490a70 <_js_array_grow+0x730>
1004909f8:      mov w8, #0x1                ; =1
1004909fc:      strb    w8, [x1]
100490a00:      ldr x8, [x26]
100490a04:      cmn x8, #0x1
100490a08:      b.eq    0x1004910cc <_js_array_grow+0xd8c>
100490a0c:      mrs x9, TPIDRRO_EL0
100490a10:      and x9, x9, #0xfffffffffffffff8
100490a14:      ldr x0, [x9, x8, lsl #3]
100490a18:      cbz x0, 0x1004910cc <_js_array_grow+0xd8c>
100490a1c:      ldr x8, [x0, #0x30]
100490a20:      ldrb    w8, [x8]
100490a24:      orr w8, w8, #0x2
100490a28:      strb    w8, [x1, #0x1]
100490a2c:      mov x0, x1
100490a30:      mov x19, x1
100490a34:      bl  0x100434c84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier19gc_note_black_birth>
100490a38:      strh    wzr, [x19, #0x2]
100490a3c:      str w27, [x19, #0x4]
100490a40:      add x19, x19, #0x8
100490a44:      b   0x100490a90 <_js_array_grow+0x750>
100490a48:      bl  0x100ccaa2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100490a4c:      lsr x1, x19, #20
100490a50:      ldr x8, [x0, #0x10]
100490a54:      ldrb    w9, [x8, #0x28]
100490a58:      tbnz    w9, #0x0, 0x1004907e0 <_js_array_grow+0x4a0>
100490a5c:      b   0x1004907fc <_js_array_grow+0x4bc>
100490a60:      bl  0x100ccaa2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100490a64:      ldr x8, [x0, #0x28]
100490a68:      ldrb    w8, [x8]
100490a6c:      tbz w8, #0x0, 0x10049093c <_js_array_grow+0x5fc>
100490a70:      add x0, x22, #0x8
100490a74:      mov w1, #0x8                ; =8
100490a78:      mov w2, #0x1                ; =1
100490a7c:      bl  0x100437304 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena10allocators18arena_alloc_gc_old>
100490a80:      mov x19, x0
100490a84:      ldurb   w8, [x0, #-0x7]
100490a88:      orr w8, w8, #0x20
100490a8c:      sturb   w8, [x0, #-0x7]
100490a90:      ldr x8, [x26]
100490a94:      cmn x8, #0x1
100490a98:      b.eq    0x100490afc <_js_array_grow+0x7bc>
100490a9c:      mrs x9, TPIDRRO_EL0
100490aa0:      and x9, x9, #0xfffffffffffffff8
100490aa4:      ldr x8, [x9, x8, lsl #3]
100490aa8:      cbz x8, 0x100490afc <_js_array_grow+0x7bc>
100490aac:      ldr x8, [x8, #0x19e8]
100490ab0:      cbz x8, 0x100490afc <_js_array_grow+0x7bc>
100490ab4:      ldr x9, [x8]
100490ab8:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100490abc:      cmp x9, x10
100490ac0:      b.hs    0x100491104 <_js_array_grow+0xdc4>
100490ac4:      add x10, x9, #0x1
100490ac8:      str x10, [x8]
100490acc:      ldr x10, [x8, #0x18]
100490ad0:      cmp x21, x10
100490ad4:      b.hs    0x100491110 <_js_array_grow+0xdd0>
100490ad8:      ldr x10, [x8, #0x10]
100490adc:      mov w11, #0x18              ; =24
100490ae0:      madd    x10, x21, x11, x10
100490ae4:      ldr x11, [x10]
100490ae8:      cmp x11, #0x1
100490aec:      b.ne    0x100491114 <_js_array_grow+0xdd4>
100490af0:      ldr x21, [x10, #0x8]
100490af4:      str x9, [x8]
100490af8:      b   0x100490b10 <_js_array_grow+0x7d0>
100490afc:      adrp    x0, 0x1010b0000 <_anon.e80e0661ef5195a01080c4f807135b03.1285+0x98>
100490b00:      add x0, x0, #0xfd0
100490b04:      add x1, sp, #0x28
100490b08:      bl  0x100135790 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
100490b0c:      mov x21, x0
100490b10:      lsl x8, x23, #3
100490b14:      add x22, x8, #0x8
100490b18:      mov x0, x19
100490b1c:      mov x1, x21
100490b20:      mov x2, x22
100490b24:      bl  0x100ce43ec <_writev+0x100ce43ec>
100490b28:      str w24, [x19, #0x4]
100490b2c:      sub x8, x24, x23
100490b30:      lsl x2, x8, #3
100490b34:      adrp    x1, 0x100dcc000 <_anon.b822d7b979bdf0233543f470364426b7.1104+0x7f5>
100490b38:      add x1, x1, #0xfe0
100490b3c:      add x0, x19, x22
100490b40:      bl  0x100ce4410 <_writev+0x100ce4410>
100490b44:      ldurh   w8, [x21, #-0x6]
100490b48:      sturh   w8, [x19, #-0x6]
100490b4c:      mov x0, x21
100490b50:      mov x1, x19
100490b54:      bl  0x1001d7100 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout15layout_transfer>
100490b58:      mov x0, x21
100490b5c:      mov x1, x19
100490b60:      bl  0x1002d212c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header35transfer_array_named_property_owner>
100490b64:      mov x0, x21
100490b68:      mov x1, x19
100490b6c:      bl  0x1003b1714 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object16descriptor_state25transfer_descriptor_owner>
100490b70:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7e0>
100490b74:      ldr w8, [x8, #0xb70]
100490b78:      cbz w8, 0x100490b88 <_js_array_grow+0x848>
100490b7c:      mov x0, x19
100490b80:      bl  0x10043b080 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array15header_gc_slots34replay_array_growth_write_barriers>
100490b84:      b   0x100490e80 <_js_array_grow+0xb40>
100490b88:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
100490b8c:      add x8, x8, #0x910
100490b90:      ldapr   x8, [x8]
100490b94:      cbnz    x8, 0x1004910e8 <_js_array_grow+0xda8>
100490b98:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
100490b9c:      ldrb    w8, [x8, #0x918]
100490ba0:      cbz w8, 0x100490e80 <_js_array_grow+0xb40>
100490ba4:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x138>
100490ba8:      ldrb    w8, [x8, #0x688]
100490bac:      cbz w8, 0x100490c14 <_js_array_grow+0x8d4>
100490bb0:      tst x19, #0xffff800000000007
100490bb4:      mov w8, #0x100000           ; =1048576
100490bb8:      ccmp    x19, x8, #0x0, eq
100490bbc:      b.lo    0x100490e80 <_js_array_grow+0xb40>
100490bc0:      ldr x8, [x26]
100490bc4:      cmn x8, #0x1
100490bc8:      b.eq    0x100490c74 <_js_array_grow+0x934>
100490bcc:      mrs x9, TPIDRRO_EL0
100490bd0:      and x9, x9, #0xfffffffffffffff8
100490bd4:      ldr x0, [x9, x8, lsl #3]
100490bd8:      cbz x0, 0x100490c74 <_js_array_grow+0x934>
100490bdc:      lsr x1, x19, #20
100490be0:      ldr x8, [x0, #0x10]
100490be4:      ldrb    w9, [x8, #0x28]
100490be8:      tbz w9, #0x0, 0x100490c88 <_js_array_grow+0x948>
100490bec:      ldr x9, [x8, #0x20]
100490bf0:      cmp x9, x1
100490bf4:      b.ne    0x100490c88 <_js_array_grow+0x948>
100490bf8:      ldr x9, [x8]
100490bfc:      cmp x9, x19
100490c00:      b.hi    0x100490c88 <_js_array_grow+0x948>
100490c04:      ldr x9, [x8, #0x8]
100490c08:      cmp x9, x19
100490c0c:      b.hi    0x100490d20 <_js_array_grow+0x9e0>
100490c10:      b   0x100490c88 <_js_array_grow+0x948>
100490c14:      mov w8, #0xc                ; =12
100490c18:      strb    w8, [sp, #0x38]
100490c1c:      adrp    x8, 0x101131000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5timer16TIMER_REF_STATES+0x28>
100490c20:      add x8, x8, #0xc30
100490c24:      ldapr   x8, [x8]
100490c28:      cbnz    x8, 0x100491124 <_js_array_grow+0xde4>
100490c2c:      adrp    x8, 0x101131000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5timer16TIMER_REF_STATES+0x28>
100490c30:      ldrb    w8, [x8, #0xc38]
100490c34:      cbz w8, 0x100490e80 <_js_array_grow+0xb40>
100490c38:      adrp    x0, 0x1010b1000 <_anon.6cecf1cf78612e316ce91ac4e2c9d1d7.19+0x78>
100490c3c:      add x0, x0, #0x598
100490c40:      add x1, sp, #0x38
100490c44:      bl  0x10012d1b4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc9telemetry20BarrierTraceCountersEE4withNCNvNtB1w_7barrier32bump_write_barrier_trace_counter0uEB1y_>
100490c48:      b   0x100490e80 <_js_array_grow+0xb40>
100490c4c:      bl  0x100ccaa2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100490c50:      ldr x25, [x0, #0x8]
100490c54:      ldr x8, [x26]
100490c58:      cmn x8, #0x1
100490c5c:      b.ne    0x100490968 <_js_array_grow+0x628>
100490c60:      bl  0x100ccaa2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100490c64:      ldr x19, [x0]
100490c68:      ldr x8, [x25]
100490c6c:      cbnz    x8, 0x100490984 <_js_array_grow+0x644>
100490c70:      b   0x1004909a4 <_js_array_grow+0x664>
100490c74:      bl  0x100ccaa2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100490c78:      lsr x1, x19, #20
100490c7c:      ldr x8, [x0, #0x10]
100490c80:      ldrb    w9, [x8, #0x28]
100490c84:      tbnz    w9, #0x0, 0x100490bec <_js_array_grow+0x8ac>
100490c88:      ldrb    w9, [x8, #0x58]
100490c8c:      cbz w9, 0x100490cbc <_js_array_grow+0x97c>
100490c90:      ldr x9, [x8, #0x50]
100490c94:      cmp x9, x1
100490c98:      b.ne    0x100490cbc <_js_array_grow+0x97c>
100490c9c:      ldr x9, [x8, #0x30]
100490ca0:      cmp x9, x19
100490ca4:      b.hi    0x100490cbc <_js_array_grow+0x97c>
100490ca8:      ldr x9, [x8, #0x38]
100490cac:      cmp x9, x19
100490cb0:      b.ls    0x100490cbc <_js_array_grow+0x97c>
100490cb4:      add x8, x8, #0x30
100490cb8:      b   0x100490d20 <_js_array_grow+0x9e0>
100490cbc:      ldrb    w9, [x8, #0x88]
100490cc0:      cbz w9, 0x100490cf0 <_js_array_grow+0x9b0>
100490cc4:      ldr x9, [x8, #0x80]
100490cc8:      cmp x9, x1
100490ccc:      b.ne    0x100490cf0 <_js_array_grow+0x9b0>
100490cd0:      ldr x9, [x8, #0x60]
100490cd4:      cmp x9, x19
100490cd8:      b.hi    0x100490cf0 <_js_array_grow+0x9b0>
100490cdc:      ldr x9, [x8, #0x68]
100490ce0:      cmp x9, x19
100490ce4:      b.ls    0x100490cf0 <_js_array_grow+0x9b0>
100490ce8:      add x8, x8, #0x60
100490cec:      b   0x100490d20 <_js_array_grow+0x9e0>
100490cf0:      ldrb    w9, [x8, #0xb8]
100490cf4:      cbz w9, 0x100490d2c <_js_array_grow+0x9ec>
100490cf8:      ldr x9, [x8, #0xb0]
100490cfc:      cmp x9, x1
100490d00:      b.ne    0x100490d2c <_js_array_grow+0x9ec>
100490d04:      ldr x9, [x8, #0x90]
100490d08:      cmp x9, x19
100490d0c:      b.hi    0x100490d2c <_js_array_grow+0x9ec>
100490d10:      ldr x9, [x8, #0x98]
100490d14:      cmp x9, x19
100490d18:      b.ls    0x100490d2c <_js_array_grow+0x9ec>
100490d1c:      add x8, x8, #0x90
100490d20:      ldrb    w8, [x8, #0x19]
100490d24:      cmp w8, #0xff
100490d28:      b.ne    0x100490d38 <_js_array_grow+0x9f8>
100490d2c:      mov x0, x19
100490d30:      bl  0x100889a20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
100490d34:      and w8, w0, #0xff
100490d38:      cmp w8, #0x3
100490d3c:      b.ne    0x100490e80 <_js_array_grow+0xb40>
100490d40:      ldr x8, [x26]
100490d44:      cmn x8, #0x1
100490d48:      b.eq    0x100490d94 <_js_array_grow+0xa54>
100490d4c:      mrs x9, TPIDRRO_EL0
100490d50:      and x9, x9, #0xfffffffffffffff8
100490d54:      ldr x0, [x9, x8, lsl #3]
100490d58:      cbz x0, 0x100490d94 <_js_array_grow+0xa54>
100490d5c:      lsr x1, x21, #20
100490d60:      ldr x8, [x0, #0x10]
100490d64:      ldrb    w9, [x8, #0x28]
100490d68:      tbz w9, #0x0, 0x100490da8 <_js_array_grow+0xa68>
100490d6c:      ldr x9, [x8, #0x20]
100490d70:      cmp x9, x1
100490d74:      b.ne    0x100490da8 <_js_array_grow+0xa68>
100490d78:      ldr x9, [x8]
100490d7c:      cmp x9, x21
100490d80:      b.hi    0x100490da8 <_js_array_grow+0xa68>
100490d84:      ldr x9, [x8, #0x8]
100490d88:      cmp x9, x21
100490d8c:      b.hi    0x100490e40 <_js_array_grow+0xb00>
100490d90:      b   0x100490da8 <_js_array_grow+0xa68>
100490d94:      bl  0x100ccaa2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100490d98:      lsr x1, x21, #20
100490d9c:      ldr x8, [x0, #0x10]
100490da0:      ldrb    w9, [x8, #0x28]
100490da4:      tbnz    w9, #0x0, 0x100490d6c <_js_array_grow+0xa2c>
100490da8:      ldrb    w9, [x8, #0x58]
100490dac:      cbz w9, 0x100490ddc <_js_array_grow+0xa9c>
100490db0:      ldr x9, [x8, #0x50]
100490db4:      cmp x9, x1
100490db8:      b.ne    0x100490ddc <_js_array_grow+0xa9c>
100490dbc:      ldr x9, [x8, #0x30]
100490dc0:      cmp x9, x21
100490dc4:      b.hi    0x100490ddc <_js_array_grow+0xa9c>
100490dc8:      ldr x9, [x8, #0x38]
100490dcc:      cmp x9, x21
100490dd0:      b.ls    0x100490ddc <_js_array_grow+0xa9c>
100490dd4:      add x8, x8, #0x30
100490dd8:      b   0x100490e40 <_js_array_grow+0xb00>
100490ddc:      ldrb    w9, [x8, #0x88]
100490de0:      cbz w9, 0x100490e10 <_js_array_grow+0xad0>
100490de4:      ldr x9, [x8, #0x80]
100490de8:      cmp x9, x1
100490dec:      b.ne    0x100490e10 <_js_array_grow+0xad0>
100490df0:      ldr x9, [x8, #0x60]
100490df4:      cmp x9, x21
100490df8:      b.hi    0x100490e10 <_js_array_grow+0xad0>
100490dfc:      ldr x9, [x8, #0x68]
100490e00:      cmp x9, x21
100490e04:      b.ls    0x100490e10 <_js_array_grow+0xad0>
100490e08:      add x8, x8, #0x60
100490e0c:      b   0x100490e40 <_js_array_grow+0xb00>
100490e10:      ldrb    w9, [x8, #0xb8]
100490e14:      cbz w9, 0x100490e4c <_js_array_grow+0xb0c>
100490e18:      ldr x9, [x8, #0xb0]
100490e1c:      cmp x9, x1
100490e20:      b.ne    0x100490e4c <_js_array_grow+0xb0c>
100490e24:      ldr x9, [x8, #0x90]
100490e28:      cmp x9, x21
100490e2c:      b.hi    0x100490e4c <_js_array_grow+0xb0c>
100490e30:      ldr x9, [x8, #0x98]
100490e34:      cmp x9, x21
100490e38:      b.ls    0x100490e4c <_js_array_grow+0xb0c>
100490e3c:      add x8, x8, #0x90
100490e40:      ldrb    w8, [x8, #0x19]
100490e44:      cmp w8, #0xff
100490e48:      b.ne    0x100490e58 <_js_array_grow+0xb18>
100490e4c:      mov x0, x21
100490e50:      bl  0x100889a20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
100490e54:      and w8, w0, #0xff
100490e58:      cmp w8, #0x3
100490e5c:      b.ne    0x100490b7c <_js_array_grow+0x83c>
100490e60:      lsr x28, x21, #12
100490e64:      add x8, x22, x21
100490e68:      str x8, [sp, #0x10]
100490e6c:      sub x8, x8, #0x1
100490e70:      lsr x8, x8, #12
100490e74:      str x8, [sp, #0x18]
100490e78:      cmp x28, x8
100490e7c:      b.ls    0x100490f24 <_js_array_grow+0xbe4>
100490e80:      mov x0, x21
100490e84:      bl  0x10044b604 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100490e88:      cbz x0, 0x1004910a0 <_js_array_grow+0xd60>
100490e8c:      ldrb    w8, [x0]
100490e90:      cmp w8, #0x1
100490e94:      b.ne    0x1004910a0 <_js_array_grow+0xd60>
100490e98:      ldrb    w8, [x0, #0x1]
100490e9c:      tbz w8, #0x1, 0x1004910a0 <_js_array_grow+0xd60>
100490ea0:      str x19, [x0, #0x8]
100490ea4:      orr w8, w8, #0x80
100490ea8:      strb    w8, [x0, #0x1]
100490eac:      ldrh    w8, [x0, #0x2]
100490eb0:      orr w8, w8, #0xc000
100490eb4:      strh    w8, [x0, #0x2]
100490eb8:      ldr x8, [x26]
100490ebc:      cmn x8, #0x1
100490ec0:      b.eq    0x100490ef0 <_js_array_grow+0xbb0>
100490ec4:      mrs x9, TPIDRRO_EL0
100490ec8:      and x9, x9, #0xfffffffffffffff8
100490ecc:      ldr x8, [x9, x8, lsl #3]
100490ed0:      cbz x8, 0x100490ef0 <_js_array_grow+0xbb0>
100490ed4:      ldr x8, [x8, #0x19e8]
100490ed8:      cbz x8, 0x100490ef0 <_js_array_grow+0xbb0>
100490edc:      ldr x9, [x8]
100490ee0:      cbz x9, 0x100490880 <_js_array_grow+0x540>
100490ee4:      adrp    x0, 0x1010b1000 <_anon.6cecf1cf78612e316ce91ac4e2c9d1d7.19+0x78>
100490ee8:      add x0, x0, #0x320
100490eec:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100490ef0:      adrp    x0, 0x1010b0000 <_anon.e80e0661ef5195a01080c4f807135b03.1285+0x98>
100490ef4:      add x0, x0, #0xfd0
100490ef8:      add x1, sp, #0x20
100490efc:      bl  0x100135d48 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
100490f00:      mov x0, x19
100490f04:      ldp x29, x30, [sp, #0xa0]
100490f08:      ldp x20, x19, [sp, #0x90]
100490f0c:      ldp x22, x21, [sp, #0x80]
100490f10:      ldp x24, x23, [sp, #0x70]
100490f14:      ldp x26, x25, [sp, #0x60]
100490f18:      ldp x28, x27, [sp, #0x50]
100490f1c:      add sp, sp, #0xb0
100490f20:      ret
100490f24:      mvn x8, x21
100490f28:      add x8, x8, x19
100490f2c:      str x8, [sp, #0x8]
100490f30:      adrp    x23, 0x101131000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5timer16TIMER_REF_STATES+0x28>
100490f34:      add x23, x23, #0xc30
100490f38:      mov w27, #0x6               ; =6
100490f3c:      adrp    x22, 0x101131000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5timer16TIMER_REF_STATES+0x28>
100490f40:      b   0x100490f54 <_js_array_grow+0xc14>
100490f44:      ldr x8, [sp, #0x18]
100490f48:      cmp x28, x8
100490f4c:      add x28, x28, #0x1
100490f50:      b.eq    0x100490e80 <_js_array_grow+0xb40>
100490f54:      str x28, [sp, #0x38]
100490f58:      add x1, sp, #0x38
100490f5c:      adrp    x0, 0x1010b1000 <_anon.6cecf1cf78612e316ce91ac4e2c9d1d7.19+0x78>
100490f60:      add x0, x0, #0x6e0
100490f64:      bl  0x100160740 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3set7HashSetjNtNtCs5gMwpk3Cs4e_13perry_runtime9fast_hash9PtrHasherEEE4withNCNvNtNtB2g_2gc7barrier24dirty_old_page_is_marked0bEB2g_>
100490f68:      cbz w0, 0x100490f44 <_js_array_grow+0xc04>
100490f6c:      lsl x9, x28, #12
100490f70:      cmp x21, x9
100490f74:      csel    x8, x21, x9, hi
100490f78:      add x9, x9, #0x1, lsl #12   ; =0x1000
100490f7c:      ldr x10, [sp, #0x10]
100490f80:      cmp x10, x9
100490f84:      csel    x9, x10, x9, lo
100490f88:      cmp x8, x9
100490f8c:      b.hs    0x100490f44 <_js_array_grow+0xc04>
100490f90:      sub x10, x19, x21
100490f94:      add x8, x10, x8
100490f98:      lsr x25, x8, #12
100490f9c:      ldr x8, [sp, #0x8]
100490fa0:      add x8, x8, x9
100490fa4:      lsr x24, x8, #12
100490fa8:      cmp x25, x24
100490fac:      b.ls    0x100490fc0 <_js_array_grow+0xc80>
100490fb0:      b   0x100490f44 <_js_array_grow+0xc04>
100490fb4:      cmp x25, x24
100490fb8:      add x25, x25, #0x1
100490fbc:      b.hs    0x100490f44 <_js_array_grow+0xc04>
100490fc0:      strb    w27, [sp, #0x38]
100490fc4:      ldapr   x8, [x23]
100490fc8:      cbnz    x8, 0x100490ff4 <_js_array_grow+0xcb4>
100490fcc:      ldrb    w8, [x22, #0xc38]
100490fd0:      cbz w8, 0x100491004 <_js_array_grow+0xcc4>
100490fd4:      add x1, sp, #0x38
100490fd8:      adrp    x0, 0x1010b1000 <_anon.6cecf1cf78612e316ce91ac4e2c9d1d7.19+0x78>
100490fdc:      add x0, x0, #0x598
100490fe0:      bl  0x10012d1b4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc9telemetry20BarrierTraceCountersEE4withNCNvNtB1w_7barrier32bump_write_barrier_trace_counter0uEB1y_>
100490fe4:      ldr x8, [x26]
100490fe8:      cmn x8, #0x1
100490fec:      b.ne    0x100491010 <_js_array_grow+0xcd0>
100490ff0:      b   0x10049103c <_js_array_grow+0xcfc>
100490ff4:      mov x0, x23
100490ff8:      bl  0x100cc5c94 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_trace_enabled0E0zEB1A_>
100490ffc:      ldrb    w8, [x22, #0xc38]
100491000:      cbnz    w8, 0x100490fd4 <_js_array_grow+0xc94>
100491004:      ldr x8, [x26]
100491008:      cmn x8, #0x1
10049100c:      b.eq    0x10049103c <_js_array_grow+0xcfc>
100491010:      mrs x9, TPIDRRO_EL0
100491014:      and x9, x9, #0xfffffffffffffff8
100491018:      ldr x0, [x9, x8, lsl #3]
10049101c:      cbz x0, 0x10049103c <_js_array_grow+0xcfc>
100491020:      and x8, x25, #0xf
100491024:      add x8, x0, x8, lsl #3
100491028:      ldr x8, [x8, #0x88]
10049102c:      cmp x25, x8
100491030:      b.ne    0x100491054 <_js_array_grow+0xd14>
100491034:      mov w8, #0x9                ; =9
100491038:      b   0x100491064 <_js_array_grow+0xd24>
10049103c:      bl  0x100ccaa2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100491040:      and x8, x25, #0xf
100491044:      add x8, x0, x8, lsl #3
100491048:      ldr x8, [x8, #0x88]
10049104c:      cmp x25, x8
100491050:      b.eq    0x100491034 <_js_array_grow+0xcf4>
100491054:      mov x0, x25
100491058:      bl  0x100435a94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier28mark_dirty_old_page_uncached>
10049105c:      tbz w0, #0x0, 0x100490fb4 <_js_array_grow+0xc74>
100491060:      mov w8, #0x7                ; =7
100491064:      strb    w8, [sp, #0x38]
100491068:      ldapr   x8, [x23]
10049106c:      cbnz    x8, 0x10049108c <_js_array_grow+0xd4c>
100491070:      ldrb    w8, [x22, #0xc38]
100491074:      cbz w8, 0x100490fb4 <_js_array_grow+0xc74>
100491078:      add x1, sp, #0x38
10049107c:      adrp    x0, 0x1010b1000 <_anon.6cecf1cf78612e316ce91ac4e2c9d1d7.19+0x78>
100491080:      add x0, x0, #0x598
100491084:      bl  0x10012d1b4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc9telemetry20BarrierTraceCountersEE4withNCNvNtB1w_7barrier32bump_write_barrier_trace_counter0uEB1y_>
100491088:      b   0x100490fb4 <_js_array_grow+0xc74>
10049108c:      mov x0, x23
100491090:      bl  0x100cc5c94 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_trace_enabled0E0zEB1A_>
100491094:      ldrb    w8, [x22, #0xc38]
100491098:      cbnz    w8, 0x100491078 <_js_array_grow+0xd38>
10049109c:      b   0x100490fb4 <_js_array_grow+0xc74>
1004910a0:      str x21, [sp, #0x30]
1004910a4:      add x8, sp, #0x30
1004910a8:      adrp    x9, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
1004910ac:      add x9, x9, #0x288
1004910b0:      stp x8, x9, [sp, #0x38]
1004910b4:      adrp    x0, 0x100dd5000 <_anon.6cecf1cf78612e316ce91ac4e2c9d1d7.883+0xc37>
1004910b8:      add x0, x0, #0xf8f
1004910bc:      adrp    x2, 0x1010b3000 <_anon.6cecf1cf78612e316ce91ac4e2c9d1d7.1138+0x280>
1004910c0:      add x2, x2, #0xf48
1004910c4:      add x1, sp, #0x38
1004910c8:      bl  0x100c987fc <__RNvNtCsjgY6bXVaRmE_4core9panicking9panic_fmt>
1004910cc:      mov x19, x1
1004910d0:      bl  0x100ccaa2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1004910d4:      mov x1, x19
1004910d8:      b   0x100490a1c <_js_array_grow+0x6dc>
1004910dc:      adrp    x0, 0x1010b1000 <_anon.6cecf1cf78612e316ce91ac4e2c9d1d7.19+0x78>
1004910e0:      add x0, x0, #0x80
1004910e4:      bl  0x100c9855c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004910e8:      adrp    x0, 0x101130000 <_perry_global_baseline_worker_ts__1>
1004910ec:      add x0, x0, #0x910
1004910f0:      bl  0x100cc5d84 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
1004910f4:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1004910f8:      ldrb    w8, [x8, #0x918]
1004910fc:      cbnz    w8, 0x100490ba4 <_js_array_grow+0x864>
100491100:      b   0x100490e80 <_js_array_grow+0xb40>
100491104:      adrp    x0, 0x1010b1000 <_anon.6cecf1cf78612e316ce91ac4e2c9d1d7.19+0x78>
100491108:      add x0, x0, #0x38
10049110c:      bl  0x100c9855c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100491110:      bl  0x100cb947c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
100491114:      adrp    x0, 0x100dd2000 <_anon.e80e0661ef5195a01080c4f807135b03.1715+0xa>
100491118:      add x0, x0, #0xd2d
10049111c:      mov w1, #0xb                ; =11
100491120:      bl  0x100cb9444 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
100491124:      adrp    x0, 0x101131000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5timer16TIMER_REF_STATES+0x28>
100491128:      add x0, x0, #0xc30
10049112c:      bl  0x100cc5c94 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_trace_enabled0E0zEB1A_>
100491130:      adrp    x8, 0x101131000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5timer16TIMER_REF_STATES+0x28>
100491134:      ldrb    w8, [x8, #0xc38]
100491138:      cbnz    w8, 0x100490c38 <_js_array_grow+0x8f8>
10049113c:      b   0x100490e80 <_js_array_grow+0xb40>
100491140:      adrp    x2, 0x1010b1000 <_anon.6cecf1cf78612e316ce91ac4e2c9d1d7.19+0x78>
100491144:      add x2, x2, #0x740
100491148:      bl  0x100c9868c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
10049114c:      adrp    x2, 0x1010b1000 <_anon.6cecf1cf78612e316ce91ac4e2c9d1d7.19+0x78>
100491150:      add x2, x2, #0x758
100491154:      mov x1, x8
100491158:      bl  0x100c9868c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
