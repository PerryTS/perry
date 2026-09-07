/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010033b824 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix>:
10033b824:      sub sp, sp, #0xb0
10033b828:      stp d9, d8, [sp, #0x40]
10033b82c:      stp x28, x27, [sp, #0x50]
10033b830:      stp x26, x25, [sp, #0x60]
10033b834:      stp x24, x23, [sp, #0x70]
10033b838:      stp x22, x21, [sp, #0x80]
10033b83c:      stp x20, x19, [sp, #0x90]
10033b840:      stp x29, x30, [sp, #0xa0]
10033b844:      add x29, sp, #0xa0
10033b848:      mov x19, x1
10033b84c:      mov x20, x0
10033b850:      adrp    x1, 0x100dcb000 <_anon.49fe03245c53ca65c809126415a26df1.1591+0x7a2>
10033b854:      add x1, x1, #0x20
10033b858:      mov x0, sp
10033b85c:      mov w2, #0x40               ; =64
10033b860:      bl  0x100ce9f90 <_writev+0x100ce9f90>
10033b864:      ldp x9, x8, [x20, #0x30]
10033b868:      cmp x8, x9
10033b86c:      b.hs    0x10033b8d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xb0>
10033b870:      ldr x9, [x20, #0x28]
10033b874:      ldrb    w9, [x9, x8]
10033b878:      cmp w9, #0x5d
10033b87c:      b.ne    0x10033b8d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xb0>
10033b880:      add x8, x8, #0x1
10033b884:      str x8, [x20, #0x38]
10033b888:      mov w0, #0x0                ; =0
10033b88c:      bl  0x10037a438 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array5alloc32js_array_alloc_with_length_exact>
10033b890:      mov x20, x0
10033b894:      bl  0x10037beb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header24set_array_numeric_layout>
10033b898:      adrp    x0, 0x10113a000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box17I32_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL+0x10>
10033b89c:      add x0, x0, #0x398
10033b8a0:      ldr x8, [x0]
10033b8a4:      blr x8
10033b8a8:      ldrb    w8, [x0, #0x20]
10033b8ac:      cbnz    w8, 0x10033bc08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x3e4>
10033b8b0:      ldr x8, [x0]
10033b8b4:      cbnz    x8, 0x10033bc30 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x40c>
10033b8b8:      ldr x8, [x0, #0x18]
10033b8bc:      cmp x19, x8
10033b8c0:      b.hi    0x10033b8c8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xa4>
10033b8c4:      str x19, [x0, #0x18]
10033b8c8:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
10033b8cc:      bfxil   x0, x20, #0, #48
10033b8d0:      b   0x10033bacc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2a8>
10033b8d4:      mov x0, x20
10033b8d8:      bl  0x10033aaa0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10033b8dc:      ldrb    w8, [x20, #0x90]
10033b8e0:      tbz w8, #0x0, 0x10033b92c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x108>
10033b8e4:      str x0, [sp]
10033b8e8:      ldp x9, x8, [x20, #0x30]
10033b8ec:      cmp x8, x9
10033b8f0:      b.hs    0x10033b970 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
10033b8f4:      ldr x10, [x20, #0x28]
10033b8f8:      mov x11, #0x2600            ; =9728
10033b8fc:      movk    x11, #0x1, lsl #32
10033b900:      mov w1, #0x1                ; =1
10033b904:      ldrb    w12, [x10, x8]
10033b908:      cmp w12, #0x20
10033b90c:      b.hi    0x10033b970 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
10033b910:      lsr x12, x11, x12
10033b914:      tbz w12, #0x0, 0x10033b970 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
10033b918:      add x8, x8, #0x1
10033b91c:      str x8, [x20, #0x38]
10033b920:      cmp x9, x8
10033b924:      b.ne    0x10033b904 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xe0>
10033b928:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033b92c:      mov x1, #0x0                ; =0
10033b930:      ldp x9, x8, [x20, #0x30]
10033b934:      cmp x8, x9
10033b938:      b.hs    0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033b93c:      ldr x10, [x20, #0x28]
10033b940:      mov x11, #0x2600            ; =9728
10033b944:      movk    x11, #0x1, lsl #32
10033b948:      ldrb    w12, [x10, x8]
10033b94c:      cmp w12, #0x20
10033b950:      b.hi    0x10033ba24 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
10033b954:      lsr x13, x11, x12
10033b958:      tbz w13, #0x0, 0x10033ba24 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
10033b95c:      add x8, x8, #0x1
10033b960:      str x8, [x20, #0x38]
10033b964:      cmp x9, x8
10033b968:      b.ne    0x10033b948 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x124>
10033b96c:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033b970:      cmp x8, x9
10033b974:      b.hs    0x10033b9e8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1c4>
10033b978:      ldr x10, [x20, #0x28]
10033b97c:      ldrb    w11, [x10, x8]
10033b980:      cmp w11, #0x2c
10033b984:      b.ne    0x10033b9f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1cc>
10033b988:      add x8, x8, #0x1
10033b98c:      str x8, [x20, #0x38]
10033b990:      mov x0, x20
10033b994:      bl  0x10033aaa0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10033b998:      ldrb    w8, [x20, #0x90]
10033b99c:      cbz w8, 0x10033ba38 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x214>
10033b9a0:      str x0, [sp, #0x8]
10033b9a4:      ldp x9, x8, [x20, #0x30]
10033b9a8:      cmp x8, x9
10033b9ac:      b.hs    0x10033ba40 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
10033b9b0:      ldr x10, [x20, #0x28]
10033b9b4:      mov x11, #0x2600            ; =9728
10033b9b8:      movk    x11, #0x1, lsl #32
10033b9bc:      mov w1, #0x2                ; =2
10033b9c0:      ldrb    w12, [x10, x8]
10033b9c4:      cmp w12, #0x20
10033b9c8:      b.hi    0x10033ba40 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
10033b9cc:      lsr x12, x11, x12
10033b9d0:      tbz w12, #0x0, 0x10033ba40 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
10033b9d4:      add x8, x8, #0x1
10033b9d8:      str x8, [x20, #0x38]
10033b9dc:      cmp x9, x8
10033b9e0:      b.ne    0x10033b9c0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x19c>
10033b9e4:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033b9e8:      mov w1, #0x1                ; =1
10033b9ec:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033b9f0:      mov w1, #0x1                ; =1
10033b9f4:      mov x11, #0x2600            ; =9728
10033b9f8:      movk    x11, #0x1, lsl #32
10033b9fc:      ldrb    w12, [x10, x8]
10033ba00:      cmp w12, #0x20
10033ba04:      b.hi    0x10033ba24 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
10033ba08:      lsr x13, x11, x12
10033ba0c:      tbz w13, #0x0, 0x10033ba24 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
10033ba10:      add x8, x8, #0x1
10033ba14:      str x8, [x20, #0x38]
10033ba18:      cmp x9, x8
10033ba1c:      b.ne    0x10033b9fc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d8>
10033ba20:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033ba24:      cmp w12, #0x5d
10033ba28:      b.ne    0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033ba2c:      add x8, x8, #0x1
10033ba30:      str x8, [x20, #0x38]
10033ba34:      b   0x10033bac0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x29c>
10033ba38:      mov w1, #0x1                ; =1
10033ba3c:      b   0x10033b930 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10033ba40:      cmp x8, x9
10033ba44:      b.hs    0x10033bab8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x294>
10033ba48:      ldr x10, [x20, #0x28]
10033ba4c:      ldrb    w11, [x10, x8]
10033ba50:      cmp w11, #0x2c
10033ba54:      b.ne    0x10033baf0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2cc>
10033ba58:      add x8, x8, #0x1
10033ba5c:      str x8, [x20, #0x38]
10033ba60:      mov x0, x20
10033ba64:      bl  0x10033aaa0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10033ba68:      ldrb    w8, [x20, #0x90]
10033ba6c:      cbz w8, 0x10033baf8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2d4>
10033ba70:      str x0, [sp, #0x10]
10033ba74:      ldp x9, x8, [x20, #0x30]
10033ba78:      cmp x8, x9
10033ba7c:      b.hs    0x10033bb00 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
10033ba80:      ldr x10, [x20, #0x28]
10033ba84:      mov x11, #0x2600            ; =9728
10033ba88:      movk    x11, #0x1, lsl #32
10033ba8c:      mov w1, #0x3                ; =3
10033ba90:      ldrb    w12, [x10, x8]
10033ba94:      cmp w12, #0x20
10033ba98:      b.hi    0x10033bb00 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
10033ba9c:      lsr x12, x11, x12
10033baa0:      tbz w12, #0x0, 0x10033bb00 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
10033baa4:      add x8, x8, #0x1
10033baa8:      str x8, [x20, #0x38]
10033baac:      cmp x9, x8
10033bab0:      b.ne    0x10033ba90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x26c>
10033bab4:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033bab8:      mov w1, #0x2                ; =2
10033babc:      strb    wzr, [x20, #0x90]
10033bac0:      mov x0, sp
10033bac4:      mov x2, x19
10033bac8:      bl  0x10033b574 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array>
10033bacc:      ldp x29, x30, [sp, #0xa0]
10033bad0:      ldp x20, x19, [sp, #0x90]
10033bad4:      ldp x22, x21, [sp, #0x80]
10033bad8:      ldp x24, x23, [sp, #0x70]
10033badc:      ldp x26, x25, [sp, #0x60]
10033bae0:      ldp x28, x27, [sp, #0x50]
10033bae4:      ldp d9, d8, [sp, #0x40]
10033bae8:      add sp, sp, #0xb0
10033baec:      ret
10033baf0:      mov w1, #0x2                ; =2
10033baf4:      b   0x10033b9f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10033baf8:      mov w1, #0x2                ; =2
10033bafc:      b   0x10033b930 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10033bb00:      cmp x8, x9
10033bb04:      b.hs    0x10033bb78 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x354>
10033bb08:      ldr x10, [x20, #0x28]
10033bb0c:      ldrb    w11, [x10, x8]
10033bb10:      cmp w11, #0x2c
10033bb14:      b.ne    0x10033bb80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x35c>
10033bb18:      add x8, x8, #0x1
10033bb1c:      str x8, [x20, #0x38]
10033bb20:      mov x0, x20
10033bb24:      bl  0x10033aaa0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10033bb28:      ldrb    w8, [x20, #0x90]
10033bb2c:      cbz w8, 0x10033bb88 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x364>
10033bb30:      str x0, [sp, #0x18]
10033bb34:      ldp x9, x8, [x20, #0x30]
10033bb38:      cmp x8, x9
10033bb3c:      b.hs    0x10033bb90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
10033bb40:      ldr x10, [x20, #0x28]
10033bb44:      mov x11, #0x2600            ; =9728
10033bb48:      movk    x11, #0x1, lsl #32
10033bb4c:      mov w1, #0x4                ; =4
10033bb50:      ldrb    w12, [x10, x8]
10033bb54:      cmp w12, #0x20
10033bb58:      b.hi    0x10033bb90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
10033bb5c:      lsr x12, x11, x12
10033bb60:      tbz w12, #0x0, 0x10033bb90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
10033bb64:      add x8, x8, #0x1
10033bb68:      str x8, [x20, #0x38]
10033bb6c:      cmp x9, x8
10033bb70:      b.ne    0x10033bb50 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x32c>
10033bb74:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033bb78:      mov w1, #0x3                ; =3
10033bb7c:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033bb80:      mov w1, #0x3                ; =3
10033bb84:      b   0x10033b9f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10033bb88:      mov w1, #0x3                ; =3
10033bb8c:      b   0x10033b930 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10033bb90:      cmp x8, x9
10033bb94:      b.hs    0x10033bc3c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x418>
10033bb98:      ldr x10, [x20, #0x28]
10033bb9c:      ldrb    w11, [x10, x8]
10033bba0:      cmp w11, #0x2c
10033bba4:      b.ne    0x10033bc50 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x42c>
10033bba8:      add x8, x8, #0x1
10033bbac:      str x8, [x20, #0x38]
10033bbb0:      mov x0, x20
10033bbb4:      bl  0x10033aaa0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10033bbb8:      ldrb    w8, [x20, #0x90]
10033bbbc:      cbz w8, 0x10033bc58 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x434>
10033bbc0:      str x0, [sp, #0x20]
10033bbc4:      ldp x9, x8, [x20, #0x30]
10033bbc8:      cmp x8, x9
10033bbcc:      b.hs    0x10033bc60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
10033bbd0:      ldr x10, [x20, #0x28]
10033bbd4:      mov x11, #0x2600            ; =9728
10033bbd8:      movk    x11, #0x1, lsl #32
10033bbdc:      mov w1, #0x5                ; =5
10033bbe0:      ldrb    w12, [x10, x8]
10033bbe4:      cmp w12, #0x20
10033bbe8:      b.hi    0x10033bc60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
10033bbec:      lsr x12, x11, x12
10033bbf0:      tbz w12, #0x0, 0x10033bc60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
10033bbf4:      add x8, x8, #0x1
10033bbf8:      str x8, [x20, #0x38]
10033bbfc:      cmp x9, x8
10033bc00:      b.ne    0x10033bbe0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x3bc>
10033bc04:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033bc08:      cmp w8, #0x1
10033bc0c:      b.ne    0x10033bc44 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x420>
10033bc10:      adrp    x1, 0x1003ed000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtB1a_7promise11keyed_table17PromiseKeyedTableNtNtB2z_11combinators15PromiseAllStateEEKj1_EEB1a_+0xf8>
10033bc14:      add x1, x1, #0x87c
10033bc18:      mov x21, x0
10033bc1c:      bl  0x100bac09c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10033bc20:      mov x0, x21
10033bc24:      strb    wzr, [x21, #0x20]
10033bc28:      ldr x8, [x21]
10033bc2c:      cbz x8, 0x10033b8b8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x94>
10033bc30:      adrp    x0, 0x1010a4000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10033bc34:      add x0, x0, #0xe58
10033bc38:      bl  0x100c9de6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10033bc3c:      mov w1, #0x4                ; =4
10033bc40:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033bc44:      adrp    x0, 0x1010a3000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10033bc48:      add x0, x0, #0xed8
10033bc4c:      bl  0x100ce071c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10033bc50:      mov w1, #0x4                ; =4
10033bc54:      b   0x10033b9f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10033bc58:      mov w1, #0x4                ; =4
10033bc5c:      b   0x10033b930 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10033bc60:      cmp x8, x9
10033bc64:      b.hs    0x10033bcd8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4b4>
10033bc68:      ldr x10, [x20, #0x28]
10033bc6c:      ldrb    w11, [x10, x8]
10033bc70:      cmp w11, #0x2c
10033bc74:      b.ne    0x10033bce0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4bc>
10033bc78:      add x8, x8, #0x1
10033bc7c:      str x8, [x20, #0x38]
10033bc80:      mov x0, x20
10033bc84:      bl  0x10033aaa0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10033bc88:      ldrb    w8, [x20, #0x90]
10033bc8c:      cbz w8, 0x10033bce8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4c4>
10033bc90:      str x0, [sp, #0x28]
10033bc94:      ldp x9, x8, [x20, #0x30]
10033bc98:      cmp x8, x9
10033bc9c:      b.hs    0x10033bcf0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
10033bca0:      ldr x10, [x20, #0x28]
10033bca4:      mov x11, #0x2600            ; =9728
10033bca8:      movk    x11, #0x1, lsl #32
10033bcac:      mov w1, #0x6                ; =6
10033bcb0:      ldrb    w12, [x10, x8]
10033bcb4:      cmp w12, #0x20
10033bcb8:      b.hi    0x10033bcf0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
10033bcbc:      lsr x12, x11, x12
10033bcc0:      tbz w12, #0x0, 0x10033bcf0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
10033bcc4:      add x8, x8, #0x1
10033bcc8:      str x8, [x20, #0x38]
10033bccc:      cmp x9, x8
10033bcd0:      b.ne    0x10033bcb0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x48c>
10033bcd4:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033bcd8:      mov w1, #0x5                ; =5
10033bcdc:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033bce0:      mov w1, #0x5                ; =5
10033bce4:      b   0x10033b9f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10033bce8:      mov w1, #0x5                ; =5
10033bcec:      b   0x10033b930 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10033bcf0:      cmp x8, x9
10033bcf4:      b.hs    0x10033bd68 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x544>
10033bcf8:      ldr x10, [x20, #0x28]
10033bcfc:      ldrb    w11, [x10, x8]
10033bd00:      cmp w11, #0x2c
10033bd04:      b.ne    0x10033bd70 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x54c>
10033bd08:      add x8, x8, #0x1
10033bd0c:      str x8, [x20, #0x38]
10033bd10:      mov x0, x20
10033bd14:      bl  0x10033aaa0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10033bd18:      ldrb    w8, [x20, #0x90]
10033bd1c:      cbz w8, 0x10033bd78 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x554>
10033bd20:      str x0, [sp, #0x30]
10033bd24:      ldp x9, x8, [x20, #0x30]
10033bd28:      cmp x8, x9
10033bd2c:      b.hs    0x10033bd80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
10033bd30:      ldr x10, [x20, #0x28]
10033bd34:      mov x11, #0x2600            ; =9728
10033bd38:      movk    x11, #0x1, lsl #32
10033bd3c:      mov w1, #0x7                ; =7
10033bd40:      ldrb    w12, [x10, x8]
10033bd44:      cmp w12, #0x20
10033bd48:      b.hi    0x10033bd80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
10033bd4c:      lsr x12, x11, x12
10033bd50:      tbz w12, #0x0, 0x10033bd80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
10033bd54:      add x8, x8, #0x1
10033bd58:      str x8, [x20, #0x38]
10033bd5c:      cmp x9, x8
10033bd60:      b.ne    0x10033bd40 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x51c>
10033bd64:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033bd68:      mov w1, #0x6                ; =6
10033bd6c:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033bd70:      mov w1, #0x6                ; =6
10033bd74:      b   0x10033b9f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10033bd78:      mov w1, #0x6                ; =6
10033bd7c:      b   0x10033b930 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10033bd80:      cmp x8, x9
10033bd84:      b.hs    0x10033bdf8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5d4>
10033bd88:      ldr x10, [x20, #0x28]
10033bd8c:      ldrb    w11, [x10, x8]
10033bd90:      cmp w11, #0x2c
10033bd94:      b.ne    0x10033be00 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5dc>
10033bd98:      add x8, x8, #0x1
10033bd9c:      str x8, [x20, #0x38]
10033bda0:      mov x0, x20
10033bda4:      bl  0x10033aaa0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10033bda8:      ldrb    w8, [x20, #0x90]
10033bdac:      cbz w8, 0x10033be08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5e4>
10033bdb0:      str x0, [sp, #0x38]
10033bdb4:      ldp x9, x8, [x20, #0x30]
10033bdb8:      cmp x8, x9
10033bdbc:      b.hs    0x10033be10 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
10033bdc0:      ldr x10, [x20, #0x28]
10033bdc4:      mov x11, #0x2600            ; =9728
10033bdc8:      movk    x11, #0x1, lsl #32
10033bdcc:      mov w1, #0x8                ; =8
10033bdd0:      ldrb    w12, [x10, x8]
10033bdd4:      cmp w12, #0x20
10033bdd8:      b.hi    0x10033be10 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
10033bddc:      lsr x12, x11, x12
10033bde0:      tbz w12, #0x0, 0x10033be10 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
10033bde4:      add x8, x8, #0x1
10033bde8:      str x8, [x20, #0x38]
10033bdec:      cmp x9, x8
10033bdf0:      b.ne    0x10033bdd0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ac>
10033bdf4:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033bdf8:      mov w1, #0x7                ; =7
10033bdfc:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033be00:      mov w1, #0x7                ; =7
10033be04:      b   0x10033b9f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10033be08:      mov w1, #0x7                ; =7
10033be0c:      b   0x10033b930 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10033be10:      cmp x8, x9
10033be14:      b.hs    0x10033bfd8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7b4>
10033be18:      ldr x10, [x20, #0x28]
10033be1c:      ldrb    w11, [x10, x8]
10033be20:      cmp w11, #0x2c
10033be24:      b.ne    0x10033bfe0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7bc>
10033be28:      add x8, x8, #0x1
10033be2c:      str x8, [x20, #0x38]
10033be30:      mov w0, #0x10               ; =16
10033be34:      bl  0x1003a92e8 <_js_array_alloc>
10033be38:      mov x21, x0
10033be3c:      mov x25, #0x0               ; =0
10033be40:      mov x27, sp
10033be44:      mov w28, #0x7ffe            ; =32766
10033be48:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
10033be4c:      fmov    d8, x8
10033be50:      lsr x26, x0, #3
10033be54:      b   0x10033be6c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x648>
10033be58:      add w8, w22, #0x1
10033be5c:      str w8, [x21]
10033be60:      add x25, x25, #0x8
10033be64:      cmp x25, #0x40
10033be68:      b.eq    0x10033bfe8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7c4>
10033be6c:      ldr x23, [x27, x25]
10033be70:      ldp w22, w8, [x21]
10033be74:      cmp w22, w8
10033be78:      b.hs    0x10033beb0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x68c>
10033be7c:      add x8, x21, #0x8
10033be80:      add x24, x8, x22, lsl #3
10033be84:      str x23, [x24]
10033be88:      mov x0, x21
10033be8c:      bl  0x10037afc4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
10033be90:      tbz w0, #0x0, 0x10033becc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6a8>
10033be94:      cmp x28, x23, lsr #48
10033be98:      b.ne    0x10033beec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6c8>
10033be9c:      mov x0, x23
10033bea0:      bl  0x1008033fc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10033bea4:      tbnz    w0, #0x0, 0x10033bf08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10033bea8:      scvtf   d0, w23
10033beac:      b   0x10033bf04 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e0>
10033beb0:      fmov    d0, x23
10033beb4:      mov x0, x21
10033beb8:      bl  0x1006d92c8 <_js_array_push_f64>
10033bebc:      add x25, x25, #0x8
10033bec0:      cmp x25, #0x40
10033bec4:      b.ne    0x10033be6c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x648>
10033bec8:      b   0x10033bfe8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7c4>
10033becc:      cmp x26, #0x201
10033bed0:      b.lo    0x10033bf08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10033bed4:      ldurb   w8, [x21, #-0x8]
10033bed8:      cmp w8, #0x1
10033bedc:      b.ne    0x10033bf08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10033bee0:      ldurh   w8, [x21, #-0x6]
10033bee4:      tbnz    w8, #0xc, 0x10033be94 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x670>
10033bee8:      b   0x10033bf08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10033beec:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
10033bef0:      cmp x23, x8
10033bef4:      b.gt    0x10033bf08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10033bef8:      fmov    d0, x23
10033befc:      fcmp    d0, d0
10033bf00:      fcsel   d0, d8, d0, vs
10033bf04:      fmov    x23, d0
10033bf08:      str x23, [x24]
10033bf0c:      cmp x28, x23, lsr #48
10033bf10:      b.ne    0x10033bf28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x704>
10033bf14:      mov x0, x23
10033bf18:      bl  0x1008033fc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10033bf1c:      tbnz    w0, #0x0, 0x10033bf34 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x710>
10033bf20:      scvtf   d0, w23
10033bf24:      b   0x10033bf7c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x758>
10033bf28:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
10033bf2c:      cmp x23, x8
10033bf30:      b.le    0x10033bf70 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x74c>
10033bf34:      cmp x26, #0x201
10033bf38:      b.lo    0x10033bfa8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10033bf3c:      ldurb   w8, [x21, #-0x8]
10033bf40:      cmp w8, #0x1
10033bf44:      b.ne    0x10033bfa8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10033bf48:      ldurh   w8, [x21, #-0x6]
10033bf4c:      mov w9, #0xef7f             ; =61311
10033bf50:      and w9, w8, w9
10033bf54:      sturh   w9, [x21, #-0x6]
10033bf58:      mov w9, #0x1080             ; =4224
10033bf5c:      tst w8, w9
10033bf60:      b.eq    0x10033bfa8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10033bf64:      mov x0, x21
10033bf68:      bl  0x10066de20 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
10033bf6c:      b   0x10033bfa8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10033bf70:      fmov    d0, x23
10033bf74:      fcmp    d0, d0
10033bf78:      fcsel   d0, d8, d0, vs
10033bf7c:      cmp x26, #0x201
10033bf80:      b.lo    0x10033bfa8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10033bf84:      ldurb   w8, [x21, #-0x8]
10033bf88:      cmp w8, #0x1
10033bf8c:      b.ne    0x10033bfa8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10033bf90:      ldurh   w8, [x21, #-0x6]
10033bf94:      tbz w8, #0x7, 0x10033bfa8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10033bf98:      ldr w8, [x21]
10033bf9c:      cmp w22, w8
10033bfa0:      b.hs    0x10033bfa8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10033bfa4:      str d0, [x24]
10033bfa8:      mov x0, x21
10033bfac:      mov x1, x22
10033bfb0:      mov x2, x23
10033bfb4:      bl  0x100242640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
10033bfb8:      mov x0, x21
10033bfbc:      bl  0x10036cee4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
10033bfc0:      cbz w0, 0x10033be58 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x634>
10033bfc4:      mov x0, x21
10033bfc8:      mov x1, x24
10033bfcc:      mov x2, x23
10033bfd0:      bl  0x10067d0a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc13barrier_store26runtime_write_barrier_slot>
10033bfd4:      b   0x10033be58 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x634>
10033bfd8:      mov w1, #0x8                ; =8
10033bfdc:      b   0x10033babc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10033bfe0:      mov w1, #0x8                ; =8
10033bfe4:      b   0x10033b9f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10033bfe8:      mov x0, x20
10033bfec:      mov x1, x21
10033bff0:      mov x2, x19
10033bff4:      ldp x29, x30, [sp, #0xa0]
10033bff8:      ldp x20, x19, [sp, #0x90]
10033bffc:      ldp x22, x21, [sp, #0x80]
10033c000:      ldp x24, x23, [sp, #0x70]
10033c004:      ldp x26, x25, [sp, #0x60]
10033c008:      ldp x28, x27, [sp, #0x50]
10033c00c:      ldp d9, d8, [sp, #0x40]
10033c010:      add sp, sp, #0xb0
10033c014:      b   0x10033b018 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>
