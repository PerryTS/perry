/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010028a7f8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array>:
10028a7f8:      sub sp, sp, #0x30
10028a7fc:      stp x20, x19, [sp, #0x10]
10028a800:      stp x29, x30, [sp, #0x20]
10028a804:      add x29, sp, #0x20
10028a808:      mov x19, x0
10028a80c:      ldp x8, x9, [x0, #0x30]
10028a810:      add x20, x9, #0x1
10028a814:      str x20, [x0, #0x38]
10028a818:      cmp x20, x8
10028a81c:      b.hs    0x10028a880 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
10028a820:      ldr x9, [x19, #0x28]
10028a824:      mov x10, #0x2600            ; =9728
10028a828:      movk    x10, #0x1, lsl #32
10028a82c:      ldrb    w11, [x9, x20]
10028a830:      cmp w11, #0x20
10028a834:      b.hi    0x10028a880 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
10028a838:      lsr x11, x10, x11
10028a83c:      tbz w11, #0x0, 0x10028a880 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
10028a840:      add x20, x20, #0x1
10028a844:      str x20, [x19, #0x38]
10028a848:      cmp x8, x20
10028a84c:      b.ne    0x10028a82c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x34>
10028a850:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
10028a854:      add x0, x0, #0x590
10028a858:      ldr x8, [x0]
10028a85c:      blr x8
10028a860:      ldrb    w8, [x0, #0x20]
10028a864:      cbnz    w8, 0x10028a958 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x160>
10028a868:      ldr x8, [x0]
10028a86c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10028a870:      cmp x8, x9
10028a874:      b.hs    0x10028a988 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
10028a878:      ldr x1, [x0, #0x18]
10028a87c:      b   0x10028a910 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
10028a880:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
10028a884:      add x0, x0, #0x590
10028a888:      ldr x9, [x0]
10028a88c:      blr x9
10028a890:      ldrb    w9, [x0, #0x20]
10028a894:      cbnz    w9, 0x10028a924 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x12c>
10028a898:      ldr x9, [x0]
10028a89c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10028a8a0:      cmp x9, x10
10028a8a4:      b.hs    0x10028a988 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
10028a8a8:      ldr x1, [x0, #0x18]
10028a8ac:      subs    x8, x8, x20
10028a8b0:      b.ls    0x10028a910 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
10028a8b4:      ldr x9, [x19, #0x28]
10028a8b8:      ldrb    w9, [x9, x20]
10028a8bc:      cmp w9, #0x7b
10028a8c0:      b.ne    0x10028a910 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
10028a8c4:      mov x9, #-0x5555555555555556 ; =-6148914691236517206
10028a8c8:      movk    x9, #0xaaab
10028a8cc:      umulh   x8, x8, x9
10028a8d0:      lsr x8, x8, #6
10028a8d4:      mov w9, #0x10               ; =16
10028a8d8:      cmp x8, #0x10
10028a8dc:      csel    x8, x8, x9, hi
10028a8e0:      mov w9, #0x4000             ; =16384
10028a8e4:      cmp x8, #0x4, lsl #12       ; =0x4000
10028a8e8:      csel    x0, x8, x9, lo
10028a8ec:      mov x20, x1
10028a8f0:      bl  0x1002f710c <_js_array_alloc>
10028a8f4:      mov x1, x0
10028a8f8:      mov x0, x19
10028a8fc:      mov x2, x20
10028a900:      ldp x29, x30, [sp, #0x20]
10028a904:      ldp x20, x19, [sp, #0x10]
10028a908:      add sp, sp, #0x30
10028a90c:      b   0x10028af18 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>
10028a910:      mov x0, x19
10028a914:      ldp x29, x30, [sp, #0x20]
10028a918:      ldp x20, x19, [sp, #0x10]
10028a91c:      add sp, sp, #0x30
10028a920:      b   0x10028b724 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix>
10028a924:      cmp w9, #0x2
10028a928:      b.eq    0x10028a994 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x19c>
10028a92c:      stp x8, x0, [sp]
10028a930:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
10028a934:      add x1, x1, #0xf78
10028a938:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10028a93c:      ldp x8, x0, [sp]
10028a940:      strb    wzr, [x0, #0x20]
10028a944:      ldr x9, [x0]
10028a948:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10028a94c:      cmp x9, x10
10028a950:      b.lo    0x10028a8a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0xb0>
10028a954:      b   0x10028a988 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
10028a958:      cmp w8, #0x1
10028a95c:      b.ne    0x10028a994 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x19c>
10028a960:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
10028a964:      add x1, x1, #0xf78
10028a968:      str x0, [sp, #0x8]
10028a96c:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10028a970:      ldr x0, [sp, #0x8]
10028a974:      strb    wzr, [x0, #0x20]
10028a978:      ldr x8, [x0]
10028a97c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10028a980:      cmp x8, x9
10028a984:      b.lo    0x10028a878 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x80>
10028a988:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10028a98c:      add x0, x0, #0xe70
10028a990:      bl  0x100c9855c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10028a994:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10028a998:      add x0, x0, #0xed8
10028a99c:      bl  0x100cdab9c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
