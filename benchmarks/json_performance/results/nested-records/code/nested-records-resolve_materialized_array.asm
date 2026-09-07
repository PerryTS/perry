/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/nested-records-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008e4580 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array>:
1008e4580:      stp x26, x25, [sp, #-0x50]!
1008e4584:      stp x24, x23, [sp, #0x10]
1008e4588:      stp x22, x21, [sp, #0x20]
1008e458c:      stp x20, x19, [sp, #0x30]
1008e4590:      stp x29, x30, [sp, #0x40]
1008e4594:      add x29, sp, #0x40
1008e4598:      mov x19, x0
1008e459c:      ldr x23, [x19, #0x20]!
1008e45a0:      cbz x23, 0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e45a4:      lsr x8, x23, #51
1008e45a8:      mov x21, x23
1008e45ac:      cmp x8, #0xfff
1008e45b0:      b.lo    0x1008e45c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x48>
1008e45b4:      mov w8, #0x7ffc             ; =32764
1008e45b8:      cmp x8, x23, lsr #48
1008e45bc:      b.eq    0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e45c0:      ands    x21, x23, #0xffffffffffff
1008e45c4:      b.eq    0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e45c8:      and x8, x21, #0xfffffffffff00000
1008e45cc:      lsr x9, x21, #47
1008e45d0:      cmp x9, #0x0
1008e45d4:      ccmp    x8, #0x0, #0x4, eq
1008e45d8:      b.eq    0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e45dc:      tst x21, #0x3
1008e45e0:      ccmp    x21, #0x7, #0x0, eq
1008e45e4:      mov x20, x0
1008e45e8:      b.ls    0x1008e46f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
1008e45ec:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1008e45f0:      add x8, x8, #0x360
1008e45f4:      ldr x8, [x8]
1008e45f8:      cmn x8, #0x1
1008e45fc:      b.eq    0x1008e49fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x47c>
1008e4600:      mrs x9, TPIDRRO_EL0
1008e4604:      and x9, x9, #0xfffffffffffffff8
1008e4608:      ldr x8, [x9, x8, lsl #3]
1008e460c:      cbz x8, 0x1008e49fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x47c>
1008e4610:      lsr x1, x21, #20
1008e4614:      ldr x8, [x8, #0x10]
1008e4618:      ldrb    w9, [x8, #0x28]
1008e461c:      tbz w9, #0x0, 0x1008e463c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
1008e4620:      ldr x9, [x8, #0x20]
1008e4624:      cmp x9, x1
1008e4628:      b.ne    0x1008e463c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
1008e462c:      ldp x9, x10, [x8]
1008e4630:      cmp x9, x21
1008e4634:      ccmp    x10, x21, #0x0, ls
1008e4638:      b.hi    0x1008e46b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
1008e463c:      ldrb    w9, [x8, #0x58]
1008e4640:      cbz w9, 0x1008e4660 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xe0>
1008e4644:      ldr x9, [x8, #0x50]
1008e4648:      cmp x9, x1
1008e464c:      b.ne    0x1008e4660 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xe0>
1008e4650:      ldp x9, x10, [x8, #0x30]
1008e4654:      cmp x9, x21
1008e4658:      ccmp    x10, x21, #0x0, ls
1008e465c:      b.hi    0x1008e46ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x12c>
1008e4660:      ldrb    w9, [x8, #0x88]
1008e4664:      cbz w9, 0x1008e4684 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x104>
1008e4668:      ldr x9, [x8, #0x80]
1008e466c:      cmp x9, x1
1008e4670:      b.ne    0x1008e4684 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x104>
1008e4674:      ldp x9, x10, [x8, #0x60]
1008e4678:      cmp x9, x21
1008e467c:      ccmp    x10, x21, #0x0, ls
1008e4680:      b.hi    0x1008e46b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x134>
1008e4684:      ldrb    w9, [x8, #0xb8]
1008e4688:      cbz w9, 0x1008e46c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
1008e468c:      ldr x9, [x8, #0xb0]
1008e4690:      cmp x9, x1
1008e4694:      b.ne    0x1008e46c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
1008e4698:      ldp x9, x10, [x8, #0x90]!
1008e469c:      cmp x9, x21
1008e46a0:      ccmp    x10, x21, #0x0, ls
1008e46a4:      b.hi    0x1008e46b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
1008e46a8:      b   0x1008e46c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
1008e46ac:      add x8, x8, #0x30
1008e46b0:      b   0x1008e46b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
1008e46b4:      add x8, x8, #0x60
1008e46b8:      ldrb    w8, [x8, #0x19]
1008e46bc:      cmp w8, #0xff
1008e46c0:      b.ne    0x1008e46d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x158>
1008e46c4:      mov x0, x21
1008e46c8:      bl  0x1002bbee0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
1008e46cc:      mov x8, x0
1008e46d0:      mov x0, x20
1008e46d4:      and w8, w8, #0xff
1008e46d8:      cbz w8, 0x1008e46f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
1008e46dc:      ldurb   w8, [x21, #-0x8]
1008e46e0:      ldurb   w9, [x21, #-0x7]
1008e46e4:      mov w10, #0x82              ; =130
1008e46e8:      and w9, w9, w10
1008e46ec:      cmp w9, #0x2
1008e46f0:      ccmp    w8, #0x1, #0x0, eq
1008e46f4:      b.eq    0x1008e4818 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x298>
1008e46f8:      mov x0, x21
1008e46fc:      bl  0x1008d5848 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008e4700:      cbz x0, 0x1008e4730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x1b0>
1008e4704:      ldrb    w9, [x0]
1008e4708:      cmp w9, #0x1
1008e470c:      b.ne    0x1008e47d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x254>
1008e4710:      ldrsb   w8, [x0, #0x1]
1008e4714:      tbnz    w8, #0x1f, 0x1008e4844 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x2c4>
1008e4718:      mov x8, x0
1008e471c:      mov x0, x20
1008e4720:      ldp w10, w9, [x21]
1008e4724:      cmp w10, w9
1008e4728:      b.hi    0x1008e48dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x35c>
1008e472c:      b   0x1008e48f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x378>
1008e4730:      adrp    x8, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
1008e4734:      add x8, x8, #0xb34
1008e4738:      ldaprb  w8, [x8]
1008e473c:      cbz w8, 0x1008e4780 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
1008e4740:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
1008e4744:      add x8, x8, #0xef8
1008e4748:      ldapr   x9, [x8]
1008e474c:      cmp x9, x21
1008e4750:      b.hi    0x1008e4780 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
1008e4754:      ldapur  x8, [x8, #0x8]
1008e4758:      cmp x8, x21
1008e475c:      b.lo    0x1008e4780 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
1008e4760:      mov x24, x0
1008e4764:      mov x0, x21
1008e4768:      bl  0x100a15a18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
1008e476c:      mov x8, x0
1008e4770:      mov x0, x24
1008e4774:      tbz w8, #0x0, 0x1008e4780 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
1008e4778:      mov x8, #0x0                ; =0
1008e477c:      b   0x1008e48c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x348>
1008e4780:      adrp    x8, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
1008e4784:      add x8, x8, #0x620
1008e4788:      ldaprb  w8, [x8]
1008e478c:      cbz w8, 0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e4790:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
1008e4794:      add x8, x8, #0xc08
1008e4798:      ldapr   x9, [x8]
1008e479c:      cmp x9, x21
1008e47a0:      b.hi    0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e47a4:      ldapur  x8, [x8, #0x8]
1008e47a8:      cmp x8, x21
1008e47ac:      b.lo    0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e47b0:      mov x24, x0
1008e47b4:      mov x0, x21
1008e47b8:      bl  0x100948dac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
1008e47bc:      mov x9, x0
1008e47c0:      mov x8, #0x0                ; =0
1008e47c4:      mov x22, #0x0               ; =0
1008e47c8:      mov x0, x20
1008e47cc:      tbnz    w9, #0x0, 0x1008e48cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x34c>
1008e47d0:      b   0x1008e49e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
1008e47d4:      mov x24, x0
1008e47d8:      mov x8, x0
1008e47dc:      cmp w9, #0x1
1008e47e0:      b.eq    0x1008e48c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x348>
1008e47e4:      cmp w9, #0x9
1008e47e8:      b.ne    0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e47ec:      ldr w8, [x21, #0x4]
1008e47f0:      mov w9, #0x5841             ; =22593
1008e47f4:      movk    w9, #0x4c5a, lsl #16
1008e47f8:      cmp w8, w9
1008e47fc:      b.ne    0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e4800:      mov x0, x21
1008e4804:      bl  0x1008ae598 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape22force_materialize_lazy>
1008e4808:      mov x22, x0
1008e480c:      mov x0, x20
1008e4810:      cbnz    x22, 0x1008e499c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
1008e4814:      b   0x1008e49e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
1008e4818:      ldr w8, [x21]
1008e481c:      mov w9, #0xe100             ; =57600
1008e4820:      movk    w9, #0x5f5, lsl #16
1008e4824:      orr w9, w9, #0x1
1008e4828:      cmp w8, w9
1008e482c:      b.hs    0x1008e46f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
1008e4830:      ldr w9, [x21, #0x4]
1008e4834:      cmp w8, w9
1008e4838:      b.hi    0x1008e46f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
1008e483c:      mov x22, x21
1008e4840:      b   0x1008e499c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
1008e4844:      mov x24, x0
1008e4848:      ldr x21, [x0, #0x8]
1008e484c:      mov x0, x21
1008e4850:      bl  0x1008d5848 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008e4854:      cbz x0, 0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e4858:      mov x8, x0
1008e485c:      ldrb    w9, [x0]
1008e4860:      cmp w9, #0x1
1008e4864:      b.ne    0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e4868:      ldrsb   w9, [x8, #0x1]
1008e486c:      tbz w9, #0x1f, 0x1008e471c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x19c>
1008e4870:      mov w25, #0x1               ; =1
1008e4874:      ldr x21, [x8, #0x8]
1008e4878:      mov x0, x21
1008e487c:      bl  0x1008d5848 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008e4880:      cbz x0, 0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e4884:      mov x8, x0
1008e4888:      mov x22, #0x0               ; =0
1008e488c:      ldrb    w9, [x0]
1008e4890:      cmp w9, #0x1
1008e4894:      b.ne    0x1008e49e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
1008e4898:      cmp w25, #0x3f
1008e489c:      b.hi    0x1008e49e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
1008e48a0:      add w25, w25, #0x1
1008e48a4:      ldrsb   w9, [x8, #0x1]
1008e48a8:      tbnz    w9, #0x1f, 0x1008e4874 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x2f4>
1008e48ac:      str x21, [x24, #0x8]
1008e48b0:      ldrb    w10, [x24, #0x1]
1008e48b4:      orr w10, w10, #0x80
1008e48b8:      strb    w10, [x24, #0x1]
1008e48bc:      ldrb    w9, [x8]
1008e48c0:      cmp w9, #0x1
1008e48c4:      b.ne    0x1008e47e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x264>
1008e48c8:      mov x0, x20
1008e48cc:      ldp w10, w9, [x21]
1008e48d0:      cmp w10, w9
1008e48d4:      b.ls    0x1008e48f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x378>
1008e48d8:      cbz x24, 0x1008e490c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x38c>
1008e48dc:      ldr w8, [x8, #0x4]
1008e48e0:      ubfiz   x9, x9, #3, #32
1008e48e4:      add x9, x9, #0x10
1008e48e8:      mov x22, x21
1008e48ec:      cmp x9, x8
1008e48f0:      b.eq    0x1008e499c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
1008e48f4:      b   0x1008e490c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x38c>
1008e48f8:      mov w8, #0xe100             ; =57600
1008e48fc:      movk    w8, #0x5f5, lsl #16
1008e4900:      mov x22, x21
1008e4904:      cmp w10, w8
1008e4908:      b.ls    0x1008e499c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
1008e490c:      adrp    x8, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
1008e4910:      add x8, x8, #0xb34
1008e4914:      ldaprb  w8, [x8]
1008e4918:      cbz w8, 0x1008e4954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
1008e491c:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
1008e4920:      add x8, x8, #0xef8
1008e4924:      ldapr   x9, [x8]
1008e4928:      cmp x9, x21
1008e492c:      b.hi    0x1008e4954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
1008e4930:      ldapur  x8, [x8, #0x8]
1008e4934:      cmp x8, x21
1008e4938:      b.lo    0x1008e4954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
1008e493c:      mov x0, x21
1008e4940:      bl  0x100a15a18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
1008e4944:      tbz w0, #0x0, 0x1008e4954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
1008e4948:      mov x0, x20
1008e494c:      mov x22, x21
1008e4950:      b   0x1008e499c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
1008e4954:      adrp    x8, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
1008e4958:      add x8, x8, #0x620
1008e495c:      ldaprb  w8, [x8]
1008e4960:      cbz w8, 0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e4964:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
1008e4968:      add x8, x8, #0xc08
1008e496c:      ldapr   x9, [x8]
1008e4970:      cmp x21, x9
1008e4974:      b.lo    0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e4978:      ldapur  x8, [x8, #0x8]
1008e497c:      cmp x21, x8
1008e4980:      b.hi    0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e4984:      mov x0, x21
1008e4988:      bl  0x100948dac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
1008e498c:      mov x8, x0
1008e4990:      mov x0, x20
1008e4994:      mov x22, x21
1008e4998:      tbz w8, #0x0, 0x1008e49dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1008e499c:      cmp x22, x23
1008e49a0:      b.eq    0x1008e4a78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
1008e49a4:      str x22, [x0, #0x20]
1008e49a8:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
1008e49ac:      add x8, x8, #0x58
1008e49b0:      ldapr   x8, [x8]
1008e49b4:      cbnz    x8, 0x1008e4a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x49c>
1008e49b8:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
1008e49bc:      ldrb    w8, [x8, #0x60]
1008e49c0:      tbz w8, #0x0, 0x1008e4a38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4b8>
1008e49c4:      mov x0, x20
1008e49c8:      mov x1, x19
1008e49cc:      mov x2, x22
1008e49d0:      mov w3, #0x0                ; =0
1008e49d4:      bl  0x1005b943c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
1008e49d8:      b   0x1008e4a50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4d0>
1008e49dc:      mov x22, #0x0               ; =0
1008e49e0:      mov x0, x22
1008e49e4:      ldp x29, x30, [sp, #0x40]
1008e49e8:      ldp x20, x19, [sp, #0x30]
1008e49ec:      ldp x22, x21, [sp, #0x20]
1008e49f0:      ldp x24, x23, [sp, #0x10]
1008e49f4:      ldp x26, x25, [sp], #0x50
1008e49f8:      ret
1008e49fc:      bl  0x100caf8ac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1008e4a00:      mov x8, x0
1008e4a04:      mov x0, x20
1008e4a08:      lsr x1, x21, #20
1008e4a0c:      ldr x8, [x8, #0x10]
1008e4a10:      ldrb    w9, [x8, #0x28]
1008e4a14:      tbnz    w9, #0x0, 0x1008e4620 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xa0>
1008e4a18:      b   0x1008e463c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
1008e4a1c:      adrp    x0, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
1008e4a20:      add x0, x0, #0x58
1008e4a24:      bl  0x100cd0144 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
1008e4a28:      mov x0, x20
1008e4a2c:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
1008e4a30:      ldrb    w8, [x8, #0x60]
1008e4a34:      tbnz    w8, #0x0, 0x1008e49c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x444>
1008e4a38:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7fb58>
1008e4a3c:      add x8, x8, #0x9c4
1008e4a40:      ldr w8, [x8]
1008e4a44:      cbz w8, 0x1008e4a54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4d4>
1008e4a48:      mov x0, x22
1008e4a4c:      bl  0x1005ba638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
1008e4a50:      mov x0, x20
1008e4a54:      ldr x8, [x19]
1008e4a58:      cbz x8, 0x1008e4a78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
1008e4a5c:      ldr x8, [x0, #0x10]
1008e4a60:      cbz x8, 0x1008e4a78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
1008e4a64:      mov x0, x20
1008e4a68:      bl  0x100949d74 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime15json_tape_store7release>
1008e4a6c:      mov x0, x20
1008e4a70:      str xzr, [x20, #0x10]
1008e4a74:      str wzr, [x20, #0xc]
1008e4a78:      ldr w8, [x22]
1008e4a7c:      str w8, [x0]
1008e4a80:      b   0x1008e49e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
