/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/nested-records-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100980708 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth>:
100980708:      sub sp, sp, #0xf0
10098070c:      stp d9, d8, [sp, #0x80]
100980710:      stp x28, x27, [sp, #0x90]
100980714:      stp x26, x25, [sp, #0xa0]
100980718:      stp x24, x23, [sp, #0xb0]
10098071c:      stp x22, x21, [sp, #0xc0]
100980720:      stp x20, x19, [sp, #0xd0]
100980724:      stp x29, x30, [sp, #0xe0]
100980728:      add x29, sp, #0xe0
10098072c:      cmp w2, #0x3e9
100980730:      b.hs    0x100982654 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f4c>
100980734:      mov x20, x2
100980738:      mov x19, x1
10098073c:      mov x22, x0
100980740:      lsr x8, x0, #51
100980744:      cmp x8, #0xfff
100980748:      b.lo    0x100980760 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x58>
10098074c:      mov w8, #0x7ffc             ; =32764
100980750:      cmp x8, x22, lsr #48
100980754:      b.eq    0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100980758:      ands    x22, x22, #0xffffffffffff
10098075c:      b.eq    0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100980760:      and x8, x22, #0xfffffffffff00000
100980764:      lsr x9, x22, #47
100980768:      cmp x9, #0x0
10098076c:      ccmp    x8, #0x0, #0x4, eq
100980770:      b.eq    0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100980774:      tst x22, #0x3
100980778:      ccmp    x22, #0x7, #0x0, eq
10098077c:      b.ls    0x100980884 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
100980780:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
100980784:      add x8, x8, #0x360
100980788:      ldr x8, [x8]
10098078c:      cmn x8, #0x1
100980790:      b.eq    0x100981520 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe18>
100980794:      mrs x9, TPIDRRO_EL0
100980798:      and x9, x9, #0xfffffffffffffff8
10098079c:      ldr x0, [x9, x8, lsl #3]
1009807a0:      cbz x0, 0x100981520 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe18>
1009807a4:      lsr x1, x22, #20
1009807a8:      ldr x8, [x0, #0x10]
1009807ac:      ldrb    w9, [x8, #0x28]
1009807b0:      tbz w9, #0x0, 0x1009807d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc8>
1009807b4:      ldr x9, [x8, #0x20]
1009807b8:      cmp x9, x1
1009807bc:      b.ne    0x1009807d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc8>
1009807c0:      ldp x9, x10, [x8]
1009807c4:      cmp x9, x22
1009807c8:      ccmp    x10, x22, #0x0, ls
1009807cc:      b.hi    0x10098084c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x144>
1009807d0:      ldrb    w9, [x8, #0x58]
1009807d4:      cbz w9, 0x1009807f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xec>
1009807d8:      ldr x9, [x8, #0x50]
1009807dc:      cmp x9, x1
1009807e0:      b.ne    0x1009807f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xec>
1009807e4:      ldp x9, x10, [x8, #0x30]
1009807e8:      cmp x9, x22
1009807ec:      ccmp    x10, x22, #0x0, ls
1009807f0:      b.hi    0x100980840 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x138>
1009807f4:      ldrb    w9, [x8, #0x88]
1009807f8:      cbz w9, 0x100980818 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x110>
1009807fc:      ldr x9, [x8, #0x80]
100980800:      cmp x9, x1
100980804:      b.ne    0x100980818 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x110>
100980808:      ldp x9, x10, [x8, #0x60]
10098080c:      cmp x9, x22
100980810:      ccmp    x10, x22, #0x0, ls
100980814:      b.hi    0x100980848 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x140>
100980818:      ldrb    w9, [x8, #0xb8]
10098081c:      cbz w9, 0x100980858 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x150>
100980820:      ldr x9, [x8, #0xb0]
100980824:      cmp x9, x1
100980828:      b.ne    0x100980858 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x150>
10098082c:      ldp x9, x10, [x8, #0x90]!
100980830:      cmp x9, x22
100980834:      ccmp    x10, x22, #0x0, ls
100980838:      b.hi    0x10098084c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x144>
10098083c:      b   0x100980858 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x150>
100980840:      add x8, x8, #0x30
100980844:      b   0x10098084c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x144>
100980848:      add x8, x8, #0x60
10098084c:      ldrb    w8, [x8, #0x19]
100980850:      cmp w8, #0xff
100980854:      b.ne    0x100980864 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x15c>
100980858:      mov x0, x22
10098085c:      bl  0x1002bbee0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
100980860:      and w8, w0, #0xff
100980864:      cbz w8, 0x100980884 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
100980868:      ldurb   w8, [x22, #-0x8]
10098086c:      ldurb   w9, [x22, #-0x7]
100980870:      mov w10, #0x82              ; =130
100980874:      and w9, w9, w10
100980878:      cmp w9, #0x2
10098087c:      ccmp    w8, #0x1, #0x0, eq
100980880:      b.eq    0x100980afc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3f4>
100980884:      mov x0, x22
100980888:      bl  0x1009879d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
10098088c:      mov x8, x0
100980890:      cbz x0, 0x100980924 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x21c>
100980894:      ldrb    w9, [x8]
100980898:      cmp w9, #0x1
10098089c:      b.ne    0x1009809b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2ac>
1009808a0:      ldrsb   w9, [x8, #0x1]
1009808a4:      mov x0, x8
1009808a8:      tbz w9, #0x1f, 0x1009809f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2ec>
1009808ac:      mov x21, x8
1009808b0:      ldr x22, [x8, #0x8]
1009808b4:      mov x0, x22
1009808b8:      bl  0x1009879d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1009808bc:      cbz x0, 0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1009808c0:      ldrb    w8, [x0]
1009808c4:      cmp w8, #0x1
1009808c8:      b.ne    0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1009808cc:      ldrsb   w8, [x0, #0x1]
1009808d0:      tbz w8, #0x1f, 0x1009809ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2a4>
1009808d4:      mov w23, #0x1               ; =1
1009808d8:      ldr x22, [x0, #0x8]
1009808dc:      mov x0, x22
1009808e0:      bl  0x1009879d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1009808e4:      cbz x0, 0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1009808e8:      ldrb    w8, [x0]
1009808ec:      cmp w8, #0x1
1009808f0:      b.ne    0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1009808f4:      cmp w23, #0x3f
1009808f8:      b.hi    0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1009808fc:      add w23, w23, #0x1
100980900:      ldrsb   w8, [x0, #0x1]
100980904:      tbnz    w8, #0x1f, 0x1009808d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d0>
100980908:      mov x8, x21
10098090c:      str x22, [x21, #0x8]
100980910:      ldrb    w9, [x21, #0x1]
100980914:      orr w9, w9, #0x80
100980918:      strb    w9, [x21, #0x1]
10098091c:      ldrb    w9, [x0]
100980920:      b   0x1009809b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2b0>
100980924:      mov x21, x8
100980928:      adrp    x8, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
10098092c:      add x8, x8, #0xb34
100980930:      ldaprb  w8, [x8]
100980934:      cbz w8, 0x100980964 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x25c>
100980938:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
10098093c:      add x8, x8, #0xef8
100980940:      ldapr   x9, [x8]
100980944:      cmp x9, x22
100980948:      b.hi    0x100980964 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x25c>
10098094c:      ldapur  x8, [x8, #0x8]
100980950:      cmp x8, x22
100980954:      b.lo    0x100980964 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x25c>
100980958:      mov x0, x22
10098095c:      bl  0x100a15a18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100980960:      tbnz    w0, #0x0, 0x1009809a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2a0>
100980964:      adrp    x8, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
100980968:      add x8, x8, #0x620
10098096c:      ldaprb  w8, [x8]
100980970:      cbz w8, 0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100980974:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
100980978:      add x8, x8, #0xc08
10098097c:      ldapr   x8, [x8]
100980980:      cmp x8, x22
100980984:      b.hi    0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100980988:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
10098098c:      add x8, x8, #0xc10
100980990:      ldapr   x8, [x8]
100980994:      cmp x8, x22
100980998:      b.lo    0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
10098099c:      mov x0, x22
1009809a0:      bl  0x100948dac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
1009809a4:      tbz w0, #0x0, 0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1009809a8:      mov x0, #0x0                ; =0
1009809ac:      mov x8, x21
1009809b0:      b   0x1009809f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2ec>
1009809b4:      mov x0, x8
1009809b8:      cmp w9, #0x1
1009809bc:      b.eq    0x1009809f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2ec>
1009809c0:      cmp w9, #0x9
1009809c4:      b.ne    0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1009809c8:      ldr w8, [x22, #0x4]
1009809cc:      mov w9, #0x5841             ; =22593
1009809d0:      movk    w9, #0x4c5a, lsl #16
1009809d4:      cmp w8, w9
1009809d8:      b.ne    0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1009809dc:      mov x0, x22
1009809e0:      bl  0x1008ae598 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape22force_materialize_lazy>
1009809e4:      mov x22, x0
1009809e8:      str x0, [sp, #0x18]
1009809ec:      cbnz    x0, 0x100980b24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x41c>
1009809f0:      b   0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
1009809f4:      ldp w10, w9, [x22]
1009809f8:      cmp w10, w9
1009809fc:      b.ls    0x100980a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x314>
100980a00:      cbz x8, 0x100980a2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x324>
100980a04:      ldr w8, [x0, #0x4]
100980a08:      lsl x9, x9, #3
100980a0c:      add x9, x9, #0x10
100980a10:      cmp x9, x8
100980a14:      b.ne    0x100980a2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x324>
100980a18:      b   0x100980b20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x418>
100980a1c:      mov w8, #0xe100             ; =57600
100980a20:      movk    w8, #0x5f5, lsl #16
100980a24:      cmp w10, w8
100980a28:      b.ls    0x100980b20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x418>
100980a2c:      adrp    x8, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
100980a30:      add x8, x8, #0xb34
100980a34:      ldaprb  w8, [x8]
100980a38:      cbz w8, 0x100980a68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x360>
100980a3c:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
100980a40:      add x8, x8, #0xef8
100980a44:      ldapr   x9, [x8]
100980a48:      cmp x9, x22
100980a4c:      b.hi    0x100980a68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x360>
100980a50:      ldapur  x8, [x8, #0x8]
100980a54:      cmp x8, x22
100980a58:      b.lo    0x100980a68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x360>
100980a5c:      mov x0, x22
100980a60:      bl  0x100a15a18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100980a64:      tbnz    w0, #0x0, 0x100980b20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x418>
100980a68:      adrp    x8, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
100980a6c:      add x8, x8, #0x620
100980a70:      ldaprb  w8, [x8]
100980a74:      cbz w8, 0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100980a78:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
100980a7c:      add x8, x8, #0xc08
100980a80:      ldapr   x8, [x8]
100980a84:      cmp x8, x22
100980a88:      b.hi    0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100980a8c:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
100980a90:      add x8, x8, #0xc10
100980a94:      ldapr   x8, [x8]
100980a98:      cmp x8, x22
100980a9c:      b.lo    0x100980aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a4>
100980aa0:      mov x0, x22
100980aa4:      bl  0x100948dac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
100980aa8:      tbnz    w0, #0x0, 0x100980b20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x418>
100980aac:      ldr x1, [x19, #0x10]
100980ab0:      ldr x8, [x19]
100980ab4:      sub x8, x8, x1
100980ab8:      cmp x8, #0x1
100980abc:      b.ls    0x100981814 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x110c>
100980ac0:      ldr x8, [x19, #0x8]
100980ac4:      mov w9, #0x5d5b             ; =23899
100980ac8:      strh    w9, [x8, x1]
100980acc:      ldr x8, [x19, #0x10]
100980ad0:      add x8, x8, #0x2
100980ad4:      str x8, [x19, #0x10]
100980ad8:      ldp x29, x30, [sp, #0xe0]
100980adc:      ldp x20, x19, [sp, #0xd0]
100980ae0:      ldp x22, x21, [sp, #0xc0]
100980ae4:      ldp x24, x23, [sp, #0xb0]
100980ae8:      ldp x26, x25, [sp, #0xa0]
100980aec:      ldp x28, x27, [sp, #0x90]
100980af0:      ldp d9, d8, [sp, #0x80]
100980af4:      add sp, sp, #0xf0
100980af8:      ret
100980afc:      ldr w8, [x22]
100980b00:      mov w9, #0xe100             ; =57600
100980b04:      movk    w9, #0x5f5, lsl #16
100980b08:      orr w9, w9, #0x1
100980b0c:      cmp w8, w9
100980b10:      b.hs    0x100980884 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
100980b14:      ldr w9, [x22, #0x4]
100980b18:      cmp w8, w9
100980b1c:      b.hi    0x100980884 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
100980b20:      str x22, [sp, #0x18]
100980b24:      mov x0, x22
100980b28:      bl  0x10097f564 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify17array_get_to_json>
100980b2c:      tbz w0, #0x0, 0x100980bc4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4bc>
100980b30:      fmov    x21, d0
100980b34:      mov w8, #0x7ffd             ; =32765
100980b38:      cmp x8, x21, lsr #48
100980b3c:      b.ne    0x1009814d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdcc>
100980b40:      and x21, x21, #0xffffffffffff
100980b44:      sub x8, x21, #0x100, lsl #12 ; =0x100000
100980b48:      mov x9, #0x7ffffff00000     ; =140737487306752
100980b4c:      cmp x8, x9
100980b50:      b.hs    0x1009814fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
100980b54:      ldurb   w8, [x21, #-0x8]
100980b58:      sub w8, w8, #0x1
100980b5c:      cmp w8, #0x1
100980b60:      b.hi    0x1009814fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
100980b64:      adrp    x8, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
100980b68:      add x8, x8, #0xb34
100980b6c:      ldaprb  w8, [x8]
100980b70:      cbz w8, 0x100980ba8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4a0>
100980b74:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
100980b78:      add x8, x8, #0xef8
100980b7c:      ldapr   x9, [x8]
100980b80:      cmp x21, x9
100980b84:      b.lo    0x100980ba8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4a0>
100980b88:      ldapur  x8, [x8, #0x8]
100980b8c:      cmp x21, x8
100980b90:      b.hi    0x100980ba8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4a0>
100980b94:      mov x0, x21
100980b98:      mov.16b v8, v0
100980b9c:      bl  0x100a15a18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100980ba0:      mov.16b v0, v8
100980ba4:      tbnz    w0, #0x0, 0x1009814fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
100980ba8:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
100980bac:      add x0, x0, #0x3a8
100980bb0:      ldr x8, [x0]
100980bb4:      blr x8
100980bb8:      mov w8, #0x1                ; =1
100980bbc:      strb    w8, [x0]
100980bc0:      b   0x1009814fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
100980bc4:      mov x0, x22
100980bc8:      bl  0x1009879d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100980bcc:      cbz x0, 0x100980bfc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
100980bd0:      ldrb    w8, [x0]
100980bd4:      cmp w8, #0x1
100980bd8:      b.ne    0x100980bfc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
100980bdc:      ldrh    w21, [x0, #0x2]
100980be0:      tbnz    w21, #0xa, 0x100980bfc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
100980be4:      ldp w8, w9, [x22]
100980be8:      cmp w8, w9
100980bec:      b.hi    0x100980bfc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
100980bf0:      mov x0, x22
100980bf4:      bl  0x100986f24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header35array_has_named_properties_resolved>
100980bf8:      tbz w0, #0x0, 0x100981538 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe30>
100980bfc:      adrp    x0, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
100980c00:      add x0, x0, #0x740
100980c04:      add x1, sp, #0x18
100980c08:      bl  0x100138248 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths_0bEB2j_>
100980c0c:      tbnz    w0, #0x0, 0x10098272c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2024>
100980c10:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
100980c14:      add x0, x0, #0x390
100980c18:      ldr x8, [x0]
100980c1c:      blr x8
100980c20:      mov x24, x0
100980c24:      ldrb    w8, [x0, #0x20]
100980c28:      cbnz    w8, 0x1009823ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ce4>
100980c2c:      ldr x8, [x24]
100980c30:      cbnz    x8, 0x100982410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d08>
100980c34:      mov x8, #-0x1               ; =-1
100980c38:      str x8, [x24]
100980c3c:      mov x0, x24
100980c40:      ldr x8, [x0, #0x8]!
100980c44:      ldr x21, [x24, #0x18]
100980c48:      cmp x21, x8
100980c4c:      b.ne    0x100980c54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x54c>
100980c50:      bl  0x100cdca00 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecjE8grow_oneCs3HfcutmYuk_10swc_common>
100980c54:      ldr x8, [x24, #0x10]
100980c58:      str x22, [x8, x21, lsl #3]
100980c5c:      add x8, x21, #0x1
100980c60:      str x8, [x24, #0x18]
100980c64:      ldr x8, [x24]
100980c68:      add x8, x8, #0x1
100980c6c:      str x8, [x24]
100980c70:      ldr w8, [x22]
100980c74:      str x8, [sp, #0x10]
100980c78:      mov x0, x22
100980c7c:      bl  0x1009879d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100980c80:      cbz x0, 0x100980c8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x584>
100980c84:      ldrh    w8, [x0, #0x2]
100980c88:      tbnz    w8, #0xa, 0x100980cc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5b8>
100980c8c:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7fb58>
100980c90:      ldrb    w8, [x8, #0x9c0]
100980c94:      cbnz    w8, 0x100980cc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5b8>
100980c98:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7fb58>
100980c9c:      ldrb    w8, [x8, #0x790]
100980ca0:      cbnz    w8, 0x100980cc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5b8>
100980ca4:      ldp w8, w9, [x22]
100980ca8:      cmp w8, w9
100980cac:      b.hi    0x100980cc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5b8>
100980cb0:      mov x0, x22
100980cb4:      bl  0x1004d612c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object15prototype_chain23object_static_prototype>
100980cb8:      cmp x0, #0x1
100980cbc:      b.ne    0x100981680 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf78>
100980cc0:      adrp    x27, 0x101130000 <_perry_global_baseline_worker_ts__1>
100980cc4:      add x27, x27, #0x360
100980cc8:      ldr x8, [x27]
100980ccc:      cmn x8, #0x1
100980cd0:      b.eq    0x100980d04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5fc>
100980cd4:      mrs x9, TPIDRRO_EL0
100980cd8:      and x9, x9, #0xfffffffffffffff8
100980cdc:      ldr x8, [x9, x8, lsl #3]
100980ce0:      cbz x8, 0x100980d04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5fc>
100980ce4:      ldr x8, [x8, #0x19e8]
100980ce8:      cbz x8, 0x100980d04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5fc>
100980cec:      ldr x9, [x8]
100980cf0:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100980cf4:      cmp x9, x10
100980cf8:      b.hs    0x100982364 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c5c>
100980cfc:      ldr x21, [x8, #0x18]
100980d00:      b   0x100980d14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x60c>
100980d04:      adrp    x0, 0x1010d2000 <_anon.0c78480e1ec3114c482e9770ddf18575.1154+0x448>
100980d08:      add x0, x0, #0xfd8
100980d0c:      bl  0x1001358ec <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
100980d10:      mov x21, x0
100980d14:      stur    x21, [x29, #-0x68]
100980d18:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100980d1c:      stp x22, x8, [sp, #0x38]
100980d20:      mov w8, #0x1                ; =1
100980d24:      str x8, [sp, #0x30]
100980d28:      add x0, sp, #0x30
100980d2c:      bl  0x100947b20 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
100980d30:      mov x22, x0
100980d34:      ldr x23, [x19, #0x10]
100980d38:      ldr x8, [x19]
100980d3c:      cmp x8, x23
100980d40:      b.eq    0x1009823b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cac>
100980d44:      ldr x8, [x19, #0x8]
100980d48:      mov w9, #0x5b               ; =91
100980d4c:      strb    w9, [x8, x23]
100980d50:      add x23, x23, #0x1
100980d54:      str x23, [x19, #0x10]
100980d58:      ldr x8, [sp, #0x10]
100980d5c:      cbz w8, 0x100981438 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd30>
100980d60:      stp x21, x24, [sp]
100980d64:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
100980d68:      add x0, x0, #0x660
100980d6c:      ldr x8, [x0]
100980d70:      blr x8
100980d74:      mov x21, x0
100980d78:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
100980d7c:      add x0, x0, #0x348
100980d80:      ldr x8, [x0]
100980d84:      blr x8
100980d88:      mov x25, x0
100980d8c:      mov x26, #0x0               ; =0
100980d90:      b   0x100980da8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x6a0>
100980d94:      str xzr, [x8]
100980d98:      add x26, x26, #0x1
100980d9c:      ldr x8, [sp, #0x10]
100980da0:      cmp x8, x26
100980da4:      b.eq    0x100981430 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd28>
100980da8:      cbz x26, 0x100980dd0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x6c8>
100980dac:      ldr x23, [x19, #0x10]
100980db0:      ldr x8, [x19]
100980db4:      cmp x8, x23
100980db8:      b.eq    0x100981300 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbf8>
100980dbc:      ldr x8, [x19, #0x8]
100980dc0:      mov w9, #0x2c               ; =44
100980dc4:      strb    w9, [x8, x23]
100980dc8:      add x8, x23, #0x1
100980dcc:      str x8, [x19, #0x10]
100980dd0:      ldr x8, [x27]
100980dd4:      cmn x8, #0x1
100980dd8:      b.eq    0x100980e3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x734>
100980ddc:      mrs x9, TPIDRRO_EL0
100980de0:      and x9, x9, #0xfffffffffffffff8
100980de4:      ldr x8, [x9, x8, lsl #3]
100980de8:      cbz x8, 0x100980e3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x734>
100980dec:      ldr x8, [x8, #0x19e8]
100980df0:      cbz x8, 0x100980e3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x734>
100980df4:      ldr x9, [x8]
100980df8:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100980dfc:      cmp x9, x10
100980e00:      b.hs    0x1009825cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
100980e04:      add x10, x9, #0x1
100980e08:      str x10, [x8]
100980e0c:      ldr x10, [x8, #0x18]
100980e10:      cmp x22, x10
100980e14:      b.hs    0x1009825c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
100980e18:      ldr x10, [x8, #0x10]
100980e1c:      mov w11, #0x18              ; =24
100980e20:      madd    x10, x22, x11, x10
100980e24:      ldr x11, [x10]
100980e28:      cmp x11, #0x1
100980e2c:      b.ne    0x1009825d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ed0>
100980e30:      ldr x0, [x10, #0x8]
100980e34:      str x9, [x8]
100980e38:      b   0x100980e88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x780>
100980e3c:      ldrb    w8, [x21, #0x20]
100980e40:      cbnz    w8, 0x10098131c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc14>
100980e44:      ldr x8, [x21]
100980e48:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100980e4c:      cmp x8, x9
100980e50:      b.hs    0x100982074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
100980e54:      add x9, x8, #0x1
100980e58:      str x9, [x21]
100980e5c:      ldr x9, [x21, #0x18]
100980e60:      cmp x22, x9
100980e64:      b.hs    0x1009825c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
100980e68:      ldr x9, [x21, #0x10]
100980e6c:      mov w10, #0x18              ; =24
100980e70:      madd    x9, x22, x10, x9
100980e74:      ldr x10, [x9]
100980e78:      cmp x10, #0x1
100980e7c:      b.ne    0x1009820c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19bc>
100980e80:      ldr x0, [x9, #0x8]
100980e84:      str x8, [x21]
100980e88:      mov x1, x26
100980e8c:      bl  0x1005f13a4 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime5array8indexing11proto_chain14array_spec_get>
100980e90:      mov.16b v8, v0
100980e94:      fmov    x23, d8
100980e98:      mov x8, #0x1                ; =1
100980e9c:      movk    x8, #0x7ffc, lsl #48
100980ea0:      cmp x23, x8
100980ea4:      mov x8, #0x10               ; =16
100980ea8:      movk    x8, #0x7ffc, lsl #48
100980eac:      ccmp    x23, x8, #0x4, ne
100980eb0:      b.ne    0x100980ee8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7e0>
100980eb4:      ldr x1, [x19, #0x10]
100980eb8:      ldr x8, [x19]
100980ebc:      sub x8, x8, x1
100980ec0:      cmp x8, #0x3
100980ec4:      b.ls    0x1009812e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbdc>
100980ec8:      ldr x8, [x19, #0x8]
100980ecc:      mov w9, #0x756e             ; =30062
100980ed0:      movk    w9, #0x6c6c, lsl #16
100980ed4:      str w9, [x8, x1]
100980ed8:      ldr x8, [x19, #0x10]
100980edc:      add x8, x8, #0x4
100980ee0:      str x8, [x19, #0x10]
100980ee4:      b   0x100980d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x690>
100980ee8:      and x24, x23, #0xffff000000000000
100980eec:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100980ef0:      cmp x24, x8
100980ef4:      b.ne    0x100980f2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x824>
100980ef8:      and x8, x23, #0xffffffffffff
100980efc:      cmp x8, #0x100, lsl #12     ; =0x100000
100980f00:      b.lo    0x100980f18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x810>
100980f04:      ldr w8, [x8, #0xc]
100980f08:      mov w9, #0x4f53             ; =20307
100980f0c:      movk    w9, #0x434c, lsl #16
100980f10:      cmp w8, w9
100980f14:      b.eq    0x100980eb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7ac>
100980f18:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100980f1c:      cmp x24, x8
100980f20:      b.ne    0x100980f58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x850>
100980f24:      and x0, x23, #0xffffffffffff
100980f28:      b   0x100980f80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x878>
100980f2c:      lsr x8, x23, #52
100980f30:      cbnz    x8, 0x100981010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100980f34:      and x8, x23, #0x7
100980f38:      cbz x23, 0x100980f64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x85c>
100980f3c:      cbnz    x8, 0x100980f64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x85c>
100980f40:      mov x0, x23
100980f44:      bl  0x100985490 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
100980f48:      mov x8, x23
100980f4c:      tbnz    w0, #0x0, 0x100980efc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7f4>
100980f50:      mov x8, #0x0                ; =0
100980f54:      b   0x100980f64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x85c>
100980f58:      lsr x8, x23, #52
100980f5c:      cbnz    x8, 0x100981010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100980f60:      and x8, x23, #0x7
100980f64:      cbz x23, 0x100981010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100980f68:      cbnz    x8, 0x100981010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100980f6c:      mov x0, x23
100980f70:      bl  0x100985490 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
100980f74:      mov x8, x0
100980f78:      mov x0, x23
100980f7c:      tbz w8, #0x0, 0x100981010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100980f80:      cmp x0, #0x100, lsl #12     ; =0x100000
100980f84:      b.lo    0x100981010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100980f88:      adrp    x8, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
100980f8c:      add x8, x8, #0xa58
100980f90:      ldaprb  w8, [x8]
100980f94:      cbz w8, 0x100981010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100980f98:      lsr x8, x0, #3
100980f9c:      mov x9, #0x7c15             ; =31765
100980fa0:      movk    x9, #0x7f4a, lsl #16
100980fa4:      movk    x9, #0x79b9, lsl #32
100980fa8:      movk    x9, #0x9e37, lsl #48
100980fac:      mul x8, x8, x9
100980fb0:      lsr x9, x8, #54
100980fb4:      lsr x10, x8, #60
100980fb8:      adrp    x11, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
100980fbc:      add x11, x11, #0xa60
100980fc0:      add x10, x11, x10, lsl #3
100980fc4:      ldapr   x10, [x10]
100980fc8:      lsr x9, x10, x9
100980fcc:      tbz w9, #0x0, 0x100981010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100980fd0:      lsr x9, x8, #44
100980fd4:      ubfx    x10, x8, #50, #4
100980fd8:      add x10, x11, x10, lsl #3
100980fdc:      ldapr   x10, [x10]
100980fe0:      lsr x9, x10, x9
100980fe4:      tbz w9, #0x0, 0x100981010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100980fe8:      lsr x9, x8, #34
100980fec:      ubfx    x8, x8, #40, #4
100980ff0:      adrp    x10, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
100980ff4:      add x10, x10, #0xa60
100980ff8:      add x8, x10, x8, lsl #3
100980ffc:      ldapr   x8, [x8]
100981000:      lsr x8, x8, x9
100981004:      tbz w8, #0x0, 0x100981010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x908>
100981008:      bl  0x1009fdc40 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6symbol25is_registered_symbol_slow>
10098100c:      tbnz    w0, #0x0, 0x100980eb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7ac>
100981010:      ldr x8, [x27]
100981014:      cmn x8, #0x1
100981018:      b.eq    0x100981048 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x940>
10098101c:      mrs x9, TPIDRRO_EL0
100981020:      and x9, x9, #0xfffffffffffffff8
100981024:      ldr x8, [x9, x8, lsl #3]
100981028:      cbz x8, 0x100981048 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x940>
10098102c:      ldr x8, [x8, #0x19e8]
100981030:      cbz x8, 0x100981048 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x940>
100981034:      ldr x9, [x8], #0x18
100981038:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10098103c:      cmp x9, x10
100981040:      b.lo    0x100981064 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x95c>
100981044:      b   0x100982364 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c5c>
100981048:      ldrb    w8, [x21, #0x20]
10098104c:      cbnz    w8, 0x100981374 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc6c>
100981050:      ldr x9, [x21]
100981054:      add x8, x21, #0x18
100981058:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10098105c:      cmp x9, x10
100981060:      b.hs    0x1009820e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19d8>
100981064:      ldr x28, [x8]
100981068:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7fb58>
10098106c:      add x8, x8, #0x9c4
100981070:      ldr w8, [x8]
100981074:      cbz w8, 0x100981080 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x978>
100981078:      mov x0, x23
10098107c:      bl  0x1005ba638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
100981080:      ldr x8, [x27]
100981084:      cmn x8, #0x1
100981088:      b.eq    0x1009810ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9e4>
10098108c:      mrs x9, TPIDRRO_EL0
100981090:      and x9, x9, #0xfffffffffffffff8
100981094:      ldr x8, [x9, x8, lsl #3]
100981098:      cbz x8, 0x1009810ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9e4>
10098109c:      ldr x24, [x8, #0x19e8]
1009810a0:      cbz x24, 0x1009810ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9e4>
1009810a4:      ldr x8, [x24]
1009810a8:      cbnz    x8, 0x100982398 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c90>
1009810ac:      mov x8, #-0x1               ; =-1
1009810b0:      str x8, [x24]
1009810b4:      mov x0, x24
1009810b8:      ldr x8, [x0, #0x8]!
1009810bc:      ldr x23, [x24, #0x18]
1009810c0:      cmp x23, x8
1009810c4:      b.ne    0x1009810cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9c4>
1009810c8:      bl  0x100cada44 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecTyyNtNtCseUPtmYZaE8V_5gimli6common13EhFrameOffsetEE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1009810cc:      ldr x8, [x24, #0x10]
1009810d0:      mov w9, #0x18               ; =24
1009810d4:      madd    x8, x23, x9, x8
1009810d8:      str xzr, [x8]
1009810dc:      str d8, [x8, #0x8]
1009810e0:      add x8, x23, #0x1
1009810e4:      str x8, [x24, #0x18]
1009810e8:      b   0x10098113c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xa34>
1009810ec:      ldrb    w8, [x21, #0x20]
1009810f0:      cbnz    w8, 0x1009813a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xca0>
1009810f4:      ldr x8, [x21]
1009810f8:      cbnz    x8, 0x1009820ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19e4>
1009810fc:      mov x8, #-0x1               ; =-1
100981100:      str x8, [x21]
100981104:      ldr x23, [x21, #0x18]
100981108:      ldr x8, [x21, #0x8]
10098110c:      cmp x23, x8
100981110:      b.ne    0x10098111c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xa14>
100981114:      add x0, x21, #0x8
100981118:      bl  0x100cada44 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecTyyNtNtCseUPtmYZaE8V_5gimli6common13EhFrameOffsetEE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
10098111c:      ldr x8, [x21, #0x10]
100981120:      mov w9, #0x18               ; =24
100981124:      madd    x8, x23, x9, x8
100981128:      str xzr, [x8]
10098112c:      str d8, [x8, #0x8]
100981130:      add x8, x23, #0x1
100981134:      str x8, [x21, #0x18]
100981138:      mov x24, x21
10098113c:      ldr x8, [x24]
100981140:      add x8, x8, #0x1
100981144:      str x8, [x24]
100981148:      str x26, [sp, #0x60]
10098114c:      ldrb    w8, [x25, #0x20]
100981150:      cbnz    w8, 0x10098134c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc44>
100981154:      ldr x8, [x25]
100981158:      cbnz    x8, 0x1009820d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19cc>
10098115c:      mov x8, #-0x1               ; =-1
100981160:      str x8, [x25]
100981164:      str xzr, [x25, #0x18]
100981168:      add x8, sp, #0x60
10098116c:      str x8, [sp, #0x30]
100981170:      adrp    x8, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
100981174:      add x8, x8, #0xf80
100981178:      str x8, [sp, #0x38]
10098117c:      add x0, x25, #0x8
100981180:      add x3, sp, #0x30
100981184:      adrp    x1, 0x1010a3000 <_anon.58120679d426c7dccd15bda76f596bde.683>
100981188:      add x1, x1, #0x590
10098118c:      adrp    x2, 0x100eef000 <_anon.58120679d426c7dccd15bda76f596bde.60+0x46>
100981190:      add x2, x2, #0x994
100981194:      bl  0x10002cf10 <__RNvNtCsjgY6bXVaRmE_4core3fmt5write>
100981198:      ldr x8, [x25]
10098119c:      add x8, x8, #0x1
1009811a0:      str x8, [x25]
1009811a4:      ldr x8, [x27]
1009811a8:      cmn x8, #0x1
1009811ac:      b.eq    0x100981224 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb1c>
1009811b0:      mrs x9, TPIDRRO_EL0
1009811b4:      and x9, x9, #0xfffffffffffffff8
1009811b8:      ldr x8, [x9, x8, lsl #3]
1009811bc:      cbz x8, 0x100981224 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb1c>
1009811c0:      ldr x8, [x8, #0x19e8]
1009811c4:      cbz x8, 0x100981224 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb1c>
1009811c8:      ldr x9, [x8]
1009811cc:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1009811d0:      cmp x9, x10
1009811d4:      b.hs    0x1009825cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
1009811d8:      add x10, x9, #0x1
1009811dc:      str x10, [x8]
1009811e0:      ldr x10, [x8, #0x18]
1009811e4:      cmp x23, x10
1009811e8:      b.hs    0x1009825c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
1009811ec:      ldr x10, [x8, #0x10]
1009811f0:      mov w11, #0x18              ; =24
1009811f4:      madd    x10, x23, x11, x10
1009811f8:      ldr x11, [x10]
1009811fc:      cbnz    x11, 0x1009823a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c9c>
100981200:      ldr d0, [x10, #0x8]
100981204:      str x9, [x8]
100981208:      add w1, w20, #0x1
10098120c:      mov x0, x19
100981210:      bl  0x1009827b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
100981214:      ldr x8, [x27]
100981218:      cmn x8, #0x1
10098121c:      b.ne    0x100981284 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb7c>
100981220:      b   0x1009812b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbb0>
100981224:      ldrb    w8, [x21, #0x20]
100981228:      cbnz    w8, 0x1009813d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xcc8>
10098122c:      ldr x8, [x21]
100981230:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100981234:      cmp x8, x9
100981238:      b.hs    0x100982074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
10098123c:      add x9, x8, #0x1
100981240:      str x9, [x21]
100981244:      ldr x9, [x21, #0x18]
100981248:      cmp x23, x9
10098124c:      b.hs    0x1009825c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
100981250:      ldr x9, [x21, #0x10]
100981254:      mov w10, #0x18              ; =24
100981258:      madd    x9, x23, x10, x9
10098125c:      ldr x10, [x9]
100981260:      cbnz    x10, 0x1009820f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19f0>
100981264:      ldr d0, [x9, #0x8]
100981268:      str x8, [x21]
10098126c:      add w1, w20, #0x1
100981270:      mov x0, x19
100981274:      bl  0x1009827b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
100981278:      ldr x8, [x27]
10098127c:      cmn x8, #0x1
100981280:      b.eq    0x1009812b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbb0>
100981284:      mrs x9, TPIDRRO_EL0
100981288:      and x9, x9, #0xfffffffffffffff8
10098128c:      ldr x8, [x9, x8, lsl #3]
100981290:      cbz x8, 0x1009812b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbb0>
100981294:      ldr x8, [x8, #0x19e8]
100981298:      cbz x8, 0x1009812b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbb0>
10098129c:      ldr x9, [x8]
1009812a0:      cbnz    x9, 0x100982370 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c68>
1009812a4:      ldr x9, [x8, #0x18]
1009812a8:      cmp x28, x9
1009812ac:      b.hi    0x100980d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x68c>
1009812b0:      str x28, [x8, #0x18]
1009812b4:      b   0x100980d94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x68c>
1009812b8:      ldrb    w8, [x21, #0x20]
1009812bc:      cbnz    w8, 0x100981400 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xcf8>
1009812c0:      ldr x8, [x21]
1009812c4:      cbnz    x8, 0x100981424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd1c>
1009812c8:      add x8, x21, #0x18
1009812cc:      ldr x8, [x8]
1009812d0:      cmp x28, x8
1009812d4:      b.hi    0x100980d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x690>
1009812d8:      add x8, x21, #0x18
1009812dc:      str x28, [x8]
1009812e0:      b   0x100980d98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x690>
1009812e4:      mov x0, x19
1009812e8:      mov w2, #0x4                ; =4
1009812ec:      mov w3, #0x1                ; =1
1009812f0:      mov w4, #0x1                ; =1
1009812f4:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009812f8:      ldr x1, [x19, #0x10]
1009812fc:      b   0x100980ec8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7c0>
100981300:      mov x0, x19
100981304:      mov x1, x23
100981308:      mov w2, #0x1                ; =1
10098130c:      mov w3, #0x1                ; =1
100981310:      mov w4, #0x1                ; =1
100981314:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100981318:      b   0x100980dbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x6b4>
10098131c:      cmp w8, #0x2
100981320:      b.eq    0x100982424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
100981324:      mov x0, x21
100981328:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
10098132c:      add x1, x1, #0xeec
100981330:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100981334:      strb    wzr, [x21, #0x20]
100981338:      ldr x8, [x21]
10098133c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100981340:      cmp x8, x9
100981344:      b.lo    0x100980e54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x74c>
100981348:      b   0x100982074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
10098134c:      cmp w8, #0x2
100981350:      b.eq    0x100982424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
100981354:      mov x0, x25
100981358:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
10098135c:      add x1, x1, #0xeec
100981360:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100981364:      strb    wzr, [x25, #0x20]
100981368:      ldr x8, [x25]
10098136c:      cbz x8, 0x10098115c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xa54>
100981370:      b   0x1009820d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19cc>
100981374:      cmp w8, #0x2
100981378:      b.eq    0x100982424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
10098137c:      mov x0, x21
100981380:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
100981384:      add x1, x1, #0xeec
100981388:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10098138c:      strb    wzr, [x21, #0x20]
100981390:      ldr x9, [x21]
100981394:      add x8, x21, #0x18
100981398:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10098139c:      cmp x9, x10
1009813a0:      b.lo    0x100981064 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x95c>
1009813a4:      b   0x1009820e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19d8>
1009813a8:      cmp w8, #0x2
1009813ac:      b.eq    0x100982424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
1009813b0:      mov x0, x21
1009813b4:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1009813b8:      add x1, x1, #0xeec
1009813bc:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1009813c0:      strb    wzr, [x21, #0x20]
1009813c4:      ldr x8, [x21]
1009813c8:      cbz x8, 0x1009810fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9f4>
1009813cc:      b   0x1009820ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19e4>
1009813d0:      cmp w8, #0x2
1009813d4:      b.eq    0x100982424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
1009813d8:      mov x0, x21
1009813dc:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1009813e0:      add x1, x1, #0xeec
1009813e4:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1009813e8:      strb    wzr, [x21, #0x20]
1009813ec:      ldr x8, [x21]
1009813f0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1009813f4:      cmp x8, x9
1009813f8:      b.lo    0x10098123c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb34>
1009813fc:      b   0x100982074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
100981400:      cmp w8, #0x2
100981404:      b.eq    0x100982424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
100981408:      mov x0, x21
10098140c:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
100981410:      add x1, x1, #0xeec
100981414:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100981418:      strb    wzr, [x21, #0x20]
10098141c:      ldr x8, [x21]
100981420:      cbz x8, 0x1009812c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbc0>
100981424:      adrp    x0, 0x1010a5000 <_anon.58120679d426c7dccd15bda76f596bde.1139>
100981428:      add x0, x0, #0x2d0
10098142c:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100981430:      ldr x23, [x19, #0x10]
100981434:      ldp x21, x24, [sp]
100981438:      ldr x8, [x19]
10098143c:      cmp x8, x23
100981440:      b.eq    0x1009823d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cc8>
100981444:      ldr x8, [x19, #0x8]
100981448:      mov w9, #0x5d               ; =93
10098144c:      strb    w9, [x8, x23]
100981450:      add x8, x23, #0x1
100981454:      str x8, [x19, #0x10]
100981458:      ldr x8, [x27]
10098145c:      cmn x8, #0x1
100981460:      b.eq    0x10098149c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd94>
100981464:      mrs x9, TPIDRRO_EL0
100981468:      and x9, x9, #0xfffffffffffffff8
10098146c:      ldr x8, [x9, x8, lsl #3]
100981470:      cbz x8, 0x10098149c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd94>
100981474:      ldr x8, [x8, #0x19e8]
100981478:      cbz x8, 0x10098149c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd94>
10098147c:      ldr x9, [x8]
100981480:      cbnz    x9, 0x100982370 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c68>
100981484:      ldr x9, [x8, #0x18]
100981488:      cmp x21, x9
10098148c:      b.hi    0x100981494 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd8c>
100981490:      str x21, [x8, #0x18]
100981494:      str xzr, [x8]
100981498:      b   0x1009814ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xda4>
10098149c:      adrp    x0, 0x1010d2000 <_anon.0c78480e1ec3114c482e9770ddf18575.1154+0x448>
1009814a0:      add x0, x0, #0xfd8
1009814a4:      sub x1, x29, #0x68
1009814a8:      bl  0x100135cc8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
1009814ac:      ldrb    w8, [x24, #0x20]
1009814b0:      cbnz    w8, 0x10098241c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d14>
1009814b4:      ldr x8, [x24]
1009814b8:      cbnz    x8, 0x10098244c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d44>
1009814bc:      ldr x8, [x24, #0x18]
1009814c0:      cbz x8, 0x1009814cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdc4>
1009814c4:      sub x8, x8, #0x1
1009814c8:      str x8, [x24, #0x18]
1009814cc:      str xzr, [x24]
1009814d0:      b   0x100980ad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3d0>
1009814d4:      lsr x8, x21, #52
1009814d8:      cbnz    x8, 0x1009814fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
1009814dc:      cbz x21, 0x1009814fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
1009814e0:      and x8, x21, #0x7
1009814e4:      cbnz    x8, 0x1009814fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdf4>
1009814e8:      mov x0, x21
1009814ec:      mov.16b v8, v0
1009814f0:      bl  0x100985490 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
1009814f4:      mov.16b v0, v8
1009814f8:      tbnz    w0, #0x0, 0x100980b44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x43c>
1009814fc:      add w1, w20, #0x1
100981500:      mov x0, x19
100981504:      bl  0x1009827b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
100981508:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
10098150c:      add x0, x0, #0x3a8
100981510:      ldr x8, [x0]
100981514:      blr x8
100981518:      strb    wzr, [x0]
10098151c:      b   0x100980ad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3d0>
100981520:      bl  0x100caf8ac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100981524:      lsr x1, x22, #20
100981528:      ldr x8, [x0, #0x10]
10098152c:      ldrb    w9, [x8, #0x28]
100981530:      tbnz    w9, #0x0, 0x1009807b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xac>
100981534:      b   0x1009807d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc8>
100981538:      tbnz    w21, #0x7, 0x100981558 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe50>
10098153c:      mov x8, x22
100981540:      ldr w9, [x8], #0x8
100981544:      add x9, x8, x9, lsl #3
100981548:      stp x8, x9, [sp, #0x30]
10098154c:      add x0, sp, #0x30
100981550:      bl  0x10093bf78 <__RINvXs2J_NtNtCsjgY6bXVaRmE_4core5slice4iterINtB7_4IterdENtNtNtNtBb_4iter6traits8iterator8Iterator3allNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json25stringify_primitive_array8try_emit0EB1J_>
100981554:      cbz w0, 0x100980bfc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4f4>
100981558:      ldurh   w24, [x22, #-0x6]
10098155c:      ldr w21, [x22]
100981560:      ldr x20, [x19, #0x10]
100981564:      ldr x8, [x19]
100981568:      cmp x8, x20
10098156c:      b.eq    0x10098261c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f14>
100981570:      ldr x8, [x19, #0x8]
100981574:      mov w9, #0x5b               ; =91
100981578:      strb    w9, [x8, x20]
10098157c:      add x20, x20, #0x1
100981580:      str x20, [x19, #0x10]
100981584:      lsl x23, x21, #3
100981588:      tbnz    w24, #0x7, 0x100981600 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xef8>
10098158c:      cbz w21, 0x10098214c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a44>
100981590:      ldr d0, [x22, #0x8]
100981594:      fmov    x0, d0
100981598:      mov x8, #-0x7ffc000000000001 ; =-9222246136947933185
10098159c:      add x8, x0, x8
1009815a0:      cmp x8, #0x2
1009815a4:      b.lo    0x100982114 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a0c>
1009815a8:      mov x8, #0x4                ; =4
1009815ac:      movk    x8, #0x7ffc, lsl #48
1009815b0:      cmp x0, x8
1009815b4:      b.eq    0x100981830 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1128>
1009815b8:      mov x8, #0x3                ; =3
1009815bc:      movk    x8, #0x7ffc, lsl #48
1009815c0:      cmp x0, x8
1009815c4:      b.ne    0x100981850 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1148>
1009815c8:      ldr x8, [x19]
1009815cc:      sub x8, x8, x20
1009815d0:      cmp x8, #0x4
1009815d4:      b.ls    0x1009826ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1fe4>
1009815d8:      ldr x8, [x19, #0x8]
1009815dc:      add x8, x8, x20
1009815e0:      mov w9, #0x65               ; =101
1009815e4:      strb    w9, [x8, #0x4]
1009815e8:      mov w9, #0x6166             ; =24934
1009815ec:      movk    w9, #0x736c, lsl #16
1009815f0:      str w9, [x8]
1009815f4:      ldr x8, [x19, #0x10]
1009815f8:      add x8, x8, #0x5
1009815fc:      b   0x10098213c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a34>
100981600:      cbz w21, 0x10098214c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a44>
100981604:      ldr d0, [x22, #0x8]
100981608:      mov x0, x19
10098160c:      bl  0x100971010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
100981610:      cmp w21, #0x1
100981614:      b.eq    0x100982148 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a40>
100981618:      add x21, x22, #0x10
10098161c:      sub x22, x23, #0x8
100981620:      mov w23, #0x2c              ; =44
100981624:      ldr d0, [x21], #0x8
100981628:      ldr x20, [x19, #0x10]
10098162c:      ldr x8, [x19]
100981630:      cmp x8, x20
100981634:      b.eq    0x10098165c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf54>
100981638:      ldr x8, [x19, #0x8]
10098163c:      strb    w23, [x8, x20]
100981640:      add x8, x20, #0x1
100981644:      str x8, [x19, #0x10]
100981648:      mov x0, x19
10098164c:      bl  0x100971010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
100981650:      subs    x22, x22, #0x8
100981654:      b.ne    0x100981624 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf1c>
100981658:      b   0x100982148 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a40>
10098165c:      mov x0, x19
100981660:      mov x1, x20
100981664:      mov w2, #0x1                ; =1
100981668:      mov w3, #0x1                ; =1
10098166c:      mov w4, #0x1                ; =1
100981670:      mov.16b v8, v0
100981674:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100981678:      mov.16b v0, v8
10098167c:      b   0x100981638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf30>
100981680:      mov x0, x22
100981684:      mov x1, x19
100981688:      mov x2, x20
10098168c:      bl  0x100974620 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records8try_emit>
100981690:      tbz w0, #0x0, 0x1009816c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xfb8>
100981694:      adrp    x0, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
100981698:      add x0, x0, #0x740
10098169c:      ldp x29, x30, [sp, #0xe0]
1009816a0:      ldp x20, x19, [sp, #0xd0]
1009816a4:      ldp x22, x21, [sp, #0xc0]
1009816a8:      ldp x24, x23, [sp, #0xb0]
1009816ac:      ldp x26, x25, [sp, #0xa0]
1009816b0:      ldp x28, x27, [sp, #0x90]
1009816b4:      ldp d9, d8, [sp, #0x80]
1009816b8:      add sp, sp, #0xf0
1009816bc:      b   0x1001380f8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths3_0INtNtBZ_6option6OptionjEEB2j_>
1009816c0:      bl  0x100947a14 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
1009816c4:      str x0, [sp, #0x20]
1009816c8:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1009816cc:      stp x22, x8, [sp, #0x38]
1009816d0:      mov w8, #0x1                ; =1
1009816d4:      str x8, [sp, #0x30]
1009816d8:      add x0, sp, #0x30
1009816dc:      bl  0x100947b20 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1009816e0:      str x0, [sp, #0x28]
1009816e4:      add x8, sp, #0x28
1009816e8:      stur    x8, [x29, #-0x68]
1009816ec:      ldr x8, [sp, #0x10]
1009816f0:      cmp w8, #0x1
1009816f4:      b.ls    0x1009818e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1009816f8:      sub x0, x29, #0x68
1009816fc:      bl  0x10093d138 <__RNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths4_0B7_>
100981700:      fmov    x21, d0
100981704:      mov w8, #0x7ffd             ; =32765
100981708:      cmp x8, x21, lsr #48
10098170c:      b.ne    0x1009818bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11b4>
100981710:      and x22, x21, #0xffffffffffff
100981714:      cmp x22, #0x100, lsl #12    ; =0x100000
100981718:      b.lo    0x1009818e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
10098171c:      and x0, x21, #0xffffffffffff
100981720:      bl  0x100950d34 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4date17is_date_cell_addr>
100981724:      tbnz    w0, #0x0, 0x1009818e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
100981728:      ldr w8, [x22]
10098172c:      mov w9, #-0xff5f            ; =-65375
100981730:      cmp w8, w9
100981734:      b.eq    0x1009818e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
100981738:      sub x0, x29, #0x68
10098173c:      bl  0x10093d138 <__RNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths4_0B7_>
100981740:      bl  0x1009b4004 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins10formatting16boxed_primitives26boxed_primitive_json_value>
100981744:      cmp x0, #0x1
100981748:      b.eq    0x1009818e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
10098174c:      add x0, sp, #0x30
100981750:      mov x1, x21
100981754:      bl  0x100975f00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template27build_shape_prefix_template>
100981758:      ldr x8, [sp, #0x30]
10098175c:      cmn x8, #0x1
100981760:      b.eq    0x1009818e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11e0>
100981764:      ldr x21, [x19, #0x10]
100981768:      ldr x8, [x19]
10098176c:      cmp x8, x21
100981770:      b.eq    0x10098276c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2064>
100981774:      ldr x8, [x19, #0x8]
100981778:      mov w9, #0x5b               ; =91
10098177c:      strb    w9, [x8, x21]
100981780:      add x8, x21, #0x1
100981784:      str x8, [x19, #0x10]
100981788:      str xzr, [sp, #0x60]
10098178c:      adrp    x0, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
100981790:      add x0, x0, #0x678
100981794:      add x1, sp, #0x60
100981798:      bl  0x1001646ec <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
10098179c:      adrp    x24, 0x101130000 <_perry_global_baseline_worker_ts__1>
1009817a0:      add x24, x24, #0x360
1009817a4:      ldr x8, [x24]
1009817a8:      cmn x8, #0x1
1009817ac:      b.eq    0x100982458 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d50>
1009817b0:      mrs x9, TPIDRRO_EL0
1009817b4:      and x9, x9, #0xfffffffffffffff8
1009817b8:      ldr x8, [x9, x8, lsl #3]
1009817bc:      cbz x8, 0x100982458 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d50>
1009817c0:      ldr x8, [x8, #0x19e8]
1009817c4:      cbz x8, 0x100982458 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d50>
1009817c8:      ldr x9, [x8]
1009817cc:      mov x10, #0x7ffffffffffffffe ; =9223372036854775806
1009817d0:      cmp x9, x10
1009817d4:      b.hi    0x1009825cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
1009817d8:      ldr x10, [sp, #0x28]
1009817dc:      add x11, x9, #0x1
1009817e0:      str x11, [x8]
1009817e4:      ldr x11, [x8, #0x18]
1009817e8:      cmp x10, x11
1009817ec:      b.hs    0x1009825c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
1009817f0:      ldr x11, [x8, #0x10]
1009817f4:      mov w12, #0x18              ; =24
1009817f8:      madd    x10, x10, x12, x11
1009817fc:      ldr x11, [x10]
100981800:      cmp x11, #0x1
100981804:      b.ne    0x1009825d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ed0>
100981808:      ldr x0, [x10, #0x8]
10098180c:      str x9, [x8]
100981810:      b   0x100982468 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d60>
100981814:      mov x0, x19
100981818:      mov w2, #0x2                ; =2
10098181c:      mov w3, #0x1                ; =1
100981820:      mov w4, #0x1                ; =1
100981824:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100981828:      ldr x1, [x19, #0x10]
10098182c:      b   0x100980ac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3b8>
100981830:      ldr x8, [x19]
100981834:      sub x8, x8, x20
100981838:      cmp x8, #0x3
10098183c:      b.ls    0x10098270c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2004>
100981840:      ldr x8, [x19, #0x8]
100981844:      mov w9, #0x7274             ; =29300
100981848:      movk    w9, #0x6575, lsl #16
10098184c:      b   0x100982130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a28>
100981850:      and x8, x0, #0xffff000000000000
100981854:      mov x9, #0x7fff000000000000 ; =9223090561878065152
100981858:      cmp x8, x9
10098185c:      b.eq    0x100982108 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a00>
100981860:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
100981864:      cmp x8, x9
100981868:      b.ne    0x100982358 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c50>
10098186c:      strb    wzr, [sp, #0x64]
100981870:      str wzr, [sp, #0x60]
100981874:      add x1, sp, #0x60
100981878:      bl  0x100941fa8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime5value7jsvalueNtB2_7JSValue19short_string_to_buf>
10098187c:      mov x1, x0
100981880:      add x8, sp, #0x30
100981884:      add x0, sp, #0x60
100981888:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
10098188c:      ldr w8, [sp, #0x30]
100981890:      tbz w8, #0x0, 0x10098237c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c74>
100981894:      ldr x1, [x19, #0x10]
100981898:      ldr x8, [x19]
10098189c:      sub x8, x8, x1
1009818a0:      cmp x8, #0x3
1009818a4:      b.ls    0x100982750 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2048>
1009818a8:      ldr x8, [x19, #0x8]
1009818ac:      mov w9, #0x756e             ; =30062
1009818b0:      movk    w9, #0x6c6c, lsl #16
1009818b4:      str w9, [x8, x1]
1009818b8:      b   0x100982134 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a2c>
1009818bc:      lsr x8, x21, #52
1009818c0:      cbnz    x8, 0x1009818e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1009818c4:      cbz x21, 0x1009818e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1009818c8:      and x8, x21, #0x7
1009818cc:      cbnz    x8, 0x1009818e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1009818d0:      mov x0, x21
1009818d4:      bl  0x100985490 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
1009818d8:      mov x22, x21
1009818dc:      cbnz    w0, 0x100981714 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x100c>
1009818e0:      mov x8, #-0x1               ; =-1
1009818e4:      str x8, [sp, #0x30]
1009818e8:      ldr x21, [x19, #0x10]
1009818ec:      ldr x8, [x19]
1009818f0:      cmp x8, x21
1009818f4:      b.eq    0x100982694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f8c>
1009818f8:      ldr x8, [x19, #0x8]
1009818fc:      mov w9, #0x5b               ; =91
100981900:      strb    w9, [x8, x21]
100981904:      add x21, x21, #0x1
100981908:      str x21, [x19, #0x10]
10098190c:      ldr x8, [sp, #0x10]
100981910:      cbz w8, 0x100982084 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x197c>
100981914:      mov x23, #0x1               ; =1
100981918:      movk    x23, #0x7ffc, lsl #48
10098191c:      mov w28, #0x756e            ; =30062
100981920:      movk    w28, #0x6c6c, lsl #16
100981924:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
100981928:      add x0, x0, #0x660
10098192c:      ldr x8, [x0]
100981930:      blr x8
100981934:      mov x21, x0
100981938:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
10098193c:      add x0, x0, #0x3a8
100981940:      ldr x8, [x0]
100981944:      blr x8
100981948:      str x0, [sp, #0x8]
10098194c:      mov x22, #0x0               ; =0
100981950:      adrp    x25, 0x101130000 <_perry_global_baseline_worker_ts__1>
100981954:      add x25, x25, #0x360
100981958:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
10098195c:      mov w24, #0x18              ; =24
100981960:      b   0x1009819e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12dc>
100981964:      ldr x1, [x19, #0x10]
100981968:      ldr x8, [x19]
10098196c:      sub x8, x8, x1
100981970:      cmp x8, #0x3
100981974:      b.ls    0x1009819a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1298>
100981978:      ldr x8, [x19, #0x8]
10098197c:      str w28, [x8, x1]
100981980:      ldr x8, [x19, #0x10]
100981984:      add x8, x8, #0x4
100981988:      str x8, [x19, #0x10]
10098198c:      add x22, x22, #0x1
100981990:      ldr x8, [sp, #0x10]
100981994:      cmp x8, x22
100981998:      b.ne    0x1009819e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12dc>
10098199c:      b   0x100982080 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1978>
1009819a0:      mov x0, x19
1009819a4:      mov w2, #0x4                ; =4
1009819a8:      mov w3, #0x1                ; =1
1009819ac:      mov w4, #0x1                ; =1
1009819b0:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009819b4:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
1009819b8:      ldr x1, [x19, #0x10]
1009819bc:      b   0x100981978 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1270>
1009819c0:      ldr w2, [x8, #0x4]
1009819c4:      add x1, x8, #0x14
1009819c8:      mov x0, x19
1009819cc:      bl  0x10097231c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1009819d0:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
1009819d4:      add x22, x22, #0x1
1009819d8:      ldr x8, [sp, #0x10]
1009819dc:      cmp x8, x22
1009819e0:      b.eq    0x100982080 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1978>
1009819e4:      cbz x22, 0x100981a0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1304>
1009819e8:      ldr x26, [x19, #0x10]
1009819ec:      ldr x8, [x19]
1009819f0:      cmp x8, x26
1009819f4:      b.eq    0x100981e58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1750>
1009819f8:      ldr x8, [x19, #0x8]
1009819fc:      mov w9, #0x2c               ; =44
100981a00:      strb    w9, [x8, x26]
100981a04:      add x8, x26, #0x1
100981a08:      str x8, [x19, #0x10]
100981a0c:      ldr x8, [x25]
100981a10:      cmn x8, #0x1
100981a14:      b.eq    0x100981a88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1380>
100981a18:      mrs x9, TPIDRRO_EL0
100981a1c:      and x9, x9, #0xfffffffffffffff8
100981a20:      ldr x8, [x9, x8, lsl #3]
100981a24:      cbz x8, 0x100981a88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1380>
100981a28:      ldr x8, [x8, #0x19e8]
100981a2c:      cbz x8, 0x100981bf4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x14ec>
100981a30:      ldr x9, [x8]
100981a34:      cmp x9, x12
100981a38:      b.hs    0x1009825cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
100981a3c:      ldr x10, [sp, #0x28]
100981a40:      add x11, x9, #0x1
100981a44:      str x11, [x8]
100981a48:      ldr x11, [x8, #0x18]
100981a4c:      cmp x10, x11
100981a50:      b.hs    0x1009825c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
100981a54:      ldr x11, [x8, #0x10]
100981a58:      madd    x10, x10, x24, x11
100981a5c:      ldr x11, [x10]
100981a60:      cmp x11, #0x1
100981a64:      b.ne    0x1009825d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ed0>
100981a68:      ldr x0, [x10, #0x8]
100981a6c:      str x9, [x8]
100981a70:      add x8, x0, x22, lsl #3
100981a74:      ldr d8, [x8, #0x8]
100981a78:      fmov    x27, d8
100981a7c:      cmp x27, x23
100981a80:      b.ne    0x100981ae4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x13dc>
100981a84:      b   0x100981964 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
100981a88:      ldr x26, [sp, #0x28]
100981a8c:      ldrb    w8, [x21, #0x20]
100981a90:      cbnz    w8, 0x100981e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1770>
100981a94:      ldr x8, [x21]
100981a98:      cmp x8, x12
100981a9c:      b.hs    0x100982074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
100981aa0:      add x9, x8, #0x1
100981aa4:      str x9, [x21]
100981aa8:      ldr x9, [x21, #0x18]
100981aac:      cmp x26, x9
100981ab0:      b.hs    0x1009825c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
100981ab4:      ldr x9, [x21, #0x10]
100981ab8:      madd    x9, x26, x24, x9
100981abc:      ldr x10, [x9]
100981ac0:      cmp x10, #0x1
100981ac4:      b.ne    0x1009820c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19bc>
100981ac8:      ldr x0, [x9, #0x8]
100981acc:      str x8, [x21]
100981ad0:      add x8, x0, x22, lsl #3
100981ad4:      ldr d8, [x8, #0x8]
100981ad8:      fmov    x27, d8
100981adc:      cmp x27, x23
100981ae0:      b.eq    0x100981964 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
100981ae4:      and x8, x27, #0xffff000000000000
100981ae8:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
100981aec:      cmp x8, x9
100981af0:      b.eq    0x100981b10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1408>
100981af4:      mov x9, #0x7fff000000000000 ; =9223090561878065152
100981af8:      cmp x8, x9
100981afc:      b.ne    0x100981b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1494>
100981b00:      and x8, x27, #0xffffffffffff
100981b04:      cmp x8, #0x1, lsl #12       ; =0x1000
100981b08:      b.hs    0x1009819c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12b8>
100981b0c:      b   0x100981964 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
100981b10:      strb    wzr, [sp, #0x5c]
100981b14:      str wzr, [sp, #0x58]
100981b18:      ubfx    x1, x27, #40, #8
100981b1c:      cbz x1, 0x100981b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
100981b20:      strb    w27, [sp, #0x58]
100981b24:      cmp x1, #0x1
100981b28:      b.eq    0x100981b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
100981b2c:      lsr x8, x27, #8
100981b30:      strb    w8, [sp, #0x59]
100981b34:      cmp x1, #0x2
100981b38:      b.eq    0x100981b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
100981b3c:      lsr x8, x27, #16
100981b40:      strb    w8, [sp, #0x5a]
100981b44:      cmp x1, #0x3
100981b48:      b.eq    0x100981b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
100981b4c:      lsr x8, x27, #24
100981b50:      strb    w8, [sp, #0x5b]
100981b54:      cmp x1, #0x4
100981b58:      b.eq    0x100981b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1464>
100981b5c:      lsr x8, x27, #32
100981b60:      strb    w8, [sp, #0x5c]
100981b64:      cmp x1, #0x5
100981b68:      b.ne    0x1009827a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x209c>
100981b6c:      add x8, sp, #0x60
100981b70:      add x0, sp, #0x58
100981b74:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
100981b78:      ldr x8, [sp, #0x60]
100981b7c:      cbz x8, 0x100981c20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1518>
100981b80:      ldr x1, [x19, #0x10]
100981b84:      ldr x8, [x19]
100981b88:      sub x8, x8, x1
100981b8c:      cmp x8, #0x3
100981b90:      b.ls    0x100981f1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1814>
100981b94:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
100981b98:      b   0x100981978 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1270>
100981b9c:      mov x9, #0x2                ; =2
100981ba0:      movk    x9, #0x7ffc, lsl #48
100981ba4:      cmp x27, x9
100981ba8:      b.eq    0x100981964 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
100981bac:      mov x9, #0x3                ; =3
100981bb0:      movk    x9, #0x7ffc, lsl #48
100981bb4:      cmp x27, x9
100981bb8:      b.eq    0x100981c28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1520>
100981bbc:      mov x9, #0x4                ; =4
100981bc0:      movk    x9, #0x7ffc, lsl #48
100981bc4:      cmp x27, x9
100981bc8:      b.ne    0x100981c78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1570>
100981bcc:      ldr x1, [x19, #0x10]
100981bd0:      ldr x8, [x19]
100981bd4:      sub x8, x8, x1
100981bd8:      cmp x8, #0x3
100981bdc:      b.ls    0x100981f58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1850>
100981be0:      ldr x8, [x19, #0x8]
100981be4:      mov w9, #0x7274             ; =29300
100981be8:      movk    w9, #0x6575, lsl #16
100981bec:      str w9, [x8, x1]
100981bf0:      b   0x100981980 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1278>
100981bf4:      add x1, sp, #0x28
100981bf8:      adrp    x0, 0x1010d2000 <_anon.0c78480e1ec3114c482e9770ddf18575.1154+0x448>
100981bfc:      add x0, x0, #0xfd8
100981c00:      bl  0x100135710 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
100981c04:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
100981c08:      add x8, x0, x22, lsl #3
100981c0c:      ldr d8, [x8, #0x8]
100981c10:      fmov    x27, d8
100981c14:      cmp x27, x23
100981c18:      b.ne    0x100981ae4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x13dc>
100981c1c:      b   0x100981964 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x125c>
100981c20:      ldp x1, x2, [sp, #0x68]
100981c24:      b   0x1009819c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c0>
100981c28:      ldr x1, [x19, #0x10]
100981c2c:      ldr x8, [x19]
100981c30:      sub x8, x8, x1
100981c34:      cmp x8, #0x4
100981c38:      b.ls    0x100981f38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1830>
100981c3c:      ldr x8, [x19, #0x8]
100981c40:      add x8, x8, x1
100981c44:      mov w9, #0x65               ; =101
100981c48:      strb    w9, [x8, #0x4]
100981c4c:      mov w9, #0x6166             ; =24934
100981c50:      movk    w9, #0x736c, lsl #16
100981c54:      str w9, [x8]
100981c58:      ldr x8, [x19, #0x10]
100981c5c:      add x8, x8, #0x5
100981c60:      str x8, [x19, #0x10]
100981c64:      add x22, x22, #0x1
100981c68:      ldr x8, [sp, #0x10]
100981c6c:      cmp x8, x22
100981c70:      b.ne    0x1009819e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12dc>
100981c74:      b   0x100982080 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1978>
100981c78:      mov x9, #0x7ffd000000000000 ; =9222527611924643840
100981c7c:      cmp x8, x9
100981c80:      b.eq    0x100981cb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x15ac>
100981c84:      mov x9, #0x7ffa000000000000 ; =9221683186994511872
100981c88:      cmp x8, x9
100981c8c:      b.ne    0x100981d30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1628>
100981c90:      str x22, [sp, #0x60]
100981c94:      add x1, sp, #0x60
100981c98:      adrp    x0, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
100981c9c:      add x0, x0, #0x678
100981ca0:      bl  0x1001646ec <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
100981ca4:      mov.16b v0, v8
100981ca8:      mov x0, x19
100981cac:      bl  0x10097151c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars16serialize_bigint>
100981cb0:      b   0x1009819d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c8>
100981cb4:      str x22, [sp, #0x60]
100981cb8:      add x1, sp, #0x60
100981cbc:      adrp    x0, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
100981cc0:      add x0, x0, #0x678
100981cc4:      bl  0x1001646ec <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
100981cc8:      and x26, x27, #0xffffffffffff
100981ccc:      cmp x26, #0x100, lsl #12    ; =0x100000
100981cd0:      b.lo    0x100981d70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1668>
100981cd4:      mov x0, x27
100981cd8:      bl  0x10097f4ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify16is_closure_value>
100981cdc:      tbnz    w0, #0x0, 0x100981d70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1668>
100981ce0:      mov x0, x27
100981ce4:      bl  0x10097e7b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify15is_symbol_value>
100981ce8:      cbnz    w0, 0x100981d70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1668>
100981cec:      mov.16b v0, v8
100981cf0:      bl  0x1009b4004 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins10formatting16boxed_primitives26boxed_primitive_json_value>
100981cf4:      cmp x0, #0x1
100981cf8:      b.ne    0x100981db4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16ac>
100981cfc:      mov.16b v9, v0
100981d00:      mov x0, x26
100981d04:      bl  0x10097fff4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify18object_get_to_json>
100981d08:      tbz w0, #0x0, 0x100981dd0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16c8>
100981d0c:      mov.16b v8, v0
100981d10:      bl  0x100984eb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify24arm_to_json_result_guard>
100981d14:      add w1, w20, #0x1
100981d18:      mov.16b v0, v8
100981d1c:      mov x0, x19
100981d20:      bl  0x1009827b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
100981d24:      ldr x8, [sp, #0x8]
100981d28:      strb    wzr, [x8]
100981d2c:      b   0x100982014 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
100981d30:      lsr x8, x27, #52
100981d34:      cbnz    x8, 0x100981da4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x169c>
100981d38:      cbz x27, 0x100981da4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x169c>
100981d3c:      and x8, x27, #0x7
100981d40:      cbnz    x8, 0x100981da4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x169c>
100981d44:      mov x0, x27
100981d48:      bl  0x100985490 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
100981d4c:      tbz w0, #0x0, 0x100981da4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x169c>
100981d50:      str x22, [sp, #0x60]
100981d54:      add x1, sp, #0x60
100981d58:      adrp    x0, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
100981d5c:      add x0, x0, #0x678
100981d60:      bl  0x1001646ec <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
100981d64:      mov x26, x27
100981d68:      cmp x27, #0x100, lsl #12    ; =0x100000
100981d6c:      b.hs    0x100981cd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x15cc>
100981d70:      ldr x1, [x19, #0x10]
100981d74:      ldr x8, [x19]
100981d78:      sub x8, x8, x1
100981d7c:      cmp x8, #0x3
100981d80:      b.ls    0x100982020 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1918>
100981d84:      ldr x8, [x19, #0x8]
100981d88:      mov w28, #0x756e            ; =30062
100981d8c:      movk    w28, #0x6c6c, lsl #16
100981d90:      str w28, [x8, x1]
100981d94:      ldr x8, [x19, #0x10]
100981d98:      add x8, x8, #0x4
100981d9c:      str x8, [x19, #0x10]
100981da0:      b   0x1009819d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c8>
100981da4:      mov x0, x19
100981da8:      mov.16b v0, v8
100981dac:      bl  0x100971010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
100981db0:      b   0x1009819d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c8>
100981db4:      mov x0, x26
100981db8:      bl  0x100950d34 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4date17is_date_cell_addr>
100981dbc:      tbz w0, #0x0, 0x100981de4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16dc>
100981dc0:      mov.16b v0, v8
100981dc4:      bl  0x1008d8f70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object17date_proto_thunks18date_to_json_value>
100981dc8:      add w1, w20, #0x1
100981dcc:      b   0x100981dd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16d0>
100981dd0:      add w1, w20, #0x1
100981dd4:      mov.16b v0, v9
100981dd8:      mov x0, x19
100981ddc:      bl  0x1009827b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
100981de0:      b   0x100982014 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
100981de4:      mov x0, x26
100981de8:      bl  0x1005bab98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json8raw_json19raw_json_text_bytes>
100981dec:      cbz x0, 0x100981eb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17a8>
100981df0:      add x8, sp, #0x60
100981df4:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
100981df8:      ldr w27, [sp, #0x60]
100981dfc:      ldp x28, x8, [sp, #0x68]
100981e00:      cmp w27, #0x0
100981e04:      mov w9, #0x4                ; =4
100981e08:      csel    x26, x9, x8, ne
100981e0c:      ldr x1, [x19, #0x10]
100981e10:      ldr x8, [x19]
100981e14:      sub x8, x8, x1
100981e18:      cmp x26, x8
100981e1c:      b.hi    0x10098203c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1934>
100981e20:      cbz x26, 0x100981e4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1744>
100981e24:      cmp w27, #0x0
100981e28:      adrp    x8, 0x100e16000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text9SB_IBM866+0x6e>
100981e2c:      add x8, x8, #0x2a4
100981e30:      csel    x8, x8, x28, ne
100981e34:      ldr x9, [x19, #0x8]
100981e38:      add x0, x9, x1
100981e3c:      mov x1, x8
100981e40:      mov x2, x26
100981e44:      bl  0x100ce596c <_writev+0x100ce596c>
100981e48:      ldr x1, [x19, #0x10]
100981e4c:      add x8, x1, x26
100981e50:      str x8, [x19, #0x10]
100981e54:      b   0x100982014 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
100981e58:      mov x0, x19
100981e5c:      mov x1, x26
100981e60:      mov w2, #0x1                ; =1
100981e64:      mov w3, #0x1                ; =1
100981e68:      mov w4, #0x1                ; =1
100981e6c:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100981e70:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
100981e74:      b   0x1009819f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12f0>
100981e78:      cmp w8, #0x2
100981e7c:      b.eq    0x100982424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
100981e80:      mov x0, x21
100981e84:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
100981e88:      add x1, x1, #0xeec
100981e8c:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100981e90:      strb    wzr, [x21, #0x20]
100981e94:      mov w28, #0x756e            ; =30062
100981e98:      movk    w28, #0x6c6c, lsl #16
100981e9c:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
100981ea0:      ldr x8, [x21]
100981ea4:      cmp x8, x12
100981ea8:      b.lo    0x100981aa0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1398>
100981eac:      b   0x100982074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x196c>
100981eb0:      mov x0, x26
100981eb4:      bl  0x100989008 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header20is_registered_buffer>
100981eb8:      tbz w0, #0x0, 0x100981ecc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c4>
100981ebc:      mov x0, x26
100981ec0:      mov x1, x19
100981ec4:      bl  0x10096e208 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json16stringify_buffer16stringify_buffer>
100981ec8:      b   0x100982014 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
100981ecc:      mov x0, x26
100981ed0:      bl  0x100948560 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray23lookup_typed_array_kind>
100981ed4:      tbz w0, #0x0, 0x100981ee8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17e0>
100981ed8:      mov x0, x26
100981edc:      mov x1, x19
100981ee0:      bl  0x10096ee90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json16stringify_buffer21stringify_typed_array>
100981ee4:      b   0x100982014 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
100981ee8:      lsr x8, x26, #47
100981eec:      cbnz    x8, 0x100981fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18b4>
100981ef0:      ldurb   w8, [x26, #-0x8]
100981ef4:      cmp w8, #0x4
100981ef8:      b.gt    0x100981f78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1870>
100981efc:      cmp w8, #0x1
100981f00:      b.eq    0x100981ff4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18ec>
100981f04:      cmp w8, #0x2
100981f08:      b.eq    0x100981fc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18c0>
100981f0c:      cmp w8, #0x3
100981f10:      b.ne    0x100981fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18b4>
100981f14:      ldr w2, [x26, #0x4]
100981f18:      b   0x100982008 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1900>
100981f1c:      mov x0, x19
100981f20:      mov w2, #0x4                ; =4
100981f24:      mov w3, #0x1                ; =1
100981f28:      mov w4, #0x1                ; =1
100981f2c:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100981f30:      ldr x1, [x19, #0x10]
100981f34:      b   0x100981b94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x148c>
100981f38:      mov x0, x19
100981f3c:      mov w2, #0x5                ; =5
100981f40:      mov w3, #0x1                ; =1
100981f44:      mov w4, #0x1                ; =1
100981f48:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100981f4c:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
100981f50:      ldr x1, [x19, #0x10]
100981f54:      b   0x100981c3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1534>
100981f58:      mov x0, x19
100981f5c:      mov w2, #0x4                ; =4
100981f60:      mov w3, #0x1                ; =1
100981f64:      mov w4, #0x1                ; =1
100981f68:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100981f6c:      mov x12, #0x7fffffffffffffff ; =9223372036854775807
100981f70:      ldr x1, [x19, #0x10]
100981f74:      b   0x100981be0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x14d8>
100981f78:      cmp w8, #0x5
100981f7c:      b.eq    0x100981f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1888>
100981f80:      cmp w8, #0x8
100981f84:      b.eq    0x100981f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1888>
100981f88:      cmp w8, #0xc
100981f8c:      b.ne    0x100981fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18b4>
100981f90:      ldr x1, [x19, #0x10]
100981f94:      ldr x8, [x19]
100981f98:      sub x8, x8, x1
100981f9c:      cmp x8, #0x1
100981fa0:      b.ls    0x100982058 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1950>
100981fa4:      ldr x8, [x19, #0x8]
100981fa8:      mov w9, #0x7d7b             ; =32123
100981fac:      strh    w9, [x8, x1]
100981fb0:      ldr x8, [x19, #0x10]
100981fb4:      add x8, x8, #0x2
100981fb8:      b   0x100981e50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1748>
100981fbc:      mov x0, x26
100981fc0:      bl  0x10097fe00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify17is_object_pointer>
100981fc4:      tbz w0, #0x0, 0x100981fdc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18d4>
100981fc8:      add w2, w20, #0x1
100981fcc:      mov x0, x26
100981fd0:      mov x1, x19
100981fd4:      bl  0x1009834c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify22stringify_object_inner>
100981fd8:      b   0x100982014 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
100981fdc:      ldp w8, w2, [x26]
100981fe0:      sub w9, w2, #0x1
100981fe4:      mov w10, #0x270f            ; =9999
100981fe8:      cmp w9, w10
100981fec:      ccmp    w8, w2, #0x2, lo
100981ff0:      b.hi    0x100982008 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1900>
100981ff4:      add w2, w20, #0x1
100981ff8:      mov x0, x26
100981ffc:      mov x1, x19
100982000:      bl  0x100980708 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth>
100982004:      b   0x100982014 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x190c>
100982008:      add x1, x26, #0x14
10098200c:      mov x0, x19
100982010:      bl  0x10097231c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
100982014:      mov w28, #0x756e            ; =30062
100982018:      movk    w28, #0x6c6c, lsl #16
10098201c:      b   0x1009819d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12c8>
100982020:      mov x0, x19
100982024:      mov w2, #0x4                ; =4
100982028:      mov w3, #0x1                ; =1
10098202c:      mov w4, #0x1                ; =1
100982030:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100982034:      ldr x1, [x19, #0x10]
100982038:      b   0x100981d84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x167c>
10098203c:      mov x0, x19
100982040:      mov x2, x26
100982044:      mov w3, #0x1                ; =1
100982048:      mov w4, #0x1                ; =1
10098204c:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100982050:      ldr x1, [x19, #0x10]
100982054:      b   0x100981e24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x171c>
100982058:      mov x0, x19
10098205c:      mov w2, #0x2                ; =2
100982060:      mov w3, #0x1                ; =1
100982064:      mov w4, #0x1                ; =1
100982068:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
10098206c:      ldr x1, [x19, #0x10]
100982070:      b   0x100981fa4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x189c>
100982074:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
100982078:      add x0, x0, #0xf70
10098207c:      bl  0x100c99c5c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100982080:      ldr x21, [x19, #0x10]
100982084:      ldr x8, [x19]
100982088:      cmp x8, x21
10098208c:      b.eq    0x1009826b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1fa8>
100982090:      ldr x8, [x19, #0x8]
100982094:      mov w9, #0x5d               ; =93
100982098:      strb    w9, [x8, x21]
10098209c:      add x8, x21, #0x1
1009820a0:      str x8, [x19, #0x10]
1009820a4:      adrp    x0, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
1009820a8:      add x0, x0, #0x740
1009820ac:      bl  0x1001381d8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths7_0INtNtBZ_6option6OptionjEEB2j_>
1009820b0:      add x0, sp, #0x30
1009820b4:      bl  0x10092e764 <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueINtNtB4_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template13ShapeTemplateEEB13_>
1009820b8:      add x0, sp, #0x20
1009820bc:      bl  0x10092e998 <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1009820c0:      b   0x100980ad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3d0>
1009820c4:      adrp    x0, 0x100dc2000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2560>
1009820c8:      add x0, x0, #0x920
1009820cc:      mov w1, #0xb                ; =11
1009820d0:      bl  0x100cb7504 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1009820d4:      adrp    x0, 0x1010a3000 <_anon.58120679d426c7dccd15bda76f596bde.683>
1009820d8:      add x0, x0, #0x5c0
1009820dc:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1009820e0:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1009820e4:      add x0, x0, #0x498
1009820e8:      bl  0x100c99c5c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1009820ec:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1009820f0:      add x0, x0, #0x4b0
1009820f4:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1009820f8:      adrp    x0, 0x100dc2000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2560>
1009820fc:      add x0, x0, #0x9a9
100982100:      mov w1, #0xf                ; =15
100982104:      bl  0x100cb7504 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
100982108:      and x8, x0, #0xffffffffffff
10098210c:      cmp x8, #0x1, lsl #12       ; =0x1000
100982110:      b.hs    0x100982384 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c7c>
100982114:      ldr x8, [x19]
100982118:      sub x8, x8, x20
10098211c:      cmp x8, #0x3
100982120:      b.ls    0x1009826cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1fc4>
100982124:      ldr x8, [x19, #0x8]
100982128:      mov w9, #0x756e             ; =30062
10098212c:      movk    w9, #0x6c6c, lsl #16
100982130:      str w9, [x8, x20]
100982134:      ldr x8, [x19, #0x10]
100982138:      add x8, x8, #0x4
10098213c:      str x8, [x19, #0x10]
100982140:      cmp w21, #0x1
100982144:      b.ne    0x10098216c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a64>
100982148:      ldr x20, [x19, #0x10]
10098214c:      ldr x8, [x19]
100982150:      cmp x8, x20
100982154:      b.eq    0x100982638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f30>
100982158:      ldr x8, [x19, #0x8]
10098215c:      mov w9, #0x5d               ; =93
100982160:      strb    w9, [x8, x20]
100982164:      add x8, x20, #0x1
100982168:      b   0x100980ad4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3cc>
10098216c:      add x21, x22, #0x10
100982170:      mov w22, #0x756e            ; =30062
100982174:      movk    w22, #0x6c6c, lsl #16
100982178:      sub x23, x23, #0x8
10098217c:      mov w25, #0x2c              ; =44
100982180:      mov x26, #-0x7ffc000000000001 ; =-9222246136947933185
100982184:      mov x27, #0x3               ; =3
100982188:      movk    x27, #0x7ffc, lsl #48
10098218c:      mov x28, #0x4               ; =4
100982190:      movk    x28, #0x7ffc, lsl #48
100982194:      mov x24, #0x7ff9000000000000 ; =9221401712017801216
100982198:      b   0x1009821cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ac4>
10098219c:      ldr x1, [x19, #0x10]
1009821a0:      ldr x8, [x19]
1009821a4:      sub x8, x8, x1
1009821a8:      cmp x8, #0x3
1009821ac:      b.ls    0x100982304 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bfc>
1009821b0:      ldr x8, [x19, #0x8]
1009821b4:      str w22, [x8, x1]
1009821b8:      ldr x8, [x19, #0x10]
1009821bc:      add x8, x8, #0x4
1009821c0:      str x8, [x19, #0x10]
1009821c4:      subs    x23, x23, #0x8
1009821c8:      b.eq    0x100982148 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a40>
1009821cc:      ldr d0, [x21], #0x8
1009821d0:      ldr x20, [x19, #0x10]
1009821d4:      ldr x8, [x19]
1009821d8:      cmp x8, x20
1009821dc:      b.eq    0x1009822e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bd8>
1009821e0:      fmov    x0, d0
1009821e4:      ldr x8, [x19, #0x8]
1009821e8:      strb    w25, [x8, x20]
1009821ec:      add x1, x20, #0x1
1009821f0:      str x1, [x19, #0x10]
1009821f4:      add x8, x0, x26
1009821f8:      cmp x8, #0x2
1009821fc:      b.lo    0x1009821a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a98>
100982200:      cmp x0, x27
100982204:      b.eq    0x100982234 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b2c>
100982208:      cmp x0, x28
10098220c:      b.ne    0x10098226c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b64>
100982210:      ldr x8, [x19]
100982214:      sub x8, x8, x1
100982218:      cmp x8, #0x3
10098221c:      b.ls    0x100982320 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c18>
100982220:      ldr x8, [x19, #0x8]
100982224:      mov w9, #0x7274             ; =29300
100982228:      movk    w9, #0x6575, lsl #16
10098222c:      str w9, [x8, x1]
100982230:      b   0x1009821b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ab0>
100982234:      ldr x8, [x19]
100982238:      sub x8, x8, x1
10098223c:      cmp x8, #0x4
100982240:      b.ls    0x10098233c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c34>
100982244:      ldr x8, [x19, #0x8]
100982248:      add x8, x8, x1
10098224c:      mov w9, #0x65               ; =101
100982250:      strb    w9, [x8, #0x4]
100982254:      mov w9, #0x6166             ; =24934
100982258:      movk    w9, #0x736c, lsl #16
10098225c:      str w9, [x8]
100982260:      ldr x8, [x19, #0x10]
100982264:      add x8, x8, #0x5
100982268:      b   0x1009821c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ab8>
10098226c:      and x8, x0, #0xffff000000000000
100982270:      cmp x8, x24
100982274:      b.eq    0x10098229c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b94>
100982278:      mov x9, #0x7fff000000000000 ; =9223090561878065152
10098227c:      cmp x8, x9
100982280:      b.ne    0x1009822d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bcc>
100982284:      and x8, x0, #0xffffffffffff
100982288:      cmp x8, #0x1, lsl #12       ; =0x1000
10098228c:      b.lo    0x1009821a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a98>
100982290:      ldr w2, [x8, #0x4]
100982294:      add x1, x8, #0x14
100982298:      b   0x1009822c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bc0>
10098229c:      strb    wzr, [sp, #0x64]
1009822a0:      str wzr, [sp, #0x60]
1009822a4:      add x1, sp, #0x60
1009822a8:      bl  0x100941fa8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime5value7jsvalueNtB2_7JSValue19short_string_to_buf>
1009822ac:      mov x1, x0
1009822b0:      add x8, sp, #0x30
1009822b4:      add x0, sp, #0x60
1009822b8:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1009822bc:      ldr w8, [sp, #0x30]
1009822c0:      tbnz    w8, #0x0, 0x10098219c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a94>
1009822c4:      ldp x1, x2, [sp, #0x38]
1009822c8:      mov x0, x19
1009822cc:      bl  0x10097231c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1009822d0:      b   0x1009821c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1abc>
1009822d4:      mov x0, x19
1009822d8:      bl  0x100971010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
1009822dc:      b   0x1009821c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1abc>
1009822e0:      mov x0, x19
1009822e4:      mov x1, x20
1009822e8:      mov w2, #0x1                ; =1
1009822ec:      mov w3, #0x1                ; =1
1009822f0:      mov w4, #0x1                ; =1
1009822f4:      mov.16b v8, v0
1009822f8:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009822fc:      mov.16b v0, v8
100982300:      b   0x1009821e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ad8>
100982304:      mov x0, x19
100982308:      mov w2, #0x4                ; =4
10098230c:      mov w3, #0x1                ; =1
100982310:      mov w4, #0x1                ; =1
100982314:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100982318:      ldr x1, [x19, #0x10]
10098231c:      b   0x1009821b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1aa8>
100982320:      mov x0, x19
100982324:      mov w2, #0x4                ; =4
100982328:      mov w3, #0x1                ; =1
10098232c:      mov w4, #0x1                ; =1
100982330:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100982334:      ldr x1, [x19, #0x10]
100982338:      b   0x100982220 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b18>
10098233c:      mov x0, x19
100982340:      mov w2, #0x5                ; =5
100982344:      mov w3, #0x1                ; =1
100982348:      mov w4, #0x1                ; =1
10098234c:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100982350:      ldr x1, [x19, #0x10]
100982354:      b   0x100982244 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b3c>
100982358:      mov x0, x19
10098235c:      bl  0x100971010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
100982360:      b   0x100982140 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a38>
100982364:      adrp    x0, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
100982368:      add x0, x0, #0xa0
10098236c:      bl  0x100c99c5c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100982370:      adrp    x0, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
100982374:      add x0, x0, #0x280
100982378:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10098237c:      ldp x1, x2, [sp, #0x38]
100982380:      b   0x10098238c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c84>
100982384:      ldr w2, [x8, #0x4]
100982388:      add x1, x8, #0x14
10098238c:      mov x0, x19
100982390:      bl  0x10097231c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
100982394:      b   0x100982140 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a38>
100982398:      adrp    x0, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
10098239c:      add x0, x0, #0xb8
1009823a0:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1009823a4:      adrp    x0, 0x100e13000 <_anon.0c78480e1ec3114c482e9770ddf18575.1396+0x64>
1009823a8:      add x0, x0, #0xdc0
1009823ac:      mov w1, #0xf                ; =15
1009823b0:      bl  0x100cb7504 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1009823b4:      mov x0, x19
1009823b8:      mov x1, x23
1009823bc:      mov w2, #0x1                ; =1
1009823c0:      mov w3, #0x1                ; =1
1009823c4:      mov w4, #0x1                ; =1
1009823c8:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009823cc:      b   0x100980d44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x63c>
1009823d0:      mov x0, x19
1009823d4:      mov x1, x23
1009823d8:      mov w2, #0x1                ; =1
1009823dc:      mov w3, #0x1                ; =1
1009823e0:      mov w4, #0x1                ; =1
1009823e4:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009823e8:      b   0x100981444 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd3c>
1009823ec:      cmp w8, #0x1
1009823f0:      b.ne    0x100982424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d1c>
1009823f4:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1009823f8:      add x1, x1, #0xeec
1009823fc:      mov x0, x24
100982400:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100982404:      strb    wzr, [x24, #0x20]
100982408:      ldr x8, [x24]
10098240c:      cbz x8, 0x100980c34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x52c>
100982410:      adrp    x0, 0x1010a3000 <_anon.58120679d426c7dccd15bda76f596bde.683>
100982414:      add x0, x0, #0x8c0
100982418:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10098241c:      cmp w8, #0x2
100982420:      b.ne    0x100982430 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d28>
100982424:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
100982428:      add x0, x0, #0xed8
10098242c:      bl  0x100cdc11c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
100982430:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
100982434:      add x1, x1, #0xeec
100982438:      mov x0, x24
10098243c:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100982440:      strb    wzr, [x24, #0x20]
100982444:      ldr x8, [x24]
100982448:      cbz x8, 0x1009814bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdb4>
10098244c:      adrp    x0, 0x1010a3000 <_anon.58120679d426c7dccd15bda76f596bde.683>
100982450:      add x0, x0, #0x8d8
100982454:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100982458:      adrp    x0, 0x1010d2000 <_anon.0c78480e1ec3114c482e9770ddf18575.1154+0x448>
10098245c:      add x0, x0, #0xfd8
100982460:      add x1, sp, #0x28
100982464:      bl  0x100135710 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
100982468:      ldr d8, [x0, #0x8]
10098246c:      fmov    x0, d8
100982470:      add x1, sp, #0x30
100982474:      add w3, w20, #0x1
100982478:      mov x2, x19
10098247c:      bl  0x100974880 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template22try_emit_shape_element>
100982480:      cbnz    w0, 0x100982494 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d8c>
100982484:      add w1, w20, #0x1
100982488:      mov.16b v0, v8
10098248c:      mov x0, x19
100982490:      bl  0x1009827b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
100982494:      mov w8, #0x1                ; =1
100982498:      ldr x9, [sp, #0x10]
10098249c:      sub x25, x8, x9
1009824a0:      mov w26, #0x2               ; =2
1009824a4:      mov w27, #0x2c              ; =44
1009824a8:      adrp    x21, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
1009824ac:      add x21, x21, #0x678
1009824b0:      mov w28, #0x18              ; =24
1009824b4:      adrp    x22, 0x1010d2000 <_anon.0c78480e1ec3114c482e9770ddf18575.1154+0x448>
1009824b8:      add x22, x22, #0xfd8
1009824bc:      b   0x1009824d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1dc8>
1009824c0:      add x26, x26, #0x1
1009824c4:      add x8, x25, x26
1009824c8:      cmp x8, #0x2
1009824cc:      b.eq    0x1009825e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ee0>
1009824d0:      ldr x23, [x19, #0x10]
1009824d4:      ldr x8, [x19]
1009824d8:      cmp x8, x23
1009824dc:      b.eq    0x1009825ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ea4>
1009824e0:      sub x8, x26, #0x1
1009824e4:      ldr x9, [x19, #0x8]
1009824e8:      strb    w27, [x9, x23]
1009824ec:      add x9, x23, #0x1
1009824f0:      str x9, [x19, #0x10]
1009824f4:      str x8, [sp, #0x60]
1009824f8:      add x1, sp, #0x60
1009824fc:      mov x0, x21
100982500:      bl  0x1001646ec <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
100982504:      ldr x8, [x24]
100982508:      cmn x8, #0x1
10098250c:      b.eq    0x100982570 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e68>
100982510:      mrs x9, TPIDRRO_EL0
100982514:      and x9, x9, #0xfffffffffffffff8
100982518:      ldr x8, [x9, x8, lsl #3]
10098251c:      cbz x8, 0x100982570 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e68>
100982520:      ldr x8, [x8, #0x19e8]
100982524:      cbz x8, 0x100982570 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e68>
100982528:      ldr x9, [x8]
10098252c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100982530:      cmp x9, x10
100982534:      b.hs    0x1009825cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec4>
100982538:      ldr x10, [sp, #0x28]
10098253c:      add x11, x9, #0x1
100982540:      str x11, [x8]
100982544:      ldr x11, [x8, #0x18]
100982548:      cmp x10, x11
10098254c:      b.hs    0x1009825c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ec0>
100982550:      ldr x11, [x8, #0x10]
100982554:      madd    x10, x10, x28, x11
100982558:      ldr x11, [x10]
10098255c:      cmp x11, #0x1
100982560:      b.ne    0x1009825d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ed0>
100982564:      ldr x0, [x10, #0x8]
100982568:      str x9, [x8]
10098256c:      b   0x10098257c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e74>
100982570:      add x1, sp, #0x28
100982574:      mov x0, x22
100982578:      bl  0x100135710 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
10098257c:      ldr d8, [x0, x26, lsl #3]
100982580:      fmov    x0, d8
100982584:      add x1, sp, #0x30
100982588:      add w3, w20, #0x1
10098258c:      mov x2, x19
100982590:      bl  0x100974880 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template22try_emit_shape_element>
100982594:      tbnz    w0, #0x0, 0x1009824c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1db8>
100982598:      add w1, w20, #0x1
10098259c:      mov.16b v0, v8
1009825a0:      mov x0, x19
1009825a4:      bl  0x1009827b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
1009825a8:      b   0x1009824c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1db8>
1009825ac:      mov x0, x19
1009825b0:      mov x1, x23
1009825b4:      mov w2, #0x1                ; =1
1009825b8:      mov w3, #0x1                ; =1
1009825bc:      mov w4, #0x1                ; =1
1009825c0:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009825c4:      b   0x1009824e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1dd8>
1009825c8:      bl  0x100cb753c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1009825cc:      adrp    x0, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
1009825d0:      add x0, x0, #0x40
1009825d4:      bl  0x100c99c5c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1009825d8:      adrp    x0, 0x100e13000 <_anon.0c78480e1ec3114c482e9770ddf18575.1396+0x64>
1009825dc:      add x0, x0, #0xd4c
1009825e0:      mov w1, #0xb                ; =11
1009825e4:      bl  0x100cb7504 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1009825e8:      ldr x20, [x19, #0x10]
1009825ec:      ldr x8, [x19]
1009825f0:      cmp x8, x20
1009825f4:      b.eq    0x100982788 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2080>
1009825f8:      ldr x8, [x19, #0x8]
1009825fc:      mov w9, #0x5d               ; =93
100982600:      strb    w9, [x8, x20]
100982604:      add x8, x20, #0x1
100982608:      str x8, [x19, #0x10]
10098260c:      adrp    x0, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
100982610:      add x0, x0, #0x740
100982614:      bl  0x100138168 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths5_0INtNtBZ_6option6OptionjEEB2j_>
100982618:      b   0x1009820b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19a8>
10098261c:      mov x0, x19
100982620:      mov x1, x20
100982624:      mov w2, #0x1                ; =1
100982628:      mov w3, #0x1                ; =1
10098262c:      mov w4, #0x1                ; =1
100982630:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100982634:      b   0x100981570 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe68>
100982638:      mov x0, x19
10098263c:      mov x1, x20
100982640:      mov w2, #0x1                ; =1
100982644:      mov w3, #0x1                ; =1
100982648:      mov w4, #0x1                ; =1
10098264c:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100982650:      b   0x100982158 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a50>
100982654:      adrp    x0, 0x100e16000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text9SB_IBM866+0x6e>
100982658:      add x0, x0, #0xd40
10098265c:      mov w1, #0x34               ; =52
100982660:      bl  0x1009e3f0c <_js_string_from_bytes>
100982664:      bl  0x10091e6ac <_js_rangeerror_new>
100982668:      mov x8, #0x1                ; =1
10098266c:      movk    x8, #0x7ffc, lsl #48
100982670:      lsr x9, x0, #52
100982674:      mov x10, #0x7ffd000000000000 ; =9222527611924643840
100982678:      bfxil   x10, x0, #0, #48
10098267c:      cmp x9, #0x7fe
100982680:      csel    x9, x0, x10, hi
100982684:      cmp x0, #0x0
100982688:      csinc   x8, x9, x8, ne
10098268c:      fmov    d0, x8
100982690:      bl  0x1002f0a5c <_js_throw>
100982694:      mov x0, x19
100982698:      mov x1, x21
10098269c:      mov w2, #0x1                ; =1
1009826a0:      mov w3, #0x1                ; =1
1009826a4:      mov w4, #0x1                ; =1
1009826a8:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009826ac:      b   0x1009818f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11f0>
1009826b0:      mov x0, x19
1009826b4:      mov x1, x21
1009826b8:      mov w2, #0x1                ; =1
1009826bc:      mov w3, #0x1                ; =1
1009826c0:      mov w4, #0x1                ; =1
1009826c4:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009826c8:      b   0x100982090 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1988>
1009826cc:      mov x0, x19
1009826d0:      mov x1, x20
1009826d4:      mov w2, #0x4                ; =4
1009826d8:      mov w3, #0x1                ; =1
1009826dc:      mov w4, #0x1                ; =1
1009826e0:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009826e4:      ldr x20, [x19, #0x10]
1009826e8:      b   0x100982124 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a1c>
1009826ec:      mov x0, x19
1009826f0:      mov x1, x20
1009826f4:      mov w2, #0x5                ; =5
1009826f8:      mov w3, #0x1                ; =1
1009826fc:      mov w4, #0x1                ; =1
100982700:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100982704:      ldr x20, [x19, #0x10]
100982708:      b   0x1009815d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xed0>
10098270c:      mov x0, x19
100982710:      mov x1, x20
100982714:      mov w2, #0x4                ; =4
100982718:      mov w3, #0x1                ; =1
10098271c:      mov w4, #0x1                ; =1
100982720:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100982724:      ldr x20, [x19, #0x10]
100982728:      b   0x100981840 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1138>
10098272c:      adrp    x0, 0x100e16000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text9SB_IBM866+0x6e>
100982730:      add x0, x0, #0xd1a
100982734:      mov w1, #0x25               ; =37
100982738:      bl  0x1009e3f0c <_js_string_from_bytes>
10098273c:      bl  0x10092baa4 <_js_typeerror_new>
100982740:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100982744:      bfxil   x8, x0, #0, #48
100982748:      fmov    d0, x8
10098274c:      bl  0x1002f0a5c <_js_throw>
100982750:      mov x0, x19
100982754:      mov w2, #0x4                ; =4
100982758:      mov w3, #0x1                ; =1
10098275c:      mov w4, #0x1                ; =1
100982760:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100982764:      ldr x1, [x19, #0x10]
100982768:      b   0x1009818a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11a0>
10098276c:      mov x0, x19
100982770:      mov x1, x21
100982774:      mov w2, #0x1                ; =1
100982778:      mov w3, #0x1                ; =1
10098277c:      mov w4, #0x1                ; =1
100982780:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100982784:      b   0x100981774 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x106c>
100982788:      mov x0, x19
10098278c:      mov x1, x20
100982790:      mov w2, #0x1                ; =1
100982794:      mov w3, #0x1                ; =1
100982798:      mov w4, #0x1                ; =1
10098279c:      bl  0x100cad828 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009827a0:      b   0x1009825f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ef0>
1009827a4:      adrp    x2, 0x1010d3000 <_anon.49b593d0fbcdde013be92cf03f83678a.4+0x120>
1009827a8:      add x2, x2, #0x448
1009827ac:      mov w0, #0x5                ; =5
1009827b0:      mov w1, #0x5                ; =5
1009827b4:      bl  0x100c99d8c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
