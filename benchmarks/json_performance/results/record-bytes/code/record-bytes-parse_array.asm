/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010033a8f8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array>:
10033a8f8:      sub sp, sp, #0x30
10033a8fc:      stp x20, x19, [sp, #0x10]
10033a900:      stp x29, x30, [sp, #0x20]
10033a904:      add x29, sp, #0x20
10033a908:      mov x19, x0
10033a90c:      ldp x8, x9, [x0, #0x30]
10033a910:      add x20, x9, #0x1
10033a914:      str x20, [x0, #0x38]
10033a918:      cmp x20, x8
10033a91c:      b.hs    0x10033a980 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
10033a920:      ldr x9, [x19, #0x28]
10033a924:      mov x10, #0x2600            ; =9728
10033a928:      movk    x10, #0x1, lsl #32
10033a92c:      ldrb    w11, [x9, x20]
10033a930:      cmp w11, #0x20
10033a934:      b.hi    0x10033a980 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
10033a938:      lsr x11, x10, x11
10033a93c:      tbz w11, #0x0, 0x10033a980 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
10033a940:      add x20, x20, #0x1
10033a944:      str x20, [x19, #0x38]
10033a948:      cmp x8, x20
10033a94c:      b.ne    0x10033a92c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x34>
10033a950:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
10033a954:      add x0, x0, #0x398
10033a958:      ldr x8, [x0]
10033a95c:      blr x8
10033a960:      ldrb    w8, [x0, #0x20]
10033a964:      cbnz    w8, 0x10033aa58 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x160>
10033a968:      ldr x8, [x0]
10033a96c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10033a970:      cmp x8, x9
10033a974:      b.hs    0x10033aa88 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
10033a978:      ldr x1, [x0, #0x18]
10033a97c:      b   0x10033aa10 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
10033a980:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
10033a984:      add x0, x0, #0x398
10033a988:      ldr x9, [x0]
10033a98c:      blr x9
10033a990:      ldrb    w9, [x0, #0x20]
10033a994:      cbnz    w9, 0x10033aa24 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x12c>
10033a998:      ldr x9, [x0]
10033a99c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10033a9a0:      cmp x9, x10
10033a9a4:      b.hs    0x10033aa88 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
10033a9a8:      ldr x1, [x0, #0x18]
10033a9ac:      subs    x8, x8, x20
10033a9b0:      b.ls    0x10033aa10 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
10033a9b4:      ldr x9, [x19, #0x28]
10033a9b8:      ldrb    w9, [x9, x20]
10033a9bc:      cmp w9, #0x7b
10033a9c0:      b.ne    0x10033aa10 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
10033a9c4:      mov x9, #-0x5555555555555556 ; =-6148914691236517206
10033a9c8:      movk    x9, #0xaaab
10033a9cc:      umulh   x8, x8, x9
10033a9d0:      lsr x8, x8, #6
10033a9d4:      mov w9, #0x10               ; =16
10033a9d8:      cmp x8, #0x10
10033a9dc:      csel    x8, x8, x9, hi
10033a9e0:      mov w9, #0x4000             ; =16384
10033a9e4:      cmp x8, #0x4, lsl #12       ; =0x4000
10033a9e8:      csel    x0, x8, x9, lo
10033a9ec:      mov x20, x1
10033a9f0:      bl  0x1003a92e8 <_js_array_alloc>
10033a9f4:      mov x1, x0
10033a9f8:      mov x0, x19
10033a9fc:      mov x2, x20
10033aa00:      ldp x29, x30, [sp, #0x20]
10033aa04:      ldp x20, x19, [sp, #0x10]
10033aa08:      add sp, sp, #0x30
10033aa0c:      b   0x10033b018 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>
10033aa10:      mov x0, x19
10033aa14:      ldp x29, x30, [sp, #0x20]
10033aa18:      ldp x20, x19, [sp, #0x10]
10033aa1c:      add sp, sp, #0x30
10033aa20:      b   0x10033b824 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix>
10033aa24:      cmp w9, #0x2
10033aa28:      b.eq    0x10033aa94 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x19c>
10033aa2c:      stp x8, x0, [sp]
10033aa30:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10033aa34:      add x1, x1, #0x87c
10033aa38:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10033aa3c:      ldp x8, x0, [sp]
10033aa40:      strb    wzr, [x0, #0x20]
10033aa44:      ldr x9, [x0]
10033aa48:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10033aa4c:      cmp x9, x10
10033aa50:      b.lo    0x10033a9a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0xb0>
10033aa54:      b   0x10033aa88 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
10033aa58:      cmp w8, #0x1
10033aa5c:      b.ne    0x10033aa94 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x19c>
10033aa60:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10033aa64:      add x1, x1, #0x87c
10033aa68:      str x0, [sp, #0x8]
10033aa6c:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10033aa70:      ldr x0, [sp, #0x8]
10033aa74:      strb    wzr, [x0, #0x20]
10033aa78:      ldr x8, [x0]
10033aa7c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10033aa80:      cmp x8, x9
10033aa84:      b.lo    0x10033a978 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x80>
10033aa88:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10033aa8c:      add x0, x0, #0xe70
10033aa90:      bl  0x100c9de9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10033aa94:      adrp    x0, 0x1010a3000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10033aa98:      add x0, x0, #0xed8
10033aa9c:      bl  0x100ce071c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
