/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008dc688 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan>:
1008dc688:      sub sp, sp, #0x120
1008dc68c:      stp x28, x27, [sp, #0xc0]
1008dc690:      stp x26, x25, [sp, #0xd0]
1008dc694:      stp x24, x23, [sp, #0xe0]
1008dc698:      stp x22, x21, [sp, #0xf0]
1008dc69c:      stp x20, x19, [sp, #0x100]
1008dc6a0:      stp x29, x30, [sp, #0x110]
1008dc6a4:      add x29, sp, #0x110
1008dc6a8:      mov x19, x0
1008dc6ac:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1008dc6b0:      add x8, x8, #0x7c4
1008dc6b4:      ldr w20, [x8]
1008dc6b8:      cmp w20, #0x300
1008dc6bc:      b.hs    0x1008dc7dc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x154>
1008dc6c0:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1008dc6c4:      add x8, x8, #0x4e8
1008dc6c8:      ldr x8, [x8]
1008dc6cc:      cmn x8, #0x1
1008dc6d0:      b.eq    0x1008dc7b4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x12c>
1008dc6d4:      mrs x9, TPIDRRO_EL0
1008dc6d8:      and x9, x9, #0xfffffffffffffff8
1008dc6dc:      ldr x0, [x9, x8, lsl #3]
1008dc6e0:      cbz x0, 0x1008dc7b4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x12c>
1008dc6e4:      add x8, x0, x20, lsl #3
1008dc6e8:      ldr x0, [x8, #0x1e8]
1008dc6ec:      cbz x0, 0x1008dc7dc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x154>
1008dc6f0:      ldr x0, [x0]
1008dc6f4:      cbz x0, 0x1008dc808 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x180>
1008dc6f8:      mov w8, #-0x40000001        ; =-1073741825
1008dc6fc:      cmp w2, w8
1008dc700:      b.gt    0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc704:      ldr x10, [x0, #0x5198]
1008dc708:      and w8, w2, #0x3fffffff
1008dc70c:      lsr x9, x8, #15
1008dc710:      cmp x9, x10
1008dc714:      b.hs    0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc718:      ldr x10, [x0, #0x5190]
1008dc71c:      ldr x9, [x10, x9, lsl #3]
1008dc720:      cbz x9, 0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc724:      ubfx    x10, x8, #5, #10
1008dc728:      ldr x9, [x9, x10, lsl #3]
1008dc72c:      cbz x9, 0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc730:      and x8, x8, #0x1f
1008dc734:      add x8, x9, x8, lsl #5
1008dc738:      ldrb    w9, [x8, #0x1c]
1008dc73c:      tbz w9, #0x0, 0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc740:      ldr x20, [x8]
1008dc744:      ldr w21, [x8, #0x14]
1008dc748:      cbz x20, 0x1008dc834 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ac>
1008dc74c:      mov x23, x1
1008dc750:      mov x24, x2
1008dc754:      mov x25, x3
1008dc758:      mov x0, x20
1008dc75c:      bl  0x10092bba8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008dc760:      cbz x0, 0x1008dc878 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1f0>
1008dc764:      ldrb    w8, [x0]
1008dc768:      cmp w8, #0x1
1008dc76c:      b.ne    0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc770:      ldrsb   w8, [x0, #0x1]
1008dc774:      tbnz    w8, #0x1f, 0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc778:      ldr w8, [x0, #0x4]
1008dc77c:      cmp w8, #0x10
1008dc780:      b.lo    0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc784:      ldp w22, w9, [x20]
1008dc788:      cmp w22, #0x20
1008dc78c:      ccmp    w22, w9, #0x2, ls
1008dc790:      b.hi    0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc794:      lsl x9, x22, #3
1008dc798:      add x9, x9, #0x10
1008dc79c:      cmp x9, x8
1008dc7a0:      mov x3, x25
1008dc7a4:      mov x2, x24
1008dc7a8:      mov x1, x23
1008dc7ac:      b.ls    0x1008dc838 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1b0>
1008dc7b0:      b   0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc7b4:      mov x21, x3
1008dc7b8:      mov x22, x2
1008dc7bc:      mov x23, x1
1008dc7c0:      bl  0x100ccaa2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1008dc7c4:      mov x1, x23
1008dc7c8:      mov x2, x22
1008dc7cc:      mov x3, x21
1008dc7d0:      add x8, x0, x20, lsl #3
1008dc7d4:      ldr x0, [x8, #0x1e8]
1008dc7d8:      cbnz    x0, 0x1008dc6f0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x68>
1008dc7dc:      adrp    x0, 0x1010b0000 <_anon.e80e0661ef5195a01080c4f807135b03.1285+0x98>
1008dc7e0:      add x0, x0, #0x828
1008dc7e4:      mov x20, x3
1008dc7e8:      mov x22, x2
1008dc7ec:      mov x21, x1
1008dc7f0:      bl  0x100cca520 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1008dc7f4:      mov x1, x21
1008dc7f8:      mov x2, x22
1008dc7fc:      mov x3, x20
1008dc800:      ldr x0, [x0]
1008dc804:      cbnz    x0, 0x1008dc6f8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x70>
1008dc808:      mov x20, x3
1008dc80c:      mov x21, x2
1008dc810:      mov x22, x1
1008dc814:      bl  0x100cc2dd4 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
1008dc818:      mov x1, x22
1008dc81c:      mov x2, x21
1008dc820:      mov x3, x20
1008dc824:      mov w8, #-0x40000001        ; =-1073741825
1008dc828:      cmp w2, w8
1008dc82c:      b.le    0x1008dc704 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x7c>
1008dc830:      b   0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc834:      mov x22, #0x0               ; =0
1008dc838:      mov x0, #0x0                ; =0
1008dc83c:      cmp x22, x21
1008dc840:      b.hi    0x1008dc878 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1f0>
1008dc844:      mov x8, x1
1008dc848:      ldr x9, [x19, #0x2398]
1008dc84c:      cmp x9, #0x40
1008dc850:      b.eq    0x1008dc878 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1f0>
1008dc854:      mov x21, x2
1008dc858:      mov x25, x3
1008dc85c:      mov x0, x8
1008dc860:      bl  0x100436ee4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe36to_json_definitely_absent_without_gc>
1008dc864:      cbz w0, 0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc868:      mov x0, x20
1008dc86c:      bl  0x10047c0cc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object13field_get_set11enumeration24keys_contain_array_index>
1008dc870:      tbz w0, #0x0, 0x1008dc898 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x210>
1008dc874:      mov x0, #0x0                ; =0
1008dc878:      ldp x29, x30, [sp, #0x110]
1008dc87c:      ldp x20, x19, [sp, #0x100]
1008dc880:      ldp x22, x21, [sp, #0xf0]
1008dc884:      ldp x24, x23, [sp, #0xe0]
1008dc888:      ldp x26, x25, [sp, #0xd0]
1008dc88c:      ldp x28, x27, [sp, #0xc0]
1008dc890:      add sp, sp, #0x120
1008dc894:      ret
1008dc898:      add x24, sp, #0x2c
1008dc89c:      movi.2d v0, #0000000000000000
1008dc8a0:      stur    q0, [x24, #0x7c]
1008dc8a4:      stur    q0, [x24, #0x6c]
1008dc8a8:      stur    q0, [x24, #0x5c]
1008dc8ac:      stur    q0, [sp, #0x78]
1008dc8b0:      stur    q0, [sp, #0x68]
1008dc8b4:      stur    q0, [sp, #0x58]
1008dc8b8:      stur    q0, [sp, #0x48]
1008dc8bc:      stur    q0, [sp, #0x38]
1008dc8c0:      stp w21, w22, [sp, #0x2c]
1008dc8c4:      mov x9, x19
1008dc8c8:      ldr x8, [x19, #0x10]
1008dc8cc:      str w8, [sp, #0x34]
1008dc8d0:      strb    wzr, [sp, #0xc]
1008dc8d4:      str wzr, [sp, #0x8]
1008dc8d8:      cbz x22, 0x1008dca44 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x3bc>
1008dc8dc:      ldr x9, [x20, #0x8]
1008dc8e0:      and x10, x9, #0xffff000000000000
1008dc8e4:      mov x11, #0x7fff000000000000 ; =9223090561878065152
1008dc8e8:      cmp x10, x11
1008dc8ec:      b.eq    0x1008dc91c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x294>
1008dc8f0:      mov x11, #0x7ff9000000000000 ; =9221401712017801216
1008dc8f4:      cmp x10, x11
1008dc8f8:      b.ne    0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc8fc:      ubfx    x10, x9, #40, #8
1008dc900:      cbz x10, 0x1008dc930 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x2a8>
1008dc904:      strb    w9, [sp, #0x8]
1008dc908:      cmp x10, #0x1
1008dc90c:      b.ne    0x1008dc93c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x2b4>
1008dc910:      add x0, sp, #0x8
1008dc914:      mov w1, #0x1                ; =1
1008dc918:      b   0x1008dc9a8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x320>
1008dc91c:      ands    x9, x9, #0xffffffffffff
1008dc920:      b.eq    0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc924:      ldr w1, [x9, #0x4]
1008dc928:      add x0, x9, #0x14
1008dc92c:      b   0x1008dc9a8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x320>
1008dc930:      mov x1, #0x0                ; =0
1008dc934:      add x0, sp, #0x8
1008dc938:      b   0x1008dc9a8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x320>
1008dc93c:      lsr x11, x9, #8
1008dc940:      strb    w11, [sp, #0x9]
1008dc944:      cmp x10, #0x2
1008dc948:      b.ne    0x1008dc958 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x2d0>
1008dc94c:      add x0, sp, #0x8
1008dc950:      mov w1, #0x2                ; =2
1008dc954:      b   0x1008dc9a8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x320>
1008dc958:      lsr x11, x9, #16
1008dc95c:      strb    w11, [sp, #0xa]
1008dc960:      cmp x10, #0x3
1008dc964:      b.ne    0x1008dc974 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x2ec>
1008dc968:      add x0, sp, #0x8
1008dc96c:      mov w1, #0x3                ; =3
1008dc970:      b   0x1008dc9a8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x320>
1008dc974:      lsr x11, x9, #24
1008dc978:      strb    w11, [sp, #0xb]
1008dc97c:      cmp x10, #0x4
1008dc980:      b.ne    0x1008dc990 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x308>
1008dc984:      add x0, sp, #0x8
1008dc988:      mov w1, #0x4                ; =4
1008dc98c:      b   0x1008dc9a8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x320>
1008dc990:      lsr x9, x9, #32
1008dc994:      strb    w9, [sp, #0xc]
1008dc998:      cmp x10, #0x5
1008dc99c:      b.ne    0x1008dccdc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x654>
1008dc9a0:      add x0, sp, #0x8
1008dc9a4:      mov w1, #0x5                ; =5
1008dc9a8:      cmp x8, #0x10, lsl #12      ; =0x10000
1008dc9ac:      b.hi    0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc9b0:      mov w9, #0x6                ; =6
1008dc9b4:      umull   x9, w1, w9
1008dc9b8:      mov w10, #0x10000           ; =65536
1008dc9bc:      add x9, x9, #0x4
1008dc9c0:      sub x8, x10, x8
1008dc9c4:      cmp x9, x8
1008dc9c8:      b.hi    0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc9cc:      add x8, sp, #0x10
1008dc9d0:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1008dc9d4:      ldr w8, [sp, #0x10]
1008dc9d8:      tbnz    w8, #0x0, 0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dc9dc:      ldp x1, x2, [sp, #0x18]
1008dc9e0:      mov x8, x19
1008dc9e4:      ldr x21, [x19, #0x10]
1008dc9e8:      ldr x9, [x19]
1008dc9ec:      cmp x9, x21
1008dc9f0:      b.eq    0x1008dcc78 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x5f0>
1008dc9f4:      ldr x9, [x8, #0x8]
1008dc9f8:      mov w10, #0x7b              ; =123
1008dc9fc:      strb    w10, [x9, x21]
1008dca00:      add x9, x21, #0x1
1008dca04:      str x9, [x8, #0x10]
1008dca08:      mov x0, x19
1008dca0c:      bl  0x1009160a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1008dca10:      mov x9, x19
1008dca14:      ldr x21, [x19, #0x10]
1008dca18:      ldr x8, [x19]
1008dca1c:      cmp x8, x21
1008dca20:      b.eq    0x1008dcca8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x620>
1008dca24:      ldr x8, [x9, #0x8]
1008dca28:      mov w10, #0x3a              ; =58
1008dca2c:      strb    w10, [x8, x21]
1008dca30:      add x8, x21, #0x1
1008dca34:      str x8, [x9, #0x10]
1008dca38:      str w8, [sp, #0x38]
1008dca3c:      subs    x28, x22, #0x1
1008dca40:      b.ne    0x1008dcac0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x438>
1008dca44:      ldr x1, [x9, #0x2398]
1008dca48:      cmp x1, #0x40
1008dca4c:      b.hs    0x1008dccc8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x640>
1008dca50:      mov w8, #0x8c               ; =140
1008dca54:      madd    x8, x1, x8, x9
1008dca58:      ldur    q0, [sp, #0x2c]
1008dca5c:      ldur    q1, [sp, #0x3c]
1008dca60:      stur    q0, [x8, #0x98]
1008dca64:      stur    q1, [x8, #0xa8]
1008dca68:      ldur    q0, [sp, #0x4c]
1008dca6c:      ldur    q1, [sp, #0x5c]
1008dca70:      stur    q0, [x8, #0xb8]
1008dca74:      stur    q1, [x8, #0xc8]
1008dca78:      ldp q0, q1, [x24, #0x60]
1008dca7c:      stur    q0, [x8, #0xf8]
1008dca80:      ldur    q0, [sp, #0x7c]
1008dca84:      ldur    q2, [sp, #0x6c]
1008dca88:      stur    q0, [x8, #0xe8]
1008dca8c:      stur    q2, [x8, #0xd8]
1008dca90:      add x8, x8, #0x98
1008dca94:      str q1, [x8, #0x70]
1008dca98:      ldur    q0, [x24, #0x7c]
1008dca9c:      stur    q0, [x8, #0x7c]
1008dcaa0:      ldr x8, [x9, #0x2398]
1008dcaa4:      add x8, x8, #0x1
1008dcaa8:      str x8, [x9, #0x2398]
1008dcaac:      add x8, x9, x25
1008dcab0:      add w9, w1, #0x1
1008dcab4:      strb    w9, [x8, #0x18]
1008dcab8:      mov w0, #0x1                ; =1
1008dcabc:      b   0x1008dc878 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1f0>
1008dcac0:      add x27, x20, #0x10
1008dcac4:      add x9, sp, #0x2c
1008dcac8:      add x26, x9, #0x10
1008dcacc:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
1008dcad0:      ldr x9, [x27], #0x8
1008dcad4:      and x11, x9, #0xffff000000000000
1008dcad8:      cmp x11, x10
1008dcadc:      b.eq    0x1008dcb00 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x478>
1008dcae0:      mov x10, #0x7fff000000000000 ; =9223090561878065152
1008dcae4:      cmp x11, x10
1008dcae8:      b.ne    0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dcaec:      ands    x9, x9, #0xffffffffffff
1008dcaf0:      b.eq    0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dcaf4:      ldr w1, [x9, #0x4]
1008dcaf8:      add x0, x9, #0x14
1008dcafc:      b   0x1008dcb98 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x510>
1008dcb00:      ubfx    x10, x9, #40, #8
1008dcb04:      cbz x10, 0x1008dcb20 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x498>
1008dcb08:      strb    w9, [sp, #0x8]
1008dcb0c:      cmp x10, #0x1
1008dcb10:      b.ne    0x1008dcb2c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x4a4>
1008dcb14:      add x0, sp, #0x8
1008dcb18:      mov w1, #0x1                ; =1
1008dcb1c:      b   0x1008dcb98 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x510>
1008dcb20:      mov x1, #0x0                ; =0
1008dcb24:      add x0, sp, #0x8
1008dcb28:      b   0x1008dcb98 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x510>
1008dcb2c:      lsr x11, x9, #8
1008dcb30:      strb    w11, [sp, #0x9]
1008dcb34:      cmp x10, #0x2
1008dcb38:      b.ne    0x1008dcb48 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x4c0>
1008dcb3c:      add x0, sp, #0x8
1008dcb40:      mov w1, #0x2                ; =2
1008dcb44:      b   0x1008dcb98 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x510>
1008dcb48:      lsr x11, x9, #16
1008dcb4c:      strb    w11, [sp, #0xa]
1008dcb50:      cmp x10, #0x3
1008dcb54:      b.ne    0x1008dcb64 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x4dc>
1008dcb58:      add x0, sp, #0x8
1008dcb5c:      mov w1, #0x3                ; =3
1008dcb60:      b   0x1008dcb98 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x510>
1008dcb64:      lsr x11, x9, #24
1008dcb68:      strb    w11, [sp, #0xb]
1008dcb6c:      cmp x10, #0x4
1008dcb70:      b.ne    0x1008dcb80 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x4f8>
1008dcb74:      add x0, sp, #0x8
1008dcb78:      mov w1, #0x4                ; =4
1008dcb7c:      b   0x1008dcb98 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x510>
1008dcb80:      lsr x9, x9, #32
1008dcb84:      strb    w9, [sp, #0xc]
1008dcb88:      cmp x10, #0x5
1008dcb8c:      b.ne    0x1008dccdc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x654>
1008dcb90:      add x0, sp, #0x8
1008dcb94:      mov w1, #0x5                ; =5
1008dcb98:      cmp x8, #0x10, lsl #12      ; =0x10000
1008dcb9c:      b.hi    0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dcba0:      mov w9, #0x6                ; =6
1008dcba4:      umull   x9, w1, w9
1008dcba8:      add x9, x9, #0x4
1008dcbac:      mov w10, #0x10000           ; =65536
1008dcbb0:      sub x8, x10, x8
1008dcbb4:      cmp x9, x8
1008dcbb8:      b.hi    0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dcbbc:      add x8, sp, #0x10
1008dcbc0:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1008dcbc4:      ldr x8, [sp, #0x10]
1008dcbc8:      cbnz    x8, 0x1008dc874 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008dcbcc:      ldp x20, x21, [sp, #0x18]
1008dcbd0:      ldr x22, [x19, #0x10]
1008dcbd4:      ldr x8, [x19]
1008dcbd8:      cmp x8, x22
1008dcbdc:      b.eq    0x1008dcc40 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x5b8>
1008dcbe0:      ldr x8, [x19, #0x8]
1008dcbe4:      mov w9, #0x2c               ; =44
1008dcbe8:      strb    w9, [x8, x22]
1008dcbec:      add x8, x22, #0x1
1008dcbf0:      str x8, [x19, #0x10]
1008dcbf4:      mov x0, x19
1008dcbf8:      mov x1, x20
1008dcbfc:      mov x2, x21
1008dcc00:      bl  0x1009160a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1008dcc04:      ldr x20, [x19, #0x10]
1008dcc08:      ldr x8, [x19]
1008dcc0c:      cmp x8, x20
1008dcc10:      b.eq    0x1008dcc5c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x5d4>
1008dcc14:      mov x9, x19
1008dcc18:      ldr x8, [x19, #0x8]
1008dcc1c:      mov w10, #0x3a              ; =58
1008dcc20:      strb    w10, [x8, x20]
1008dcc24:      add x8, x20, #0x1
1008dcc28:      str x8, [x19, #0x10]
1008dcc2c:      str w8, [x26], #0x4
1008dcc30:      subs    x28, x28, #0x1
1008dcc34:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
1008dcc38:      b.ne    0x1008dcad0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x448>
1008dcc3c:      b   0x1008dca44 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x3bc>
1008dcc40:      mov x0, x19
1008dcc44:      mov x1, x22
1008dcc48:      mov w2, #0x1                ; =1
1008dcc4c:      mov w3, #0x1                ; =1
1008dcc50:      mov w4, #0x1                ; =1
1008dcc54:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008dcc58:      b   0x1008dcbe0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x558>
1008dcc5c:      mov x0, x19
1008dcc60:      mov x1, x20
1008dcc64:      mov w2, #0x1                ; =1
1008dcc68:      mov w3, #0x1                ; =1
1008dcc6c:      mov w4, #0x1                ; =1
1008dcc70:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008dcc74:      b   0x1008dcc14 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x58c>
1008dcc78:      mov x0, x19
1008dcc7c:      mov x23, x1
1008dcc80:      mov x1, x21
1008dcc84:      mov x26, x2
1008dcc88:      mov w2, #0x1                ; =1
1008dcc8c:      mov w3, #0x1                ; =1
1008dcc90:      mov w4, #0x1                ; =1
1008dcc94:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008dcc98:      mov x2, x26
1008dcc9c:      mov x1, x23
1008dcca0:      mov x8, x19
1008dcca4:      b   0x1008dc9f4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x36c>
1008dcca8:      mov x0, x19
1008dccac:      mov x1, x21
1008dccb0:      mov w2, #0x1                ; =1
1008dccb4:      mov w3, #0x1                ; =1
1008dccb8:      mov w4, #0x1                ; =1
1008dccbc:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008dccc0:      mov x9, x19
1008dccc4:      b   0x1008dca24 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x39c>
1008dccc8:      adrp    x2, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
1008dcccc:      add x2, x2, #0xce0
1008dccd0:      mov x0, x1
1008dccd4:      mov w1, #0x40               ; =64
1008dccd8:      bl  0x100c9868c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1008dccdc:      adrp    x2, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
1008dcce0:      add x2, x2, #0xd10
1008dcce4:      mov w0, #0x5                ; =5
1008dcce8:      mov w1, #0x5                ; =5
1008dccec:      bl  0x100c9868c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
