/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001009245cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth>:
1009245cc:      sub sp, sp, #0xf0
1009245d0:      stp d9, d8, [sp, #0x80]
1009245d4:      stp x28, x27, [sp, #0x90]
1009245d8:      stp x26, x25, [sp, #0xa0]
1009245dc:      stp x24, x23, [sp, #0xb0]
1009245e0:      stp x22, x21, [sp, #0xc0]
1009245e4:      stp x20, x19, [sp, #0xd0]
1009245e8:      stp x29, x30, [sp, #0xe0]
1009245ec:      add x29, sp, #0xe0
1009245f0:      cmp w2, #0x3e9
1009245f4:      b.hs    0x100926518 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f4c>
1009245f8:      mov x20, x2
1009245fc:      mov x19, x1
100924600:      mov x22, x0
100924604:      lsr x8, x0, #51
100924608:      cmp x8, #0xfff
10092460c:      b.lo    0x100924624 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x58>
100924610:      mov w8, #0x7ffc             ; =32764
100924614:      cmp x8, x22, lsr #48
100924618:      b.eq    0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
10092461c:      ands    x22, x22, #0xffffffffffff
100924620:      b.eq    0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100924624:      and x8, x22, #0xfffffffffff00000
100924628:      lsr x9, x22, #47
10092462c:      cmp x9, #0x0
100924630:      ccmp    x8, #0x0, #0x4, eq
100924634:      b.eq    0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100924638:      tst x22, #0x3
10092463c:      ccmp    x22, #0x7, #0x0, eq
100924640:      b.ls    0x100924748 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
100924644:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
100924648:      add x8, x8, #0x4e8
10092464c:      ldr x8, [x8]
100924650:      cmn x8, #0x1
100924654:      b.eq    0x1009253e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe18>
100924658:      mrs x9, TPIDRRO_EL0
10092465c:      and x9, x9, #0xfffffffffffffff8
100924660:      ldr x0, [x9, x8, lsl #3]
100924664:      cbz x0, 0x1009253e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe18>
100924668:      lsr x1, x22, #20
10092466c:      ldr x8, [x0, #0x10]
100924670:      ldrb    w9, [x8, #0x28]
100924674:      tbz w9, #0x0, 0x100924694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc8>
100924678:      ldr x9, [x8, #0x20]
10092467c:      cmp x9, x1
100924680:      b.ne    0x100924694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc8>
100924684:      ldp x9, x10, [x8]
100924688:      cmp x9, x22
10092468c:      ccmp    x10, x22, #0x0, ls
100924690:      b.hi    0x100924710 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x144>
100924694:      ldrb    w9, [x8, #0x58]
100924698:      cbz w9, 0x1009246b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xec>
10092469c:      ldr x9, [x8, #0x50]
1009246a0:      cmp x9, x1
1009246a4:      b.ne    0x1009246b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xec>
1009246a8:      ldp x9, x10, [x8, #0x30]
1009246ac:      cmp x9, x22
1009246b0:      ccmp    x10, x22, #0x0, ls
1009246b4:      b.hi    0x100924704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x138>
1009246b8:      ldrb    w9, [x8, #0x88]
1009246bc:      cbz w9, 0x1009246dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x110>
1009246c0:      ldr x9, [x8, #0x80]
1009246c4:      cmp x9, x1
1009246c8:      b.ne    0x1009246dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x110>
1009246cc:      ldp x9, x10, [x8, #0x60]
1009246d0:      cmp x9, x22
1009246d4:      ccmp    x10, x22, #0x0, ls
1009246d8:      b.hi    0x10092470c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x140>
1009246dc:      ldrb    w9, [x8, #0xb8]
1009246e0:      cbz w9, 0x10092471c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x150>
1009246e4:      ldr x9, [x8, #0xb0]
1009246e8:      cmp x9, x1
1009246ec:      b.ne    0x10092471c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x150>
1009246f0:      ldp x9, x10, [x8, #0x90]!
1009246f4:      cmp x9, x22
1009246f8:      ccmp    x10, x22, #0x0, ls
1009246fc:      b.hi    0x100924710 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x144>
100924700:      b   0x10092471c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x150>
100924704:      add x8, x8, #0x30
100924708:      b   0x100924710 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x144>
10092470c:      add x8, x8, #0x60
100924710:      ldrb    w8, [x8, #0x19]
100924714:      cmp w8, #0xff
100924718:      b.ne    0x100924728 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x15c>
10092471c:      mov x0, x22
100924720:      bl  0x100889a20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
100924724:      and w8, w0, #0xff
100924728:      cbz w8, 0x100924748 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
10092472c:      ldurb   w8, [x22, #-0x8]
100924730:      ldurb   w9, [x22, #-0x7]
100924734:      mov w10, #0x82              ; =130
100924738:      and w9, w9, w10
10092473c:      cmp w9, #0x2
100924740:      ccmp    w8, #0x1, #0x0, eq
100924744:      b.eq    0x1009249c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3f4>
100924748:      mov x0, x22
10092474c:      bl  0x10092bba8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100924750:      mov x8, x0
100924754:      cbz x0, 0x1009247e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x21c>
100924758:      ldrb    w9, [x8]
10092475c:      cmp w9, #0x1
100924760:      b.ne    0x100924878 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2ac>
100924764:      ldrsb   w9, [x8, #0x1]
100924768:      mov x0, x8
10092476c:      tbz w9, #0x1f, 0x1009248b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2ec>
100924770:      mov x21, x8
100924774:      ldr x22, [x8, #0x8]
100924778:      mov x0, x22
10092477c:      bl  0x10092bba8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100924780:      cbz x0, 0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100924784:      ldrb    w8, [x0]
100924788:      cmp w8, #0x1
10092478c:      b.ne    0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100924790:      ldrsb   w8, [x0, #0x1]
100924794:      tbz w8, #0x1f, 0x100924870 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2a4>
100924798:      mov w23, #0x1               ; =1
10092479c:      ldr x22, [x0, #0x8]
1009247a0:      mov x0, x22
1009247a4:      bl  0x10092bba8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1009247a8:      cbz x0, 0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1009247ac:      ldrb    w8, [x0]
1009247b0:      cmp w8, #0x1
1009247b4:      b.ne    0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1009247b8:      cmp w23, #0x3f
1009247bc:      b.hi    0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1009247c0:      add w23, w23, #0x1
1009247c4:      ldrsb   w8, [x0, #0x1]
1009247c8:      tbnz    w8, #0x1f, 0x10092479c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d0>
1009247cc:      mov x8, x21
1009247d0:      str x22, [x21, #0x8]
1009247d4:      ldrb    w9, [x21, #0x1]
1009247d8:      orr w9, w9, #0x80
1009247dc:      strb    w9, [x21, #0x1]
1009247e0:      ldrb    w9, [x0]
1009247e4:      b   0x10092487c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2b0>
1009247e8:      mov x21, x8
1009247ec:      adrp    x8, 0x10117c000 <_out_buf+0x3dc8>
1009247f0:      add x8, x8, #0x814
1009247f4:      ldaprb  w8, [x8]
1009247f8:      cbz w8, 0x100924828 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x25c>
1009247fc:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
100924800:      add x8, x8, #0x238
100924804:      ldapr   x9, [x8]
100924808:      cmp x9, x22
10092480c:      b.hi    0x100924828 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x25c>
100924810:      ldapur  x8, [x8, #0x8]
100924814:      cmp x8, x22
100924818:      b.lo    0x100924828 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x25c>
10092481c:      mov x0, x22
100924820:      bl  0x100192b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100924824:      tbnz    w0, #0x0, 0x10092486c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2a0>
100924828:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x138>
10092482c:      add x8, x8, #0xb78
100924830:      ldaprb  w8, [x8]
100924834:      cbz w8, 0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100924838:      adrp    x8, 0x101131000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5timer16TIMER_REF_STATES+0x28>
10092483c:      add x8, x8, #0xd88
100924840:      ldapr   x8, [x8]
100924844:      cmp x8, x22
100924848:      b.hi    0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
10092484c:      adrp    x8, 0x101131000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5timer16TIMER_REF_STATES+0x28>
100924850:      add x8, x8, #0xd90
100924854:      ldapr   x8, [x8]
100924858:      cmp x8, x22
10092485c:      b.lo    0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100924860:      mov x0, x22
100924864:      bl  0x1008e35a8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
100924868:      tbz w0, #0x0, 0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
10092486c:      mov x0, #0x0                ; =0
100924870:      mov x8, x21
100924874:      b   0x1009248b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2ec>
100924878:      mov x0, x8
10092487c:      cmp w9, #0x1
100924880:      b.eq    0x1009248b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2ec>
100924884:      cmp w9, #0x9
100924888:      b.ne    0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
10092488c:      ldr w8, [x22, #0x4]
100924890:      mov w9, #0x5841             ; =22593
100924894:      movk    w9, #0x4c5a, lsl #16
100924898:      cmp w8, w9
10092489c:      b.ne    0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1009248a0:      mov x0, x22
1009248a4:      bl  0x1002ac118 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape22force_materialize_lazy>
1009248a8:      mov x22, x0
1009248ac:      str x0, [sp, #0x18]
1009248b0:      cbnz    x0, 0x1009249e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x41c>
1009248b4:      b   0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1009248b8:      ldp w10, w9, [x22]
1009248bc:      cmp w10, w9
1009248c0:      b.ls    0x1009248e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x314>
1009248c4:      cbz x8, 0x1009248f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x324>
1009248c8:      ldr w8, [x0, #0x4]
1009248cc:      lsl x9, x9, #3
1009248d0:      add x9, x9, #0x10
1009248d4:      cmp x9, x8
1009248d8:      b.ne    0x1009248f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x324>
1009248dc:      b   0x1009249e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x418>
1009248e0:      mov w8, #0xe100             ; =57600
1009248e4:      movk    w8, #0x5f5, lsl #16
1009248e8:      cmp w10, w8
1009248ec:      b.ls    0x1009249e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x418>
1009248f0:      adrp    x8, 0x10117c000 <_out_buf+0x3dc8>
1009248f4:      add x8, x8, #0x814
1009248f8:      ldaprb  w8, [x8]
1009248fc:      cbz w8, 0x10092492c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x360>
100924900:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
100924904:      add x8, x8, #0x238
100924908:      ldapr   x9, [x8]
10092490c:      cmp x9, x22
100924910:      b.hi    0x10092492c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x360>
100924914:      ldapur  x8, [x8, #0x8]
100924918:      cmp x8, x22
10092491c:      b.lo    0x10092492c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x360>
100924920:      mov x0, x22
100924924:      bl  0x100192b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100924928:      tbnz    w0, #0x0, 0x1009249e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x418>
10092492c:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x138>
100924930:      add x8, x8, #0xb78
100924934:      ldaprb  w8, [x8]
100924938:      cbz w8, 0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
10092493c:      adrp    x8, 0x101131000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5timer16TIMER_REF_STATES+0x28>
100924940:      add x8, x8, #0xd88
100924944:      ldapr   x8, [x8]
100924948:      cmp x8, x22
10092494c:      b.hi    0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100924950:      adrp    x8, 0x101131000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5timer16TIMER_REF_STATES+0x28>
100924954:      add x8, x8, #0xd90
100924958:      ldapr   x8, [x8]
10092495c:      cmp x8, x22
100924960:      b.lo    0x100924970 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100924964:      mov x0, x22
100924968:      bl  0x1008e35a8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
10092496c:      tbnz    w0, #0x0, 0x1009249e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x418>
100924970:      ldr x1, [x19, #0x10]
100924974:      ldr x8, [x19]
100924978:      sub x8, x8, x1
10092497c:      cmp x8, #0x1
100924980:      b.ls    0x1009256d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x110c>
100924984:      ldr x8, [x19, #0x8]
100924988:      mov w9, #0x5d5b             ; =23899
10092498c:      strh    w9, [x8, x1]
100924990:      ldr x8, [x19, #0x10]
100924994:      add x8, x8, #0x2
100924998:      str x8, [x19, #0x10]
10092499c:      ldp x29, x30, [sp, #0xe0]
1009249a0:      ldp x20, x19, [sp, #0xd0]
1009249a4:      ldp x22, x21, [sp, #0xc0]
1009249a8:      ldp x24, x23, [sp, #0xb0]
1009249ac:      ldp x26, x25, [sp, #0xa0]
1009249b0:      ldp x28, x27, [sp, #0x90]
1009249b4:      ldp d9, d8, [sp, #0x80]
1009249b8:      add sp, sp, #0xf0
1009249bc:      ret
1009249c0:      ldr w8, [x22]
1009249c4:      mov w9, #0xe100             ; =57600
1009249c8:      movk    w9, #0x5f5, lsl #16
1009249cc:      orr w9, w9, #0x1
1009249d0:      cmp w8, w9
1009249d4:      b.hs    0x100924748 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
1009249d8:      ldr w9, [x22, #0x4]
1009249dc:      cmp w8, w9
1009249e0:      b.hi    0x100924748 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
1009249e4:      str x22, [sp, #0x18]
1009249e8:      mov x0, x22
1009249ec:      bl  0x100923424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify17array_get_to_json>
1009249f0:      tbz w0, #0x0, 0x100924a88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4bc>
1009249f4:      fmov    x21, d0
1009249f8:      mov w8, #0x7ffd             ; =32765
1009249fc:      cmp x8, x21, lsr #48
100924a00:      b.ne    0x100925398 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdcc>
100924a04:      and x21, x21, #0xffffffffffff
100924a08:      sub x8, x21, #0x100, lsl #12 ; =0x100000
100924a0c:      mov x9, #0x7ffffff00000     ; =140737487306752
100924a10:      cmp x8, x9
100924a14:      b.hs    0x1009253c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
100924a18:      ldurb   w8, [x21, #-0x8]
100924a1c:      sub w8, w8, #0x1
100924a20:      cmp w8, #0x1
100924a24:      b.hi    0x1009253c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
100924a28:      adrp    x8, 0x10117c000 <_out_buf+0x3dc8>
100924a2c:      add x8, x8, #0x814
100924a30:      ldaprb  w8, [x8]
100924a34:      cbz w8, 0x100924a6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4a0>
100924a38:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
100924a3c:      add x8, x8, #0x238
100924a40:      ldapr   x9, [x8]
100924a44:      cmp x21, x9
100924a48:      b.lo    0x100924a6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4a0>
100924a4c:      ldapur  x8, [x8, #0x8]
100924a50:      cmp x21, x8
100924a54:      b.hi    0x100924a6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4a0>
100924a58:      mov x0, x21
100924a5c:      mov.16b v8, v0
100924a60:      bl  0x100192b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100924a64:      mov.16b v0, v8
100924a68:      tbnz    w0, #0x0, 0x1009253c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
100924a6c:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
100924a70:      add x0, x0, #0x680
100924a74:      ldr x8, [x0]
100924a78:      blr x8
100924a7c:      mov w8, #0x1                ; =1
100924a80:      strb    w8, [x0]
100924a84:      b   0x1009253c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
100924a88:      mov x0, x22
100924a8c:      bl  0x10092bba8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100924a90:      cbz x0, 0x100924ac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
100924a94:      ldrb    w8, [x0]
100924a98:      cmp w8, #0x1
100924a9c:      b.ne    0x100924ac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
100924aa0:      ldrh    w21, [x0, #0x2]
100924aa4:      tbnz    w21, #0xa, 0x100924ac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
100924aa8:      ldp w8, w9, [x22]
100924aac:      cmp w8, w9
100924ab0:      b.hi    0x100924ac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
100924ab4:      mov x0, x22
100924ab8:      bl  0x10092ade4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header35array_has_named_properties_resolved>
100924abc:      tbz w0, #0x0, 0x1009253fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe30>
100924ac0:      adrp    x0, 0x1010d7000 <_anon.c91c46594139130ff40967685eae250e.775+0x180>
100924ac4:      add x0, x0, #0x28
100924ac8:      add x1, sp, #0x18
100924acc:      bl  0x1001382c8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths_0bEB2j_>
100924ad0:      tbnz    w0, #0x0, 0x1009265f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2024>
100924ad4:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
100924ad8:      add x0, x0, #0x668
100924adc:      ldr x8, [x0]
100924ae0:      blr x8
100924ae4:      mov x24, x0
100924ae8:      ldrb    w8, [x0, #0x20]
100924aec:      cbnz    w8, 0x1009262b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ce4>
100924af0:      ldr x8, [x24]
100924af4:      cbnz    x8, 0x1009262d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d08>
100924af8:      mov x8, #-0x1               ; =-1
100924afc:      str x8, [x24]
100924b00:      mov x0, x24
100924b04:      ldr x8, [x0, #0x8]!
100924b08:      ldr x21, [x24, #0x18]
100924b0c:      cmp x21, x8
100924b10:      b.ne    0x100924b18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x54c>
100924b14:      bl  0x100cdb480 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecjE8grow_oneCs3HfcutmYuk_10swc_common>
100924b18:      ldr x8, [x24, #0x10]
100924b1c:      str x22, [x8, x21, lsl #3]
100924b20:      add x8, x21, #0x1
100924b24:      str x8, [x24, #0x18]
100924b28:      ldr x8, [x24]
100924b2c:      add x8, x8, #0x1
100924b30:      str x8, [x24]
100924b34:      ldr w8, [x22]
100924b38:      str x8, [sp, #0x10]
100924b3c:      mov x0, x22
100924b40:      bl  0x10092bba8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100924b44:      cbz x0, 0x100924b50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x584>
100924b48:      ldrh    w8, [x0, #0x2]
100924b4c:      tbnz    w8, #0xa, 0x100924b84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5b8>
100924b50:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7e0>
100924b54:      ldrb    w8, [x8, #0xb41]
100924b58:      cbnz    w8, 0x100924b84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5b8>
100924b5c:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7e0>
100924b60:      ldrb    w8, [x8, #0xb43]
100924b64:      cbnz    w8, 0x100924b84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5b8>
100924b68:      ldp w8, w9, [x22]
100924b6c:      cmp w8, w9
100924b70:      b.hi    0x100924b84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5b8>
100924b74:      mov x0, x22
100924b78:      bl  0x1007625a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object15prototype_chain23object_static_prototype>
100924b7c:      cmp x0, #0x1
100924b80:      b.ne    0x100925544 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf78>
100924b84:      adrp    x27, 0x101130000 <_perry_global_baseline_worker_ts__1>
100924b88:      add x27, x27, #0x4e8
100924b8c:      ldr x8, [x27]
100924b90:      cmn x8, #0x1
100924b94:      b.eq    0x100924bc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5fc>
100924b98:      mrs x9, TPIDRRO_EL0
100924b9c:      and x9, x9, #0xfffffffffffffff8
100924ba0:      ldr x8, [x9, x8, lsl #3]
100924ba4:      cbz x8, 0x100924bc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5fc>
100924ba8:      ldr x8, [x8, #0x19e8]
100924bac:      cbz x8, 0x100924bc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5fc>
100924bb0:      ldr x9, [x8]
100924bb4:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100924bb8:      cmp x9, x10
100924bbc:      b.hs    0x100926228 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c5c>
100924bc0:      ldr x21, [x8, #0x18]
100924bc4:      b   0x100924bd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x60c>
100924bc8:      adrp    x0, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
100924bcc:      add x0, x0, #0x950
100924bd0:      bl  0x10013596c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
100924bd4:      mov x21, x0
100924bd8:      stur    x21, [x29, #-0x68]
100924bdc:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100924be0:      stp x22, x8, [sp, #0x38]
100924be4:      mov w8, #0x1                ; =1
100924be8:      str x8, [sp, #0x30]
100924bec:      add x0, sp, #0x30
100924bf0:      bl  0x1008e22d8 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
100924bf4:      mov x22, x0
100924bf8:      ldr x23, [x19, #0x10]
100924bfc:      ldr x8, [x19]
100924c00:      cmp x8, x23
100924c04:      b.eq    0x100926278 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cac>
100924c08:      ldr x8, [x19, #0x8]
100924c0c:      mov w9, #0x5b               ; =91
100924c10:      strb    w9, [x8, x23]
100924c14:      add x23, x23, #0x1
100924c18:      str x23, [x19, #0x10]
100924c1c:      ldr x8, [sp, #0x10]
100924c20:      cbz w8, 0x1009252fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd30>
100924c24:      stp x21, x24, [sp]
100924c28:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
100924c2c:      add x0, x0, #0x6f8
100924c30:      ldr x8, [x0]
100924c34:      blr x8
100924c38:      mov x21, x0
100924c3c:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
100924c40:      add x0, x0, #0x620
100924c44:      ldr x8, [x0]
100924c48:      blr x8
100924c4c:      mov x25, x0
100924c50:      mov x26, #0x0               ; =0
100924c54:      b   0x100924c6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x6a0>
100924c58:      str xzr, [x8]
100924c5c:      add x26, x26, #0x1
100924c60:      ldr x8, [sp, #0x10]
100924c64:      cmp x8, x26
100924c68:      b.eq    0x1009252f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd28>
100924c6c:      cbz x26, 0x100924c94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x6c8>
100924c70:      ldr x23, [x19, #0x10]
100924c74:      ldr x8, [x19]
100924c78:      cmp x8, x23
100924c7c:      b.eq    0x1009251c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbf8>
100924c80:      ldr x8, [x19, #0x8]
100924c84:      mov w9, #0x2c               ; =44
100924c88:      strb    w9, [x8, x23]
100924c8c:      add x8, x23, #0x1
100924c90:      str x8, [x19, #0x10]
100924c94:      ldr x8, [x27]
100924c98:      cmn x8, #0x1
100924c9c:      b.eq    0x100924d00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x734>
100924ca0:      mrs x9, TPIDRRO_EL0
100924ca4:      and x9, x9, #0xfffffffffffffff8
100924ca8:      ldr x8, [x9, x8, lsl #3]
100924cac:      cbz x8, 0x100924d00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x734>
100924cb0:      ldr x8, [x8, #0x19e8]
100924cb4:      cbz x8, 0x100924d00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x734>
100924cb8:      ldr x9, [x8]
100924cbc:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100924cc0:      cmp x9, x10
100924cc4:      b.hs    0x100926490 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
100924cc8:      add x10, x9, #0x1
100924ccc:      str x10, [x8]
100924cd0:      ldr x10, [x8, #0x18]
100924cd4:      cmp x22, x10
100924cd8:      b.hs    0x10092648c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
100924cdc:      ldr x10, [x8, #0x10]
100924ce0:      mov w11, #0x18              ; =24
100924ce4:      madd    x10, x22, x11, x10
100924ce8:      ldr x11, [x10]
100924cec:      cmp x11, #0x1
100924cf0:      b.ne    0x10092649c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ed0>
100924cf4:      ldr x0, [x10, #0x8]
100924cf8:      str x9, [x8]
100924cfc:      b   0x100924d4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x780>
100924d00:      ldrb    w8, [x21, #0x20]
100924d04:      cbnz    w8, 0x1009251e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc14>
100924d08:      ldr x8, [x21]
100924d0c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100924d10:      cmp x8, x9
100924d14:      b.hs    0x100925f38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
100924d18:      add x9, x8, #0x1
100924d1c:      str x9, [x21]
100924d20:      ldr x9, [x21, #0x18]
100924d24:      cmp x22, x9
100924d28:      b.hs    0x10092648c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
100924d2c:      ldr x9, [x21, #0x10]
100924d30:      mov w10, #0x18              ; =24
100924d34:      madd    x9, x22, x10, x9
100924d38:      ldr x10, [x9]
100924d3c:      cmp x10, #0x1
100924d40:      b.ne    0x100925f88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19bc>
100924d44:      ldr x0, [x9, #0x8]
100924d48:      str x8, [x21]
100924d4c:      mov x1, x26
100924d50:      bl  0x10046d0a8 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime5array8indexing11proto_chain14array_spec_get>
100924d54:      mov.16b v8, v0
100924d58:      fmov    x23, d8
100924d5c:      mov x8, #0x1                ; =1
100924d60:      movk    x8, #0x7ffc, lsl #48
100924d64:      cmp x23, x8
100924d68:      mov x8, #0x10               ; =16
100924d6c:      movk    x8, #0x7ffc, lsl #48
100924d70:      ccmp    x23, x8, #0x4, ne
100924d74:      b.ne    0x100924dac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7e0>
100924d78:      ldr x1, [x19, #0x10]
100924d7c:      ldr x8, [x19]
100924d80:      sub x8, x8, x1
100924d84:      cmp x8, #0x3
100924d88:      b.ls    0x1009251a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbdc>
100924d8c:      ldr x8, [x19, #0x8]
100924d90:      mov w9, #0x756e             ; =30062
100924d94:      movk    w9, #0x6c6c, lsl #16
100924d98:      str w9, [x8, x1]
100924d9c:      ldr x8, [x19, #0x10]
100924da0:      add x8, x8, #0x4
100924da4:      str x8, [x19, #0x10]
100924da8:      b   0x100924c5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x690>
100924dac:      and x24, x23, #0xffff000000000000
100924db0:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100924db4:      cmp x24, x8
100924db8:      b.ne    0x100924df0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x824>
100924dbc:      and x8, x23, #0xffffffffffff
100924dc0:      cmp x8, #0x100, lsl #12     ; =0x100000
100924dc4:      b.lo    0x100924ddc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x810>
100924dc8:      ldr w8, [x8, #0xc]
100924dcc:      mov w9, #0x4f53             ; =20307
100924dd0:      movk    w9, #0x434c, lsl #16
100924dd4:      cmp w8, w9
100924dd8:      b.eq    0x100924d78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7ac>
100924ddc:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100924de0:      cmp x24, x8
100924de4:      b.ne    0x100924e1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x850>
100924de8:      and x0, x23, #0xffffffffffff
100924dec:      b   0x100924e44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x878>
100924df0:      lsr x8, x23, #52
100924df4:      cbnz    x8, 0x100924ed4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100924df8:      and x8, x23, #0x7
100924dfc:      cbz x23, 0x100924e28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x85c>
100924e00:      cbnz    x8, 0x100924e28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x85c>
100924e04:      mov x0, x23
100924e08:      bl  0x100929350 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
100924e0c:      mov x8, x23
100924e10:      tbnz    w0, #0x0, 0x100924dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7f4>
100924e14:      mov x8, #0x0                ; =0
100924e18:      b   0x100924e28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x85c>
100924e1c:      lsr x8, x23, #52
100924e20:      cbnz    x8, 0x100924ed4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100924e24:      and x8, x23, #0x7
100924e28:      cbz x23, 0x100924ed4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100924e2c:      cbnz    x8, 0x100924ed4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100924e30:      mov x0, x23
100924e34:      bl  0x100929350 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
100924e38:      mov x8, x0
100924e3c:      mov x0, x23
100924e40:      tbz w8, #0x0, 0x100924ed4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100924e44:      cmp x0, #0x100, lsl #12     ; =0x100000
100924e48:      b.lo    0x100924ed4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100924e4c:      adrp    x8, 0x10117c000 <_out_buf+0x3dc8>
100924e50:      add x8, x8, #0x78c
100924e54:      ldaprb  w8, [x8]
100924e58:      cbz w8, 0x100924ed4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100924e5c:      lsr x8, x0, #3
100924e60:      mov x9, #0x7c15             ; =31765
100924e64:      movk    x9, #0x7f4a, lsl #16
100924e68:      movk    x9, #0x79b9, lsl #32
100924e6c:      movk    x9, #0x9e37, lsl #48
100924e70:      mul x8, x8, x9
100924e74:      lsr x9, x8, #54
100924e78:      lsr x10, x8, #60
100924e7c:      adrp    x11, 0x10117c000 <_out_buf+0x3dc8>
100924e80:      add x11, x11, #0x790
100924e84:      add x10, x11, x10, lsl #3
100924e88:      ldapr   x10, [x10]
100924e8c:      lsr x9, x10, x9
100924e90:      tbz w9, #0x0, 0x100924ed4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100924e94:      lsr x9, x8, #44
100924e98:      ubfx    x10, x8, #50, #4
100924e9c:      add x10, x11, x10, lsl #3
100924ea0:      ldapr   x10, [x10]
100924ea4:      lsr x9, x10, x9
100924ea8:      tbz w9, #0x0, 0x100924ed4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100924eac:      lsr x9, x8, #34
100924eb0:      ubfx    x8, x8, #40, #4
100924eb4:      adrp    x10, 0x10117c000 <_out_buf+0x3dc8>
100924eb8:      add x10, x10, #0x790
100924ebc:      add x8, x10, x8, lsl #3
100924ec0:      ldapr   x8, [x8]
100924ec4:      lsr x8, x8, x9
100924ec8:      tbz w8, #0x0, 0x100924ed4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100924ecc:      bl  0x10017bca0 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6symbol25is_registered_symbol_slow>
100924ed0:      tbnz    w0, #0x0, 0x100924d78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7ac>
100924ed4:      ldr x8, [x27]
100924ed8:      cmn x8, #0x1
100924edc:      b.eq    0x100924f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x940>
100924ee0:      mrs x9, TPIDRRO_EL0
100924ee4:      and x9, x9, #0xfffffffffffffff8
100924ee8:      ldr x8, [x9, x8, lsl #3]
100924eec:      cbz x8, 0x100924f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x940>
100924ef0:      ldr x8, [x8, #0x19e8]
100924ef4:      cbz x8, 0x100924f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x940>
100924ef8:      ldr x9, [x8], #0x18
100924efc:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100924f00:      cmp x9, x10
100924f04:      b.lo    0x100924f28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x95c>
100924f08:      b   0x100926228 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c5c>
100924f0c:      ldrb    w8, [x21, #0x20]
100924f10:      cbnz    w8, 0x100925238 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc6c>
100924f14:      ldr x9, [x21]
100924f18:      add x8, x21, #0x18
100924f1c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100924f20:      cmp x9, x10
100924f24:      b.hs    0x100925fa4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19d8>
100924f28:      ldr x28, [x8]
100924f2c:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7e0>
100924f30:      add x8, x8, #0xb70
100924f34:      ldr w8, [x8]
100924f38:      cbz w8, 0x100924f44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x978>
100924f3c:      mov x0, x23
100924f40:      bl  0x1004369e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
100924f44:      ldr x8, [x27]
100924f48:      cmn x8, #0x1
100924f4c:      b.eq    0x100924fb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9e4>
100924f50:      mrs x9, TPIDRRO_EL0
100924f54:      and x9, x9, #0xfffffffffffffff8
100924f58:      ldr x8, [x9, x8, lsl #3]
100924f5c:      cbz x8, 0x100924fb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9e4>
100924f60:      ldr x24, [x8, #0x19e8]
100924f64:      cbz x24, 0x100924fb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9e4>
100924f68:      ldr x8, [x24]
100924f6c:      cbnz    x8, 0x10092625c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c90>
100924f70:      mov x8, #-0x1               ; =-1
100924f74:      str x8, [x24]
100924f78:      mov x0, x24
100924f7c:      ldr x8, [x0, #0x8]!
100924f80:      ldr x23, [x24, #0x18]
100924f84:      cmp x23, x8
100924f88:      b.ne    0x100924f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9c4>
100924f8c:      bl  0x100ccd3e0 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecTyyNtNtCseUPtmYZaE8V_5gimli6common13EhFrameOffsetEE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
100924f90:      ldr x8, [x24, #0x10]
100924f94:      mov w9, #0x18               ; =24
100924f98:      madd    x8, x23, x9, x8
100924f9c:      str xzr, [x8]
100924fa0:      str d8, [x8, #0x8]
100924fa4:      add x8, x23, #0x1
100924fa8:      str x8, [x24, #0x18]
100924fac:      b   0x100925000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xa34>
100924fb0:      ldrb    w8, [x21, #0x20]
100924fb4:      cbnz    w8, 0x10092526c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xca0>
100924fb8:      ldr x8, [x21]
100924fbc:      cbnz    x8, 0x100925fb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19e4>
100924fc0:      mov x8, #-0x1               ; =-1
100924fc4:      str x8, [x21]
100924fc8:      ldr x23, [x21, #0x18]
100924fcc:      ldr x8, [x21, #0x8]
100924fd0:      cmp x23, x8
100924fd4:      b.ne    0x100924fe0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xa14>
100924fd8:      add x0, x21, #0x8
100924fdc:      bl  0x100ccd3e0 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecTyyNtNtCseUPtmYZaE8V_5gimli6common13EhFrameOffsetEE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
100924fe0:      ldr x8, [x21, #0x10]
100924fe4:      mov w9, #0x18               ; =24
100924fe8:      madd    x8, x23, x9, x8
100924fec:      str xzr, [x8]
100924ff0:      str d8, [x8, #0x8]
100924ff4:      add x8, x23, #0x1
100924ff8:      str x8, [x21, #0x18]
100924ffc:      mov x24, x21
100925000:      ldr x8, [x24]
100925004:      add x8, x8, #0x1
100925008:      str x8, [x24]
10092500c:      str x26, [sp, #0x60]
100925010:      ldrb    w8, [x25, #0x20]
100925014:      cbnz    w8, 0x100925210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc44>
100925018:      ldr x8, [x25]
10092501c:      cbnz    x8, 0x100925f98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19cc>
100925020:      mov x8, #-0x1               ; =-1
100925024:      str x8, [x25]
100925028:      str xzr, [x25, #0x18]
10092502c:      add x8, sp, #0x60
100925030:      str x8, [sp, #0x30]
100925034:      adrp    x8, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
100925038:      add x8, x8, #0xf80
10092503c:      str x8, [sp, #0x38]
100925040:      add x0, x25, #0x8
100925044:      add x3, sp, #0x30
100925048:      adrp    x1, 0x1010a3000 <_anon.58120679d426c7dccd15bda76f596bde.683>
10092504c:      add x1, x1, #0x590
100925050:      adrp    x2, 0x100eee000 <_anon.58120679d426c7dccd15bda76f596bde.264+0x1f>
100925054:      add x2, x2, #0x594
100925058:      bl  0x10002cf10 <__RNvNtCsjgY6bXVaRmE_4core3fmt5write>
10092505c:      ldr x8, [x25]
100925060:      add x8, x8, #0x1
100925064:      str x8, [x25]
100925068:      ldr x8, [x27]
10092506c:      cmn x8, #0x1
100925070:      b.eq    0x1009250e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb1c>
100925074:      mrs x9, TPIDRRO_EL0
100925078:      and x9, x9, #0xfffffffffffffff8
10092507c:      ldr x8, [x9, x8, lsl #3]
100925080:      cbz x8, 0x1009250e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb1c>
100925084:      ldr x8, [x8, #0x19e8]
100925088:      cbz x8, 0x1009250e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb1c>
10092508c:      ldr x9, [x8]
100925090:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100925094:      cmp x9, x10
100925098:      b.hs    0x100926490 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
10092509c:      add x10, x9, #0x1
1009250a0:      str x10, [x8]
1009250a4:      ldr x10, [x8, #0x18]
1009250a8:      cmp x23, x10
1009250ac:      b.hs    0x10092648c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
1009250b0:      ldr x10, [x8, #0x10]
1009250b4:      mov w11, #0x18              ; =24
1009250b8:      madd    x10, x23, x11, x10
1009250bc:      ldr x11, [x10]
1009250c0:      cbnz    x11, 0x100926268 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c9c>
1009250c4:      ldr d0, [x10, #0x8]
1009250c8:      str x9, [x8]
1009250cc:      add w1, w20, #0x1
1009250d0:      mov x0, x19
1009250d4:      bl  0x10092667c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
1009250d8:      ldr x8, [x27]
1009250dc:      cmn x8, #0x1
1009250e0:      b.ne    0x100925148 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb7c>
1009250e4:      b   0x10092517c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbb0>
1009250e8:      ldrb    w8, [x21, #0x20]
1009250ec:      cbnz    w8, 0x100925294 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xcc8>
1009250f0:      ldr x8, [x21]
1009250f4:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1009250f8:      cmp x8, x9
1009250fc:      b.hs    0x100925f38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
100925100:      add x9, x8, #0x1
100925104:      str x9, [x21]
100925108:      ldr x9, [x21, #0x18]
10092510c:      cmp x23, x9
100925110:      b.hs    0x10092648c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
100925114:      ldr x9, [x21, #0x10]
100925118:      mov w10, #0x18              ; =24
10092511c:      madd    x9, x23, x10, x9
100925120:      ldr x10, [x9]
100925124:      cbnz    x10, 0x100925fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19f0>
100925128:      ldr d0, [x9, #0x8]
10092512c:      str x8, [x21]
100925130:      add w1, w20, #0x1
100925134:      mov x0, x19
100925138:      bl  0x10092667c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
10092513c:      ldr x8, [x27]
100925140:      cmn x8, #0x1
100925144:      b.eq    0x10092517c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbb0>
100925148:      mrs x9, TPIDRRO_EL0
10092514c:      and x9, x9, #0xfffffffffffffff8
100925150:      ldr x8, [x9, x8, lsl #3]
100925154:      cbz x8, 0x10092517c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbb0>
100925158:      ldr x8, [x8, #0x19e8]
10092515c:      cbz x8, 0x10092517c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbb0>
100925160:      ldr x9, [x8]
100925164:      cbnz    x9, 0x100926234 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c68>
100925168:      ldr x9, [x8, #0x18]
10092516c:      cmp x28, x9
100925170:      b.hi    0x100924c58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x68c>
100925174:      str x28, [x8, #0x18]
100925178:      b   0x100924c58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x68c>
10092517c:      ldrb    w8, [x21, #0x20]
100925180:      cbnz    w8, 0x1009252c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xcf8>
100925184:      ldr x8, [x21]
100925188:      cbnz    x8, 0x1009252e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd1c>
10092518c:      add x8, x21, #0x18
100925190:      ldr x8, [x8]
100925194:      cmp x28, x8
100925198:      b.hi    0x100924c5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x690>
10092519c:      add x8, x21, #0x18
1009251a0:      str x28, [x8]
1009251a4:      b   0x100924c5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x690>
1009251a8:      mov x0, x19
1009251ac:      mov w2, #0x4                ; =4
1009251b0:      mov w3, #0x1                ; =1
1009251b4:      mov w4, #0x1                ; =1
1009251b8:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009251bc:      ldr x1, [x19, #0x10]
1009251c0:      b   0x100924d8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7c0>
1009251c4:      mov x0, x19
1009251c8:      mov x1, x23
1009251cc:      mov w2, #0x1                ; =1
1009251d0:      mov w3, #0x1                ; =1
1009251d4:      mov w4, #0x1                ; =1
1009251d8:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009251dc:      b   0x100924c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x6b4>
1009251e0:      cmp w8, #0x2
1009251e4:      b.eq    0x1009262e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
1009251e8:      mov x0, x21
1009251ec:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1009251f0:      add x1, x1, #0xf78
1009251f4:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1009251f8:      strb    wzr, [x21, #0x20]
1009251fc:      ldr x8, [x21]
100925200:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100925204:      cmp x8, x9
100925208:      b.lo    0x100924d18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x74c>
10092520c:      b   0x100925f38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
100925210:      cmp w8, #0x2
100925214:      b.eq    0x1009262e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
100925218:      mov x0, x25
10092521c:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
100925220:      add x1, x1, #0xf78
100925224:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100925228:      strb    wzr, [x25, #0x20]
10092522c:      ldr x8, [x25]
100925230:      cbz x8, 0x100925020 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xa54>
100925234:      b   0x100925f98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19cc>
100925238:      cmp w8, #0x2
10092523c:      b.eq    0x1009262e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
100925240:      mov x0, x21
100925244:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
100925248:      add x1, x1, #0xf78
10092524c:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100925250:      strb    wzr, [x21, #0x20]
100925254:      ldr x9, [x21]
100925258:      add x8, x21, #0x18
10092525c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100925260:      cmp x9, x10
100925264:      b.lo    0x100924f28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x95c>
100925268:      b   0x100925fa4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19d8>
10092526c:      cmp w8, #0x2
100925270:      b.eq    0x1009262e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
100925274:      mov x0, x21
100925278:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
10092527c:      add x1, x1, #0xf78
100925280:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100925284:      strb    wzr, [x21, #0x20]
100925288:      ldr x8, [x21]
10092528c:      cbz x8, 0x100924fc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9f4>
100925290:      b   0x100925fb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19e4>
100925294:      cmp w8, #0x2
100925298:      b.eq    0x1009262e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
10092529c:      mov x0, x21
1009252a0:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1009252a4:      add x1, x1, #0xf78
1009252a8:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1009252ac:      strb    wzr, [x21, #0x20]
1009252b0:      ldr x8, [x21]
1009252b4:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1009252b8:      cmp x8, x9
1009252bc:      b.lo    0x100925100 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb34>
1009252c0:      b   0x100925f38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
1009252c4:      cmp w8, #0x2
1009252c8:      b.eq    0x1009262e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
1009252cc:      mov x0, x21
1009252d0:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1009252d4:      add x1, x1, #0xf78
1009252d8:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1009252dc:      strb    wzr, [x21, #0x20]
1009252e0:      ldr x8, [x21]
1009252e4:      cbz x8, 0x10092518c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbc0>
1009252e8:      adrp    x0, 0x1010a5000 <_anon.58120679d426c7dccd15bda76f596bde.1139>
1009252ec:      add x0, x0, #0x2d0
1009252f0:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1009252f4:      ldr x23, [x19, #0x10]
1009252f8:      ldp x21, x24, [sp]
1009252fc:      ldr x8, [x19]
100925300:      cmp x8, x23
100925304:      b.eq    0x100926294 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cc8>
100925308:      ldr x8, [x19, #0x8]
10092530c:      mov w9, #0x5d               ; =93
100925310:      strb    w9, [x8, x23]
100925314:      add x8, x23, #0x1
100925318:      str x8, [x19, #0x10]
10092531c:      ldr x8, [x27]
100925320:      cmn x8, #0x1
100925324:      b.eq    0x100925360 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd94>
100925328:      mrs x9, TPIDRRO_EL0
10092532c:      and x9, x9, #0xfffffffffffffff8
100925330:      ldr x8, [x9, x8, lsl #3]
100925334:      cbz x8, 0x100925360 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd94>
100925338:      ldr x8, [x8, #0x19e8]
10092533c:      cbz x8, 0x100925360 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd94>
100925340:      ldr x9, [x8]
100925344:      cbnz    x9, 0x100926234 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c68>
100925348:      ldr x9, [x8, #0x18]
10092534c:      cmp x21, x9
100925350:      b.hi    0x100925358 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd8c>
100925354:      str x21, [x8, #0x18]
100925358:      str xzr, [x8]
10092535c:      b   0x100925370 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xda4>
100925360:      adrp    x0, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
100925364:      add x0, x0, #0x950
100925368:      sub x1, x29, #0x68
10092536c:      bl  0x100135d48 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
100925370:      ldrb    w8, [x24, #0x20]
100925374:      cbnz    w8, 0x1009262e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d14>
100925378:      ldr x8, [x24]
10092537c:      cbnz    x8, 0x100926310 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d44>
100925380:      ldr x8, [x24, #0x18]
100925384:      cbz x8, 0x100925390 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdc4>
100925388:      sub x8, x8, #0x1
10092538c:      str x8, [x24, #0x18]
100925390:      str xzr, [x24]
100925394:      b   0x10092499c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3d0>
100925398:      lsr x8, x21, #52
10092539c:      cbnz    x8, 0x1009253c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
1009253a0:      cbz x21, 0x1009253c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
1009253a4:      and x8, x21, #0x7
1009253a8:      cbnz    x8, 0x1009253c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
1009253ac:      mov x0, x21
1009253b0:      mov.16b v8, v0
1009253b4:      bl  0x100929350 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
1009253b8:      mov.16b v0, v8
1009253bc:      tbnz    w0, #0x0, 0x100924a08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x43c>
1009253c0:      add w1, w20, #0x1
1009253c4:      mov x0, x19
1009253c8:      bl  0x10092667c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
1009253cc:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
1009253d0:      add x0, x0, #0x680
1009253d4:      ldr x8, [x0]
1009253d8:      blr x8
1009253dc:      strb    wzr, [x0]
1009253e0:      b   0x10092499c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3d0>
1009253e4:      bl  0x100ccaa2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1009253e8:      lsr x1, x22, #20
1009253ec:      ldr x8, [x0, #0x10]
1009253f0:      ldrb    w9, [x8, #0x28]
1009253f4:      tbnz    w9, #0x0, 0x100924678 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xac>
1009253f8:      b   0x100924694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc8>
1009253fc:      tbnz    w21, #0x7, 0x10092541c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe50>
100925400:      mov x8, x22
100925404:      ldr w9, [x8], #0x8
100925408:      add x9, x8, x9, lsl #3
10092540c:      stp x8, x9, [sp, #0x30]
100925410:      add x0, sp, #0x30
100925414:      bl  0x1008d74d0 <__RINvXs2J_NtNtCsjgY6bXVaRmE_4core5slice4iterINtB7_4IterdENtNtNtNtBb_4iter6traits8iterator8Iterator3allNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json25stringify_primitive_array8try_emit0EB1J_>
100925418:      cbz w0, 0x100924ac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
10092541c:      ldurh   w24, [x22, #-0x6]
100925420:      ldr w21, [x22]
100925424:      ldr x20, [x19, #0x10]
100925428:      ldr x8, [x19]
10092542c:      cmp x8, x20
100925430:      b.eq    0x1009264e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f14>
100925434:      ldr x8, [x19, #0x8]
100925438:      mov w9, #0x5b               ; =91
10092543c:      strb    w9, [x8, x20]
100925440:      add x20, x20, #0x1
100925444:      str x20, [x19, #0x10]
100925448:      lsl x23, x21, #3
10092544c:      tbnz    w24, #0x7, 0x1009254c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xef8>
100925450:      cbz w21, 0x100926010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a44>
100925454:      ldr d0, [x22, #0x8]
100925458:      fmov    x0, d0
10092545c:      mov x8, #-0x7ffc000000000001 ; =-9222246136947933185
100925460:      add x8, x0, x8
100925464:      cmp x8, #0x2
100925468:      b.lo    0x100925fd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a0c>
10092546c:      mov x8, #0x4                ; =4
100925470:      movk    x8, #0x7ffc, lsl #48
100925474:      cmp x0, x8
100925478:      b.eq    0x1009256f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1128>
10092547c:      mov x8, #0x3                ; =3
100925480:      movk    x8, #0x7ffc, lsl #48
100925484:      cmp x0, x8
100925488:      b.ne    0x100925714 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1148>
10092548c:      ldr x8, [x19]
100925490:      sub x8, x8, x20
100925494:      cmp x8, #0x4
100925498:      b.ls    0x1009265b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1fe4>
10092549c:      ldr x8, [x19, #0x8]
1009254a0:      add x8, x8, x20
1009254a4:      mov w9, #0x65               ; =101
1009254a8:      strb    w9, [x8, #0x4]
1009254ac:      mov w9, #0x6166             ; =24934
1009254b0:      movk    w9, #0x736c, lsl #16
1009254b4:      str w9, [x8]
1009254b8:      ldr x8, [x19, #0x10]
1009254bc:      add x8, x8, #0x5
1009254c0:      b   0x100926000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a34>
1009254c4:      cbz w21, 0x100926010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a44>
1009254c8:      ldr d0, [x22, #0x8]
1009254cc:      mov x0, x19
1009254d0:      bl  0x100914d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
1009254d4:      cmp w21, #0x1
1009254d8:      b.eq    0x10092600c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a40>
1009254dc:      add x21, x22, #0x10
1009254e0:      sub x22, x23, #0x8
1009254e4:      mov w23, #0x2c              ; =44
1009254e8:      ldr d0, [x21], #0x8
1009254ec:      ldr x20, [x19, #0x10]
1009254f0:      ldr x8, [x19]
1009254f4:      cmp x8, x20
1009254f8:      b.eq    0x100925520 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf54>
1009254fc:      ldr x8, [x19, #0x8]
100925500:      strb    w23, [x8, x20]
100925504:      add x8, x20, #0x1
100925508:      str x8, [x19, #0x10]
10092550c:      mov x0, x19
100925510:      bl  0x100914d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
100925514:      subs    x22, x22, #0x8
100925518:      b.ne    0x1009254e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf1c>
10092551c:      b   0x10092600c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a40>
100925520:      mov x0, x19
100925524:      mov x1, x20
100925528:      mov w2, #0x1                ; =1
10092552c:      mov w3, #0x1                ; =1
100925530:      mov w4, #0x1                ; =1
100925534:      mov.16b v8, v0
100925538:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
10092553c:      mov.16b v0, v8
100925540:      b   0x1009254fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf30>
100925544:      mov x0, x22
100925548:      mov x1, x19
10092554c:      mov x2, x20
100925550:      bl  0x100918640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit>
100925554:      tbz w0, #0x0, 0x100925584 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xfb8>
100925558:      adrp    x0, 0x1010d7000 <_anon.c91c46594139130ff40967685eae250e.775+0x180>
10092555c:      add x0, x0, #0x28
100925560:      ldp x29, x30, [sp, #0xe0]
100925564:      ldp x20, x19, [sp, #0xd0]
100925568:      ldp x22, x21, [sp, #0xc0]
10092556c:      ldp x24, x23, [sp, #0xb0]
100925570:      ldp x26, x25, [sp, #0xa0]
100925574:      ldp x28, x27, [sp, #0x90]
100925578:      ldp d9, d8, [sp, #0x80]
10092557c:      add sp, sp, #0xf0
100925580:      b   0x100138178 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths3_0INtNtBZ_6option6OptionjEEB2j_>
100925584:      bl  0x1008e21cc <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
100925588:      str x0, [sp, #0x20]
10092558c:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100925590:      stp x22, x8, [sp, #0x38]
100925594:      mov w8, #0x1                ; =1
100925598:      str x8, [sp, #0x30]
10092559c:      add x0, sp, #0x30
1009255a0:      bl  0x1008e22d8 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1009255a4:      str x0, [sp, #0x28]
1009255a8:      add x8, sp, #0x28
1009255ac:      stur    x8, [x29, #-0x68]
1009255b0:      ldr x8, [sp, #0x10]
1009255b4:      cmp w8, #0x1
1009255b8:      b.ls    0x1009257a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1009255bc:      sub x0, x29, #0x68
1009255c0:      bl  0x1008d8990 <__RNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths4_0B7_>
1009255c4:      fmov    x21, d0
1009255c8:      mov w8, #0x7ffd             ; =32765
1009255cc:      cmp x8, x21, lsr #48
1009255d0:      b.ne    0x100925780 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11b4>
1009255d4:      and x22, x21, #0xffffffffffff
1009255d8:      cmp x22, #0x100, lsl #12    ; =0x100000
1009255dc:      b.lo    0x1009257a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1009255e0:      and x0, x21, #0xffffffffffff
1009255e4:      bl  0x1008eb4f4 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4date17is_date_cell_addr>
1009255e8:      tbnz    w0, #0x0, 0x1009257a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1009255ec:      ldr w8, [x22]
1009255f0:      mov w9, #-0xff5f            ; =-65375
1009255f4:      cmp w8, w9
1009255f8:      b.eq    0x1009257a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1009255fc:      sub x0, x29, #0x68
100925600:      bl  0x1008d8990 <__RNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths4_0B7_>
100925604:      bl  0x10094f92c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins10formatting16boxed_primitives26boxed_primitive_json_value>
100925608:      cmp x0, #0x1
10092560c:      b.eq    0x1009257a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
100925610:      add x0, sp, #0x30
100925614:      mov x1, x21
100925618:      bl  0x100919dc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template27build_shape_prefix_template>
10092561c:      ldr x8, [sp, #0x30]
100925620:      cmn x8, #0x1
100925624:      b.eq    0x1009257ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11e0>
100925628:      ldr x21, [x19, #0x10]
10092562c:      ldr x8, [x19]
100925630:      cmp x8, x21
100925634:      b.eq    0x100926630 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2064>
100925638:      ldr x8, [x19, #0x8]
10092563c:      mov w9, #0x5b               ; =91
100925640:      strb    w9, [x8, x21]
100925644:      add x8, x21, #0x1
100925648:      str x8, [x19, #0x10]
10092564c:      str xzr, [sp, #0x60]
100925650:      adrp    x0, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
100925654:      add x0, x0, #0xf60
100925658:      add x1, sp, #0x60
10092565c:      bl  0x10016482c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
100925660:      adrp    x24, 0x101130000 <_perry_global_baseline_worker_ts__1>
100925664:      add x24, x24, #0x4e8
100925668:      ldr x8, [x24]
10092566c:      cmn x8, #0x1
100925670:      b.eq    0x10092631c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d50>
100925674:      mrs x9, TPIDRRO_EL0
100925678:      and x9, x9, #0xfffffffffffffff8
10092567c:      ldr x8, [x9, x8, lsl #3]
100925680:      cbz x8, 0x10092631c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d50>
100925684:      ldr x8, [x8, #0x19e8]
100925688:      cbz x8, 0x10092631c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d50>
10092568c:      ldr x9, [x8]
100925690:      mov x10, #0x7ffffffffffffffe ; =9223372036854775806
100925694:      cmp x9, x10
100925698:      b.hi    0x100926490 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
10092569c:      ldr x10, [sp, #0x28]
1009256a0:      add x11, x9, #0x1
1009256a4:      str x11, [x8]
1009256a8:      ldr x11, [x8, #0x18]
1009256ac:      cmp x10, x11
1009256b0:      b.hs    0x10092648c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
1009256b4:      ldr x11, [x8, #0x10]
1009256b8:      mov w12, #0x18              ; =24
1009256bc:      madd    x10, x10, x12, x11
1009256c0:      ldr x11, [x10]
1009256c4:      cmp x11, #0x1
1009256c8:      b.ne    0x10092649c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ed0>
1009256cc:      ldr x0, [x10, #0x8]
1009256d0:      str x9, [x8]
1009256d4:      b   0x10092632c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d60>
1009256d8:      mov x0, x19
1009256dc:      mov w2, #0x2                ; =2
1009256e0:      mov w3, #0x1                ; =1
1009256e4:      mov w4, #0x1                ; =1
1009256e8:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009256ec:      ldr x1, [x19, #0x10]
1009256f0:      b   0x100924984 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3b8>
1009256f4:      ldr x8, [x19]
1009256f8:      sub x8, x8, x20
1009256fc:      cmp x8, #0x3
100925700:      b.ls    0x1009265d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2004>
100925704:      ldr x8, [x19, #0x8]
100925708:      mov w9, #0x7274             ; =29300
10092570c:      movk    w9, #0x6575, lsl #16
100925710:      b   0x100925ff4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a28>
100925714:      and x8, x0, #0xffff000000000000
100925718:      mov x9, #0x7fff000000000000 ; =9223090561878065152
10092571c:      cmp x8, x9
100925720:      b.eq    0x100925fcc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a00>
100925724:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
100925728:      cmp x8, x9
10092572c:      b.ne    0x10092621c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c50>
100925730:      strb    wzr, [sp, #0x64]
100925734:      str wzr, [sp, #0x60]
100925738:      add x1, sp, #0x60
10092573c:      bl  0x1008dda34 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime5value7jsvalueNtB2_7JSValue19short_string_to_buf>
100925740:      mov x1, x0
100925744:      add x8, sp, #0x30
100925748:      add x0, sp, #0x60
10092574c:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
100925750:      ldr w8, [sp, #0x30]
100925754:      tbz w8, #0x0, 0x100926240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c74>
100925758:      ldr x1, [x19, #0x10]
10092575c:      ldr x8, [x19]
100925760:      sub x8, x8, x1
100925764:      cmp x8, #0x3
100925768:      b.ls    0x100926614 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2048>
10092576c:      ldr x8, [x19, #0x8]
100925770:      mov w9, #0x756e             ; =30062
100925774:      movk    w9, #0x6c6c, lsl #16
100925778:      str w9, [x8, x1]
10092577c:      b   0x100925ff8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a2c>
100925780:      lsr x8, x21, #52
100925784:      cbnz    x8, 0x1009257a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
100925788:      cbz x21, 0x1009257a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
10092578c:      and x8, x21, #0x7
100925790:      cbnz    x8, 0x1009257a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
100925794:      mov x0, x21
100925798:      bl  0x100929350 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
10092579c:      mov x22, x21
1009257a0:      cbnz    w0, 0x1009255d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x100c>
1009257a4:      mov x8, #-0x1               ; =-1
1009257a8:      str x8, [sp, #0x30]
1009257ac:      ldr x21, [x19, #0x10]
1009257b0:      ldr x8, [x19]
1009257b4:      cmp x8, x21
1009257b8:      b.eq    0x100926558 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f8c>
1009257bc:      ldr x8, [x19, #0x8]
1009257c0:      mov w9, #0x5b               ; =91
1009257c4:      strb    w9, [x8, x21]
1009257c8:      add x21, x21, #0x1
1009257cc:      str x21, [x19, #0x10]
1009257d0:      ldr x8, [sp, #0x10]
1009257d4:      cbz w8, 0x100925f48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x197c>
1009257d8:      mov x23, #0x1               ; =1
1009257dc:      movk    x23, #0x7ffc, lsl #48
1009257e0:      mov w28, #0x756e            ; =30062
1009257e4:      movk    w28, #0x6c6c, lsl #16
1009257e8:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
1009257ec:      add x0, x0, #0x6f8
1009257f0:      ldr x8, [x0]
1009257f4:      blr x8
1009257f8:      mov x21, x0
1009257fc:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
100925800:      add x0, x0, #0x680
100925804:      ldr x8, [x0]
100925808:      blr x8
10092580c:      str x0, [sp, #0x8]
100925810:      mov x22, #0x0               ; =0
100925814:      adrp    x25, 0x101130000 <_perry_global_baseline_worker_ts__1>
100925818:      add x25, x25, #0x4e8
10092581c:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
100925820:      mov w24, #0x18              ; =24
100925824:      b   0x1009258a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12dc>
100925828:      ldr x1, [x19, #0x10]
10092582c:      ldr x8, [x19]
100925830:      sub x8, x8, x1
100925834:      cmp x8, #0x3
100925838:      b.ls    0x100925864 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1298>
10092583c:      ldr x8, [x19, #0x8]
100925840:      str w28, [x8, x1]
100925844:      ldr x8, [x19, #0x10]
100925848:      add x8, x8, #0x4
10092584c:      str x8, [x19, #0x10]
100925850:      add x22, x22, #0x1
100925854:      ldr x8, [sp, #0x10]
100925858:      cmp x8, x22
10092585c:      b.ne    0x1009258a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12dc>
100925860:      b   0x100925f44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1978>
100925864:      mov x0, x19
100925868:      mov w2, #0x4                ; =4
10092586c:      mov w3, #0x1                ; =1
100925870:      mov w4, #0x1                ; =1
100925874:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100925878:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
10092587c:      ldr x1, [x19, #0x10]
100925880:      b   0x10092583c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1270>
100925884:      ldr w2, [x8, #0x4]
100925888:      add x1, x8, #0x14
10092588c:      mov x0, x19
100925890:      bl  0x1009160a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
100925894:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
100925898:      add x22, x22, #0x1
10092589c:      ldr x8, [sp, #0x10]
1009258a0:      cmp x8, x22
1009258a4:      b.eq    0x100925f44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1978>
1009258a8:      cbz x22, 0x1009258d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1304>
1009258ac:      ldr x26, [x19, #0x10]
1009258b0:      ldr x8, [x19]
1009258b4:      cmp x8, x26
1009258b8:      b.eq    0x100925d1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1750>
1009258bc:      ldr x8, [x19, #0x8]
1009258c0:      mov w9, #0x2c               ; =44
1009258c4:      strb    w9, [x8, x26]
1009258c8:      add x8, x26, #0x1
1009258cc:      str x8, [x19, #0x10]
1009258d0:      ldr x8, [x25]
1009258d4:      cmn x8, #0x1
1009258d8:      b.eq    0x10092594c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1380>
1009258dc:      mrs x9, TPIDRRO_EL0
1009258e0:      and x9, x9, #0xfffffffffffffff8
1009258e4:      ldr x8, [x9, x8, lsl #3]
1009258e8:      cbz x8, 0x10092594c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1380>
1009258ec:      ldr x8, [x8, #0x19e8]
1009258f0:      cbz x8, 0x100925ab8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x14ec>
1009258f4:      ldr x9, [x8]
1009258f8:      cmp x9, x12
1009258fc:      b.hs    0x100926490 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
100925900:      ldr x10, [sp, #0x28]
100925904:      add x11, x9, #0x1
100925908:      str x11, [x8]
10092590c:      ldr x11, [x8, #0x18]
100925910:      cmp x10, x11
100925914:      b.hs    0x10092648c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
100925918:      ldr x11, [x8, #0x10]
10092591c:      madd    x10, x10, x24, x11
100925920:      ldr x11, [x10]
100925924:      cmp x11, #0x1
100925928:      b.ne    0x10092649c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ed0>
10092592c:      ldr x0, [x10, #0x8]
100925930:      str x9, [x8]
100925934:      add x8, x0, x22, lsl #3
100925938:      ldr d8, [x8, #0x8]
10092593c:      fmov    x27, d8
100925940:      cmp x27, x23
100925944:      b.ne    0x1009259a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x13dc>
100925948:      b   0x100925828 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
10092594c:      ldr x26, [sp, #0x28]
100925950:      ldrb    w8, [x21, #0x20]
100925954:      cbnz    w8, 0x100925d3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1770>
100925958:      ldr x8, [x21]
10092595c:      cmp x8, x12
100925960:      b.hs    0x100925f38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
100925964:      add x9, x8, #0x1
100925968:      str x9, [x21]
10092596c:      ldr x9, [x21, #0x18]
100925970:      cmp x26, x9
100925974:      b.hs    0x10092648c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
100925978:      ldr x9, [x21, #0x10]
10092597c:      madd    x9, x26, x24, x9
100925980:      ldr x10, [x9]
100925984:      cmp x10, #0x1
100925988:      b.ne    0x100925f88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19bc>
10092598c:      ldr x0, [x9, #0x8]
100925990:      str x8, [x21]
100925994:      add x8, x0, x22, lsl #3
100925998:      ldr d8, [x8, #0x8]
10092599c:      fmov    x27, d8
1009259a0:      cmp x27, x23
1009259a4:      b.eq    0x100925828 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
1009259a8:      and x8, x27, #0xffff000000000000
1009259ac:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
1009259b0:      cmp x8, x9
1009259b4:      b.eq    0x1009259d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1408>
1009259b8:      mov x9, #0x7fff000000000000 ; =9223090561878065152
1009259bc:      cmp x8, x9
1009259c0:      b.ne    0x100925a60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1494>
1009259c4:      and x8, x27, #0xffffffffffff
1009259c8:      cmp x8, #0x1, lsl #12       ; =0x1000
1009259cc:      b.hs    0x100925884 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12b8>
1009259d0:      b   0x100925828 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
1009259d4:      strb    wzr, [sp, #0x5c]
1009259d8:      str wzr, [sp, #0x58]
1009259dc:      ubfx    x1, x27, #40, #8
1009259e0:      cbz x1, 0x100925a30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
1009259e4:      strb    w27, [sp, #0x58]
1009259e8:      cmp x1, #0x1
1009259ec:      b.eq    0x100925a30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
1009259f0:      lsr x8, x27, #8
1009259f4:      strb    w8, [sp, #0x59]
1009259f8:      cmp x1, #0x2
1009259fc:      b.eq    0x100925a30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
100925a00:      lsr x8, x27, #16
100925a04:      strb    w8, [sp, #0x5a]
100925a08:      cmp x1, #0x3
100925a0c:      b.eq    0x100925a30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
100925a10:      lsr x8, x27, #24
100925a14:      strb    w8, [sp, #0x5b]
100925a18:      cmp x1, #0x4
100925a1c:      b.eq    0x100925a30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
100925a20:      lsr x8, x27, #32
100925a24:      strb    w8, [sp, #0x5c]
100925a28:      cmp x1, #0x5
100925a2c:      b.ne    0x100926668 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x209c>
100925a30:      add x8, sp, #0x60
100925a34:      add x0, sp, #0x58
100925a38:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
100925a3c:      ldr x8, [sp, #0x60]
100925a40:      cbz x8, 0x100925ae4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1518>
100925a44:      ldr x1, [x19, #0x10]
100925a48:      ldr x8, [x19]
100925a4c:      sub x8, x8, x1
100925a50:      cmp x8, #0x3
100925a54:      b.ls    0x100925de0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1814>
100925a58:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
100925a5c:      b   0x10092583c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1270>
100925a60:      mov x9, #0x2                ; =2
100925a64:      movk    x9, #0x7ffc, lsl #48
100925a68:      cmp x27, x9
100925a6c:      b.eq    0x100925828 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
100925a70:      mov x9, #0x3                ; =3
100925a74:      movk    x9, #0x7ffc, lsl #48
100925a78:      cmp x27, x9
100925a7c:      b.eq    0x100925aec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1520>
100925a80:      mov x9, #0x4                ; =4
100925a84:      movk    x9, #0x7ffc, lsl #48
100925a88:      cmp x27, x9
100925a8c:      b.ne    0x100925b3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1570>
100925a90:      ldr x1, [x19, #0x10]
100925a94:      ldr x8, [x19]
100925a98:      sub x8, x8, x1
100925a9c:      cmp x8, #0x3
100925aa0:      b.ls    0x100925e1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1850>
100925aa4:      ldr x8, [x19, #0x8]
100925aa8:      mov w9, #0x7274             ; =29300
100925aac:      movk    w9, #0x6575, lsl #16
100925ab0:      str w9, [x8, x1]
100925ab4:      b   0x100925844 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1278>
100925ab8:      add x1, sp, #0x28
100925abc:      adrp    x0, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
100925ac0:      add x0, x0, #0x950
100925ac4:      bl  0x100135790 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
100925ac8:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
100925acc:      add x8, x0, x22, lsl #3
100925ad0:      ldr d8, [x8, #0x8]
100925ad4:      fmov    x27, d8
100925ad8:      cmp x27, x23
100925adc:      b.ne    0x1009259a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x13dc>
100925ae0:      b   0x100925828 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
100925ae4:      ldp x1, x2, [sp, #0x68]
100925ae8:      b   0x10092588c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c0>
100925aec:      ldr x1, [x19, #0x10]
100925af0:      ldr x8, [x19]
100925af4:      sub x8, x8, x1
100925af8:      cmp x8, #0x4
100925afc:      b.ls    0x100925dfc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1830>
100925b00:      ldr x8, [x19, #0x8]
100925b04:      add x8, x8, x1
100925b08:      mov w9, #0x65               ; =101
100925b0c:      strb    w9, [x8, #0x4]
100925b10:      mov w9, #0x6166             ; =24934
100925b14:      movk    w9, #0x736c, lsl #16
100925b18:      str w9, [x8]
100925b1c:      ldr x8, [x19, #0x10]
100925b20:      add x8, x8, #0x5
100925b24:      str x8, [x19, #0x10]
100925b28:      add x22, x22, #0x1
100925b2c:      ldr x8, [sp, #0x10]
100925b30:      cmp x8, x22
100925b34:      b.ne    0x1009258a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12dc>
100925b38:      b   0x100925f44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1978>
100925b3c:      mov x9, #0x7ffd000000000000 ; =9222527611924643840
100925b40:      cmp x8, x9
100925b44:      b.eq    0x100925b78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x15ac>
100925b48:      mov x9, #0x7ffa000000000000 ; =9221683186994511872
100925b4c:      cmp x8, x9
100925b50:      b.ne    0x100925bf4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1628>
100925b54:      str x22, [sp, #0x60]
100925b58:      add x1, sp, #0x60
100925b5c:      adrp    x0, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
100925b60:      add x0, x0, #0xf60
100925b64:      bl  0x10016482c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
100925b68:      mov.16b v0, v8
100925b6c:      mov x0, x19
100925b70:      bl  0x1009152a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars16serialize_bigint>
100925b74:      b   0x100925894 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c8>
100925b78:      str x22, [sp, #0x60]
100925b7c:      add x1, sp, #0x60
100925b80:      adrp    x0, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
100925b84:      add x0, x0, #0xf60
100925b88:      bl  0x10016482c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
100925b8c:      and x26, x27, #0xffffffffffff
100925b90:      cmp x26, #0x100, lsl #12    ; =0x100000
100925b94:      b.lo    0x100925c34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1668>
100925b98:      mov x0, x27
100925b9c:      bl  0x1009233ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify16is_closure_value>
100925ba0:      tbnz    w0, #0x0, 0x100925c34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1668>
100925ba4:      mov x0, x27
100925ba8:      bl  0x100922678 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify15is_symbol_value>
100925bac:      cbnz    w0, 0x100925c34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1668>
100925bb0:      mov.16b v0, v8
100925bb4:      bl  0x10094f92c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins10formatting16boxed_primitives26boxed_primitive_json_value>
100925bb8:      cmp x0, #0x1
100925bbc:      b.ne    0x100925c78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16ac>
100925bc0:      mov.16b v9, v0
100925bc4:      mov x0, x26
100925bc8:      bl  0x100923eb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify18object_get_to_json>
100925bcc:      tbz w0, #0x0, 0x100925c94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16c8>
100925bd0:      mov.16b v8, v0
100925bd4:      bl  0x100928d70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify24arm_to_json_result_guard>
100925bd8:      add w1, w20, #0x1
100925bdc:      mov.16b v0, v8
100925be0:      mov x0, x19
100925be4:      bl  0x10092667c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
100925be8:      ldr x8, [sp, #0x8]
100925bec:      strb    wzr, [x8]
100925bf0:      b   0x100925ed8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
100925bf4:      lsr x8, x27, #52
100925bf8:      cbnz    x8, 0x100925c68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x169c>
100925bfc:      cbz x27, 0x100925c68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x169c>
100925c00:      and x8, x27, #0x7
100925c04:      cbnz    x8, 0x100925c68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x169c>
100925c08:      mov x0, x27
100925c0c:      bl  0x100929350 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
100925c10:      tbz w0, #0x0, 0x100925c68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x169c>
100925c14:      str x22, [sp, #0x60]
100925c18:      add x1, sp, #0x60
100925c1c:      adrp    x0, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
100925c20:      add x0, x0, #0xf60
100925c24:      bl  0x10016482c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
100925c28:      mov x26, x27
100925c2c:      cmp x27, #0x100, lsl #12    ; =0x100000
100925c30:      b.hs    0x100925b98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x15cc>
100925c34:      ldr x1, [x19, #0x10]
100925c38:      ldr x8, [x19]
100925c3c:      sub x8, x8, x1
100925c40:      cmp x8, #0x3
100925c44:      b.ls    0x100925ee4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1918>
100925c48:      ldr x8, [x19, #0x8]
100925c4c:      mov w28, #0x756e            ; =30062
100925c50:      movk    w28, #0x6c6c, lsl #16
100925c54:      str w28, [x8, x1]
100925c58:      ldr x8, [x19, #0x10]
100925c5c:      add x8, x8, #0x4
100925c60:      str x8, [x19, #0x10]
100925c64:      b   0x100925894 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c8>
100925c68:      mov x0, x19
100925c6c:      mov.16b v0, v8
100925c70:      bl  0x100914d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
100925c74:      b   0x100925894 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c8>
100925c78:      mov x0, x26
100925c7c:      bl  0x1008eb4f4 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4date17is_date_cell_addr>
100925c80:      tbz w0, #0x0, 0x100925ca8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16dc>
100925c84:      mov.16b v0, v8
100925c88:      bl  0x1002d73f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object17date_proto_thunks18date_to_json_value>
100925c8c:      add w1, w20, #0x1
100925c90:      b   0x100925c9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16d0>
100925c94:      add w1, w20, #0x1
100925c98:      mov.16b v0, v9
100925c9c:      mov x0, x19
100925ca0:      bl  0x10092667c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
100925ca4:      b   0x100925ed8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
100925ca8:      mov x0, x26
100925cac:      bl  0x100a937c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json8raw_json19raw_json_text_bytes>
100925cb0:      cbz x0, 0x100925d74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17a8>
100925cb4:      add x8, sp, #0x60
100925cb8:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
100925cbc:      ldr w27, [sp, #0x60]
100925cc0:      ldp x28, x8, [sp, #0x68]
100925cc4:      cmp w27, #0x0
100925cc8:      mov w9, #0x4                ; =4
100925ccc:      csel    x26, x9, x8, ne
100925cd0:      ldr x1, [x19, #0x10]
100925cd4:      ldr x8, [x19]
100925cd8:      sub x8, x8, x1
100925cdc:      cmp x26, x8
100925ce0:      b.hi    0x100925f00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1934>
100925ce4:      cbz x26, 0x100925d10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1744>
100925ce8:      cmp w27, #0x0
100925cec:      adrp    x8, 0x100e01000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text17SB_X_USER_DEFINED+0x44>
100925cf0:      add x8, x8, #0x75a
100925cf4:      csel    x8, x8, x28, ne
100925cf8:      ldr x9, [x19, #0x8]
100925cfc:      add x0, x9, x1
100925d00:      mov x1, x8
100925d04:      mov x2, x26
100925d08:      bl  0x100ce43ec <_writev+0x100ce43ec>
100925d0c:      ldr x1, [x19, #0x10]
100925d10:      add x8, x1, x26
100925d14:      str x8, [x19, #0x10]
100925d18:      b   0x100925ed8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
100925d1c:      mov x0, x19
100925d20:      mov x1, x26
100925d24:      mov w2, #0x1                ; =1
100925d28:      mov w3, #0x1                ; =1
100925d2c:      mov w4, #0x1                ; =1
100925d30:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100925d34:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
100925d38:      b   0x1009258bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12f0>
100925d3c:      cmp w8, #0x2
100925d40:      b.eq    0x1009262e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
100925d44:      mov x0, x21
100925d48:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
100925d4c:      add x1, x1, #0xf78
100925d50:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100925d54:      strb    wzr, [x21, #0x20]
100925d58:      mov w28, #0x756e            ; =30062
100925d5c:      movk    w28, #0x6c6c, lsl #16
100925d60:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
100925d64:      ldr x8, [x21]
100925d68:      cmp x8, x12
100925d6c:      b.lo    0x100925964 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1398>
100925d70:      b   0x100925f38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
100925d74:      mov x0, x26
100925d78:      bl  0x10092d1e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header20is_registered_buffer>
100925d7c:      tbz w0, #0x0, 0x100925d90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c4>
100925d80:      mov x0, x26
100925d84:      mov x1, x19
100925d88:      bl  0x100911f9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json16stringify_buffer16stringify_buffer>
100925d8c:      b   0x100925ed8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
100925d90:      mov x0, x26
100925d94:      bl  0x1008e2d38 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray23lookup_typed_array_kind>
100925d98:      tbz w0, #0x0, 0x100925dac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17e0>
100925d9c:      mov x0, x26
100925da0:      mov x1, x19
100925da4:      bl  0x100912c24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json16stringify_buffer21stringify_typed_array>
100925da8:      b   0x100925ed8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
100925dac:      lsr x8, x26, #47
100925db0:      cbnz    x8, 0x100925e80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18b4>
100925db4:      ldurb   w8, [x26, #-0x8]
100925db8:      cmp w8, #0x4
100925dbc:      b.gt    0x100925e3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1870>
100925dc0:      cmp w8, #0x1
100925dc4:      b.eq    0x100925eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18ec>
100925dc8:      cmp w8, #0x2
100925dcc:      b.eq    0x100925e8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18c0>
100925dd0:      cmp w8, #0x3
100925dd4:      b.ne    0x100925e80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18b4>
100925dd8:      ldr w2, [x26, #0x4]
100925ddc:      b   0x100925ecc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1900>
100925de0:      mov x0, x19
100925de4:      mov w2, #0x4                ; =4
100925de8:      mov w3, #0x1                ; =1
100925dec:      mov w4, #0x1                ; =1
100925df0:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100925df4:      ldr x1, [x19, #0x10]
100925df8:      b   0x100925a58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x148c>
100925dfc:      mov x0, x19
100925e00:      mov w2, #0x5                ; =5
100925e04:      mov w3, #0x1                ; =1
100925e08:      mov w4, #0x1                ; =1
100925e0c:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100925e10:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
100925e14:      ldr x1, [x19, #0x10]
100925e18:      b   0x100925b00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1534>
100925e1c:      mov x0, x19
100925e20:      mov w2, #0x4                ; =4
100925e24:      mov w3, #0x1                ; =1
100925e28:      mov w4, #0x1                ; =1
100925e2c:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100925e30:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
100925e34:      ldr x1, [x19, #0x10]
100925e38:      b   0x100925aa4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x14d8>
100925e3c:      cmp w8, #0x5
100925e40:      b.eq    0x100925e54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1888>
100925e44:      cmp w8, #0x8
100925e48:      b.eq    0x100925e54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1888>
100925e4c:      cmp w8, #0xc
100925e50:      b.ne    0x100925e80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18b4>
100925e54:      ldr x1, [x19, #0x10]
100925e58:      ldr x8, [x19]
100925e5c:      sub x8, x8, x1
100925e60:      cmp x8, #0x1
100925e64:      b.ls    0x100925f1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1950>
100925e68:      ldr x8, [x19, #0x8]
100925e6c:      mov w9, #0x7d7b             ; =32123
100925e70:      strh    w9, [x8, x1]
100925e74:      ldr x8, [x19, #0x10]
100925e78:      add x8, x8, #0x2
100925e7c:      b   0x100925d14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1748>
100925e80:      mov x0, x26
100925e84:      bl  0x100923cc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify17is_object_pointer>
100925e88:      tbz w0, #0x0, 0x100925ea0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18d4>
100925e8c:      add w2, w20, #0x1
100925e90:      mov x0, x26
100925e94:      mov x1, x19
100925e98:      bl  0x100927380 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify22stringify_object_inner>
100925e9c:      b   0x100925ed8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
100925ea0:      ldp w8, w2, [x26]
100925ea4:      sub w9, w2, #0x1
100925ea8:      mov w10, #0x270f            ; =9999
100925eac:      cmp w9, w10
100925eb0:      ccmp    w8, w2, #0x2, lo
100925eb4:      b.hi    0x100925ecc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1900>
100925eb8:      add w2, w20, #0x1
100925ebc:      mov x0, x26
100925ec0:      mov x1, x19
100925ec4:      bl  0x1009245cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth>
100925ec8:      b   0x100925ed8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
100925ecc:      add x1, x26, #0x14
100925ed0:      mov x0, x19
100925ed4:      bl  0x1009160a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
100925ed8:      mov w28, #0x756e            ; =30062
100925edc:      movk    w28, #0x6c6c, lsl #16
100925ee0:      b   0x100925894 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c8>
100925ee4:      mov x0, x19
100925ee8:      mov w2, #0x4                ; =4
100925eec:      mov w3, #0x1                ; =1
100925ef0:      mov w4, #0x1                ; =1
100925ef4:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100925ef8:      ldr x1, [x19, #0x10]
100925efc:      b   0x100925c48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x167c>
100925f00:      mov x0, x19
100925f04:      mov x2, x26
100925f08:      mov w3, #0x1                ; =1
100925f0c:      mov w4, #0x1                ; =1
100925f10:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100925f14:      ldr x1, [x19, #0x10]
100925f18:      b   0x100925ce8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x171c>
100925f1c:      mov x0, x19
100925f20:      mov w2, #0x2                ; =2
100925f24:      mov w3, #0x1                ; =1
100925f28:      mov w4, #0x1                ; =1
100925f2c:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100925f30:      ldr x1, [x19, #0x10]
100925f34:      b   0x100925e68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x189c>
100925f38:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
100925f3c:      add x0, x0, #0xf70
100925f40:      bl  0x100c9855c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100925f44:      ldr x21, [x19, #0x10]
100925f48:      ldr x8, [x19]
100925f4c:      cmp x8, x21
100925f50:      b.eq    0x100926574 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1fa8>
100925f54:      ldr x8, [x19, #0x8]
100925f58:      mov w9, #0x5d               ; =93
100925f5c:      strb    w9, [x8, x21]
100925f60:      add x8, x21, #0x1
100925f64:      str x8, [x19, #0x10]
100925f68:      adrp    x0, 0x1010d7000 <_anon.c91c46594139130ff40967685eae250e.775+0x180>
100925f6c:      add x0, x0, #0x28
100925f70:      bl  0x100138258 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths7_0INtNtBZ_6option6OptionjEEB2j_>
100925f74:      add x0, sp, #0x30
100925f78:      bl  0x1008c9b60 <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueINtNtB4_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template13ShapeTemplateEEB13_>
100925f7c:      add x0, sp, #0x20
100925f80:      bl  0x1008c9d88 <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
100925f84:      b   0x10092499c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3d0>
100925f88:      adrp    x0, 0x100dc1000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2ae0>
100925f8c:      add x0, x0, #0x3a0
100925f90:      mov w1, #0xb                ; =11
100925f94:      bl  0x100cb9444 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
100925f98:      adrp    x0, 0x1010a3000 <_anon.58120679d426c7dccd15bda76f596bde.683>
100925f9c:      add x0, x0, #0x5c0
100925fa0:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100925fa4:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
100925fa8:      add x0, x0, #0x498
100925fac:      bl  0x100c9855c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100925fb0:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
100925fb4:      add x0, x0, #0x4b0
100925fb8:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100925fbc:      adrp    x0, 0x100dc1000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2ae0>
100925fc0:      add x0, x0, #0x429
100925fc4:      mov w1, #0xf                ; =15
100925fc8:      bl  0x100cb9444 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
100925fcc:      and x8, x0, #0xffffffffffff
100925fd0:      cmp x8, #0x1, lsl #12       ; =0x1000
100925fd4:      b.hs    0x100926248 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c7c>
100925fd8:      ldr x8, [x19]
100925fdc:      sub x8, x8, x20
100925fe0:      cmp x8, #0x3
100925fe4:      b.ls    0x100926590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1fc4>
100925fe8:      ldr x8, [x19, #0x8]
100925fec:      mov w9, #0x756e             ; =30062
100925ff0:      movk    w9, #0x6c6c, lsl #16
100925ff4:      str w9, [x8, x20]
100925ff8:      ldr x8, [x19, #0x10]
100925ffc:      add x8, x8, #0x4
100926000:      str x8, [x19, #0x10]
100926004:      cmp w21, #0x1
100926008:      b.ne    0x100926030 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a64>
10092600c:      ldr x20, [x19, #0x10]
100926010:      ldr x8, [x19]
100926014:      cmp x8, x20
100926018:      b.eq    0x1009264fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f30>
10092601c:      ldr x8, [x19, #0x8]
100926020:      mov w9, #0x5d               ; =93
100926024:      strb    w9, [x8, x20]
100926028:      add x8, x20, #0x1
10092602c:      b   0x100924998 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3cc>
100926030:      add x21, x22, #0x10
100926034:      mov w22, #0x756e            ; =30062
100926038:      movk    w22, #0x6c6c, lsl #16
10092603c:      sub x23, x23, #0x8
100926040:      mov w25, #0x2c              ; =44
100926044:      mov x26, #-0x7ffc000000000001 ; =-9222246136947933185
100926048:      mov x27, #0x3               ; =3
10092604c:      movk    x27, #0x7ffc, lsl #48
100926050:      mov x28, #0x4               ; =4
100926054:      movk    x28, #0x7ffc, lsl #48
100926058:      mov x24, #0x7ff9000000000000 ; =9221401712017801216
10092605c:      b   0x100926090 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ac4>
100926060:      ldr x1, [x19, #0x10]
100926064:      ldr x8, [x19]
100926068:      sub x8, x8, x1
10092606c:      cmp x8, #0x3
100926070:      b.ls    0x1009261c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bfc>
100926074:      ldr x8, [x19, #0x8]
100926078:      str w22, [x8, x1]
10092607c:      ldr x8, [x19, #0x10]
100926080:      add x8, x8, #0x4
100926084:      str x8, [x19, #0x10]
100926088:      subs    x23, x23, #0x8
10092608c:      b.eq    0x10092600c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a40>
100926090:      ldr d0, [x21], #0x8
100926094:      ldr x20, [x19, #0x10]
100926098:      ldr x8, [x19]
10092609c:      cmp x8, x20
1009260a0:      b.eq    0x1009261a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bd8>
1009260a4:      fmov    x0, d0
1009260a8:      ldr x8, [x19, #0x8]
1009260ac:      strb    w25, [x8, x20]
1009260b0:      add x1, x20, #0x1
1009260b4:      str x1, [x19, #0x10]
1009260b8:      add x8, x0, x26
1009260bc:      cmp x8, #0x2
1009260c0:      b.lo    0x100926064 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a98>
1009260c4:      cmp x0, x27
1009260c8:      b.eq    0x1009260f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b2c>
1009260cc:      cmp x0, x28
1009260d0:      b.ne    0x100926130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b64>
1009260d4:      ldr x8, [x19]
1009260d8:      sub x8, x8, x1
1009260dc:      cmp x8, #0x3
1009260e0:      b.ls    0x1009261e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c18>
1009260e4:      ldr x8, [x19, #0x8]
1009260e8:      mov w9, #0x7274             ; =29300
1009260ec:      movk    w9, #0x6575, lsl #16
1009260f0:      str w9, [x8, x1]
1009260f4:      b   0x10092607c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ab0>
1009260f8:      ldr x8, [x19]
1009260fc:      sub x8, x8, x1
100926100:      cmp x8, #0x4
100926104:      b.ls    0x100926200 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c34>
100926108:      ldr x8, [x19, #0x8]
10092610c:      add x8, x8, x1
100926110:      mov w9, #0x65               ; =101
100926114:      strb    w9, [x8, #0x4]
100926118:      mov w9, #0x6166             ; =24934
10092611c:      movk    w9, #0x736c, lsl #16
100926120:      str w9, [x8]
100926124:      ldr x8, [x19, #0x10]
100926128:      add x8, x8, #0x5
10092612c:      b   0x100926084 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ab8>
100926130:      and x8, x0, #0xffff000000000000
100926134:      cmp x8, x24
100926138:      b.eq    0x100926160 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b94>
10092613c:      mov x9, #0x7fff000000000000 ; =9223090561878065152
100926140:      cmp x8, x9
100926144:      b.ne    0x100926198 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bcc>
100926148:      and x8, x0, #0xffffffffffff
10092614c:      cmp x8, #0x1, lsl #12       ; =0x1000
100926150:      b.lo    0x100926064 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a98>
100926154:      ldr w2, [x8, #0x4]
100926158:      add x1, x8, #0x14
10092615c:      b   0x10092618c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bc0>
100926160:      strb    wzr, [sp, #0x64]
100926164:      str wzr, [sp, #0x60]
100926168:      add x1, sp, #0x60
10092616c:      bl  0x1008dda34 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime5value7jsvalueNtB2_7JSValue19short_string_to_buf>
100926170:      mov x1, x0
100926174:      add x8, sp, #0x30
100926178:      add x0, sp, #0x60
10092617c:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
100926180:      ldr w8, [sp, #0x30]
100926184:      tbnz    w8, #0x0, 0x100926060 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a94>
100926188:      ldp x1, x2, [sp, #0x38]
10092618c:      mov x0, x19
100926190:      bl  0x1009160a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
100926194:      b   0x100926088 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1abc>
100926198:      mov x0, x19
10092619c:      bl  0x100914d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
1009261a0:      b   0x100926088 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1abc>
1009261a4:      mov x0, x19
1009261a8:      mov x1, x20
1009261ac:      mov w2, #0x1                ; =1
1009261b0:      mov w3, #0x1                ; =1
1009261b4:      mov w4, #0x1                ; =1
1009261b8:      mov.16b v8, v0
1009261bc:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009261c0:      mov.16b v0, v8
1009261c4:      b   0x1009260a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ad8>
1009261c8:      mov x0, x19
1009261cc:      mov w2, #0x4                ; =4
1009261d0:      mov w3, #0x1                ; =1
1009261d4:      mov w4, #0x1                ; =1
1009261d8:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009261dc:      ldr x1, [x19, #0x10]
1009261e0:      b   0x100926074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1aa8>
1009261e4:      mov x0, x19
1009261e8:      mov w2, #0x4                ; =4
1009261ec:      mov w3, #0x1                ; =1
1009261f0:      mov w4, #0x1                ; =1
1009261f4:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009261f8:      ldr x1, [x19, #0x10]
1009261fc:      b   0x1009260e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b18>
100926200:      mov x0, x19
100926204:      mov w2, #0x5                ; =5
100926208:      mov w3, #0x1                ; =1
10092620c:      mov w4, #0x1                ; =1
100926210:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100926214:      ldr x1, [x19, #0x10]
100926218:      b   0x100926108 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b3c>
10092621c:      mov x0, x19
100926220:      bl  0x100914d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
100926224:      b   0x100926004 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a38>
100926228:      adrp    x0, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
10092622c:      add x0, x0, #0x9e8
100926230:      bl  0x100c9855c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100926234:      adrp    x0, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
100926238:      add x0, x0, #0xb68
10092623c:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100926240:      ldp x1, x2, [sp, #0x38]
100926244:      b   0x100926250 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c84>
100926248:      ldr w2, [x8, #0x4]
10092624c:      add x1, x8, #0x14
100926250:      mov x0, x19
100926254:      bl  0x1009160a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
100926258:      b   0x100926004 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a38>
10092625c:      adrp    x0, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
100926260:      add x0, x0, #0xa00
100926264:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100926268:      adrp    x0, 0x100dff000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1976+0x52c>
10092626c:      add x0, x0, #0xa6
100926270:      mov w1, #0xf                ; =15
100926274:      bl  0x100cb9444 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
100926278:      mov x0, x19
10092627c:      mov x1, x23
100926280:      mov w2, #0x1                ; =1
100926284:      mov w3, #0x1                ; =1
100926288:      mov w4, #0x1                ; =1
10092628c:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100926290:      b   0x100924c08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x63c>
100926294:      mov x0, x19
100926298:      mov x1, x23
10092629c:      mov w2, #0x1                ; =1
1009262a0:      mov w3, #0x1                ; =1
1009262a4:      mov w4, #0x1                ; =1
1009262a8:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009262ac:      b   0x100925308 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd3c>
1009262b0:      cmp w8, #0x1
1009262b4:      b.ne    0x1009262e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
1009262b8:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1009262bc:      add x1, x1, #0xf78
1009262c0:      mov x0, x24
1009262c4:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1009262c8:      strb    wzr, [x24, #0x20]
1009262cc:      ldr x8, [x24]
1009262d0:      cbz x8, 0x100924af8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x52c>
1009262d4:      adrp    x0, 0x1010a3000 <_anon.58120679d426c7dccd15bda76f596bde.683>
1009262d8:      add x0, x0, #0x8c0
1009262dc:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1009262e0:      cmp w8, #0x2
1009262e4:      b.ne    0x1009262f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d28>
1009262e8:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1009262ec:      add x0, x0, #0xed8
1009262f0:      bl  0x100cdab9c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1009262f4:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
1009262f8:      add x1, x1, #0xf78
1009262fc:      mov x0, x24
100926300:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100926304:      strb    wzr, [x24, #0x20]
100926308:      ldr x8, [x24]
10092630c:      cbz x8, 0x100925380 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdb4>
100926310:      adrp    x0, 0x1010a3000 <_anon.58120679d426c7dccd15bda76f596bde.683>
100926314:      add x0, x0, #0x8d8
100926318:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10092631c:      adrp    x0, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
100926320:      add x0, x0, #0x950
100926324:      add x1, sp, #0x28
100926328:      bl  0x100135790 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
10092632c:      ldr d8, [x0, #0x8]
100926330:      fmov    x0, d8
100926334:      add x1, sp, #0x30
100926338:      add w3, w20, #0x1
10092633c:      mov x2, x19
100926340:      bl  0x100918740 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template22try_emit_shape_element>
100926344:      cbnz    w0, 0x100926358 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d8c>
100926348:      add w1, w20, #0x1
10092634c:      mov.16b v0, v8
100926350:      mov x0, x19
100926354:      bl  0x10092667c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
100926358:      mov w8, #0x1                ; =1
10092635c:      ldr x9, [sp, #0x10]
100926360:      sub x25, x8, x9
100926364:      mov w26, #0x2               ; =2
100926368:      mov w27, #0x2c              ; =44
10092636c:      adrp    x21, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
100926370:      add x21, x21, #0xf60
100926374:      mov w28, #0x18              ; =24
100926378:      adrp    x22, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
10092637c:      add x22, x22, #0x950
100926380:      b   0x100926394 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1dc8>
100926384:      add x26, x26, #0x1
100926388:      add x8, x25, x26
10092638c:      cmp x8, #0x2
100926390:      b.eq    0x1009264ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ee0>
100926394:      ldr x23, [x19, #0x10]
100926398:      ldr x8, [x19]
10092639c:      cmp x8, x23
1009263a0:      b.eq    0x100926470 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ea4>
1009263a4:      sub x8, x26, #0x1
1009263a8:      ldr x9, [x19, #0x8]
1009263ac:      strb    w27, [x9, x23]
1009263b0:      add x9, x23, #0x1
1009263b4:      str x9, [x19, #0x10]
1009263b8:      str x8, [sp, #0x60]
1009263bc:      add x1, sp, #0x60
1009263c0:      mov x0, x21
1009263c4:      bl  0x10016482c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
1009263c8:      ldr x8, [x24]
1009263cc:      cmn x8, #0x1
1009263d0:      b.eq    0x100926434 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e68>
1009263d4:      mrs x9, TPIDRRO_EL0
1009263d8:      and x9, x9, #0xfffffffffffffff8
1009263dc:      ldr x8, [x9, x8, lsl #3]
1009263e0:      cbz x8, 0x100926434 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e68>
1009263e4:      ldr x8, [x8, #0x19e8]
1009263e8:      cbz x8, 0x100926434 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e68>
1009263ec:      ldr x9, [x8]
1009263f0:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1009263f4:      cmp x9, x10
1009263f8:      b.hs    0x100926490 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
1009263fc:      ldr x10, [sp, #0x28]
100926400:      add x11, x9, #0x1
100926404:      str x11, [x8]
100926408:      ldr x11, [x8, #0x18]
10092640c:      cmp x10, x11
100926410:      b.hs    0x10092648c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
100926414:      ldr x11, [x8, #0x10]
100926418:      madd    x10, x10, x28, x11
10092641c:      ldr x11, [x10]
100926420:      cmp x11, #0x1
100926424:      b.ne    0x10092649c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ed0>
100926428:      ldr x0, [x10, #0x8]
10092642c:      str x9, [x8]
100926430:      b   0x100926440 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e74>
100926434:      add x1, sp, #0x28
100926438:      mov x0, x22
10092643c:      bl  0x100135790 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
100926440:      ldr d8, [x0, x26, lsl #3]
100926444:      fmov    x0, d8
100926448:      add x1, sp, #0x30
10092644c:      add w3, w20, #0x1
100926450:      mov x2, x19
100926454:      bl  0x100918740 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template22try_emit_shape_element>
100926458:      tbnz    w0, #0x0, 0x100926384 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1db8>
10092645c:      add w1, w20, #0x1
100926460:      mov.16b v0, v8
100926464:      mov x0, x19
100926468:      bl  0x10092667c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
10092646c:      b   0x100926384 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1db8>
100926470:      mov x0, x19
100926474:      mov x1, x23
100926478:      mov w2, #0x1                ; =1
10092647c:      mov w3, #0x1                ; =1
100926480:      mov w4, #0x1                ; =1
100926484:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100926488:      b   0x1009263a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1dd8>
10092648c:      bl  0x100cb947c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
100926490:      adrp    x0, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
100926494:      add x0, x0, #0x9a0
100926498:      bl  0x100c9855c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10092649c:      adrp    x0, 0x100dff000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1976+0x52c>
1009264a0:      add x0, x0, #0x64
1009264a4:      mov w1, #0xb                ; =11
1009264a8:      bl  0x100cb9444 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1009264ac:      ldr x20, [x19, #0x10]
1009264b0:      ldr x8, [x19]
1009264b4:      cmp x8, x20
1009264b8:      b.eq    0x10092664c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2080>
1009264bc:      ldr x8, [x19, #0x8]
1009264c0:      mov w9, #0x5d               ; =93
1009264c4:      strb    w9, [x8, x20]
1009264c8:      add x8, x20, #0x1
1009264cc:      str x8, [x19, #0x10]
1009264d0:      adrp    x0, 0x1010d7000 <_anon.c91c46594139130ff40967685eae250e.775+0x180>
1009264d4:      add x0, x0, #0x28
1009264d8:      bl  0x1001381e8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths5_0INtNtBZ_6option6OptionjEEB2j_>
1009264dc:      b   0x100925f74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19a8>
1009264e0:      mov x0, x19
1009264e4:      mov x1, x20
1009264e8:      mov w2, #0x1                ; =1
1009264ec:      mov w3, #0x1                ; =1
1009264f0:      mov w4, #0x1                ; =1
1009264f4:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009264f8:      b   0x100925434 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe68>
1009264fc:      mov x0, x19
100926500:      mov x1, x20
100926504:      mov w2, #0x1                ; =1
100926508:      mov w3, #0x1                ; =1
10092650c:      mov w4, #0x1                ; =1
100926510:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100926514:      b   0x10092601c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a50>
100926518:      adrp    x0, 0x100e02000 <_anon.c91c46594139130ff40967685eae250e.686+0xf>
10092651c:      add x0, x0, #0x4d1
100926520:      mov w1, #0x34               ; =52
100926524:      bl  0x10097f65c <_js_string_from_bytes>
100926528:      bl  0x10031bea0 <_js_rangeerror_new>
10092652c:      mov x8, #0x1                ; =1
100926530:      movk    x8, #0x7ffc, lsl #48
100926534:      lsr x9, x0, #52
100926538:      mov x10, #0x7ffd000000000000 ; =9222527611924643840
10092653c:      bfxil   x10, x0, #0, #48
100926540:      cmp x9, #0x7fe
100926544:      csel    x9, x0, x10, hi
100926548:      cmp x0, #0x0
10092654c:      csinc   x8, x9, x8, ne
100926550:      fmov    d0, x8
100926554:      bl  0x1008c0e30 <_js_throw>
100926558:      mov x0, x19
10092655c:      mov x1, x21
100926560:      mov w2, #0x1                ; =1
100926564:      mov w3, #0x1                ; =1
100926568:      mov w4, #0x1                ; =1
10092656c:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100926570:      b   0x1009257bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11f0>
100926574:      mov x0, x19
100926578:      mov x1, x21
10092657c:      mov w2, #0x1                ; =1
100926580:      mov w3, #0x1                ; =1
100926584:      mov w4, #0x1                ; =1
100926588:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
10092658c:      b   0x100925f54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1988>
100926590:      mov x0, x19
100926594:      mov x1, x20
100926598:      mov w2, #0x4                ; =4
10092659c:      mov w3, #0x1                ; =1
1009265a0:      mov w4, #0x1                ; =1
1009265a4:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009265a8:      ldr x20, [x19, #0x10]
1009265ac:      b   0x100925fe8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a1c>
1009265b0:      mov x0, x19
1009265b4:      mov x1, x20
1009265b8:      mov w2, #0x5                ; =5
1009265bc:      mov w3, #0x1                ; =1
1009265c0:      mov w4, #0x1                ; =1
1009265c4:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009265c8:      ldr x20, [x19, #0x10]
1009265cc:      b   0x10092549c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xed0>
1009265d0:      mov x0, x19
1009265d4:      mov x1, x20
1009265d8:      mov w2, #0x4                ; =4
1009265dc:      mov w3, #0x1                ; =1
1009265e0:      mov w4, #0x1                ; =1
1009265e4:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009265e8:      ldr x20, [x19, #0x10]
1009265ec:      b   0x100925704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1138>
1009265f0:      adrp    x0, 0x100e02000 <_anon.c91c46594139130ff40967685eae250e.686+0xf>
1009265f4:      add x0, x0, #0x4ab
1009265f8:      mov w1, #0x25               ; =37
1009265fc:      bl  0x10097f65c <_js_string_from_bytes>
100926600:      bl  0x1003293d0 <_js_typeerror_new>
100926604:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100926608:      bfxil   x8, x0, #0, #48
10092660c:      fmov    d0, x8
100926610:      bl  0x1008c0e30 <_js_throw>
100926614:      mov x0, x19
100926618:      mov w2, #0x4                ; =4
10092661c:      mov w3, #0x1                ; =1
100926620:      mov w4, #0x1                ; =1
100926624:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100926628:      ldr x1, [x19, #0x10]
10092662c:      b   0x10092576c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11a0>
100926630:      mov x0, x19
100926634:      mov x1, x21
100926638:      mov w2, #0x1                ; =1
10092663c:      mov w3, #0x1                ; =1
100926640:      mov w4, #0x1                ; =1
100926644:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100926648:      b   0x100925638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x106c>
10092664c:      mov x0, x19
100926650:      mov x1, x20
100926654:      mov w2, #0x1                ; =1
100926658:      mov w3, #0x1                ; =1
10092665c:      mov w4, #0x1                ; =1
100926660:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100926664:      b   0x1009264bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ef0>
100926668:      adrp    x2, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
10092666c:      add x2, x2, #0xd10
100926670:      mov w0, #0x5                ; =5
100926674:      mov w1, #0x5                ; =5
100926678:      bl  0x100c9868c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
