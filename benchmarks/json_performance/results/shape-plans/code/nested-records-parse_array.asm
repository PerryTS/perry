/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/nested-records-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008907f8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array>:
1008907f8:      sub sp, sp, #0x30
1008907fc:      stp x20, x19, [sp, #0x10]
100890800:      stp x29, x30, [sp, #0x20]
100890804:      add x29, sp, #0x20
100890808:      mov x19, x0
10089080c:      ldp x8, x9, [x0, #0x30]
100890810:      add x20, x9, #0x1
100890814:      str x20, [x0, #0x38]
100890818:      cmp x20, x8
10089081c:      b.hs    0x100890880 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
100890820:      ldr x9, [x19, #0x28]
100890824:      mov x10, #0x2600            ; =9728
100890828:      movk    x10, #0x1, lsl #32
10089082c:      ldrb    w11, [x9, x20]
100890830:      cmp w11, #0x20
100890834:      b.hi    0x100890880 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
100890838:      lsr x11, x10, x11
10089083c:      tbz w11, #0x0, 0x100890880 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
100890840:      add x20, x20, #0x1
100890844:      str x20, [x19, #0x38]
100890848:      cmp x8, x20
10089084c:      b.ne    0x10089082c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x34>
100890850:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
100890854:      add x0, x0, #0x2a0
100890858:      ldr x8, [x0]
10089085c:      blr x8
100890860:      ldrb    w8, [x0, #0x20]
100890864:      cbnz    w8, 0x100890958 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x160>
100890868:      ldr x8, [x0]
10089086c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100890870:      cmp x8, x9
100890874:      b.hs    0x100890988 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
100890878:      ldr x1, [x0, #0x18]
10089087c:      b   0x100890910 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
100890880:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
100890884:      add x0, x0, #0x2a0
100890888:      ldr x9, [x0]
10089088c:      blr x9
100890890:      ldrb    w9, [x0, #0x20]
100890894:      cbnz    w9, 0x100890924 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x12c>
100890898:      ldr x9, [x0]
10089089c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1008908a0:      cmp x9, x10
1008908a4:      b.hs    0x100890988 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
1008908a8:      ldr x1, [x0, #0x18]
1008908ac:      subs    x8, x8, x20
1008908b0:      b.ls    0x100890910 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
1008908b4:      ldr x9, [x19, #0x28]
1008908b8:      ldrb    w9, [x9, x20]
1008908bc:      cmp w9, #0x7b
1008908c0:      b.ne    0x100890910 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
1008908c4:      mov x9, #-0x5555555555555556 ; =-6148914691236517206
1008908c8:      movk    x9, #0xaaab
1008908cc:      umulh   x8, x8, x9
1008908d0:      lsr x8, x8, #6
1008908d4:      mov w9, #0x10               ; =16
1008908d8:      cmp x8, #0x10
1008908dc:      csel    x8, x8, x9, hi
1008908e0:      mov w9, #0x4000             ; =16384
1008908e4:      cmp x8, #0x4, lsl #12       ; =0x4000
1008908e8:      csel    x0, x8, x9, lo
1008908ec:      mov x20, x1
1008908f0:      bl  0x1008f7cc0 <_js_array_alloc>
1008908f4:      mov x1, x0
1008908f8:      mov x0, x19
1008908fc:      mov x2, x20
100890900:      ldp x29, x30, [sp, #0x20]
100890904:      ldp x20, x19, [sp, #0x10]
100890908:      add sp, sp, #0x30
10089090c:      b   0x100890f18 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>
100890910:      mov x0, x19
100890914:      ldp x29, x30, [sp, #0x20]
100890918:      ldp x20, x19, [sp, #0x10]
10089091c:      add sp, sp, #0x30
100890920:      b   0x100891724 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix>
100890924:      cmp w9, #0x2
100890928:      b.eq    0x100890994 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x19c>
10089092c:      stp x8, x0, [sp]
100890930:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
100890934:      add x1, x1, #0xeec
100890938:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10089093c:      ldp x8, x0, [sp]
100890940:      strb    wzr, [x0, #0x20]
100890944:      ldr x9, [x0]
100890948:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10089094c:      cmp x9, x10
100890950:      b.lo    0x1008908a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0xb0>
100890954:      b   0x100890988 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
100890958:      cmp w8, #0x1
10089095c:      b.ne    0x100890994 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x19c>
100890960:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
100890964:      add x1, x1, #0xeec
100890968:      str x0, [sp, #0x8]
10089096c:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100890970:      ldr x0, [sp, #0x8]
100890974:      strb    wzr, [x0, #0x20]
100890978:      ldr x8, [x0]
10089097c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100890980:      cmp x8, x9
100890984:      b.lo    0x100890878 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x80>
100890988:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10089098c:      add x0, x0, #0xe70
100890990:      bl  0x100c99c5c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100890994:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
100890998:      add x0, x0, #0xed8
10089099c:      bl  0x100cdc11c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
